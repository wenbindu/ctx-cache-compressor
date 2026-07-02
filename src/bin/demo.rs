use std::sync::Arc;

use ctx_cache_compressor::{
    api::routes::build_demo_router,
    config::AppConfig,
    service::{build_app_state, init_tracing, serve},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Arc::new(AppConfig::load()?);
    let state = build_app_state(config.clone())?;
    let app = build_demo_router(state);

    serve(config, app, "ctx-cache-compressor-demo").await
}
