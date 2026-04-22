use crate::config::{AppConfig, LlmConfig};

#[derive(Debug, Clone)]
pub struct DemoRuntimeConfig {
    pub llm: LlmConfig,
    pub compression_every_n_turns: u32,
}

impl DemoRuntimeConfig {
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            llm: config.llm.clone(),
            compression_every_n_turns: config.compression.every_n_turns.max(1),
        }
    }

    pub fn llm_api_key_configured(&self) -> bool {
        !self.llm.api_key.trim().is_empty()
    }

    pub fn llm_api_key_preview(&self) -> String {
        if !self.llm_api_key_configured() {
            return "未配置".to_string();
        }

        let trimmed = self.llm.api_key.trim();
        let suffix_chars: Vec<char> = trimmed.chars().rev().take(4).collect();
        let suffix: String = suffix_chars.into_iter().rev().collect();
        format!("已配置 · ****{suffix}")
    }
}
