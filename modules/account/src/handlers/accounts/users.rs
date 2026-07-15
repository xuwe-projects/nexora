//! 用户与用户角色 handlers。

use api::{ApiJson, ApiPath, ApiQuery};
use axum::{Json, extract::State};
use contracts::{
    account::{
        AccessProfileResponse, ReplaceUserRolesRequest, UpdateUserStatusRequest, UserPageResponse,
        UserResponse,
    },
    pagination::PageQuery,
};
use uuid::Uuid;

use crate::{
    AccountState, ApiError,
    authorization::{
        Authorized,
        accounts::{ReadUsers, WriteUserRoles, WriteUserStatus},
    },
    models::account::{
        access_profile_response, domain_user_status, user_page_response, user_response,
    },
};

/// 分页返回用户集合。
pub(crate) async fn list_users(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AccountState>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<UserPageResponse>, ApiError> {
    let page = state
        .application()
        .list_users(query.page, query.page_size)
        .await?;
    Ok(Json(user_page_response(page)))
}

/// 返回指定用户及其授权快照。
pub(crate) async fn get_user(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<Uuid>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    Ok(Json(access_profile_response(
        state.application().user_access(user_id).await?,
    )))
}

/// 修改指定用户的访问状态。
pub(crate) async fn update_user_status(
    _authorization: Authorized<WriteUserStatus>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateUserStatusRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = state
        .application()
        .set_user_status(user_id, domain_user_status(request.status))
        .await?;
    Ok(Json(user_response(user)))
}

/// 原子替换指定用户的直接角色集合。
pub(crate) async fn replace_user_roles(
    authorization: Authorized<WriteUserRoles>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<ReplaceUserRolesRequest>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    let profile = state
        .application()
        .replace_user_roles(user_id, request.role_ids, authorization.profile().user.id)
        .await?;
    Ok(Json(access_profile_response(profile)))
}
