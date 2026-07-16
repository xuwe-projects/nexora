use axum::{Router, extract::State, http::StatusCode, routing::get};

use crate::AppState;

pub(crate) fn routers() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health(State(state): State<AppState>) -> StatusCode {
    match state.pool().acquire().await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
