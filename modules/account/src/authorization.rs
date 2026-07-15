//! Axum 认证与权限 extractors。

/// 账号 HTTP 资源使用的编译期权限标记。
pub mod accounts;

use std::marker::PhantomData;

use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};

use crate::{AccessProfile, AccountState, ApiError, ExternalIdentity};

/// 已通过 Bearer token 验证、身份同步和停用状态检查的当前用户。
pub struct AuthenticatedUser {
    profile: AccessProfile,
}

impl AuthenticatedUser {
    /// 返回当前用户、角色和合并权限的只读快照。
    pub fn profile(&self) -> &AccessProfile {
        &self.profile
    }

    /// 消费 extractor 并返回当前用户的授权快照。
    pub fn into_profile(self) -> AccessProfile {
        self.profile
    }
}

impl FromRequestParts<AccountState> for AuthenticatedUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AccountState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?;
        let identity = state.token_verifier().verify(token).await?;
        let profile = state
            .application()
            .authenticate(&ExternalIdentity {
                issuer: identity.issuer,
                subject: identity.subject,
                email: identity.email,
                display_name: identity.display_name,
                avatar_url: identity.avatar_url,
            })
            .await?;
        Ok(Self { profile })
    }
}

/// 权限 extractor 使用的编译期权限标记。
pub trait RequiredPermission: Send + Sync {
    /// 受保护 handler 要求的稳定权限键。
    const KEY: &'static str;
}

/// 已认证且拥有 `P` 标记所要求权限的当前用户。
pub struct Authorized<P> {
    profile: AccessProfile,
    permission: PhantomData<P>,
}

impl<P> Authorized<P> {
    /// 返回通过权限门禁时使用的授权快照。
    pub fn profile(&self) -> &AccessProfile {
        &self.profile
    }
}

impl<P> FromRequestParts<AccountState> for Authorized<P>
where
    P: RequiredPermission,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AccountState,
    ) -> Result<Self, Self::Rejection> {
        let authenticated = AuthenticatedUser::from_request_parts(parts, state).await?;
        state
            .application()
            .require_permission(authenticated.profile(), P::KEY)?;
        Ok(Self {
            profile: authenticated.profile,
            permission: PhantomData,
        })
    }
}

fn bearer_token(parts: &Parts) -> Result<&str, ApiError> {
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
