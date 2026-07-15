//! 权限目录 handlers。

use axum::{Json, extract::State};
use contracts::{account::PermissionResponse, collection::ItemsResponse};

use crate::{
    AccountState, ApiError,
    authorization::{Authorized, accounts::ReadPermissions},
    models::account::permission_response,
};

/// 返回系统支持的完整权限目录。
pub(crate) async fn list_permissions(
    _authorization: Authorized<ReadPermissions>,
    State(state): State<AccountState>,
) -> Result<Json<ItemsResponse<PermissionResponse>>, ApiError> {
    let items = state
        .application()
        .list_permissions()
        .await?
        .into_iter()
        .map(permission_response)
        .collect();
    Ok(Json(ItemsResponse { items }))
}
