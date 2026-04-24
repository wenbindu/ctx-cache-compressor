use crate::config::{AppConfig, LlmConfig};

#[derive(Debug, Clone)]
pub struct DemoRuntimeConfig {
    pub conversation_llm: LlmConfig,
}

impl DemoRuntimeConfig {
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            conversation_llm: config.llm.clone(),
        }
    }

    pub fn conversation_llm_api_key_configured(&self) -> bool {
        !self.conversation_llm.api_key.trim().is_empty()
    }

    pub fn conversation_llm_api_key_preview(&self) -> String {
        if !self.conversation_llm_api_key_configured() {
            return "未配置".to_string();
        }

        let trimmed = self.conversation_llm.api_key.trim();
        let suffix_chars: Vec<char> = trimmed.chars().rev().take(4).collect();
        let suffix: String = suffix_chars.into_iter().rev().collect();
        format!("已配置 · ****{suffix}")
    }
}
