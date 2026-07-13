//! 桌面应用可复用的 OIDC Authorization Code + PKCE 客户端。
//!
//! 该 crate 面向已部署的 ZITADEL 或其他标准 OIDC Provider，提供桌面端常用的
//! loopback redirect 登录流程。调用方负责打开系统浏览器、持久化 token，并把会话接入自己的 UI 状态。

use std::{
    collections::HashMap,
    fmt,
    io::{BufRead as _, BufReader, Write as _},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{
    Algorithm, AlgorithmFamily, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, Jwk, JwkSet, KeyOperations, PublicKeyUse},
};
use rand::{TryRngCore as _, rngs::OsRng};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use url::Url;

const DEFAULT_SCOPE: &str = "openid profile email offline_access";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// OIDC 桌面端认证配置。
///
/// 该配置只包含 public/native client 所需的非敏感信息。桌面端应配合 PKCE 使用，
/// 不应把 client secret 放入应用包或本地配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcConfig {
    issuer_url: Url,
    client_id: String,
    scopes: Vec<String>,
    redirect_uri: Url,
}

impl OidcConfig {
    /// 创建 OIDC 配置并校验 issuer、client id 与本地 loopback redirect URI。
    ///
    /// `issuer_url` 是 Provider 的 issuer，例如 ZITADEL 的 `https://id.example.com`。
    /// `redirect_uri` 必须是形如 `http://127.0.0.1:0/auth/callback` 的 URI 模板；端口为
    /// `0` 时会由系统分配临时端口，并在发起授权请求前替换为实际端口。
    ///
    /// # Errors
    ///
    /// issuer URL 或 redirect URI 无效、client id 为空，或 redirect URI 不是 IPv4 loopback HTTP
    /// 地址时返回错误。
    pub fn new(
        issuer_url: impl AsRef<str>,
        client_id: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
        redirect_uri: impl AsRef<str>,
    ) -> Result<Self, OidcError> {
        let issuer_url = parse_issuer_url(issuer_url.as_ref())?;
        let client_id = client_id.into();
        if client_id.trim().is_empty() {
            return Err(OidcError::MissingClientId);
        }

        Ok(Self {
            issuer_url,
            client_id,
            scopes: normalize_scopes(scopes),
            redirect_uri: parse_loopback_redirect_uri(redirect_uri.as_ref())?,
        })
    }

    /// 使用 `openid profile email offline_access` 默认 scope 创建配置。
    ///
    /// # Errors
    ///
    /// 参数校验失败时返回 [`OidcError`]。
    pub fn with_default_scopes(
        issuer_url: impl AsRef<str>,
        client_id: impl Into<String>,
        redirect_uri: impl AsRef<str>,
    ) -> Result<Self, OidcError> {
        Self::new(
            issuer_url,
            client_id,
            DEFAULT_SCOPE.split_whitespace(),
            redirect_uri,
        )
    }

    /// 返回 Provider issuer URL。
    pub fn issuer_url(&self) -> &Url {
        &self.issuer_url
    }

    /// 返回 OAuth/OIDC public client id。
    pub fn client_id(&self) -> &str {
        self.client_id.as_str()
    }

    /// 返回授权请求使用的 scope 列表。
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// 返回本地 loopback 回调 URI 模板。
    ///
    /// URI 中的端口可以为 `0`，表示登录时由操作系统选择空闲端口。
    pub fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    /// 返回本地 loopback 回调端口；`0` 表示由系统自动分配。
    pub fn redirect_port(&self) -> u16 {
        self.redirect_uri.port().unwrap_or_default()
    }

    /// 返回回调路径，例如 `/auth/callback`。
    pub fn callback_path(&self) -> &str {
        self.redirect_uri.path()
    }

    fn resolved_redirect_uri(&self, port: u16) -> String {
        let mut redirect_uri = self.redirect_uri.clone();
        redirect_uri
            .set_port(Some(port))
            .expect("validated loopback redirect URI must accept a port");
        redirect_uri.into()
    }
}

/// OIDC 登录客户端。
///
/// 调用方可以为每个桌面应用创建一个客户端实例，然后通过 [`OidcClient::begin_login`]
/// 获取授权 URL 和待完成登录流程。
#[derive(Debug, Clone)]
pub struct OidcClient {
    config: OidcConfig,
    http: Client,
}

