use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    llm::types::ToolSpec,
    session::types::{Message, MessageContent, Role, SessionTraceEvent, ToolCall},
};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionListItemResponse {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub turn_count: u32,
    pub message_count: usize,
    pub is_compressing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionListItemResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppendMessageRequest {
    pub role: Role,
    #[serde(default)]
    pub content: Option<MessageContent>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl From<AppendMessageRequest> for Message {
    fn from(value: AppendMessageRequest) -> Self {
        Message {
            role: value.role,
            content: value.content,
            reasoning_content: value.reasoning_content,
            tool_calls: value.tool_calls,
            tool_call_id: value.tool_call_id,
            name: value.name,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppendMessageResponse {
    pub turn_count: u32,
    pub message_count: usize,
    pub compression_triggered: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FetchContextResponse {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub turn_count: u32,
    pub is_compressing: bool,
    pub compressed_turns: u32,
    pub token_estimate: usize,
    pub stable_message_count: usize,
    pub pending_message_count: usize,
    pub summary_message_count: usize,
    pub latest_summary_preview: Option<String>,
    pub last_compression_triggered_at: Option<DateTime<Utc>>,
    pub last_compression_finished_at: Option<DateTime<Utc>>,
    pub traces: Vec<SessionTraceEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub sessions: usize,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoConfigResponse {
    pub llm_model: String,
    pub llm_base_url: String,
    pub llm_api_key_configured: bool,
    pub llm_api_key_preview: String,
    pub conversation_llm_model: String,
    pub conversation_llm_base_url: String,
    pub conversation_llm_api_key_configured: bool,
    pub conversation_llm_api_key_preview: String,
    pub compression_every_n_turns: u32,
    pub keep_recent_turns: u32,
    pub llm_timeout_seconds: u64,
    pub max_retries: u32,
    pub session_ttl_seconds: u64,
    pub max_sessions: usize,
    pub default_system_prompt: String,
    pub recommended_poll_interval_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateDemoConfigRequest {
    #[serde(default)]
    #[serde(alias = "llm_base_url")]
    pub conversation_llm_base_url: Option<String>,
    #[serde(default)]
    #[serde(alias = "llm_api_key")]
    pub conversation_llm_api_key: Option<String>,
    #[serde(default)]
    #[serde(alias = "llm_model")]
    pub conversation_llm_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DemoChatRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub user_message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoChatResponse {
    pub session_id: String,
    pub assistant_message: String,
    pub completion_latency_ms: u128,
    pub user_append: AppendMessageResponse,
    pub assistant_append: AppendMessageResponse,
    pub context: FetchContextResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DemoCompleteRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoCompleteResponse {
    pub session_id: String,
    pub assistant_message: String,
    pub completion_latency_ms: u128,
    pub assistant_append: AppendMessageResponse,
    pub context: FetchContextResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DemoToolCallRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub user_message: String,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoToolCallResponse {
    pub session_id: String,
    pub tool_call_count: usize,
    pub user_append: AppendMessageResponse,
    pub assistant_append: AppendMessageResponse,
    pub context: FetchContextResponse,
}
