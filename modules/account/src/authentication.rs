//! Bearer access token 验证端口与 OIDC 实现。

mod oidc;

use async_trait::async_trait;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts},
};
use thiserror::Error;

use std::sync::Arc;

use crate::ApiError;

pub use oidc::OidcAccessTokenVerifier;

/// ZITADEL token 中携带的组织上下文。
///
/// 对 portal 或 public API 这类独立 resource server，业务层可以用该值把 ZITADEL
/// Organization 映射到自己的 customer/tenant 表。该结构只表达 token 中已经验签的声明，
/// 不代表 Nexora Account 本地用户或 RBAC 已完成授权。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOrganizationContext {
    /// ZITADEL Organization 的稳定唯一 ID。
    pub id: String,
    /// ZITADEL 返回的组织名称；token 未携带时为 `None`。
    pub name: Option<String>,
    /// ZITADEL 返回的组织主域名；token 未携带时为 `None`。
    pub primary_domain: Option<String>,
}

/// 验证 access token 后得到的可信外部身份声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    /// 令牌的规范 issuer。
    pub issuer: String,
    /// issuer 内稳定且唯一的 subject。
    pub subject: String,
    /// 身份提供方返回的可选登录用户名。
    pub username: Option<String>,
    /// 可选邮箱，只用于本地资料展示。
    pub email: Option<String>,
    /// 适合本地界面展示的名称。
    pub display_name: String,
    /// 可选头像地址。
    pub avatar_url: Option<String>,
    /// 可选 ZITADEL Organization 上下文，供 portal/openapi 映射业务租户。
    pub organization: Option<VerifiedOrganizationContext>,
}

/// Access token 校验失败原因。
#[derive(Debug, Error)]
pub enum VerificationError {
    /// OIDC issuer 或 audience 配置无效。
    #[error("OIDC 资源服务配置无效: {0}")]
    InvalidConfiguration(
        /// 不包含密钥或 token 的配置错误说明。
        String,
    ),
    /// Provider discovery 或 JWKS 暂时不可用。
    #[error("OIDC Provider 暂时不可用")]
    ProviderUnavailable(
        /// 网络请求返回的底层错误，仅用于日志诊断。
        #[source]
        reqwest::Error,
    ),
    /// Provider discovery 文档不完整或与配置不一致。
    #[error("OIDC Provider 元数据无效: {0}")]
    InvalidMetadata(
        /// 元数据错误说明，不包含令牌内容。
        String,
    ),
    /// Bearer token 的格式、签名或标准声明无效。
    #[error("Bearer token 无效")]
    InvalidToken,
}

/// 将 access token 转换为可信身份的验证端口。
#[async_trait]
pub trait AccessTokenVerifier: Send + Sync {
    /// 校验签名、issuer、audience、过期时间与 subject，并返回可信身份。
    ///
    /// # Errors
    ///
    /// Token 无效、Provider 元数据不一致或 JWKS 暂时不可用时返回错误。
    async fn verify(&self, token: &str) -> Result<VerifiedIdentity, VerificationError>;
}

/// 可放入 Axum State 的独立 OIDC resource server verifier。
///
/// 该句柄只验证 Bearer access token 的 issuer、audience、签名和标准时效声明，并返回
/// [`VerifiedIdentity`]。它不会访问 Nexora Account 本地用户表，也不会执行 Account RBAC，
/// 因此适合 customer portal 或 public API 自己把 subject/org 映射到业务租户。
#[derive(Clone)]
pub struct OidcResourceServer {
    verifier: Arc<dyn AccessTokenVerifier>,
}

impl OidcResourceServer {
    /// 使用任意实现了 [`AccessTokenVerifier`] 的 verifier 构造 resource server 句柄。
    pub fn new(verifier: Arc<dyn AccessTokenVerifier>) -> Self {
        Self { verifier }
    }

    /// 验证 access token 并返回可信身份声明。
    ///
    /// # Errors
    ///
    /// token 无效、Provider 元数据不一致或 JWKS 暂时不可用时返回 [`VerificationError`]。
    pub async fn verify(&self, token: &str) -> Result<VerifiedIdentity, VerificationError> {
        self.verifier.verify(token).await
    }
}

/// 从 `Authorization: Bearer ...` 请求头提取出的原始 access token。
///
/// 该 extractor 只解析 HTTP Bearer scheme，不验证签名，也不触碰数据库。需要可信身份时使用
/// [`VerifiedBearerIdentity`]。
pub struct BearerAccessToken {
    token: String,
}

impl BearerAccessToken {
    /// 返回去除首尾空白后的 access token。
    pub fn as_str(&self) -> &str {
        self.token.as_str()
    }

    /// 消费 extractor 并返回 access token 字符串。
    pub fn into_string(self) -> String {
        self.token
    }
}

impl<S> FromRequestParts<S> for BearerAccessToken
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?;
        Ok(Self {
            token: token.to_owned(),
        })
    }
}

/// 已通过独立 OIDC resource server verifier 验证的 Bearer 身份。
///
/// 宿主 State 需要实现 `FromRef<AppState> for OidcResourceServer`。该 extractor 不要求也不会
/// 使用 [`crate::Account`]，因此不会把 customer portal 用户混入默认内部 Account 用户管理。
pub struct VerifiedBearerIdentity {
    identity: VerifiedIdentity,
}

impl VerifiedBearerIdentity {
    /// 返回已验签且通过 issuer/audience/有效期校验的身份声明。
    pub fn identity(&self) -> &VerifiedIdentity {
        &self.identity
    }

    /// 消费 extractor 并返回身份声明。
    pub fn into_identity(self) -> VerifiedIdentity {
        self.identity
    }
}

impl<S> FromRequestParts<S> for VerifiedBearerIdentity
where
    S: Send + Sync,
    OidcResourceServer: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = BearerAccessToken::from_request_parts(parts, state).await?;
        let identity = OidcResourceServer::from_ref(state)
            .verify(token.as_str())
            .await?;
        Ok(Self { identity })
    }
}

pub(crate) fn bearer_token(parts: &Parts) -> Result<&str, ApiError> {
    let value = parts
        .headers
        .get(AUTHORIZATION)
        .ok_or_else(|| ApiError::unauthorized("missing_access_token", "缺少 Bearer token"))?
        .to_str()
        .map_err(|_| ApiError::unauthorized("invalid_access_token", "Authorization 头无效"))?;
    let (scheme, token) = value.split_once(' ').ok_or_else(|| {
        ApiError::unauthorized(
            "invalid_access_token",
            "Authorization 头必须使用 Bearer scheme",
        )
    })?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.trim().is_empty() {
        return Err(ApiError::unauthorized(
            "invalid_access_token",
            "Authorization 头必须使用 Bearer scheme",
        ));
    }
    Ok(token.trim())
}