impl OidcClient {
    /// 创建使用默认 HTTP 超时的客户端。
    ///
    /// # Errors
    ///
    /// HTTP client 构造失败时返回 [`OidcError`]。
    pub fn new(config: OidcConfig) -> Result<Self, OidcError> {
        Ok(Self {
            config,
            http: http_client()?,
        })
    }

    /// 返回该客户端使用的 OIDC 配置。
    pub fn config(&self) -> &OidcConfig {
        &self.config
    }

    /// 启动一次 Authorization Code + PKCE 登录。
    ///
    /// 返回值包含需要打开的授权 URL，以及在后台等待回调和换取 token 的上下文。
    ///
    /// # Errors
    ///
    /// discovery 请求失败、回调端口绑定失败或 PKCE 随机值生成失败时返回错误。
    pub fn begin_login(&self) -> Result<PendingOidcLogin, OidcError> {
        let metadata = discover(&self.http, &self.config)?;
        let listener = TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            self.config.redirect_port(),
        ))?;
        let verifier = random_url_token(32)?;
        let state = random_url_token(24)?;
        let nonce = random_url_token(24)?;
        let port = listener.local_addr()?.port();
        let redirect_uri = self.config.resolved_redirect_uri(port);
        let authorization_url = authorization_url(
            &metadata,
            &self.config,
            &state,
            &nonce,
            &verifier,
            &redirect_uri,
        );

        Ok(PendingOidcLogin {
            authorization_url,
            listener,
            metadata,
            config: self.config.clone(),
            state,
            nonce,
            verifier,
            http: self.http.clone(),
        })
    }

    /// 使用缓存中的 refresh token 刷新认证会话。
    ///
    /// Provider 轮换 refresh token 时会保存新值；响应未返回新 refresh token 时会继续保留旧值。
    /// 若刷新响应携带新的 ID Token，该 Token 仍会执行签名、issuer、audience 与过期时间校验。
    ///
    /// # Errors
    ///
    /// 缓存没有 refresh token、discovery 或 token 请求失败、刷新响应无 access token、
    /// 新 ID Token 校验失败，或无法恢复用户资料时返回 [`OidcError`]。
    pub fn refresh(&self, tokens: &OidcTokenCache) -> Result<OidcSession, OidcError> {
        let refresh_token = tokens
            .refresh_token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(OidcError::MissingRefreshToken)?;
        let metadata = discover(&self.http, &self.config)?;
        let (mut refreshed_tokens, refreshed_id_token) =
            refresh_tokens(&self.http, &metadata, &self.config, refresh_token, tokens)?;
        let refreshed_profile = refreshed_id_token
            .as_deref()
            .map(|id_token| validate_id_token(&self.http, &metadata, &self.config, id_token, None))
            .transpose()?;

        if let (Some(previous), Some(refreshed)) = (&tokens.profile, &refreshed_profile)
            && previous.subject != refreshed.subject
        {
            return Err(OidcError::SubjectMismatch);
        }

        let profile = if let Some(profile) = tokens.profile.clone().or(refreshed_profile) {
            profile
        } else if let Some(endpoint) = &metadata.userinfo_endpoint {
            load_userinfo(&self.http, endpoint, &refreshed_tokens)?
        } else {
            return Err(OidcError::MissingProfile);
        };
        refreshed_tokens.profile = Some(profile.clone());

        Ok(OidcSession {
            profile,
            tokens: refreshed_tokens,
        })
    }
}

/// 登录流程启动后等待浏览器回调的上下文。
///
/// 该类型持有本地 TCP listener。调用方应先打开 [`PendingOidcLogin::authorization_url`]，
/// 然后在后台线程或后台任务中调用 [`PendingOidcLogin::finish`]。
pub struct PendingOidcLogin {
    authorization_url: String,
    listener: TcpListener,
    metadata: OidcMetadata,
    config: OidcConfig,
    state: String,
    nonce: String,
    verifier: String,
    http: Client,
}

impl PendingOidcLogin {
    /// 返回需要在系统浏览器中打开的授权 URL。
    pub fn authorization_url(&self) -> &str {
        self.authorization_url.as_str()
    }

