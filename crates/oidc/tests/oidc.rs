use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{
    Algorithm, EncodingKey, Header, encode,
    jwk::{Jwk, PublicKeyUse},
};
use oidc::{OidcClient, OidcConfig, OidcError, OidcSession, OidcTokenCache, OidcUserProfile};
use serde_json::{Value, json};
use url::Url;

const CLIENT_ID: &str = "desktop-client";
const KEY_ID: &str = "test-key";
const PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgWTFfCGljY6aw3Hrt
kHmPRiazukxPLb6ilpRAewjW8nihRANCAATDskChT+Altkm9X7MI69T3IUmrQU0L
950IxEzvw/x5BMEINRMrXLBJhqzO9Bm+d6JbqA21YQmd1Kt4RzLJR1W+
-----END PRIVATE KEY-----"#;

#[derive(Debug, Clone)]
struct HttpRequest {
    path: String,
    body: String,
}

struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

impl HttpResponse {
    fn json(value: Value) -> Self {
        Self::json_with_status("200 OK", value)
    }

    fn json_with_status(status: &'static str, value: Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum InvalidTokenCase {
    Issuer,
    Audience,
    Expired,
    Nonce,
    Signature,
}

#[test]
fn config_and_cached_session_keep_existing_behavior() {
    let config = OidcConfig::new(
        "https://id.example.com/",
        CLIENT_ID,
        ["profile email"],
        "http://127.0.0.1:0/auth/callback",
    )
    .unwrap();

    assert_eq!(config.scopes(), &["openid", "profile", "email"]);
    assert_eq!(config.redirect_port(), 0);
    assert_eq!(config.callback_path(), "/auth/callback");

    let tokens = OidcTokenCache {
        access_token: "access".to_owned(),
        profile: Some(profile()),
        ..Default::default()
    };
    let session = OidcSession::from_token_cache(tokens).unwrap();
    assert_eq!(session.profile().display_name(), "Ada");
}

#[test]
fn login_uses_pkce_nonce_and_validates_id_token_before_userinfo() {
    let nonce = Arc::new(Mutex::new(None::<String>));
    let server_nonce = Arc::clone(&nonce);
    let (encoding_key, jwk) = signing_material();
    let (issuer, server) = spawn_provider(4, move |issuer| {
        let issuer = issuer.to_owned();
        move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                HttpResponse::json(discovery_document(&issuer, true))
            }
            "/token" => {
                let nonce = server_nonce.lock().unwrap().clone().unwrap();
                let id_token = signed_id_token_with_picture(
                    &encoding_key,
                    &issuer,
                    CLIENT_ID,
                    &nonce,
                    now() + 3_600,
                    "https://cdn.example.com/id-token-avatar.png",
                );
                HttpResponse::json(json!({
                    "access_token": "access-1",
                    "refresh_token": "refresh-1",
                    "id_token": id_token,
                    "token_type": "Bearer",
                    "expires_in": 300
                }))
            }
            "/jwks" => HttpResponse::json(json!({ "keys": [jwk] })),
            "/userinfo" => HttpResponse::json(json!({
                "sub": "user-1",
                "name": "Ada Lovelace",
                "email": "ada@example.com"
            })),
            path => panic!("unexpected provider request: {path}"),
        }
    });
    let client = client(&issuer);
    let pending = client.begin_login().unwrap();
    let authorization_url = Url::parse(pending.authorization_url()).unwrap();
    let parameters = query_parameters(&authorization_url);

    assert_eq!(
        parameters.get("response_type").map(String::as_str),
        Some("code")
    );
    assert_eq!(
        parameters.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert!(
        parameters
            .get("code_challenge")
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        parameters
            .get("nonce")
            .is_some_and(|value| !value.is_empty())
    );
    *nonce.lock().unwrap() = parameters.get("nonce").cloned();

    let session = finish_login(pending, &parameters).unwrap();
    server.join().unwrap();

    assert_eq!(session.profile().subject, "user-1");
    assert_eq!(session.profile().display_name(), "Ada Lovelace");
    assert_eq!(
        session.profile().picture.as_deref(),
        Some("https://cdn.example.com/id-token-avatar.png")
    );
    assert_eq!(session.tokens().refresh_token.as_deref(), Some("refresh-1"));
}

#[test]
fn login_rejects_invalid_signature_issuer_audience_expiration_and_nonce() {
    for case in [
        InvalidTokenCase::Issuer,
        InvalidTokenCase::Audience,
        InvalidTokenCase::Expired,
        InvalidTokenCase::Nonce,
        InvalidTokenCase::Signature,
    ] {
        let error = invalid_login(case);
        if matches!(case, InvalidTokenCase::Nonce) {
            assert!(matches!(error, OidcError::InvalidNonce));
        } else {
            assert!(matches!(error, OidcError::InvalidIdToken(_)));
        }
    }
}

