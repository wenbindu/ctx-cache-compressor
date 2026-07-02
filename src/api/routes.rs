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
        demo::{demo_chat, demo_complete, demo_config, demo_tool_call, update_demo_config},
        fetch::fetch_context,
        health::health,
        playground_example::playground_example,
        session::{create_session, delete_session, list_sessions},
    },
    AppState,
};

pub fn build_router(state: AppState) -> Router {
    let enable_demo_routes = state.config.server.enable_demo_routes;
    let mut router = core_routes();

    if enable_demo_routes {
        router = router.merge(demo_routes());
    }

    finalize_router(router, state)
}

pub fn build_api_router(state: AppState) -> Router {
    finalize_router(core_routes(), state)
}

pub fn build_demo_router(state: AppState) -> Router {
    finalize_router(core_routes().merge(demo_routes()), state)
}

fn core_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{session_id}", delete(delete_session))
        .route("/sessions/{session_id}/messages", post(append_message))
        .route("/sessions/{session_id}/context", get(fetch_context))
}

fn demo_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/ex/dashboard", get(dashboard))
        .route("/ex/playground", get(playground_example))
        .route("/compressor", get(ctx_cache_compressor_playground))
        .route("/demo/config", get(demo_config).patch(update_demo_config))
        .route("/demo/chat", post(demo_chat))
        .route("/demo/tool-call", post(demo_tool_call))
        .route("/demo/complete", post(demo_complete))
}

fn finalize_router(router: Router<AppState>, state: AppState) -> Router {
    let permissive_cors = state.config.server.permissive_cors;
    let mut router = router.layer(TraceLayer::new_for_http());

    if permissive_cors {
        router = router.layer(CorsLayer::permissive());
    }

    router.with_state(state)
}
