//! 账号模块路由组合入口。

mod accounts;

use axum::Router;

use crate::AccountState;

pub(crate) fn initialize() -> Router<AccountState> {
    Router::new().merge(accounts::initialize())
}