#[test]
fn discovery_rejects_an_issuer_different_from_configuration() {
    let (issuer, server) = spawn_provider(1, |issuer| {
        let issuer = issuer.to_owned();
        move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                let mut document = discovery_document(&issuer, false);
                document["issuer"] = json!("https://unexpected.example.com");
                HttpResponse::json(document)
            }
            path => panic!("unexpected provider request: {path}"),
        }
    });

    let error = client(&issuer).begin_login().unwrap_err();
    server.join().unwrap();
    assert!(matches!(error, OidcError::DiscoveryIssuerMismatch { .. }));
}

#[test]
fn callback_failure_returns_a_complete_safe_html_page() {
    let (issuer, server) = spawn_provider(1, |issuer| {
        let issuer = issuer.to_owned();
        move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                HttpResponse::json(discovery_document(&issuer, false))
            }
            path => panic!("unexpected provider request: {path}"),
        }
    });
    let pending = client(&issuer).begin_login().unwrap();
    let parameters = query_parameters(&Url::parse(pending.authorization_url()).unwrap());
    let mut callback = Url::parse(parameters.get("redirect_uri").unwrap()).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("code", "secret-authorization-code")
        .append_pair("state", "unexpected-state");
    let callback_thread = thread::spawn(move || send_get(&callback));

    let error = pending.finish().unwrap_err();
    let response = callback_thread.join().unwrap();
    server.join().unwrap();

    assert!(matches!(error, OidcError::InvalidState));
    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.contains("content-security-policy:"));
    assert!(response.contains("cache-control: no-store"));
    assert!(response.contains("登录未完成"));
    assert!(response.contains("返回桌面应用"));
    assert!(response.contains("prefers-color-scheme: dark"));
    assert!(!response.contains("secret-authorization-code"));
    assert!(!response.contains("unexpected-state"));
}

#[test]
fn refresh_grant_rotates_tokens_and_validates_a_new_id_token() {
    let requests = Arc::new(Mutex::new(Vec::<HttpRequest>::new()));
    let server_requests = Arc::clone(&requests);
    let (encoding_key, jwk) = signing_material();
    let (issuer, server) = spawn_provider(3, move |issuer| {
        let issuer = issuer.to_owned();
        move |request| {
            server_requests.lock().unwrap().push(request.clone());
            match request.path.as_str() {
                "/.well-known/openid-configuration" => {
                    HttpResponse::json(discovery_document(&issuer, false))
                }
                "/token" => {
                    let id_token = signed_id_token(
                        &encoding_key,
                        &issuer,
                        CLIENT_ID,
                        "unused-on-refresh",
                        now() + 3_600,
                    );
                    HttpResponse::json(json!({
                        "access_token": "access-2",
                        "refresh_token": "refresh-2",
                        "id_token": id_token,
                        "expires_in": 600
                    }))
                }
                "/jwks" => HttpResponse::json(json!({ "keys": [jwk] })),
                path => panic!("unexpected provider request: {path}"),
            }
        }
    });
    let tokens = OidcTokenCache {
        access_token: "access-1".to_owned(),
        refresh_token: Some("refresh-1".to_owned()),
        id_token: Some("old-id-token".to_owned()),
        token_type: Some("Bearer".to_owned()),
        scope: Some("openid profile".to_owned()),
        profile: Some(profile()),
        ..Default::default()
    };

    let session = client(&issuer).refresh(&tokens).unwrap();
    server.join().unwrap();

    assert_eq!(session.tokens().access_token, "access-2");
    assert_eq!(session.tokens().refresh_token.as_deref(), Some("refresh-2"));
    assert_ne!(session.tokens().id_token.as_deref(), Some("old-id-token"));
    assert_eq!(session.tokens().token_type.as_deref(), Some("Bearer"));
    assert_eq!(session.tokens().scope.as_deref(), Some("openid profile"));
    assert_eq!(session.profile(), &profile());

    let requests = requests.lock().unwrap();
    let token_request = requests
        .iter()
        .find(|request| request.path == "/token")
        .unwrap();
    let form = url::form_urlencoded::parse(token_request.body.as_bytes())
        .into_owned()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        form.get("grant_type").map(String::as_str),
        Some("refresh_token")
    );
    assert_eq!(form.get("client_id").map(String::as_str), Some(CLIENT_ID));
    assert_eq!(
        form.get("refresh_token").map(String::as_str),
        Some("refresh-1")
    );
}

