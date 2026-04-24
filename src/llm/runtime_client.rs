use std::{future::Future, pin::Pin, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    error::AppResult,
    llm::{
        client::{ChatLlm, CompressionLlm, LlmClient},
        types::ChatMessage,
    },
    runtime::DemoRuntimeConfig,
};

#[derive(Clone)]
pub struct RuntimeLlmClient {
    runtime: Arc<RwLock<DemoRuntimeConfig>>,
}

impl RuntimeLlmClient {
    pub fn new(runtime: Arc<RwLock<DemoRuntimeConfig>>) -> Self {
        Self { runtime }
    }
}

impl CompressionLlm for RuntimeLlmClient {
    fn compress<'a>(
        &'a self,
        system_prompt: &'a str,
        user_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
        Box::pin(async move {
            let llm_config = {
                let guard = self.runtime.read().await;
                guard.conversation_llm.clone()
            };
            let client = LlmClient::new(llm_config)?;
            client.compress(system_prompt, user_prompt).await
        })
    }
}

impl ChatLlm for RuntimeLlmClient {
    fn complete<'a>(
        &'a self,
        messages: &'a [ChatMessage],
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
        Box::pin(async move {
            let llm_config = {
                let guard = self.runtime.read().await;
                guard.conversation_llm.clone()
            };
            let client = LlmClient::new(llm_config)?;
            client.complete(messages).await
        })
    }
}
