//! ZITADEL opaque/PAT 实时 introspection 与 JWT 自动分流验证器。

use std::{fmt, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use jsonwebtoken::decode_header;
use reqwest::redirect::Policy;
use serde::Deserialize;
use url::{Host, Url};

use super::{
    AccessTokenVerifier, OidcAccessTokenVerifier, VerificationError, VerifiedIdentity,
    VerifiedOrganizationContext,
};

/// 对 JWT 使用本地 JWKS 校验、对 opaque/PAT 使用实时 introspection 的统一验证器。
pub struct ZitadelAccessTokenVerifier {
    jwt: OidcAccessTokenVerifier,
    introspection: ZitadelIntrospectionVerifier,
}

impl ZitadelAccessTokenVerifier {
    /// 初始化 JWT discovery/JWKS 和 PAT introspection 验证能力。
    ///
    /// # Errors
    ///
    /// issuer、audience 或 introspection resource-server 凭据无效，或者 OIDC discovery/JWKS
    /// 初始化失败时返回错误。
    pub async fn discover(
        issuer: impl AsRef<str>,
        audience: impl Into<String>,
        introspection_client_id: impl Into<String>,
        introspection_client_secret: impl Into<String>,
    ) -> Result<Self, VerificationError> {
        let audience = audience.into();
        let jwt = OidcAccessTokenVerifier::discover(issuer.as_ref(), audience.clone()).await?;
        let introspection = ZitadelIntrospectionVerifier::new(
            issuer.as_ref(),
            audience,
            introspection_client_id,
            introspection_client_secret,
        )?;
        Ok(Self { jwt, introspection })
    }
}

#[async_trait]
impl AccessTokenVerifier for ZitadelAccessTokenVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, VerificationError> {
        if jwt_shaped(token) {
            self.jwt.verify(token).await
        } else {
            self.introspection.verify(token).await
        }
    }
}

/// 使用 HTTP Basic resource-server 凭据实时验证 ZITADEL PAT/opaque token。
pub struct ZitadelIntrospectionVerifier {
    http: reqwest::Client,
    issuer: Url,
    identity_issuer: String,
    audience: String,
    endpoint: Url,
    client_id: String,
    client_secret: String,
}

impl fmt::Debug for ZitadelIntrospectionVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZitadelIntrospectionVerifier")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("endpoint", &self.endpoint)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

impl ZitadelIntrospectionVerifier {
    /// 创建无缓存的 ZITADEL introspection 验证器。
    ///
    /// # Errors
    ///
    /// issuer/audience/client ID/secret 无效，或无法构造安全 HTTP 客户端时返回错误。
    pub fn new(
        issuer: impl AsRef<str>,
        audience: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Result<Self, VerificationError> {
        let issuer = normalized_issuer(issuer.as_ref())?;
        let identity_issuer = issuer.to_string();
        let audience = required_configuration(audience.into(), "audience")?;
        let client_id = required_configuration(client_id.into(), "introspection client ID")?;
        let client_secret =
            required_configuration(client_secret.into(), "introspection client secret")?;
        let mut endpoint = issuer.clone();
        let path = issuer.path().trim_end_matches('/');
        endpoint.set_path(&format!("{path}/oauth/v2/introspect"));
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(VerificationError::ProviderUnavailable)?;
        Ok(Self {
            http,
            issuer,
            identity_issuer,
            audience,
            endpoint,
            client_id,
            client_secret,
        })
    }
}

#[async_trait]
impl AccessTokenVerifier for ZitadelIntrospectionVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, VerificationError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(VerificationError::InvalidToken);
        }
        let response = self
            .http
            .post(self.endpoint.clone())
            .basic_auth(self.client_id.as_str(), Some(self.client_secret.as_str()))
            .form(&[("token", token)])
            .send()
            .await
            .map_err(|error| {
                VerificationError::IntrospectionUnavailable(format!(
                    "请求失败（{}）",
                    error
                        .status()
                        .map(|status| status.as_u16().to_string())
                        .unwrap_or_else(|| "network".to_owned())
                ))
            })?;
        if !response.status().is_success() {
            return Err(VerificationError::IntrospectionUnavailable(format!(
                "端点返回 HTTP {}",
                response.status().as_u16()
            )));
        }
        let response = response
            .json::<IntrospectionResponse>()
            .await
            .map_err(|_| {
                VerificationError::IntrospectionUnavailable("响应不是有效 JSON".to_owned())
            })?;
        response.verified_identity(self)
    }
}

