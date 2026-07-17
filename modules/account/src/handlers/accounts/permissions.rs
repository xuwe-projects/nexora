//! 权限目录 handlers。

use axum::{Json, extract::State};
use contracts::{account::PermissionResponse, collection::ItemsResponse};

use crate::{
    Account, AccountState, ApiError,
    authorization::{Authorized, accounts::ReadPermissions},
    handlers::accounts::permission_response,
};

/// 返回系统支持的完整权限目录。
pub(crate) async fn list_permissions(
    _authorization: Authorized<ReadPermissions>,
    State(state): State<AccountState>,
) -> Result<Json<ItemsResponse<PermissionResponse>>, ApiError> {
    let items = Account { state }
        .permissions()
        .await?
        .into_iter()
        .map(permission_response)
        .collect();
    Ok(Json(ItemsResponse { items }))
}
