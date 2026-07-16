use std::{
    io::{BufRead as _, BufReader, Write as _},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use account::authentication::{AccessTokenVerifier, OidcAccessTokenVerifier, VerificationError};
use jsonwebtoken::{
    Algorithm, EncodingKey, Header, encode,
    jwk::{Jwk, PublicKeyUse},
};
use serde_json::json;

const AUDIENCE: &str = "xuwe-api";
const KEY_ID: &str = "test-key";
const PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgWTFfCGljY6aw3Hrt
kHmPRiazukxPLb6ilpRAewjW8nihRANCAATDskChT+Altkm9X7MI69T3IUmrQU0L
950IxEzvw/x5BMEINRMrXLBJhqzO9Bm+d6JbqA21YQmd1Kt4RzLJR1W+
-----END PRIVATE KEY-----"#;

#[tokio::test]
async fn verifier_enforces_standard_access_token_claims() {
    let encoding_key =
        EncodingKey::from_ec_pem(PRIVATE_KEY.as_bytes()).expect("测试私钥应当可以解析");
    let mut jwk =
        Jwk::from_encoding_key(&encoding_key, Algorithm::ES256).expect("测试公钥应当可以导出");
    jwk.common.key_id = Some(KEY_ID.to_owned());
    jwk.common.public_key_use = Some(PublicKeyUse::Signature);
    let (issuer, server) = spawn_provider(jwk);
    let verifier = OidcAccessTokenVerifier::discover(&issuer, AUDIENCE)
        .await
        .expect("OIDC discovery 和 JWKS 应当加载成功");

    let identity = verifier
        .verify(&signed_token(
            &encoding_key,
            &issuer,
            AUDIENCE,
            now() + 3_600,
            Some(now() - 60),
        ))
        .await
        .expect("签名和标准声明有效的 token 应当通过");
    assert_eq!(identity.issuer, format!("{issuer}/"));
    assert_eq!(identity.subject, "user-1");
    assert_eq!(identity.display_name, "Ada");

    let wrong_audience = verifier
        .verify(&signed_token(
            &encoding_key,
            &issuer,
            "another-api",
            now() + 3_600,
            None,
        ))
        .await
        .expect_err("错误 audience 应当被拒绝");
    assert!(matches!(wrong_audience, VerificationError::InvalidToken));

    let expired = verifier
        .verify(&signed_token(
            &encoding_key,
            &issuer,
            AUDIENCE,
            now() - 120,
            None,
        ))
        .await
        .expect_err("过期 token 应当被拒绝");
    assert!(matches!(expired, VerificationError::InvalidToken));

    let not_active_yet = verifier
        .verify(&signed_token(
            &encoding_key,
            &issuer,
            AUDIENCE,
            now() + 3_600,
            Some(now() + 600),
        ))
        .await
        .expect_err("尚未到 nbf 的 token 应当被拒绝");
    assert!(matches!(not_active_yet, VerificationError::InvalidToken));
    server.join().expect("测试 Provider 应当正常退出");
}

#[tokio::test]
async fn discovery_rejects_non_loopback_http_issuer_before_request() {
    let error = match OidcAccessTokenVerifier::discover("http://id.example.com", AUDIENCE).await {
        Ok(_) => panic!("非 loopback HTTP issuer 必须在发起网络请求前被拒绝"),
        Err(error) => error,
    };

    assert!(matches!(error, VerificationError::InvalidConfiguration(_)));
}

#[tokio::test]
async fn discovery_rejects_insecure_jwks_uri() {
    let (issuer, server) = spawn_discovery("http://id.example.com/jwks");
    let error = match OidcAccessTokenVerifier::discover(&issuer, AUDIENCE).await {
        Ok(_) => panic!("非 loopback HTTP jwks_uri 必须在加载密钥前被拒绝"),
        Err(error) => error,
    };

    assert!(matches!(error, VerificationError::InvalidMetadata(_)));
    server.join().expect("测试 Provider 应当正常退出");
}

#[tokio::test]
async fn unknown_key_ids_share_and_throttle_jwks_refresh() {
    let encoding_key =
        EncodingKey::from_ec_pem(PRIVATE_KEY.as_bytes()).expect("测试私钥应当可以解析");
    let mut jwk =
        Jwk::from_encoding_key(&encoding_key, Algorithm::ES256).expect("测试公钥应当可以导出");
    jwk.common.key_id = Some(KEY_ID.to_owned());
    jwk.common.public_key_use = Some(PublicKeyUse::Signature);
    let provider = CountingProvider::spawn(jwk);
    let verifier = OidcAccessTokenVerifier::discover(&provider.issuer, AUDIENCE)
        .await
        .expect("OIDC discovery 和初始 JWKS 应当加载成功");
    let first_token = signed_token_with_key_id(
        &encoding_key,
        &provider.issuer,
        AUDIENCE,
        now() + 3_600,
        None,
        "unknown-key-1",
    );
    let second_token = signed_token_with_key_id(
        &encoding_key,
        &provider.issuer,
        AUDIENCE,
        now() + 3_600,
        None,
        "unknown-key-2",
    );

    let (first, second) = tokio::join!(
        verifier.verify(&first_token),
        verifier.verify(&second_token)
    );
    assert!(matches!(first, Err(VerificationError::InvalidToken)));
    assert!(matches!(second, Err(VerificationError::InvalidToken)));

    let third = verifier
        .verify(&second_token)
        .await
        .expect_err("刷新间隔内的未知 key 必须直接拒绝");
    assert!(matches!(third, VerificationError::InvalidToken));
    assert_eq!(provider.jwks_requests(), 2, "初始加载后只允许一次刷新");
}

fn signed_token(
    key: &EncodingKey,
    issuer: &str,
    audience: &str,
    expires_at: u64,
    not_before: Option<u64>,
) -> String {
    signed_token_with_key_id(key, issuer, audience, expires_at, not_before, KEY_ID)
}

fn signed_token_with_key_id(
    key: &EncodingKey,
    issuer: &str,
    audience: &str,
    expires_at: u64,
    not_before: Option<u64>,
    key_id: &str,
) -> String {
    let mut claims = json!({
        "iss": issuer,
        "sub": "user-1",
        "aud": audience,
        "exp": expires_at,
        "iat": now(),
        "name": "Ada",
        "email": "ada@example.com"
    });
    if let Some(not_before) = not_before {
        claims["nbf"] = json!(not_before);
    }
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_owned());
    encode(&header, &claims, key).expect("测试 token 应当可以签发")
}

