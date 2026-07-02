use std::sync::Arc;

use axum::Router;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    api::AppState,
    compression::{compressor::Compressor, scheduler::CompressionScheduler},
    config::AppConfig,
    error::AppResult,
    llm::{
        client::{ChatLlm, CompressionLlm, LlmClient},
        runtime_client::RuntimeLlmClient,
    },
    runtime::DemoRuntimeConfig,
    session::store::SessionStore,
};

pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

pub fn build_app_state(config: Arc<AppConfig>) -> AppResult<AppState> {
    let runtime = Arc::new(RwLock::new(DemoRuntimeConfig::from_app_config(&config)));

    let store = Arc::new(SessionStore::new(
        config.server.max_sessions,
        config.server.session_ttl_seconds,
    ));
    store
        .clone()
        .spawn_ttl_cleanup_with_interval(config.server.session_cleanup_interval_seconds);

    let chat_runtime_llm = Arc::new(RuntimeLlmClient::new(runtime.clone()));
    let compression_client = Arc::new(LlmClient::new(config.llm.clone())?);
    let compression_llm: Arc<dyn CompressionLlm> = compression_client;
    let chat_llm: Arc<dyn ChatLlm> = chat_runtime_llm;
    let compressor = Arc::new(Compressor::new(
        compression_llm,
        config.compression.prompt.clone(),
    ));
    let scheduler = Arc::new(CompressionScheduler::new(
        compressor,
        config.compression.every_n_turns,
        config.compression.keep_recent_turns,
        config.compression.llm_timeout_seconds,
        config.compression.max_retries,
        config.compression.warn_on_failure,
    ));

    Ok(AppState {
        config,
        runtime,
        store,
        scheduler,
        chat_llm,
    })
}

pub async fn serve(config: Arc<AppConfig>, app: Router, service_name: &str) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind_addr()).await?;
    tracing::info!(
        service = service_name,
        address = %listener.local_addr()?,
        "ctx-cache-compressor started"
    );

    axum::serve(listener, app).await?;

    Ok(())
}
