//! 基于 OIDC discovery 与 JWKS 的异步 JWT access token 验证器。

use std::time::{Duration, Instant};

use async_trait::async_trait;
use jsonwebtoken::{
    Algorithm, AlgorithmFamily, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, Jwk, JwkSet, KeyOperations, PublicKeyUse},
};
use reqwest::redirect::Policy;
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use url::{Host, Url};

use super::{
    AccessTokenVerifier, VerificationError, VerifiedIdentity, VerifiedOrganizationContext,
};

const MIN_JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// 使用 OIDC discovery 和可轮换 JWKS 验证 JWT access token。
pub struct OidcAccessTokenVerifier {
    http: reqwest::Client,
    issuer: String,
    identity_issuer: String,
    audiences: Vec<String>,
    jwks_uri: Url,
    signing_algorithms: Vec<String>,
    jwks: RwLock<JwkSet>,
    last_jwks_refresh: Mutex<Option<Instant>>,
}

impl OidcAccessTokenVerifier {
    /// 发现 Provider 元数据、校验 issuer 并预加载 JWKS。
    ///
    /// # Errors
    ///
    /// issuer URL 或 audience 无效、discovery 与配置不一致、元数据缺字段，或网络请求失败时
    /// 返回 [`VerificationError`]。
    pub async fn discover(
        issuer: impl AsRef<str>,
        audience: impl Into<String>,
    ) -> Result<Self, VerificationError> {
        Self::discover_many(issuer, [audience.into()]).await
    }

    /// 发现 Provider 元数据、校验 issuer，并创建支持多个 audience 的 access token verifier。
    ///
    /// 任一配置的 audience 命中 token `aud` claim 即可通过验证，适合同一 issuer 下 portal、
    /// openapi 等多个 resource server 共用 verifier 的场景。
    ///
    /// # Errors
    ///
    /// issuer URL 无效、audience 列表为空或包含空值、discovery 与配置不一致、元数据缺字段，
    /// 或网络请求失败时返回 [`VerificationError`]。
    pub async fn discover_many<I, A>(
        issuer: impl AsRef<str>,
        audiences: I,
    ) -> Result<Self, VerificationError>
    where
        I: IntoIterator<Item = A>,
        A: Into<String>,
    {
        let issuer_url = normalized_issuer(issuer.as_ref())?;
        let identity_issuer = issuer_url.to_string();
        let audiences = normalize_audiences(audiences)?;
        if audiences.is_empty() {
            return Err(VerificationError::InvalidConfiguration(
                "audience 列表不能为空".to_owned(),
            ));
        }
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(VerificationError::ProviderUnavailable)?;
        let discovery_url = discovery_url(&issuer_url);
        let document = http
            .get(discovery_url)
            .send()
            .await
            .map_err(VerificationError::ProviderUnavailable)?
            .error_for_status()
            .map_err(VerificationError::ProviderUnavailable)?
            .json::<DiscoveryDocument>()
            .await
            .map_err(VerificationError::ProviderUnavailable)?;
        let discovered_issuer = normalized_issuer(document.issuer.as_str())?;
        if discovered_issuer != issuer_url {
            return Err(VerificationError::InvalidMetadata(
                "discovery issuer 与配置不一致".to_owned(),
            ));
        }
        if document.id_token_signing_alg_values_supported.is_empty() {
            return Err(VerificationError::InvalidMetadata(
                "缺少签名算法目录".to_owned(),
            ));
        }
        validate_jwks_uri(&document.jwks_uri)?;
        let jwks = load_jwks(&http, &document.jwks_uri).await?;
        Ok(Self {
            http,
            issuer: document.issuer.trim().to_owned(),
            identity_issuer,
            audiences,
            jwks_uri: document.jwks_uri,
            signing_algorithms: document.id_token_signing_alg_values_supported,
            jwks: RwLock::new(jwks),
            last_jwks_refresh: Mutex::new(None),
        })
    }

