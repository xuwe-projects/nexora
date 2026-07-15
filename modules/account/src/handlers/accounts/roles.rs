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
};
use uuid::Uuid;

use crate::{
    AccountState, ApiError,
    authorization::{
        Authorized,
        accounts::{ReadRoles, WriteRoles},
    },
    models::account::{create_role_input, role_response, update_role_input},
};

/// 返回全部角色及其直接权限。
pub(crate) async fn list_roles(
    _authorization: Authorized<ReadRoles>,
    State(state): State<AccountState>,
) -> Result<Json<ItemsResponse<RoleResponse>>, ApiError> {
    let items = state
        .application()
        .list_roles()
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
    ApiPath(role_id): ApiPath<Uuid>,
) -> Result<Json<RoleResponse>, ApiError> {
    Ok(Json(role_response(
        state.application().role(role_id).await?,
    )))
}

/// 创建自定义角色并返回新资源位置。
pub(crate) async fn create_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<CreateRoleRequest>,
) -> Result<Response, ApiError> {
    let role = state
        .application()
        .create_role(create_role_input(request))
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
    ApiPath(role_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    let role = state
        .application()
        .update_role(role_id, update_role_input(request))
        .await?;
    Ok(Json(role_response(role)))
}

/// 删除指定自定义角色。
pub(crate) async fn delete_role(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.application().delete_role(role_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 原子替换指定自定义角色的权限集合。
pub(crate) async fn replace_role_permissions(
    _authorization: Authorized<WriteRoles>,
    State(state): State<AccountState>,
    ApiPath(role_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<ReplaceRolePermissionsRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    let role = state
        .application()
        .replace_role_permissions(role_id, request.permission_ids)
        .await?;
    Ok(Json(role_response(role)))
}