    /// 等待浏览器回调并完成 token 交换和用户资料加载。
    ///
    /// # Errors
    ///
    /// 用户取消、回调 state 不匹配、网络请求失败或 Provider 返回错误时返回 [`OidcError`]。
    pub fn finish(self) -> Result<OidcSession, OidcError> {
        let code = wait_for_authorization_code(
            &self.listener,
            self.config.callback_path(),
            self.state.as_str(),
        )?;
        let mut tokens = exchange_code(
            &self.http,
            &self.metadata,
            &self.config,
            &self.verifier,
            code.as_str(),
            self.listener.local_addr()?.port(),
        )?;
        let id_token = tokens
            .id_token
            .as_deref()
            .ok_or(OidcError::MissingIdToken)?;
        let id_token_profile = validate_id_token(
            &self.http,
            &self.metadata,
            &self.config,
            id_token,
            Some(self.nonce.as_str()),
        )?;
        let profile = load_profile(&self.http, &self.metadata, &tokens, id_token_profile)?;
        tokens.profile = Some(profile.clone());

        Ok(OidcSession { profile, tokens })
    }
}

/// 已认证 OIDC 会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcSession {
    profile: OidcUserProfile,
    tokens: OidcTokenCache,
}

impl OidcSession {
    /// 返回当前登录用户资料。
    pub fn profile(&self) -> &OidcUserProfile {
        &self.profile
    }

    /// 返回当前会话 token 缓存。
    pub fn tokens(&self) -> &OidcTokenCache {
        &self.tokens
    }

    /// 拆分会话，便于调用方分别保存 token 和展示用户资料。
    pub fn into_parts(self) -> (OidcUserProfile, OidcTokenCache) {
        (self.profile, self.tokens)
    }

    /// 从已持久化的 token 缓存恢复会话展示态。
    ///
    /// 该函数只恢复本地展示态，不会联网验证 token。调用方可以在后续 API 请求失败或过期时再刷新。
    pub fn from_token_cache(tokens: OidcTokenCache) -> Option<Self> {
        let profile = tokens.profile.clone()?;
        tokens
            .has_access_token()
            .then_some(Self { profile, tokens })
    }
}

/// 当前登录用户的 OIDC 资料。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OidcUserProfile {
    /// OIDC subject，稳定表示 Provider 中的用户身份。
    pub subject: String,
    /// 用户展示名。
    pub name: Option<String>,
    /// 用户邮箱。
    pub email: Option<String>,
    /// 用户首选用户名。
    pub preferred_username: Option<String>,
    /// 用户头像地址。
    pub picture: Option<String>,
}

impl Default for OidcUserProfile {
    /// 创建空用户资料，主要用于容错反序列化。
    fn default() -> Self {
        Self {
            subject: String::new(),
            name: None,
            email: None,
            preferred_username: None,
            picture: None,
        }
    }
}

impl OidcUserProfile {
    /// 返回适合桌面 UI 展示的用户名称。
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.preferred_username.as_deref())
            .or(self.email.as_deref())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("已登录用户")
    }
}

/// 可持久化的 OIDC token 缓存。
///
/// 该结构可以序列化到调用方自己的配置文件、数据库或系统 keychain 中。它不提供加密存储；
/// 是否需要加密由宿主应用决定。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OidcTokenCache {
    /// OAuth access token，用于访问当前用户授权的受保护资源。
    pub access_token: String,
    /// OAuth refresh token；只有授权范围和 Provider 配置允许时才会返回。
    pub refresh_token: Option<String>,
    /// OIDC ID token，用于表示身份提供方对当前认证事件的声明。
    pub id_token: Option<String>,
    /// token 类型，通常是 `Bearer`。
    pub token_type: Option<String>,
    /// Provider 实际授予的 scope。
    pub scope: Option<String>,
    /// access token 过期时间的 Unix 秒级时间戳。
    pub expires_at: Option<u64>,
    /// token 对应的用户资料，用于下次启动时恢复展示态。
    pub profile: Option<OidcUserProfile>,
}

impl OidcTokenCache {
    /// 判断缓存中是否包含可以用于认证请求的 access token。
    pub fn has_access_token(&self) -> bool {
        !self.access_token.trim().is_empty()
    }

