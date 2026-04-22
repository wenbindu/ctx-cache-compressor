use std::sync::Arc;

use tokio::sync::RwLock;

use crate::{
    compression::scheduler::CompressionScheduler,
    config::AppConfig,
    llm::client::ChatLlm,
    runtime::DemoRuntimeConfig,
    session::{store::SessionStore, types::Message},
};

pub mod dto;
pub mod handlers;
pub mod routes;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub runtime: Arc<RwLock<DemoRuntimeConfig>>,
    pub store: Arc<SessionStore>,
    pub scheduler: Arc<CompressionScheduler>,
    pub chat_llm: Arc<dyn ChatLlm>,
}

impl AppState {
    pub fn estimate_tokens(&self, messages: &[Message]) -> usize {
        let chars: usize = messages.iter().map(Message::estimated_char_len).sum();
        let denom = self.config.token_estimation.chars_per_token.max(1);
        chars.div_ceil(denom)
    }
}
