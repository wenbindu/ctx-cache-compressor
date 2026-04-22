use crate::error::AppResult;
use serde::Deserialize;
use std::{env, path::PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub compression: CompressionConfig,
    pub llm: LlmConfig,
    pub token_estimation: TokenEstimationConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub max_sessions: usize,
    pub session_ttl_seconds: u64,
    pub session_cleanup_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompressionConfig {
    pub every_n_turns: u32,
    pub keep_recent_turns: u32,
    pub llm_timeout_seconds: u64,
    pub max_retries: u32,
    pub warn_on_failure: bool,
    #[serde(default)]
    pub prompt: CompressionPromptConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenEstimationConfig {
    pub chars_per_token: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CompressionPromptConfig {
    pub system_instructions: String,
    pub user_prompt_template: String,
    pub language_instruction_template: String,
    pub chinese_label: String,
    pub english_label: String,
    pub enforce_dominant_language: bool,
}

impl Default for CompressionPromptConfig {
    fn default() -> Self {
        Self {
            system_instructions: [
                "你是一个对话历史压缩专家。",
                "你的任务是将给定对话压缩成高信息密度摘要，并保留关键语义。",
                "要求：",
                "1. 保留关键决策、重要事实、用户意图和阶段性结论。",
                "2. 保留重要工具调用结果与关键观察。",
                "3. 去除寒暄、重复解释、过渡性话术。",
                "4. 用客观第三人称组织内容，结构为：",
                "   [用户目标] -> [关键过程] -> [当前结论/状态]。",
                "5. 摘要长度控制在原文 20%-30%。",
                "6. 输出语言必须与原始对话主要语言一致：主要中文则输出中文，主要英文则输出英文。",
                "7. 仅输出摘要正文，不要附加前缀或解释。",
            ]
            .join("\n"),
            user_prompt_template:
                "以下是需要压缩的对话历史（共 {turn_count} 轮）：\n{serialized_messages}"
                    .to_string(),
            language_instruction_template:
                "本次对话主要语言判定为：{language_label}。请严格使用{language_label}输出。"
                    .to_string(),
            chinese_label: "中文".to_string(),
            english_label: "English".to_string(),
            enforce_dominant_language: true,
        }
    }
}

impl AppConfig {
    pub fn load() -> AppResult<Self> {
        let config_file = env::var("CTX_CACHE_COMPRESSOR_CONFIG_FILE")
            .ok()
            .or_else(|| env::var("CTX_COMPRESSOR_CONFIG_FILE").ok())
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty());

        let mut builder = config::Config::builder()
            .set_default("server.host", "0.0.0.0")?
            .set_default("server.port", 8080)?
            .set_default("server.max_sessions", 10_000)?
            .set_default("server.session_ttl_seconds", 3600)?
            .set_default("server.session_cleanup_interval_seconds", 60)?
            .set_default("compression.every_n_turns", 5)?
            .set_default("compression.keep_recent_turns", 2)?
            .set_default("compression.llm_timeout_seconds", 30)?
            .set_default("compression.max_retries", 1)?
            .set_default("compression.warn_on_failure", true)?
            .set_default("llm.base_url", "https://api.deepseek.com")?
            .set_default("llm.api_key", "")?
            .set_default("llm.model", "deepseek-chat")?
            .set_default("llm.max_tokens", 1024)?
            .set_default("llm.temperature", 0.3)?
            .set_default("token_estimation.chars_per_token", 3)?;

        if let Some(path) = config_file.as_ref() {
            builder = builder.add_source(config::File::from(PathBuf::from(path)).required(true));
        } else {
            builder = builder.add_source(config::File::with_name("config").required(false));
        }

        let cfg = builder
            .add_source(config::Environment::with_prefix("CTX_COMPRESSOR").separator("__"))
            .add_source(config::Environment::with_prefix("CTX_CACHE_COMPRESSOR").separator("__"))
            .build()?;

        let mut app = cfg.try_deserialize::<AppConfig>()?;

        if app.llm.api_key.trim().is_empty() {
            if let Ok(value) = env::var("OPENAI_API_KEY") {
                if !value.trim().is_empty() {
                    app.llm.api_key = value;
                }
            }
        }

        Ok(app)
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}
