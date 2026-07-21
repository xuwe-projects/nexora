//! 用户与用户角色 handlers。

use api::{ApiJson, ApiPath, ApiQuery};
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::LOCATION},
    response::IntoResponse,
};
use contracts::{
    account::{
        AccessProfileResponse, AvatarUploadResponse, ProvisionUserRequest, ReplaceUserRolesRequest,
        UpdateUserAvatarRequest, UpdateUserStatusRequest, UserPageResponse, UserResponse,
    },
    pagination::PageQuery,
};

use crate::{
    Account, AccountError, AccountState, ApiError, AvatarUpload, CreateHumanIdentity,
    authorization::{
        Authorized, RequiredPermission,
        accounts::{ProvisionUsers, ReadUsers, WriteUserAvatar, WriteUserRoles, WriteUserStatus},
    },
    handlers::accounts::{access_profile_response, user_page_response, user_response, user_status},
};

/// 显式开通一个经过管理员确认的外部身份。
pub(crate) async fn provision_user(
    authorization: Authorized<ProvisionUsers>,
    State(state): State<AccountState>,
    ApiJson(request): ApiJson<ProvisionUserRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let ProvisionUserRequest {
        username,
        given_name,
        family_name,
        email,
        display_name,
        avatar_url,
        initial_password,
        require_password_change,
        role_ids,
    } = request;
    let roles_write_permission = <WriteUserRoles as RequiredPermission>::KEY;
    if !role_ids.is_empty()
        && !authorization
            .profile()
            .allows(roles_write_permission.clone())
    {
        return Err(AccountError::Forbidden(roles_write_permission).into());
    }
    let granted_by = authorization.profile().user.id.clone();
    let user = Account { state }
        .create_managed_user_with_roles(
            CreateHumanIdentity {
                username,
                given_name,
                family_name,
                email,
                display_name,
                avatar_url,
                initial_password,
                require_password_change,
            },
            role_ids.as_slice(),
            granted_by.as_str(),
        )
        .await?;
    let location = format!("/users/{}", user.id);
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(user_response(user)),
    ))
}

/// 分页返回用户集合。
/// 上传头像并返回可访问 URL。
pub(crate) async fn upload_avatar(
    _authorization: Authorized<WriteUserAvatar>,
    State(state): State<AccountState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AvatarUploadResponse>, ApiError> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let avatar_url = Account { state }
        .upload_avatar(AvatarUpload {
            content_type,
            bytes: body.to_vec(),
        })
        .await?;
    Ok(Json(AvatarUploadResponse { avatar_url }))
}

/// 修改指定用户的头像 URL。
pub(crate) async fn update_user_avatar(
    _authorization: Authorized<WriteUserAvatar>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateUserAvatarRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = Account { state }
        .update_user_avatar(user_id.as_str(), request.avatar_url.as_deref())
        .await?;
    Ok(Json(user_response(user)))
}

pub(crate) async fn list_users(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AccountState>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<UserPageResponse>, ApiError> {
    let page = Account { state }.users(query.page, query.page_size).await?;
    Ok(Json(user_page_response(page)))
}

/// 返回指定用户及其授权快照。
pub(crate) async fn get_user(
    _authorization: Authorized<ReadUsers>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<String>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    let profile = Account { state }.user_access(user_id.as_str()).await?;
    Ok(Json(access_profile_response(profile)))
}

/// 修改指定用户的访问状态。
pub(crate) async fn update_user_status(
    _authorization: Authorized<WriteUserStatus>,
    State(state): State<AccountState>,
    ApiPath(user_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateUserStatusRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = Account { state }
        .update_user_status(user_id.as_str(), user_status(request.status))
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
    let profile = Account { state }
        .replace_user_roles(
            user_id.as_str(),
            request.role_ids.as_slice(),
            authorization.profile().user.id.as_str(),
        )
        .await?;
    Ok(Json(access_profile_response(profile)))
}