struct CountingProvider {
    issuer: String,
    jwks_requests: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl CountingProvider {
    fn spawn(jwk: Jwk) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以绑定测试 Provider");
        listener
            .set_nonblocking(true)
            .expect("测试 Provider 应当支持非阻塞监听");
        let issuer = format!(
            "http://{}",
            listener.local_addr().expect("应当可以读取监听地址")
        );
        let server_issuer = issuer.clone();
        let jwks_requests = Arc::new(AtomicUsize::new(0));
        let server_jwks_requests = Arc::clone(&jwks_requests);
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let server = thread::spawn(move || {
            while !server_shutdown.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("测试 Provider 接收请求失败: {error}"),
                };
                stream
                    .set_nonblocking(false)
                    .expect("已接受的测试连接应切换为阻塞读取");
                let path = read_path(&mut stream);
                let body = match path.as_str() {
                    "/.well-known/openid-configuration" => json!({
                        "issuer": server_issuer.as_str(),
                        "jwks_uri": format!("{server_issuer}/jwks"),
                        "id_token_signing_alg_values_supported": ["ES256"]
                    })
                    .to_string(),
                    "/jwks" => {
                        server_jwks_requests.fetch_add(1, Ordering::AcqRel);
                        json!({ "keys": [jwk.clone()] }).to_string()
                    }
                    _ => panic!("未预期的 Provider 路径: {path}"),
                };
                write_response(&mut stream, body.as_str());
            }
        });
        Self {
            issuer,
            jwks_requests,
            shutdown,
            server: Some(server),
        }
    }

    fn jwks_requests(&self) -> usize {
        self.jwks_requests.load(Ordering::Acquire)
    }
}

impl Drop for CountingProvider {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.server
            .take()
            .expect("测试 Provider 线程句柄应存在")
            .join()
            .expect("测试 Provider 应当正常退出");
    }
}

fn spawn_provider(jwk: Jwk) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以绑定测试 Provider");
    let issuer = format!(
        "http://{}",
        listener.local_addr().expect("应当可以读取监听地址")
    );
    let server_issuer = issuer.clone();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("应当接收 Provider 请求");
            let path = read_path(&mut stream);
            let body = match path.as_str() {
                "/.well-known/openid-configuration" => json!({
                    "issuer": server_issuer.as_str(),
                    "jwks_uri": format!("{server_issuer}/jwks"),
                    "id_token_signing_alg_values_supported": ["ES256"]
                })
                .to_string(),
                "/jwks" => json!({ "keys": [jwk.clone()] }).to_string(),
                _ => panic!("未预期的 Provider 路径: {path}"),
            };
            write_response(&mut stream, body.as_str());
        }
    });
    (issuer, server)
}

fn spawn_discovery(jwks_uri: &'static str) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("应当可以绑定测试 Provider");
    let issuer = format!(
        "http://{}",
        listener.local_addr().expect("应当可以读取监听地址")
    );
    let server_issuer = issuer.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("应当接收 discovery 请求");
        let path = read_path(&mut stream);
        assert_eq!(path, "/.well-known/openid-configuration");
        let body = json!({
            "issuer": server_issuer,
            "jwks_uri": jwks_uri,
            "id_token_signing_alg_values_supported": ["ES256"]
        })
        .to_string();
        write_response(&mut stream, body.as_str());
    });
    (issuer, server)
}

fn read_path(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).expect("应当读取请求行");
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("应当读取请求头");
        if header == "\r\n" || header.is_empty() {
            break;
        }
    }
    request_line
        .split_whitespace()
        .nth(1)
        .expect("请求行应当包含路径")
        .to_owned()
}

fn write_response(stream: &mut TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("应当写入 Provider 响应");
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间应当晚于 Unix epoch")
        .as_secs()
}
