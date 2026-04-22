use std::sync::Arc;

use ctx_cache_compressor::{
    api::{routes::build_router, AppState},
    compression::{compressor::Compressor, scheduler::CompressionScheduler},
    config::AppConfig,
    llm::{
        client::{ChatLlm, CompressionLlm},
        runtime_client::RuntimeLlmClient,
    },
    runtime::DemoRuntimeConfig,
    session::store::SessionStore,
};
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Arc::new(AppConfig::load()?);
    let runtime = Arc::new(RwLock::new(DemoRuntimeConfig::from_app_config(&config)));

    let store = Arc::new(SessionStore::new(
        config.server.max_sessions,
        config.server.session_ttl_seconds,
    ));
    store
        .clone()
        .spawn_ttl_cleanup_with_interval(config.server.session_cleanup_interval_seconds);

    let llm_client = Arc::new(RuntimeLlmClient::new(runtime.clone()));
    let compression_llm: Arc<dyn CompressionLlm> = llm_client.clone();
    let chat_llm: Arc<dyn ChatLlm> = llm_client;
    let compressor = Arc::new(Compressor::new(
        compression_llm,
        config.compression.prompt.clone(),
    ));
    let scheduler = Arc::new(CompressionScheduler::new(
        runtime.clone(),
        compressor,
        config.compression.keep_recent_turns,
        config.compression.llm_timeout_seconds,
        config.compression.max_retries,
        config.compression.warn_on_failure,
    ));

    let state = AppState {
        config: config.clone(),
        runtime,
        store,
        scheduler,
        chat_llm,
    };

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(config.bind_addr()).await?;
    tracing::info!(address = %listener.local_addr()?, "ctx-cache-compressor started");

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}
