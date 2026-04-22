use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::api::{
    handlers::{
        append::append_message,
        ctx_cache_compressor_playground::ctx_cache_compressor_playground,
        dashboard::dashboard,
        demo::{demo_chat, demo_config, update_demo_config},
        fetch::fetch_context,
        health::health,
        playground_example::playground_example,
        session::{create_session, delete_session, list_sessions},
    },
    AppState,
};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/ex/dashboard", get(dashboard))
        .route("/ex/playground", get(playground_example))
        .route("/compressor", get(ctx_cache_compressor_playground))
        .route("/health", get(health))
        .route("/demo/config", get(demo_config).patch(update_demo_config))
        .route("/demo/chat", post(demo_chat))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{session_id}", delete(delete_session))
        .route("/sessions/{session_id}/messages", post(append_message))
        .route("/sessions/{session_id}/context", get(fetch_context))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