#[test]
fn refresh_updates_the_profile_picture_from_userinfo() {
    let (encoding_key, jwk) = signing_material();
    let (issuer, server) = spawn_provider(4, move |issuer| {
        let issuer = issuer.to_owned();
        move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                HttpResponse::json(discovery_document(&issuer, true))
            }
            "/token" => {
                let id_token = signed_id_token(
                    &encoding_key,
                    &issuer,
                    CLIENT_ID,
                    "unused-on-refresh",
                    now() + 3_600,
                );
                HttpResponse::json(json!({
                    "access_token": "access-2",
                    "refresh_token": "refresh-2",
                    "id_token": id_token,
                    "expires_in": 600
                }))
            }
            "/jwks" => HttpResponse::json(json!({ "keys": [jwk] })),
            "/userinfo" => HttpResponse::json(json!({
                "sub": "user-1",
                "name": "Ada Lovelace",
                "picture": "https://cdn.example.com/new-avatar.png"
            })),
            path => panic!("unexpected provider request: {path}"),
        }
    });
    let mut cached_profile = profile();
    cached_profile.picture = Some("https://cdn.example.com/old-avatar.png".to_owned());
    let tokens = OidcTokenCache {
        access_token: "access-1".to_owned(),
        refresh_token: Some("refresh-1".to_owned()),
        profile: Some(cached_profile),
        ..Default::default()
    };

    let session = client(&issuer).refresh(&tokens).unwrap();
    server.join().unwrap();

    assert_eq!(session.profile().display_name(), "Ada Lovelace");
    assert_eq!(
        session.profile().picture.as_deref(),
        Some("https://cdn.example.com/new-avatar.png")
    );
}

#[test]
fn refresh_requires_a_refresh_token() {
    let config = OidcConfig::new(
        "https://id.example.com",
        CLIENT_ID,
        ["openid"],
        "http://127.0.0.1:0/auth/callback",
    )
    .unwrap();
    let error = OidcClient::new(config)
        .unwrap()
        .refresh(&OidcTokenCache::default())
        .unwrap_err();

    assert!(matches!(error, OidcError::MissingRefreshToken));
}

#[test]
fn refresh_invalid_grant_is_distinguishable_from_transient_failures() {
    let (issuer, server) = spawn_provider(2, |issuer| {
        let issuer = issuer.to_owned();
        move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                HttpResponse::json(discovery_document(&issuer, false))
            }
            "/token" => HttpResponse::json_with_status(
                "400 Bad Request",
                json!({
                    "error": "invalid_grant",
                    "error_description": "refresh token has expired"
                }),
            ),
            path => panic!("unexpected provider request: {path}"),
        }
    });
    let tokens = OidcTokenCache {
        access_token: "expired-access".to_owned(),
        refresh_token: Some("rejected-refresh".to_owned()),
        profile: Some(profile()),
        ..Default::default()
    };

    let error = client(&issuer).refresh(&tokens).unwrap_err();
    server.join().unwrap();

    assert!(error.is_refresh_token_rejected());
    assert!(error.to_string().contains("refresh token has expired"));
    assert!(!OidcError::MissingRefreshToken.is_refresh_token_rejected());
}

fn invalid_login(case: InvalidTokenCase) -> OidcError {
    let nonce = Arc::new(Mutex::new(None::<String>));
    let server_nonce = Arc::clone(&nonce);
    let (encoding_key, jwk) = signing_material();
    let (issuer, server) = spawn_provider(3, move |issuer| {
        let issuer = issuer.to_owned();
        move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                HttpResponse::json(discovery_document(&issuer, false))
            }
            "/token" => {
                let expected_nonce = server_nonce.lock().unwrap().clone().unwrap();
                let token_issuer = if matches!(case, InvalidTokenCase::Issuer) {
                    "https://wrong-issuer.example.com"
                } else {
                    issuer.as_str()
                };
                let audience = if matches!(case, InvalidTokenCase::Audience) {
                    "other-client"
                } else {
                    CLIENT_ID
                };
                let nonce = if matches!(case, InvalidTokenCase::Nonce) {
                    "wrong-nonce"
                } else {
                    expected_nonce.as_str()
                };
                let expires_at = if matches!(case, InvalidTokenCase::Expired) {
                    now().saturating_sub(3_600)
                } else {
                    now() + 3_600
                };
                let mut id_token =
                    signed_id_token(&encoding_key, token_issuer, audience, nonce, expires_at);
                if matches!(case, InvalidTokenCase::Signature) {
                    corrupt_signature(&mut id_token);
                }
                HttpResponse::json(json!({
                    "access_token": "access-1",
                    "id_token": id_token,
                    "expires_in": 300
                }))
            }
            "/jwks" => HttpResponse::json(json!({ "keys": [jwk] })),
            path => panic!("unexpected provider request: {path}"),
        }
    });
    let pending = client(&issuer).begin_login().unwrap();
    let parameters = query_parameters(&Url::parse(pending.authorization_url()).unwrap());
    *nonce.lock().unwrap() = parameters.get("nonce").cloned();
    let error = finish_login(pending, &parameters).unwrap_err();
    server.join().unwrap();
    error
}

