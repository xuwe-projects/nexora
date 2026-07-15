//! 账号领域和持久化错误。

use api::ApiError;
use axum::http::StatusCode;
use thiserror::Error;

use kernel::ValidationError;

use crate::authentication::VerificationError;

/// Store 在执行持久化操作时返回的结构化错误。
#[derive(Debug, Error)]
pub enum StoreError {
    /// PostgreSQL 或连接池返回了底层错误。
    #[error("数据库操作失败")]
    Database(
        /// SQLx 返回的底层错误，仅供服务日志和错误链诊断使用。
        #[from]
        sqlx::Error,
    ),
    /// 指定资源不存在。
    #[error("{0}不存在")]
    NotFound(
        /// 面向领域层的资源名称，不包含数据库表名或 SQL 信息。
        &'static str,
    ),
    /// 唯一键或当前资源状态与操作冲突。
    #[error("资源状态冲突: {0}")]
    Conflict(
        /// 稳定冲突原因，供领域层映射为公开错误码。
        &'static str,
    ),
    /// 当前操作试图修改数据库预置的系统角色。
    #[error("系统角色不可修改或删除")]
    SystemRole,
    /// 当前操作会让系统失去最后一个可用管理员。
    #[error("必须至少保留一个启用状态的系统管理员")]
    LastAdministrator,
    /// 当前操作试图修改唯一内置超级管理员的身份、状态或角色。
    #[error("内置超级管理员账号不可修改或删除")]
    SuperAdministratorImmutable,
    /// 数据库已经绑定了另一个超级管理员身份。
    #[error("系统已经绑定超级管理员")]
    SuperAdministratorAlreadyBound,
    /// 当前操作试图把保留的超级管理员角色授予普通用户。
    #[error("超级管理员角色只能属于内置超级管理员账号")]
    SuperAdministratorRoleReserved,
    /// 数据库中的值不符合当前领域模型约束。
    #[error("数据库中的{0}无效")]
    InvalidData(
        /// 无效数据对应的领域字段名称。
        &'static str,
    ),
}

/// 账号与授权用例向 API 层返回的领域错误。
#[derive(Debug, Error)]
pub enum AccountError {
    /// 外部身份缺少稳定 issuer 或 subject。
    #[error("认证身份不完整")]
    InvalidIdentity,
    /// 当前用户已被停用。
    #[error("当前用户已被停用")]
    UserSuspended,
    /// 当前用户没有执行操作所需权限。
    #[error("缺少权限: {0}")]
    Forbidden(
        /// 被拒绝操作要求的稳定权限键。
        &'static str,
    ),
    /// 请求字段没有通过 application 层校验。
    #[error(transparent)]
    InvalidInput(
        /// 跨业务模块共享的字段校验详情。
        #[from]
        ValidationError,
    ),
    /// 指定领域资源不存在。
    #[error("{0}不存在")]
    NotFound(
        /// 稳定资源名称。
        &'static str,
    ),
    /// 操作与当前领域状态冲突。
    #[error("操作冲突: {code}")]
    Conflict {
        /// 可映射为 API 错误码的稳定原因。
        code: &'static str,
        /// 面向调用方的冲突说明。
        message: &'static str,
    },
    /// Store 返回了未被更具体规则映射的错误。
    #[error(transparent)]
    Store(
        /// 持久化层错误。
        StoreError,
    ),
}

impl From<StoreError> for AccountError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound(resource) => Self::NotFound(resource),
            StoreError::Conflict(code) => Self::Conflict {
                code,
                message: "资源已存在或当前状态不允许该操作",
            },
            StoreError::SystemRole => Self::Conflict {
                code: "system_role_immutable",
                message: "系统角色不可修改或删除",
            },
            StoreError::LastAdministrator => Self::Conflict {
                code: "last_administrator",
                message: "必须至少保留一个启用状态的系统管理员",
            },
            StoreError::SuperAdministratorImmutable => Self::Conflict {
                code: "super_administrator_immutable",
                message: "内置超级管理员账号不可修改、停用、删除或更换角色",
            },
            StoreError::SuperAdministratorAlreadyBound => Self::Conflict {
                code: "super_administrator_already_bound",
                message: "系统已经绑定另一个超级管理员",
            },
            StoreError::SuperAdministratorRoleReserved => Self::Conflict {
                code: "super_administrator_role_reserved",
                message: "超级管理员角色只能属于内置超级管理员账号",
            },
            other => Self::Store(other),
        }
    }
}

impl From<AccountError> for ApiError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::InvalidIdentity => {
                Self::unauthorized("invalid_identity", "认证身份不完整")
            }
            AccountError::UserSuspended => Self::new(
                StatusCode::FORBIDDEN,
                "account_suspended",
                "当前用户已被停用",
            ),
            AccountError::Forbidden(_) => Self::new(
                StatusCode::FORBIDDEN,
                "permission_denied",
                "没有执行该操作的权限",
            ),
            AccountError::InvalidInput(validation) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                validation.message(),
            )
            .with_details(serde_json::json!({ "field": validation.field() })),
            AccountError::NotFound(_) => {
                Self::new(StatusCode::NOT_FOUND, "resource_not_found", "资源不存在")
            }
            AccountError::Conflict { code, message } => {
                Self::new(StatusCode::CONFLICT, code, message)
            }
            AccountError::Store(error) => {
                tracing::error!(error = ?error, "账号 store 操作失败");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "服务暂时无法完成请求",
                )
            }
        }
    }
}

impl From<VerificationError> for ApiError {
    fn from(error: VerificationError) -> Self {
        match error {
            VerificationError::InvalidToken => {
                Self::unauthorized("invalid_access_token", "Bearer token 无效或已过期")
            }
            VerificationError::ProviderUnavailable(source) => {
                tracing::warn!(error = ?source, "OIDC Provider 或 JWKS 暂时不可用");
                Self::service_unavailable("identity_provider_unavailable", "认证服务暂时不可用")
            }
            error => {
                tracing::error!(error = ?error, "OIDC verifier 配置或元数据错误");
                Self::service_unavailable("identity_provider_unavailable", "认证服务暂时不可用")
            }
        }
    }
}