    /// 判断 access token 是否已经接近过期。
    ///
    /// 为避免边界抖动，该方法会在过期前 60 秒就返回 `true`。
    pub fn is_expired(&self) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        now_unix_seconds().saturating_add(60) >= expires_at
    }
}

/// OIDC 登录过程中的错误。
#[derive(Debug, Error)]
pub enum OidcError {
    /// 未配置 OAuth/OIDC client id。
    #[error("未配置 OIDC client id")]
    MissingClientId,
    /// issuer URL 格式不正确。
    #[error("OIDC issuer URL 无效: {0}")]
    InvalidIssuer(
        /// 解析 issuer URL 时返回的底层错误。
        #[from]
        url::ParseError,
    ),
    /// issuer 必须使用 HTTP 或 HTTPS。
    #[error("OIDC issuer URL 必须使用 http 或 https")]
    UnsupportedIssuerScheme,
    /// 本地 loopback redirect URI 格式不正确。
    #[error("OIDC redirect URI 无效: {0}")]
    InvalidRedirectUri(
        /// 解析 redirect URI 时返回的底层错误。
        #[source]
        url::ParseError,
    ),
    /// redirect URI 不是当前客户端支持的 IPv4 loopback HTTP 地址。
    #[error("OIDC redirect URI 必须使用 http://127.0.0.1:<port>/...")]
    UnsupportedRedirectUri,
    /// redirect URI 未显式指定端口。
    #[error("OIDC redirect URI 必须显式指定端口")]
    MissingRedirectPort,
    /// redirect URI 包含当前 loopback 回调处理器不支持的 query 或 fragment。
    #[error("OIDC redirect URI 不能包含 query 或 fragment")]
    RedirectUriContainsQueryOrFragment,
    /// 本地回调服务器 I/O 失败。
    #[error(transparent)]
    Io(
        /// 绑定、接收或响应 loopback 回调时的底层输入输出错误。
        #[from]
        std::io::Error,
    ),
    /// 网络请求失败。
    #[error(transparent)]
    Http(
        /// 访问 discovery、token、JWKS 或 UserInfo 端点时的底层 HTTP 错误。
        #[from]
        reqwest::Error,
    ),
    /// 随机数生成失败。
    #[error("无法生成 OIDC PKCE 随机值")]
    Random,
    /// Provider discovery 缺少必要端点。
    #[error("OIDC discovery 缺少必要端点: {0}")]
    MissingMetadata(
        /// discovery 文档中缺失的必要字段名。
        &'static str,
    ),
    /// Provider discovery 声明的 issuer 与客户端配置不一致。
    #[error("OIDC discovery issuer 与配置不一致: 期望 {expected}，实际 {actual}")]
    DiscoveryIssuerMismatch {
        /// 客户端配置的 issuer。
        expected: String,
        /// discovery 文档返回的 issuer。
        actual: String,
    },
    /// 授权回调没有返回 code。
    #[error("OIDC 登录回调缺少授权码")]
    MissingCode,
    /// 授权回调 state 与本地请求不匹配。
    #[error("OIDC 登录回调 state 校验失败")]
    InvalidState,
    /// 用户或身份提供方返回 OAuth 错误。
    #[error("OIDC 登录失败: {0}")]
    OAuth(
        /// Provider 返回的 OAuth 错误码与可选错误描述。
        String,
    ),
    /// token endpoint 以非成功状态码返回了结构化 OAuth 错误。
    #[error("OIDC token endpoint 返回 OAuth 错误: {message}")]
    TokenEndpointOAuth {
        /// Provider 返回的标准 OAuth 错误码，例如 `invalid_grant`。
        error: String,
        /// 组合错误码与可选描述后的用户可读消息。
        message: String,
    },
    /// token 响应缺少 access token。
    #[error("OIDC token 响应缺少 access_token")]
    MissingAccessToken,
    /// 登录 token 响应缺少 ID Token。
    #[error("OIDC token 响应缺少 id_token")]
    MissingIdToken,
    /// 本地 token 缓存中没有可用的 refresh token。
    #[error("OIDC token 缓存缺少 refresh_token")]
    MissingRefreshToken,
    /// ID Token 的签名算法不适用于 discovery JWKS 中的公钥。
    #[error("OIDC ID token 使用了不支持的签名算法")]
    UnsupportedIdTokenAlgorithm,
    /// discovery JWKS 中找不到唯一匹配的验签公钥。
    #[error("OIDC JWKS 中找不到唯一匹配的 ID token 签名公钥")]
    MissingSigningKey,
    /// ID Token 的 JWT 签名或标准 claims 校验失败。
    #[error("OIDC ID token 校验失败: {0}")]
    InvalidIdToken(
        /// JWT 解析、验签或标准 claims 校验时返回的底层错误。
        #[from]
        jsonwebtoken::errors::Error,
    ),
    /// ID Token 的 nonce 缺失或与授权请求不一致。
    #[error("OIDC ID token nonce 校验失败")]
    InvalidNonce,
    /// ID Token 中的 authorized party 与当前客户端不一致。
    #[error("OIDC ID token azp 校验失败")]
    InvalidAuthorizedParty,
    /// UserInfo、刷新前后的 ID Token 或本地资料表示了不同用户。
    #[error("OIDC 认证信息的 subject 不一致")]
    SubjectMismatch,
    /// 刷新成功后无法从缓存、ID Token 或 UserInfo 恢复用户资料。
    #[error("OIDC 会话缺少用户资料")]
    MissingProfile,
    /// token 或用户资料不是有效 JSON。
    #[error(transparent)]
    Json(
        /// 序列化或反序列化 token 相关 JSON 时的底层错误。
        #[from]
        serde_json::Error,
    ),
}