    async fn decoding_key(
        &self,
        key_id: Option<&str>,
        algorithm: Algorithm,
    ) -> Result<DecodingKey, VerificationError> {
        let cached_key = {
            let jwks = self.jwks.read().await;
            select_signing_key(&jwks, key_id, algorithm)?
        };
        if let Some(jwk) = cached_key {
            return DecodingKey::from_jwk(&jwk).map_err(|_| VerificationError::InvalidToken);
        }

        let mut last_refresh = self.last_jwks_refresh.lock().await;
        let cached_key = {
            let jwks = self.jwks.read().await;
            select_signing_key(&jwks, key_id, algorithm)?
        };
        if let Some(jwk) = cached_key {
            return DecodingKey::from_jwk(&jwk).map_err(|_| VerificationError::InvalidToken);
        }
        if last_refresh
            .is_some_and(|last_refresh| last_refresh.elapsed() < MIN_JWKS_REFRESH_INTERVAL)
        {
            return Err(VerificationError::InvalidToken);
        }

        *last_refresh = Some(Instant::now());
        let refreshed = load_jwks(&self.http, &self.jwks_uri).await?;
        let key = select_signing_key(&refreshed, key_id, algorithm)?;
        *self.jwks.write().await = refreshed;
        let key = key.ok_or(VerificationError::InvalidToken)?;
        let decoding_key =
            DecodingKey::from_jwk(&key).map_err(|_| VerificationError::InvalidToken)?;
        Ok(decoding_key)
    }
}

#[async_trait]
impl AccessTokenVerifier for OidcAccessTokenVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, VerificationError> {
        if token.trim().is_empty() {
            return Err(VerificationError::InvalidToken);
        }
        let header = decode_header(token).map_err(|_| VerificationError::InvalidToken)?;
        if header.alg.family() == AlgorithmFamily::Hmac
            || !self
                .signing_algorithms
                .iter()
                .any(|algorithm| algorithm == &format!("{:?}", header.alg))
        {
            return Err(VerificationError::InvalidToken);
        }
        let decoding_key = self.decoding_key(header.kid.as_deref(), header.alg).await?;
        let mut validation = Validation::new(header.alg);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.set_issuer(&[self.issuer.as_str()]);
        let audiences = self
            .audiences
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        validation.set_audience(audiences.as_slice());
        validation.validate_nbf = true;
        let claims = decode::<AccessTokenClaims>(token, &decoding_key, &validation)
            .map_err(|_| VerificationError::InvalidToken)?
            .claims;
        if claims.subject.trim().is_empty() {
            return Err(VerificationError::InvalidToken);
        }
        let display_name = claims
            .name
            .as_deref()
            .or(claims.preferred_username.as_deref())
            .or(claims.email.as_deref())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(claims.subject.as_str())
            .to_owned();
        let organization = organization_context(&claims)?;
        Ok(VerifiedIdentity {
            issuer: self.identity_issuer.clone(),
            subject: claims.subject,
            username: claims.preferred_username,
            email: claims.email,
            display_name,
            avatar_url: claims.picture,
            organization,
        })
    }
}

