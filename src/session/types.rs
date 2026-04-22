use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const CONTEXT_SUMMARY_PREFIX: &str = "[CONTEXT SUMMARY]";
const MAX_TRACE_EVENTS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(MessageContent::Text(content.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn system_summary(summary: impl Into<String>) -> Self {
        Self::text(
            Role::System,
            format!("{CONTEXT_SUMMARY_PREFIX}\n{}", summary.into()),
        )
    }

    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .as_ref()
            .map(|calls| !calls.is_empty())
            .unwrap_or(false)
    }

    pub fn is_assistant_final(&self) -> bool {
        self.role == Role::Assistant && !self.has_tool_calls()
    }

    pub fn is_context_summary(&self) -> bool {
        self.role == Role::System && self.content_text().starts_with(CONTEXT_SUMMARY_PREFIX)
    }

    pub fn content_text(&self) -> String {
        match self.content.as_ref() {
            Some(MessageContent::Text(text)) => text.clone(),
            Some(MessageContent::Parts(parts)) => parts
                .iter()
                .filter_map(|part| part.text.clone())
                .collect::<Vec<String>>()
                .join("\n"),
            None => String::new(),
        }
    }

    pub fn estimated_char_len(&self) -> usize {
        let mut total = self.content_text().chars().count();

        if let Some(tool_calls) = self.tool_calls.as_ref() {
            for call in tool_calls {
                total += call.id.chars().count();
                total += call.call_type.chars().count();
                total += call.function.name.chars().count();
                total += call.function.arguments.chars().count();
            }
        }

        if let Some(tool_call_id) = self.tool_call_id.as_ref() {
            total += tool_call_id.chars().count();
        }

        if let Some(name) = self.name.as_ref() {
            total += name.chars().count();
        }

        total
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTraceKind {
    SessionCreated,
    SystemMessageAppended,
    UserMessageAppended,
    AssistantMessageAppended,
    ToolMessageAppended,
    CompressionTriggered,
    CompressionSucceeded,
    CompressionFailed,
    DemoChatStarted,
    DemoChatCompleted,
    DemoChatFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTraceEvent {
    pub at: DateTime<Utc>,
    pub kind: SessionTraceKind,
    pub message: String,
    pub turn_count: u32,
    pub stable_count: usize,
    pub pending_count: usize,
}

#[derive(Debug)]
pub struct Session {
    pub id: String,
    pub stable: Vec<Message>,
    pub pending: Vec<Message>,
    pub traces: Vec<SessionTraceEvent>,
    pub is_compressing: AtomicBool,
    pub turn_count: u32,
    pub compressed_turns: u32,
    pub next_compress_at: u32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
}

impl Session {
    pub fn new(id: String, every_n_turns: u32, system_prompt: Option<String>) -> Self {
        let now = Utc::now();
        let mut stable = Vec::new();

        if let Some(prompt) = system_prompt {
            stable.push(Message::text(Role::System, prompt));
        }

        let mut session = Self {
            id,
            stable,
            pending: Vec::new(),
            traces: Vec::new(),
            is_compressing: AtomicBool::new(false),
            turn_count: 0,
            compressed_turns: 0,
            next_compress_at: every_n_turns.max(1),
            created_at: now,
            last_accessed: now,
        };
        session.push_trace(SessionTraceKind::SessionCreated, "session created");
        session
    }

    pub fn full_messages(&self) -> Vec<Message> {
        let mut messages = self.stable.clone();
        messages.extend(self.pending.clone());
        messages
    }

    pub fn message_count(&self) -> usize {
        self.stable.len() + self.pending.len()
    }

    pub fn touch(&mut self) {
        self.last_accessed = Utc::now();
    }

    pub fn push_trace(&mut self, kind: SessionTraceKind, message: impl Into<String>) {
        self.traces.push(SessionTraceEvent {
            at: Utc::now(),
            kind,
            message: message.into(),
            turn_count: self.turn_count,
            stable_count: self.stable.len(),
            pending_count: self.pending.len(),
        });

        if self.traces.len() > MAX_TRACE_EVENTS {
            let overflow = self.traces.len() - MAX_TRACE_EVENTS;
            self.traces.drain(0..overflow);
        }
    }
}
