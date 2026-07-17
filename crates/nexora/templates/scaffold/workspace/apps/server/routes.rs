use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::PgPool;

pub(crate) fn routers() -> Router<PgPool> {
    Router::new().route("/health", get(health))
}

async fn health(State(pool): State<PgPool>) -> StatusCode {
    match pool.acquire().await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
