//! 角色与角色权限 handlers。

use api::{ApiJson, ApiPath};
use axum::{
    Json,
    extract::State,
    http::{StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
};
use contracts::{
    account::{CreateRoleRequest, ReplaceRolePermissionsRequest, RoleResponse, UpdateRoleRequest},
    collection::ItemsResponse,
    patch::PatchField,
};

use crate::{
    Account, AccountError, AccountState, ApiError,
    authorization::{
        Authorized,
        accounts::{ReadRoles, WriteRoles},
    },
    handlers::accounts::role_response,
};

/// 返回全部角色及其直接权限。
pub(crate) async fn list_roles(
    _authorization: Authorized<ReadRoles>,
    State(state): State<AccountState>,
) -> Result<Json<ItemsResponse<RoleResponse>>, ApiError> {
    let items = Account { state }
        .roles()
        .await?
        .into_iter()
        .map(role_response)
        .collect();
    Ok(Json(ItemsResponse { items }))
}

/// 返回指定角色及其直接权限。
pub(crate) async fn get_role(
    _authorization: Authorized<ReadRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<i64>,
) -> Result<Json<RoleResponse>, ApiError> {
    let role = Account { state }.role(role_id).await?;
    Ok(Json(role_response(role)))
}

/// 创建自定义角色并返回新资源位置。
pub(crate) async fn create_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<CreateRoleRequest>,
) -> Result<Response, ApiError> {
    let role = Account { state }
        .create_role(
            request.key.as_str(),
            request.name.as_str(),
            request.description.as_deref(),
            request.permission_ids.as_slice(),
        )
        .await?;
    let location = format!("/roles/{}", role.id);
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(role_response(role)),
    )
        .into_response())
}

/// 局部修改指定自定义角色。
pub(crate) async fn update_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<i64>,
    ApiJson(request): ApiJson<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    if request.name.is_none() && request.description == PatchField::Missing {
        return Err(AccountError::InvalidInput(kernel::ValidationError::new(
            "body",
            "至少需要提供一个要修改的角色字段",
        ))
        .into());
    }
    let description = match &request.description {
        PatchField::Missing => None,
        PatchField::Null => Some(None),
        PatchField::Value(value) => Some(Some(value.as_str())),
    };
    let role = Account { state }
        .update_role(role_id, request.name.as_deref(), description)
        .await?;
    Ok(Json(role_response(role)))
}

/// 删除指定自定义角色。
pub(crate) async fn delete_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<i64>,
) -> Result<StatusCode, ApiError> {
    Account { state }.delete_role(role_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 原子替换指定自定义角色的权限集合。
pub(crate) async fn replace_role_permissions(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<i64>,
    ApiJson(request): ApiJson<ReplaceRolePermissionsRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    let role = Account { state }
        .replace_role_permissions(role_id, request.permission_ids.as_slice())
        .await?;
    Ok(Json(role_response(role)))
}