impl OidcError {
    /// 判断 token endpoint 是否以 `invalid_grant` 拒绝了当前凭据。
    ///
    /// 调用方应在 refresh token 授权失败后使用此方法；返回 `true` 表示
    /// refresh token 已失效、被撤销或已经轮换，应清除本地凭据并重新登录。
    /// 网络错误、Provider 临时失败以及其他 OAuth 错误均返回 `false`。
    pub fn is_refresh_token_rejected(&self) -> bool {
        matches!(
            self,
            Self::TokenEndpointOAuth { error, .. } if error == "invalid_grant"
        )
    }
}

fn parse_issuer_url(value: &str) -> Result<Url, OidcError> {
    let mut url = Url::parse(value.trim())?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(OidcError::UnsupportedIssuerScheme),
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn parse_loopback_redirect_uri(value: &str) -> Result<Url, OidcError> {
    let url = Url::parse(value.trim()).map_err(OidcError::InvalidRedirectUri)?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
        return Err(OidcError::UnsupportedRedirectUri);
    }
    if url.port().is_none() {
        return Err(OidcError::MissingRedirectPort);
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(OidcError::RedirectUriContainsQueryOrFragment);
    }

    Ok(url)
}

fn normalize_scopes(scopes: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    let mut values = scopes
        .into_iter()
        .flat_map(|scope| {
            scope
                .into()
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|scope| !scope.trim().is_empty())
        .collect::<Vec<_>>();
    if !values.iter().any(|scope| scope == "openid") {
        values.insert(0, "openid".to_owned());
    }
    values
}

fn http_client() -> Result<Client, OidcError> {
    Ok(Client::builder().timeout(Duration::from_secs(30)).build()?)
}

#[derive(Debug, Deserialize)]
struct DiscoveryDocument {
    issuer: Option<String>,
    authorization_endpoint: Option<Url>,
    token_endpoint: Option<Url>,
    userinfo_endpoint: Option<Url>,
    jwks_uri: Option<Url>,
    id_token_signing_alg_values_supported: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct OidcMetadata {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    userinfo_endpoint: Option<Url>,
    jwks_uri: Url,
    id_token_signing_alg_values_supported: Vec<String>,
}

fn discover(client: &Client, config: &OidcConfig) -> Result<OidcMetadata, OidcError> {
    let mut discovery_url = config.issuer_url().clone();
    let issuer_path = discovery_url.path().trim_end_matches('/');
    discovery_url.set_path(&format!("{issuer_path}/.well-known/openid-configuration"));
    let document = client
        .get(discovery_url)
        .send()?
        .error_for_status()?
        .json::<DiscoveryDocument>()?;
    let issuer = document
        .issuer
        .filter(|value| !value.trim().is_empty())
        .ok_or(OidcError::MissingMetadata("issuer"))?;
    let discovered_issuer = parse_issuer_url(issuer.as_str())?;
    if &discovered_issuer != config.issuer_url() {
        return Err(OidcError::DiscoveryIssuerMismatch {
            expected: config.issuer_url().to_string(),
            actual: issuer,
        });
    }
    let signing_algorithms = document
        .id_token_signing_alg_values_supported
        .filter(|values| !values.is_empty())
        .ok_or(OidcError::MissingMetadata(
            "id_token_signing_alg_values_supported",
        ))?;

    Ok(OidcMetadata {
        issuer,
        authorization_endpoint: document
            .authorization_endpoint
            .ok_or(OidcError::MissingMetadata("authorization_endpoint"))?,
        token_endpoint: document
            .token_endpoint
            .ok_or(OidcError::MissingMetadata("token_endpoint"))?,
        userinfo_endpoint: document.userinfo_endpoint,
        jwks_uri: document
            .jwks_uri
            .ok_or(OidcError::MissingMetadata("jwks_uri"))?,
        id_token_signing_alg_values_supported: signing_algorithms,
    })
}

fn authorization_url(
    metadata: &OidcMetadata,
    config: &OidcConfig,
    state: &str,
    nonce: &str,
    verifier: &str,
    redirect_uri: &str,
) -> String {
    let challenge = pkce_challenge(verifier);
    let mut url = metadata.authorization_endpoint.clone();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", config.client_id())
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", config.scopes().join(" ").as_str())
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("code_challenge", challenge.as_str())
        .append_pair("code_challenge_method", "S256");
    url.into()
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_url_token(length: usize) -> Result<String, OidcError> {
    let mut bytes = vec![0_u8; length];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| OidcError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn wait_for_authorization_code(
    listener: &TcpListener,
    callback_path: &str,
    expected_state: &str,
) -> Result<String, OidcError> {
    let (stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(CALLBACK_TIMEOUT))?;
    handle_callback_stream(stream, callback_path, expected_state)
}

fn handle_callback_stream(
    mut stream: TcpStream,
    callback_path: &str,
    expected_state: &str,
) -> Result<String, OidcError> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let target =
        request_target(request_line.trim(), callback_path).ok_or(OidcError::MissingCode)?;
    let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
    let params = url
        .query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<HashMap<_, _>>();

    let result = callback_result(params, expected_state);
    let body = match &result {
        Ok(_) => "登录已完成，可以回到桌面应用。",
        Err(_) => "登录未完成，请回到桌面应用查看错误。",
    };
    write_callback_response(&mut stream, body)?;
    result
}

fn request_target<'a>(request_line: &'a str, callback_path: &str) -> Option<&'a str> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    (method == "GET" && target.starts_with(callback_path)).then_some(target)
}

fn callback_result(
    params: HashMap<String, String>,
    expected_state: &str,
) -> Result<String, OidcError> {
    if let Some(error) = params.get("error") {
        return Err(OidcError::OAuth(
            params
                .get("error_description")
                .map(|description| format!("{error}: {description}"))
                .unwrap_or_else(|| error.to_owned()),
        ));
    }
    if params.get("state").map(String::as_str) != Some(expected_state) {
        return Err(OidcError::InvalidState);
    }
    params.get("code").cloned().ok_or(OidcError::MissingCode)
}

fn write_callback_response(stream: &mut TcpStream, body: &str) -> Result<(), std::io::Error> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>OIDC Login</title></head><body>{body}</body></html>"
    );
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
    error_description: Option<String>,
}

