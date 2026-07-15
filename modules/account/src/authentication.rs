//! Bearer access token 验证端口与 OIDC 实现。

mod oidc;

use async_trait::async_trait;
use thiserror::Error;

pub use oidc::OidcAccessTokenVerifier;

/// 验证 access token 后得到的可信外部身份声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    /// 令牌的规范 issuer。
    pub issuer: String,
    /// issuer 内稳定且唯一的 subject。
    pub subject: String,
    /// 可选邮箱，只用于本地资料展示。
    pub email: Option<String>,
    /// 适合本地界面展示的名称。
    pub display_name: String,
    /// 可选头像地址。
    pub avatar_url: Option<String>,
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
