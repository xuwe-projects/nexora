//! 当前认证用户 handler。

use axum::Json;
use contracts::account::AccessProfileResponse;

use crate::{authorization::AuthenticatedUser, models::account::access_profile_response};

/// 返回当前认证用户及其角色和合并权限。
pub(crate) async fn current_user(authenticated: AuthenticatedUser) -> Json<AccessProfileResponse> {
    Json(access_profile_response(authenticated.into_profile()))
}