fn exchange_code(
    client: &Client,
    metadata: &OidcMetadata,
    config: &OidcConfig,
    verifier: &str,
    code: &str,
    port: u16,
) -> Result<OidcTokenCache, OidcError> {
    let redirect_uri = config.resolved_redirect_uri(port);
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", config.client_id()),
        ("code", code),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_verifier", verifier),
    ];
    let response = client
        .post(metadata.token_endpoint.clone())
        .form(&form)
        .send()?;
    let response = parse_token_response(response)?;
    token_cache_from_response(response, None)
}

fn refresh_tokens(
    client: &Client,
    metadata: &OidcMetadata,
    config: &OidcConfig,
    refresh_token: &str,
    previous: &OidcTokenCache,
) -> Result<(OidcTokenCache, Option<String>), OidcError> {
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", config.client_id()),
        ("refresh_token", refresh_token),
    ];
    let response = client
        .post(metadata.token_endpoint.clone())
        .form(&form)
        .send()?;
    let response = parse_token_response(response)?;
    let refreshed_id_token = response.id_token.clone();
    let tokens = token_cache_from_response(response, Some(previous))?;
    Ok((tokens, refreshed_id_token))
}

fn parse_token_response(response: reqwest::blocking::Response) -> Result<TokenResponse, OidcError> {
    let Some(status_error) = response.error_for_status_ref().err() else {
        return Ok(response.json::<TokenResponse>()?);
    };
    let Ok(error_response) = response.json::<OAuthErrorResponse>() else {
        return Err(OidcError::Http(status_error));
    };
    let error = error_response.error.trim().to_owned();
    if error.is_empty() {
        return Err(OidcError::Http(status_error));
    }
    let message = error_response
        .error_description
        .filter(|description| !description.trim().is_empty())
        .map(|description| format!("{error}: {description}"))
        .unwrap_or_else(|| error.clone());
    Err(OidcError::TokenEndpointOAuth { error, message })
}

