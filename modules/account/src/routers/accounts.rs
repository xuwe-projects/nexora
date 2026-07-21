//! 账号、用户、角色与权限资源路由。

use axum::{
    Router,
    routing::{get, put},
};

use crate::{AccountState, handlers};

/// 构建账号、用户、角色与权限资源的全部路由。
pub(super) fn initialize() -> Router<AccountState> {
    Router::new()
        .route("/me", get(handlers::accounts::me::current_user))
        .route(
            "/users",
            get(handlers::accounts::users::list_users)
                .post(handlers::accounts::users::provision_user),
        )
        .route(
            "/avatars",
            axum::routing::post(handlers::accounts::users::upload_avatar),
        )
        .route(
            "/users/{user_id}",
            get(handlers::accounts::users::get_user)
                .patch(handlers::accounts::users::update_user_status),
        )
        .route(
            "/users/{user_id}/roles",
            put(handlers::accounts::users::replace_user_roles),
        )
        .route(
            "/users/{user_id}/avatar",
            axum::routing::patch(handlers::accounts::users::update_user_avatar),
        )
        .route(
            "/roles",
            get(handlers::accounts::roles::list_roles).post(handlers::accounts::roles::create_role),
        )
        .route(
            "/roles/{role_id}",
            get(handlers::accounts::roles::get_role)
                .patch(handlers::accounts::roles::update_role)
                .delete(handlers::accounts::roles::delete_role),
        )
        .route(
            "/roles/{role_id}/permissions",
            put(handlers::accounts::roles::replace_role_permissions),
        )
        .route(
            "/permissions",
            get(handlers::accounts::permissions::list_permissions),
        )
}
