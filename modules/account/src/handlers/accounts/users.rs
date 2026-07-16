//! 用户与用户角色 handlers。

use api::{ApiJson, ApiPath, ApiQuery};
use axum::{
    Json,
    extract::State,
    http::{StatusCode, header::LOCATION},
    response::IntoResponse,
};
use contracts::{
    account::{
        AccessProfileResponse, ProvisionUserRequest, ReplaceUserRolesRequest,
        UpdateUserStatusRequest, UserPageResponse, UserResponse,
    },
    pagination::PageQuery,
};

use crate::{
    AccountError, AccountState, ApiError, ExternalIdentity, StoreError,
    authorization::{
        Authorized,
        accounts::{ProvisionUsers, ReadUsers, WriteUserRoles, WriteUserStatus},
    },
    handlers::accounts::{
        access_profile_response, page_request, user_page_response, user_response, user_role_ids,
        user_status,
    },
    stores,
};

/// 显式开通一个经过管理员确认的外部身份。
pub(crate) async fn provision_user(
    _authorization: Authorized<ProvisionUsers>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<ProvisionUserRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let user = state
        .provision_user(ExternalIdentity {
            identity_id: request.identity_id,
            email: request.email,
            display_name: request.display_name,
            avatar_url: request.avatar_url,
        })
        .await?;
    let location = format!("/users/{}", user.id);
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(user_response(user)),
    ))
}

/// 分页返回用户集合。
pub(crate) async fn list_users(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AccountState>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<UserPageResponse>, ApiError> {
    let request = page_request(query.page, query.page_size)?;
    let page = stores::users::query_page(request, state.pool())
        .await
        .map_err(StoreError::from)?;
    Ok(Json(user_page_response(page)))
}

/// 返回指定用户及其授权快照。
pub(crate) async fn get_user(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<String>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    let profile = stores::users::query_access_profile(user_id.as_str(), state.pool())
        .await?
        .ok_or(AccountError::NotFound("用户"))?;
    Ok(Json(access_profile_response(profile)))
}

/// 修改指定用户的访问状态。
pub(crate) async fn update_user_status(
    _authorization: Authorized<WriteUserStatus>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateUserStatusRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let user =
        stores::users::update_status(user_id.as_str(), user_status(request.status), state.pool())
            .await?;
    Ok(Json(user_response(user)))
}

/// 原子替换指定用户的直接角色集合。
pub(crate) async fn replace_user_roles(
    authorization: Authorized<WriteUserRoles>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<String>,
    ApiJson(request): ApiJson<ReplaceUserRolesRequest>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    let role_ids = user_role_ids(request.role_ids)?;
    let profile = stores::users::replace_roles(
        user_id.as_str(),
        &role_ids,
        authorization.profile().user.id.as_str(),
        state.pool(),
    )
    .await?;
    Ok(Json(access_profile_response(profile)))
}