fn token_cache_from_response(
    response: TokenResponse,
    previous: Option<&OidcTokenCache>,
) -> Result<OidcTokenCache, OidcError> {
    let access_token = response
        .access_token
        .filter(|value| !value.trim().is_empty())
        .ok_or(OidcError::MissingAccessToken)?;
    let expires_at = response
        .expires_in
        .map(|expires_in| now_unix_seconds().saturating_add(expires_in));

    Ok(OidcTokenCache {
        access_token,
        refresh_token: response
            .refresh_token
            .or_else(|| previous.and_then(|tokens| tokens.refresh_token.clone())),
        id_token: response
            .id_token
            .or_else(|| previous.and_then(|tokens| tokens.id_token.clone())),
        token_type: response
            .token_type
            .or_else(|| previous.and_then(|tokens| tokens.token_type.clone())),
        scope: response
            .scope
            .or_else(|| previous.and_then(|tokens| tokens.scope.clone())),
        expires_at,
        profile: previous.and_then(|tokens| tokens.profile.clone()),
    })
}

fn load_profile(
    client: &Client,
    metadata: &OidcMetadata,
    tokens: &OidcTokenCache,
    id_token_profile: OidcUserProfile,
) -> Result<OidcUserProfile, OidcError> {
    if let Some(userinfo_endpoint) = &metadata.userinfo_endpoint {
        let profile = load_userinfo(client, userinfo_endpoint, tokens)?;
        if profile.subject != id_token_profile.subject {
            return Err(OidcError::SubjectMismatch);
        }
        return Ok(profile);
    }

    Ok(id_token_profile)
}

fn load_userinfo(
    client: &Client,
    userinfo_endpoint: &Url,
    tokens: &OidcTokenCache,
) -> Result<OidcUserProfile, OidcError> {
    let profile = client
        .get(userinfo_endpoint.clone())
        .bearer_auth(tokens.access_token.as_str())
        .send()?
        .error_for_status()?
        .json::<UserInfoResponse>()?;
    Ok(profile.into())
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    #[serde(rename = "sub")]
    subject: String,
    name: Option<String>,
    email: Option<String>,
    preferred_username: Option<String>,
    picture: Option<String>,
}

