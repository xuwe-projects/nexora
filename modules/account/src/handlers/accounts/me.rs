//! 当前认证用户 handler。

use axum::{Json, extract::State};
use contracts::account::AccessProfileResponse;

use crate::{
    Account, AccountState, ApiError, UserType, authorization::AuthenticatedUser,
    handlers::accounts::access_profile_response,
};

/// 返回当前认证用户及其角色和合并权限。
pub(crate) async fn current_user(
    authenticated: AuthenticatedUser,
    State(state): State<AccountState>,
) -> Result<Json<AccessProfileResponse>, ApiError> {
    if authenticated.profile().user.user_type == UserType::ServiceAccount {
        return Ok(Json(access_profile_response(authenticated.into_profile())));
    }
    let identity_id = authenticated.profile().user.identity_id.clone();
    let profile = Account { state }
        .refresh_user_from_directory(identity_id.as_str())
        .await?;
    Ok(Json(access_profile_response(profile)))
}
