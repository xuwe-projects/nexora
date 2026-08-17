//! 服务账号资料管理 handlers。

use api::{ApiJson, ApiPath};
use axum::{
    Json,
    extract::State,
    http::{StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
};
use contracts::{
    account::{CreateServiceAccountRequest, UpdateServiceAccountRequest, UserResponse},
    patch::PatchField,
};
use kernel::ValidationError;

use crate::{
    Account, AccountError, AccountState, ApiError, CreateServiceAccountIdentity, UserType,
    authorization::{
        Authorized, RequiredPermission,
        accounts::{ProvisionServiceAccounts, WriteServiceAccountProfiles, WriteUserRoles},
    },
    handlers::accounts::user_response,
};

pub(crate) async fn create_service_account(
    authorization: Authorized<ProvisionServiceAccounts>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<CreateServiceAccountRequest>,
) -> Result<Response, ApiError> {
    let roles_write = <WriteUserRoles as RequiredPermission>::KEY;
    if !request.role_ids.is_empty() && !authorization.profile().allows(roles_write.clone()) {
        return Err(AccountError::Forbidden(roles_write).into());
    }
    let granted_by = authorization.profile().user.id.clone();
    let user = Account { state }
        .create_or_reuse_service_account(
            CreateServiceAccountIdentity {
                username: request.username,
                display_name: request.display_name,
                description: request.description,
            },
            request.role_ids.as_slice(),
            granted_by.as_str(),
            request.use_existing,
        )
        .await?;
    let location = format!("/users/{}", user.id);
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(user_response(user)),
    )
        .into_response())
}

pub(crate) async fn update_service_account(
    _authorization: Authorized<WriteServiceAccountProfiles>,
    State(state): State<AccountState>,
    ApiPath(service_account_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateServiceAccountRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    if request.username.is_some() {
        return Err(AccountError::Conflict {
            code: "service_account_identifier_immutable",
            message: "服务账号 username 创建后不可修改",
        }
        .into());
    }
    if request.display_name.is_none() && request.description == PatchField::Missing {
        return Err(AccountError::InvalidInput(ValidationError::new(
            "body",
            "至少需要提供一个要修改的服务账号字段",
        ))
        .into());
    }
    let account = Account { state };
    let current = account.user_access(service_account_id.as_str()).await?.user;
    if current.user_type != UserType::ServiceAccount {
        return Err(AccountError::Conflict {
            code: "service_account_required",
            message: "该操作只允许用于服务账号",
        }
        .into());
    }
    let display_name = request
        .display_name
        .as_deref()
        .unwrap_or(current.display_name.as_str());
    let description = match &request.description {
        PatchField::Missing => current.description.as_deref(),
        PatchField::Null => None,
        PatchField::Value(value) => Some(value.as_str()),
    };
    let user = account
        .update_service_account_profile(service_account_id.as_str(), display_name, description)
        .await?;
    Ok(Json(user_response(user)))
}