#[derive(Debug, Deserialize)]
struct IntrospectionResponse {
    active: bool,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    iss: Option<String>,
    #[serde(default)]
    aud: Option<Audience>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    nbf: Option<i64>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "urn:zitadel:iam:org:id", default)]
    organization_id: Option<String>,
    #[serde(rename = "urn:zitadel:iam:user:resourceowner:id", default)]
    resource_owner_id: Option<String>,
    #[serde(rename = "urn:zitadel:iam:user:resourceowner:name", default)]
    resource_owner_name: Option<String>,
    #[serde(rename = "urn:zitadel:iam:user:resourceowner:primary_domain", default)]
    resource_owner_primary_domain: Option<String>,
}

impl IntrospectionResponse {
    fn verified_identity(
        self,
        verifier: &ZitadelIntrospectionVerifier,
    ) -> Result<VerifiedIdentity, VerificationError> {
        if !self.active {
            return Err(VerificationError::InvalidToken);
        }
        let subject = required_claim(self.sub.as_deref())?;
        let issuer = self
            .iss
            .as_deref()
            .ok_or(VerificationError::InvalidToken)
            .and_then(normalized_issuer)?;
        if issuer != verifier.issuer
            || !self
                .aud
                .as_ref()
                .is_some_and(|audience| audience.contains(verifier.audience.as_str()))
        {
            return Err(VerificationError::InvalidToken);
        }
        let now = Utc::now().timestamp();
        if self.exp.is_some_and(|expiration| expiration <= now)
            || self.nbf.is_some_and(|not_before| not_before > now)
        {
            return Err(VerificationError::InvalidToken);
        }
        let username = optional_claim(self.preferred_username.or(self.username));
        let email = optional_claim(self.email);
        let display_name = optional_claim(self.name)
            .or_else(|| username.clone())
            .or_else(|| email.clone())
            .unwrap_or_else(|| subject.clone());
        let organization_id =
            optional_claim(self.organization_id).or_else(|| optional_claim(self.resource_owner_id));
        let organization = organization_id.map(|id| VerifiedOrganizationContext {
            id,
            name: optional_claim(self.resource_owner_name),
            primary_domain: optional_claim(self.resource_owner_primary_domain),
        });
        Ok(VerifiedIdentity {
            issuer: verifier.identity_issuer.clone(),
            subject,
            username,
            email,
            display_name,
            organization,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, expected: &str) -> bool {
        match self {
            Self::One(value) => value == expected,
            Self::Many(values) => values.iter().any(|value| value == expected),
        }
    }
}

fn jwt_shaped(token: &str) -> bool {
    let mut segments = token.split('.');
    let three_non_empty = segments.next().is_some_and(|value| !value.is_empty())
        && segments.next().is_some_and(|value| !value.is_empty())
        && segments.next().is_some_and(|value| !value.is_empty())
        && segments.next().is_none();
    three_non_empty && decode_header(token).is_ok()
}

fn normalized_issuer(value: &str) -> Result<Url, VerificationError> {
    let mut url = Url::parse(value.trim())
        .map_err(|_| VerificationError::InvalidConfiguration("issuer URL 无效".to_owned()))?;
    let secure = url.scheme() == "https"
        || (url.scheme() == "http"
            && match url.host() {
                Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
                Some(Host::Ipv4(address)) => address.is_loopback(),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                None => false,
            });
    if !secure
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(VerificationError::InvalidConfiguration(
            "issuer 必须使用 HTTPS；仅 loopback 可使用 HTTP，且不能包含凭据、query 或 fragment"
                .to_owned(),
        ));
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(if path.is_empty() { "/" } else { path.as_str() });
    Ok(url)
}

fn required_configuration(value: String, field: &str) -> Result<String, VerificationError> {
    let value = value.trim();
    if value.is_empty() {
        Err(VerificationError::InvalidConfiguration(format!(
            "{field} 不能为空"
        )))
    } else {
        Ok(value.to_owned())
    }
}

fn required_claim(value: Option<&str>) -> Result<String, VerificationError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(VerificationError::InvalidToken)
}

fn optional_claim(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