impl From<UserInfoResponse> for OidcUserProfile {
    fn from(value: UserInfoResponse) -> Self {
        Self {
            subject: value.subject,
            name: value.name,
            email: value.email,
            preferred_username: value.preferred_username,
            picture: value.picture,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IdTokenAudience {
    One(String),
    Many(Vec<String>),
}

impl IdTokenAudience {
    fn len(&self) -> usize {
        match self {
            Self::One(value) => usize::from(!value.is_empty()),
            Self::Many(values) => values.len(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    #[serde(rename = "sub")]
    subject: String,
    #[serde(rename = "aud")]
    audience: IdTokenAudience,
    #[serde(rename = "azp")]
    authorized_party: Option<String>,
    nonce: Option<String>,
    name: Option<String>,
    email: Option<String>,
    preferred_username: Option<String>,
    picture: Option<String>,
}

impl From<IdTokenClaims> for OidcUserProfile {
    fn from(value: IdTokenClaims) -> Self {
        Self {
            subject: value.subject,
            name: value.name,
            email: value.email,
            preferred_username: value.preferred_username,
            picture: value.picture,
        }
    }
}

fn validate_id_token(
    client: &Client,
    metadata: &OidcMetadata,
    config: &OidcConfig,
    token: &str,
    expected_nonce: Option<&str>,
) -> Result<OidcUserProfile, OidcError> {
    let header = decode_header(token)?;
    if header.alg.family() == AlgorithmFamily::Hmac {
        return Err(OidcError::UnsupportedIdTokenAlgorithm);
    }
    let algorithm_name = format!("{:?}", header.alg);
    if !metadata
        .id_token_signing_alg_values_supported
        .iter()
        .any(|algorithm| algorithm == &algorithm_name)
    {
        return Err(OidcError::UnsupportedIdTokenAlgorithm);
    }

    let jwks = client
        .get(metadata.jwks_uri.clone())
        .send()?
        .error_for_status()?
        .json::<JwkSet>()?;
    let jwk = select_signing_key(&jwks, header.kid.as_deref(), header.alg)?;
    let decoding_key = DecodingKey::from_jwk(jwk)?;
    let mut validation = Validation::new(header.alg);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.set_issuer(&[metadata.issuer.as_str()]);
    validation.set_audience(&[config.client_id()]);
    let claims = decode::<IdTokenClaims>(token, &decoding_key, &validation)?.claims;

    if let Some(expected_nonce) = expected_nonce
        && claims.nonce.as_deref() != Some(expected_nonce)
    {
        return Err(OidcError::InvalidNonce);
    }
    if claims
        .authorized_party
        .as_deref()
        .is_some_and(|authorized_party| authorized_party != config.client_id())
        || (claims.audience.len() > 1
            && claims.authorized_party.as_deref() != Some(config.client_id()))
    {
        return Err(OidcError::InvalidAuthorizedParty);
    }

    Ok(claims.into())
}

fn select_signing_key<'a>(
    jwks: &'a JwkSet,
    key_id: Option<&str>,
    algorithm: Algorithm,
) -> Result<&'a Jwk, OidcError> {
    let mut candidates = jwks.keys.iter().filter(|jwk| {
        key_id.is_none_or(|key_id| jwk.common.key_id.as_deref() == Some(key_id))
            && jwk_supports_algorithm(jwk, algorithm)
    });
    let candidate = candidates.next().ok_or(OidcError::MissingSigningKey)?;
    if candidates.next().is_some() {
        return Err(OidcError::MissingSigningKey);
    }
    Ok(candidate)
}

fn jwk_supports_algorithm(jwk: &Jwk, algorithm: Algorithm) -> bool {
    let supports_signature = jwk
        .common
        .public_key_use
        .as_ref()
        .is_none_or(|key_use| key_use == &PublicKeyUse::Signature);
    let supports_verification = jwk
        .common
        .key_operations
        .as_ref()
        .is_none_or(|operations| operations.contains(&KeyOperations::Verify));
    let algorithm_matches = jwk
        .common
        .key_algorithm
        .is_none_or(|key_algorithm| key_algorithm.to_string() == format!("{algorithm:?}"));
    let family_matches = matches!(
        (&jwk.algorithm, algorithm.family()),
        (AlgorithmParameters::RSA(_), AlgorithmFamily::Rsa)
            | (AlgorithmParameters::EllipticCurve(_), AlgorithmFamily::Ec)
            | (AlgorithmParameters::OctetKeyPair(_), AlgorithmFamily::Ed)
    );
    supports_signature && supports_verification && algorithm_matches && family_matches
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl fmt::Debug for PendingOidcLogin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingOidcLogin")
            .field("authorization_url", &self.authorization_url)
            .field("metadata", &self.metadata)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}
