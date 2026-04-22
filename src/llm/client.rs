use std::{future::Future, pin::Pin};

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use tracing::instrument;

use crate::{
    config::LlmConfig,
    error::{AppError, AppResult},
    llm::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage},
};

pub trait CompressionLlm: Send + Sync {
    fn compress<'a>(
        &'a self,
        system_prompt: &'a str,
        user_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>>;
}

pub trait ChatLlm: Send + Sync {
    fn complete<'a>(
        &'a self,
        messages: &'a [ChatMessage],
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    config: LlmConfig,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> AppResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !config.api_key.trim().is_empty() {
            let token = format!("Bearer {}", config.api_key);
            let value = HeaderValue::from_str(&token)
                .map_err(|err| AppError::Config(format!("invalid llm api_key: {err}")))?;
            headers.insert(AUTHORIZATION, value);
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self { http, config })
    }

    fn chat_completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    #[instrument(skip(self, messages))]
    async fn request_chat(&self, messages: &[ChatMessage]) -> AppResult<String> {
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: messages.to_vec(),
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let response = self
            .http
            .post(self.chat_completions_url())
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_else(|_| String::new());
            return Err(AppError::Upstream(format!(
                "chat completion failed with status {status}: {body}"
            )));
        }

        let body: ChatCompletionResponse = response.json().await?;
        body.choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| AppError::Upstream("empty completion response".to_string()))
    }

    #[instrument(skip(self, system_prompt, user_prompt))]
    async fn request_compression(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> AppResult<String> {
        self.request_chat(&[
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ])
        .await
    }
}

impl CompressionLlm for LlmClient {
    fn compress<'a>(
        &'a self,
        system_prompt: &'a str,
        user_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
        Box::pin(async move { self.request_compression(system_prompt, user_prompt).await })
    }
}

impl ChatLlm for LlmClient {
    fn complete<'a>(
        &'a self,
        messages: &'a [ChatMessage],
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
        Box::pin(async move { self.request_chat(messages).await })
    }
}
