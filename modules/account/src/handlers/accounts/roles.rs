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
    AccountError, AccountState, ApiError,
    authorization::{
        Authorized,
        accounts::{ReadRoles, WriteRoles},
    },
    handlers::accounts::{
        role_permission_ids, role_response, validate_role_fields, validate_role_key,
    },
    stores,
};

/// 返回全部角色及其直接权限。
pub(crate) async fn list_roles(
    _authorization: Authorized<ReadRoles>,
    State(state): State<AccountState>,
) -> Result<Json<ItemsResponse<RoleResponse>>, ApiError> {
    let items = stores::roles::query_all(state.pool())
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
    let role = stores::roles::query_by_id(role_id, state.pool())
        .await?
        .ok_or(AccountError::NotFound("角色"))?;
    Ok(Json(role_response(role)))
}

/// 创建自定义角色并返回新资源位置。
pub(crate) async fn create_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<CreateRoleRequest>,
) -> Result<Response, ApiError> {
    validate_role_key(request.key.as_str())?;
    validate_role_fields(request.name.as_str(), request.description.as_deref())?;
    let permission_ids = role_permission_ids(request.permission_ids)?;
    let role = stores::roles::create(
        request.key.as_str(),
        request.name.as_str(),
        request.description.as_deref(),
        &permission_ids,
        state.pool(),
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
    let current = stores::roles::query_by_id(role_id, state.pool())
        .await?
        .ok_or(AccountError::NotFound("角色"))?;
    if current.is_system {
        return Err(AccountError::Conflict {
            code: "system_role_immutable",
            message: "系统角色不可修改或删除",
        }
        .into());
    }
    let description = match &request.description {
        PatchField::Missing => None,
        PatchField::Null => Some(None),
        PatchField::Value(value) => Some(Some(value.as_str())),
    };
    let final_name = request.name.as_deref().unwrap_or(current.name.as_str());
    let final_description = description.unwrap_or(current.description.as_deref());
    validate_role_fields(final_name, final_description)?;
    let role =
        stores::roles::update(role_id, request.name.as_deref(), description, state.pool()).await?;
    Ok(Json(role_response(role)))
}

/// 删除指定自定义角色。
pub(crate) async fn delete_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<i64>,
) -> Result<StatusCode, ApiError> {
    stores::roles::delete(role_id, state.pool()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 原子替换指定自定义角色的权限集合。
pub(crate) async fn replace_role_permissions(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<i64>,
    ApiJson(request): ApiJson<ReplaceRolePermissionsRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    let permission_ids = role_permission_ids(request.permission_ids)?;
    let role = stores::roles::replace_permissions(role_id, &permission_ids, state.pool()).await?;
    Ok(Json(role_response(role)))
}