fn client(issuer: &str) -> OidcClient {
    let config = OidcConfig::new(
        issuer,
        CLIENT_ID,
        ["openid profile email offline_access"],
        "http://127.0.0.1:0/auth/callback",
    )
    .unwrap();
    OidcClient::new(config).unwrap()
}

fn profile() -> OidcUserProfile {
    OidcUserProfile {
        subject: "user-1".to_owned(),
        name: Some("Ada".to_owned()),
        email: Some("ada@example.com".to_owned()),
        preferred_username: None,
        picture: None,
    }
}

fn signing_material() -> (EncodingKey, Jwk) {
    let encoding_key = EncodingKey::from_ec_pem(PRIVATE_KEY.as_bytes()).unwrap();
    let mut jwk = Jwk::from_encoding_key(&encoding_key, Algorithm::ES256).unwrap();
    jwk.common.key_id = Some(KEY_ID.to_owned());
    jwk.common.public_key_use = Some(PublicKeyUse::Signature);
    (encoding_key, jwk)
}

fn signed_id_token(
    key: &EncodingKey,
    issuer: &str,
    audience: &str,
    nonce: &str,
    expires_at: u64,
) -> String {
    signed_id_token_with_optional_picture(key, issuer, audience, nonce, expires_at, None)
}

fn signed_id_token_with_picture(
    key: &EncodingKey,
    issuer: &str,
    audience: &str,
    nonce: &str,
    expires_at: u64,
    picture: &str,
) -> String {
    signed_id_token_with_optional_picture(key, issuer, audience, nonce, expires_at, Some(picture))
}

fn signed_id_token_with_optional_picture(
    key: &EncodingKey,
    issuer: &str,
    audience: &str,
    nonce: &str,
    expires_at: u64,
    picture: Option<&str>,
) -> String {
    let claims = json!({
        "iss": issuer,
        "sub": "user-1",
        "aud": audience,
        "exp": expires_at,
        "iat": now(),
        "nonce": nonce,
        "name": "Ada",
        "picture": picture
    });
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(KEY_ID.to_owned());
    encode(&header, &claims, key).unwrap()
}

fn corrupt_signature(token: &mut String) {
    let replacement = if token.ends_with('A') { 'B' } else { 'A' };
    token.pop();
    token.push(replacement);
}

fn discovery_document(issuer: &str, include_userinfo: bool) -> Value {
    let mut document = json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "id_token_signing_alg_values_supported": ["ES256"]
    });
    if include_userinfo {
        document["userinfo_endpoint"] = json!(format!("{issuer}/userinfo"));
    }
    document
}

fn finish_login(
    pending: oidc::PendingOidcLogin,
    parameters: &HashMap<String, String>,
) -> Result<OidcSession, OidcError> {
    let mut callback = Url::parse(parameters.get("redirect_uri").unwrap()).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("code", "authorization-code")
        .append_pair("state", parameters.get("state").unwrap());
    let callback_thread = thread::spawn(move || send_get(&callback));
    let result = pending.finish();
    let response = callback_thread.join().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("content-security-policy:"));
    assert!(response.contains("cache-control: no-store"));
    assert!(response.contains("登录已完成"));
    assert!(response.contains("返回桌面应用"));
    assert!(response.contains("prefers-color-scheme: dark"));
    assert!(!response.contains("authorization-code"));
    result
}

fn query_parameters(url: &Url) -> HashMap<String, String> {
    url.query_pairs().into_owned().collect()
}

fn send_get(url: &Url) -> String {
    let address = (url.host_str().unwrap(), url.port().unwrap());
    let mut stream = TcpStream::connect(address).unwrap();
    let target = url[url::Position::BeforePath..].to_owned();
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nhost: {}\r\nconnection: close\r\n\r\n",
        url.host_str().unwrap()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn spawn_provider<F, M>(request_count: usize, make_handler: M) -> (String, JoinHandle<()>)
where
    F: Fn(HttpRequest) -> HttpResponse + Send + 'static,
    M: FnOnce(&str) -> F,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let handler = make_handler(&issuer);
    let thread = thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            let response = handler(request);
            write_response(&mut stream, response);
        }
    });
    (issuer, thread)
}

fn read_request(stream: &mut TcpStream) -> HttpRequest {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let path = request_line.split_whitespace().nth(1).unwrap().to_owned();
    let mut content_length = 0_usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header == "\r\n" || header.is_empty() {
            break;
        }
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).unwrap();
    HttpRequest {
        path,
        body: String::from_utf8(body).unwrap(),
    }
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) {
    write!(
        stream,
        "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        response.status,
        response.content_type,
        response.body.len(),
        response.body
    )
    .unwrap();
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
