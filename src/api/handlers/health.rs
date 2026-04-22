use axum::{extract::State, Json};
use tracing::instrument;

use crate::api::{dto::HealthResponse, AppState};

#[instrument(skip(state))]
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        sessions: state.store.len(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
