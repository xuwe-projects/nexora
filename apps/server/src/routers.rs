//! 顶层应用 State、健康检查与业务模块路由组合。

use account::Account;
use api::{ApiError, with_http_layers};
use axum::{Json, Router, extract::State, routing::get};
use contracts::health::{HealthResponse, HealthStatus};
use sqlx::PgPool;

/// 服务端边界最终注入的应用 State。
///
/// 业务模块不会依赖该具体类型；每个模块先注入自己的 State，再返回可与
/// `Router<AppState>` 合并的路由。
#[derive(Clone)]
pub(crate) struct AppState {
    pool: PgPool,
}

impl AppState {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 合并健康检查和账号模块，并挂载统一 HTTP 中间件。
pub(crate) fn initialize(account: Account) -> Router<AppState> {
    with_http_layers(
        Router::new()
            .route("/health", get(health))
            .merge(account.routers::<AppState>()),
    )
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|error| {
            tracing::warn!(error = ?error, "PostgreSQL 健康检查失败");
            ApiError::service_unavailable("database_unavailable", "数据库暂时不可用")
        })?;
    Ok(Json(HealthResponse {
        status: HealthStatus::Ok,
    }))
}