#[derive(Debug, Deserialize)]
struct DiscoveryDocument {
    issuer: String,
    jwks_uri: Url,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AccessTokenClaims {
    #[serde(rename = "sub")]
    subject: String,
    name: Option<String>,
    email: Option<String>,
    preferred_username: Option<String>,
    picture: Option<String>,
    #[serde(rename = "urn:zitadel:iam:org:id")]
    zitadel_organization_id: Option<String>,
    #[serde(rename = "urn:zitadel:iam:user:resourceowner:id")]
    resource_owner_id: Option<String>,
    #[serde(rename = "urn:zitadel:iam:user:resourceowner:name")]
    resource_owner_name: Option<String>,
    #[serde(rename = "urn:zitadel:iam:user:resourceowner:primary_domain")]
    resource_owner_primary_domain: Option<String>,
}

fn normalize_audiences<I, A>(audiences: I) -> Result<Vec<String>, VerificationError>
where
    I: IntoIterator<Item = A>,
    A: Into<String>,
{
    let mut normalized = Vec::new();
    for audience in audiences {
        let audience = audience.into();
        let audience = audience.trim();
        if audience.is_empty() {
            return Err(VerificationError::InvalidConfiguration(
                "audience 不能为空".to_owned(),
            ));
        }
        if !normalized.iter().any(|existing| existing == audience) {
            normalized.push(audience.to_owned());
        }
    }
    Ok(normalized)
}

fn organization_context(
    claims: &AccessTokenClaims,
) -> Result<Option<VerifiedOrganizationContext>, VerificationError> {
    let organization_id = claims
        .zitadel_organization_id
        .as_deref()
        .or(claims.resource_owner_id.as_deref())
        .map(required_claim("ZITADEL organization id"))
        .transpose()?;
    let Some(id) = organization_id else {
        return Ok(None);
    };
    Ok(Some(VerifiedOrganizationContext {
        id,
        name: claims
            .resource_owner_name
            .as_deref()
            .map(optional_claim)
            .transpose()?
            .flatten(),
        primary_domain: claims
            .resource_owner_primary_domain
            .as_deref()
            .map(optional_claim)
            .transpose()?
            .flatten(),
    }))
}

fn required_claim(field: &'static str) -> impl FnOnce(&str) -> Result<String, VerificationError> {
    move |value| {
        let value = value.trim();
        if value.is_empty() {
            tracing::warn!(
                claim = field,
                "OIDC token 中的 ZITADEL 组织上下文 claim 为空"
            );
            return Err(VerificationError::InvalidToken);
        }
        Ok(value.to_owned())
    }
}

fn optional_claim(value: &str) -> Result<Option<String>, VerificationError> {
    let value = value.trim();
    Ok((!value.is_empty()).then_some(value.to_owned()))
}

async fn load_jwks(http: &reqwest::Client, uri: &Url) -> Result<JwkSet, VerificationError> {
    http.get(uri.clone())
        .send()
        .await
        .map_err(VerificationError::ProviderUnavailable)?
        .error_for_status()
        .map_err(VerificationError::ProviderUnavailable)?
        .json::<JwkSet>()
        .await
        .map_err(VerificationError::ProviderUnavailable)
}

fn normalized_issuer(value: &str) -> Result<Url, VerificationError> {
    let mut url = Url::parse(value.trim()).map_err(|error| {
        VerificationError::InvalidConfiguration(format!("issuer URL 无效: {error}"))
    })?;
    if url.host().is_none() {
        return Err(VerificationError::InvalidConfiguration(
            "issuer 必须是包含主机的绝对 URL".to_owned(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(VerificationError::InvalidConfiguration(
            "issuer 不能包含凭据、query 或 fragment".to_owned(),
        ));
    }
    if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback(&url)) {
        return Err(VerificationError::InvalidConfiguration(
            "issuer 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP".to_owned(),
        ));
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(if path.is_empty() { "/" } else { path.as_str() });
    Ok(url)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn validate_jwks_uri(url: &Url) -> Result<(), VerificationError> {
    let secure_transport = url.scheme() == "https" || (url.scheme() == "http" && is_loopback(url));
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !secure_transport
    {
        return Err(VerificationError::InvalidMetadata(
            "jwks_uri 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP，且不能包含凭据".to_owned(),
        ));
    }
    Ok(())
}

fn discovery_url(issuer: &Url) -> Url {
    let mut url = issuer.clone();
    let path = issuer.path().trim_end_matches('/');
    url.set_path(&format!("{path}/.well-known/openid-configuration"));
    url
}

fn select_signing_key(
    jwks: &JwkSet,
    key_id: Option<&str>,
    algorithm: Algorithm,
) -> Result<Option<Jwk>, VerificationError> {
    let mut candidates = jwks.keys.iter().filter(|jwk| {
        key_id.is_none_or(|key_id| jwk.common.key_id.as_deref() == Some(key_id))
            && jwk_supports_algorithm(jwk, algorithm)
    });
    let candidate = candidates.next().cloned();
    if candidates.next().is_some() {
        return Err(VerificationError::InvalidToken);
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
