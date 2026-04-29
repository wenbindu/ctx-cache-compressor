use std::time::{Duration, Instant};

use axum::{extract::State, Json};
use tracing::instrument;

use crate::{
    api::{
        dto::{
            DemoChatRequest, DemoChatResponse, DemoCompleteRequest, DemoCompleteResponse,
            DemoConfigResponse, DemoToolCallRequest, DemoToolCallResponse, UpdateDemoConfigRequest,
        },
        handlers::{append::append_message_to_session, fetch::fetch_context_response},
        AppState,
    },
    error::{AppError, AppResult},
    llm::types::{ChatMessage, ChatMessageResponse},
    runtime::DemoRuntimeConfig,
    session::types::{Message, MessageContent, Role, SessionTraceKind},
};

const DASHBOARD_DEFAULT_SYSTEM_PROMPT: &str = "You are an observability-first assistant. Answer clearly, preserve key facts, and help the operator understand when compression changes the active context.";

pub async fn demo_config(State(state): State<AppState>) -> Json<DemoConfigResponse> {
    let runtime = {
        let guard = state.runtime.read().await;
        guard.clone()
    };

    Json(demo_config_response(&state, &runtime))
}

#[instrument(skip(state, payload))]
pub async fn update_demo_config(
    State(state): State<AppState>,
    Json(payload): Json<UpdateDemoConfigRequest>,
) -> AppResult<Json<DemoConfigResponse>> {
    {
        let mut guard = state.runtime.write().await;

        if let Some(base_url) = payload
            .conversation_llm_base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            guard.conversation_llm.base_url = base_url.to_string();
        }

        if let Some(api_key) = payload.conversation_llm_api_key {
            let trimmed = api_key.trim();
            if !trimmed.is_empty() {
                guard.conversation_llm.api_key = trimmed.to_string();
            }
        }

        if let Some(model) = payload
            .conversation_llm_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            guard.conversation_llm.model = model.to_string();
        }
    }

    let runtime = {
        let guard = state.runtime.read().await;
        guard.clone()
    };

    Ok(Json(demo_config_response(&state, &runtime)))
}

fn demo_config_response(state: &AppState, runtime: &DemoRuntimeConfig) -> DemoConfigResponse {
    DemoConfigResponse {
        llm_model: state.config.llm.model.clone(),
        llm_base_url: state.config.llm.base_url.clone(),
        llm_api_key_configured: !state.config.llm.api_key.trim().is_empty(),
        llm_api_key_preview: masked_api_key_preview(&state.config.llm.api_key),
        conversation_llm_model: runtime.conversation_llm.model.clone(),
        conversation_llm_base_url: runtime.conversation_llm.base_url.clone(),
        conversation_llm_api_key_configured: runtime.conversation_llm_api_key_configured(),
        conversation_llm_api_key_preview: runtime.conversation_llm_api_key_preview(),
        compression_every_n_turns: state.config.compression.every_n_turns.max(1),
        keep_recent_turns: state.config.compression.keep_recent_turns,
        llm_timeout_seconds: state.config.compression.llm_timeout_seconds,
        max_retries: state.config.compression.max_retries,
        session_ttl_seconds: state.config.server.session_ttl_seconds,
        max_sessions: state.config.server.max_sessions,
        default_system_prompt: DASHBOARD_DEFAULT_SYSTEM_PROMPT.to_string(),
        recommended_poll_interval_ms: 1200,
    }
}

#[instrument(skip(state, payload))]
pub async fn demo_chat(
    State(state): State<AppState>,
    Json(payload): Json<DemoChatRequest>,
) -> AppResult<Json<DemoChatResponse>> {
    let user_message = payload.user_message.trim().to_string();
    if user_message.is_empty() {
        return Err(AppError::BadRequest(
            "user_message cannot be empty".to_string(),
        ));
    }

    let requested_system_prompt = payload
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string());

    let session_id = if let Some(raw_session_id) = payload
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if state.store.get(raw_session_id).is_none() {
            return Err(AppError::NotFound(format!(
                "session '{}' not found",
                raw_session_id
            )));
        }

        if let Some(system_prompt) = requested_system_prompt.as_deref() {
            sync_demo_session_system_prompt(&state, raw_session_id, system_prompt).await?;
        }

        raw_session_id.to_string()
    } else {
        let system_prompt = requested_system_prompt
            .clone()
            .or_else(|| Some(DASHBOARD_DEFAULT_SYSTEM_PROMPT.to_string()));
        let every_n_turns = state.config.compression.every_n_turns.max(1);
        let (session_id, _) = state.store.create_session(every_n_turns, system_prompt)?;
        session_id
    };

    let user_append = append_message_to_session(
        &state,
        &session_id,
        Message::text(Role::User, user_message.clone()),
    )
    .await?;

    push_trace(
        &state,
        &session_id,
        SessionTraceKind::DemoChatStarted,
        "demo chat completion requested",
    )
    .await;

    let chat_messages = demo_chat_messages(&state, &session_id).await?;
    let started_at = Instant::now();
    let timeout = Duration::from_secs(state.config.compression.llm_timeout_seconds);
    let assistant_response =
        match tokio::time::timeout(timeout, state.chat_llm.complete(&chat_messages)).await {
            Ok(Ok(message)) => message,
            Ok(Err(err)) => {
                push_trace(
                    &state,
                    &session_id,
                    SessionTraceKind::DemoChatFailed,
                    format!("demo chat failed before assistant append: {err}"),
                )
                .await;
                return Err(err);
            }
            Err(_) => {
                let err = AppError::Timeout(format!(
                    "demo chat timeout after {} seconds",
                    state.config.compression.llm_timeout_seconds
                ));
                push_trace(
                    &state,
                    &session_id,
                    SessionTraceKind::DemoChatFailed,
                    format!("demo chat failed before assistant append: {err}"),
                )
                .await;
                return Err(err);
            }
        };
    let completion_latency_ms = started_at.elapsed().as_millis();
    let assistant_message = assistant_text(&assistant_response)?;

    let assistant_append = match append_message_to_session(
        &state,
        &session_id,
        assistant_response_to_message(assistant_response),
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            push_trace(
                &state,
                &session_id,
                SessionTraceKind::DemoChatFailed,
                format!("assistant append failed after completion: {err}"),
            )
            .await;
            return Err(err);
        }
    };

    push_trace(
        &state,
        &session_id,
        SessionTraceKind::DemoChatCompleted,
        format!("demo chat completed in {completion_latency_ms} ms"),
    )
    .await;

    let context = fetch_context_response(&state, &session_id).await?;

    Ok(Json(DemoChatResponse {
        session_id,
        assistant_message,
        completion_latency_ms,
        user_append,
        assistant_append,
        context,
    }))
}

#[instrument(skip(state, payload))]
pub async fn demo_complete(
    State(state): State<AppState>,
    Json(payload): Json<DemoCompleteRequest>,
) -> AppResult<Json<DemoCompleteResponse>> {
    let session_id = payload.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(AppError::BadRequest(
            "session_id cannot be empty".to_string(),
        ));
    }

    if state.store.get(&session_id).is_none() {
        return Err(AppError::NotFound(format!(
            "session '{}' not found",
            session_id
        )));
    }

    push_trace(
        &state,
        &session_id,
        SessionTraceKind::DemoChatStarted,
        "demo completion requested from current transcript",
    )
    .await;

    let chat_messages = demo_chat_messages(&state, &session_id).await?;
    let started_at = Instant::now();
    let timeout = Duration::from_secs(state.config.compression.llm_timeout_seconds);
    let assistant_response =
        match tokio::time::timeout(timeout, state.chat_llm.complete(&chat_messages)).await {
            Ok(Ok(message)) => message,
            Ok(Err(err)) => {
                push_trace(
                    &state,
                    &session_id,
                    SessionTraceKind::DemoChatFailed,
                    format!("demo completion failed before assistant append: {err}"),
                )
                .await;
                return Err(err);
            }
            Err(_) => {
                let err = AppError::Timeout(format!(
                    "demo completion timeout after {} seconds",
                    state.config.compression.llm_timeout_seconds
                ));
                push_trace(
                    &state,
                    &session_id,
                    SessionTraceKind::DemoChatFailed,
                    format!("demo completion failed before assistant append: {err}"),
                )
                .await;
                return Err(err);
            }
        };
    let completion_latency_ms = started_at.elapsed().as_millis();
    let assistant_message = assistant_text(&assistant_response)?;

    let assistant_append = match append_message_to_session(
        &state,
        &session_id,
        assistant_response_to_message(assistant_response),
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            push_trace(
                &state,
                &session_id,
                SessionTraceKind::DemoChatFailed,
                format!("assistant append failed after demo completion: {err}"),
            )
            .await;
            return Err(err);
        }
    };

    push_trace(
        &state,
        &session_id,
        SessionTraceKind::DemoChatCompleted,
        format!("demo completion finished in {completion_latency_ms} ms"),
    )
    .await;

    let context = fetch_context_response(&state, &session_id).await?;

    Ok(Json(DemoCompleteResponse {
        session_id,
        assistant_message,
        completion_latency_ms,
        assistant_append,
        context,
    }))
}

#[instrument(skip(state, payload))]
pub async fn demo_tool_call(
    State(state): State<AppState>,
    Json(payload): Json<DemoToolCallRequest>,
) -> AppResult<Json<DemoToolCallResponse>> {
    let user_message = payload.user_message.trim().to_string();
    if user_message.is_empty() {
        return Err(AppError::BadRequest(
            "user_message cannot be empty".to_string(),
        ));
    }
    if payload.tools.is_empty() {
        return Err(AppError::BadRequest("tools cannot be empty".to_string()));
    }

    let requested_system_prompt = payload
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string());

    let session_id = if let Some(raw_session_id) = payload
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if state.store.get(raw_session_id).is_none() {
            return Err(AppError::NotFound(format!(
                "session '{}' not found",
                raw_session_id
            )));
        }

        if let Some(system_prompt) = requested_system_prompt.as_deref() {
            sync_demo_session_system_prompt(&state, raw_session_id, system_prompt).await?;
        }

        raw_session_id.to_string()
    } else {
        let system_prompt = requested_system_prompt
            .clone()
            .or_else(|| Some(DASHBOARD_DEFAULT_SYSTEM_PROMPT.to_string()));
        let every_n_turns = state.config.compression.every_n_turns.max(1);
        let (session_id, _) = state.store.create_session(every_n_turns, system_prompt)?;
        session_id
    };

    push_trace(
        &state,
        &session_id,
        SessionTraceKind::DemoChatStarted,
        "demo tool-call completion requested",
    )
    .await;

    let mut chat_messages = match demo_chat_messages(&state, &session_id).await {
        Ok(messages) => messages,
        Err(AppError::Conflict(_)) => Vec::new(),
        Err(err) => return Err(err),
    };
    let pending_tool_call_ids = pending_tool_call_ids(&chat_messages);
    if !pending_tool_call_ids.is_empty() {
        return Err(AppError::Conflict(format!(
            "pending tool_call requires a matching role=tool result before continuing: {}",
            pending_tool_call_ids.join(", ")
        )));
    }

    chat_messages.push(ChatMessage {
        role: "user".to_string(),
        content: Some(user_message.clone()),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });
    let timeout = Duration::from_secs(state.config.compression.llm_timeout_seconds);
    let assistant_message = match tokio::time::timeout(
        timeout,
        state
            .chat_llm
            .complete_tool_call(&chat_messages, &payload.tools),
    )
    .await
    {
        Ok(Ok(message)) => message,
        Ok(Err(err)) => {
            push_trace(
                &state,
                &session_id,
                SessionTraceKind::DemoChatFailed,
                format!("demo tool-call completion failed before assistant append: {err}"),
            )
            .await;
            return Err(err);
        }
        Err(_) => {
            let err = AppError::Timeout(format!(
                "demo tool-call completion timeout after {} seconds",
                state.config.compression.llm_timeout_seconds
            ));
            push_trace(
                &state,
                &session_id,
                SessionTraceKind::DemoChatFailed,
                format!("demo tool-call completion failed before assistant append: {err}"),
            )
            .await;
            return Err(err);
        }
    };

    let tool_call_count = assistant_message
        .tool_calls
        .as_ref()
        .map(Vec::len)
        .unwrap_or_default();

    let user_append = append_message_to_session(
        &state,
        &session_id,
        Message::text(Role::User, user_message.clone()),
    )
    .await?;

    let assistant_append = append_message_to_session(
        &state,
        &session_id,
        assistant_response_to_message(assistant_message),
    )
    .await?;

    push_trace(
        &state,
        &session_id,
        SessionTraceKind::DemoChatCompleted,
        if tool_call_count > 0 {
            format!("demo tool-call completion returned {tool_call_count} tool call(s)")
        } else {
            "demo tool-call completion returned final assistant content".to_string()
        },
    )
    .await;

    let context = fetch_context_response(&state, &session_id).await?;

    Ok(Json(DemoToolCallResponse {
        session_id,
        tool_call_count,
        user_append,
        assistant_append,
        context,
    }))
}

#[instrument(skip(state, system_prompt))]
async fn sync_demo_session_system_prompt(
    state: &AppState,
    session_id: &str,
    system_prompt: &str,
) -> AppResult<()> {
    let session = state
        .store
        .get(session_id)
        .ok_or_else(|| AppError::NotFound(format!("session '{}' not found", session_id)))?;

    let mut guard = session.write().await;
    guard.touch();

    let already_synced = guard
        .stable
        .first()
        .map(|message| {
            message.role == Role::System
                && !message.is_context_summary()
                && message.content_text() == system_prompt
        })
        .unwrap_or(false);
    if already_synced {
        return Ok(());
    }

    let next_message = Message::text(Role::System, system_prompt.to_string());
    match guard.stable.first_mut() {
        Some(message) if message.role == Role::System && !message.is_context_summary() => {
            *message = next_message;
        }
        _ => guard.stable.insert(0, next_message),
    }

    guard.push_trace(
        SessionTraceKind::SystemMessageAppended,
        "demo system prompt synchronized into current session",
    );

    Ok(())
}

fn masked_api_key_preview(api_key: &str) -> String {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return "未配置".to_string();
    }

    let suffix_chars: Vec<char> = trimmed.chars().rev().take(4).collect();
    let suffix: String = suffix_chars.into_iter().rev().collect();
    format!("已配置 · ****{suffix}")
}

async fn demo_chat_messages(state: &AppState, session_id: &str) -> AppResult<Vec<ChatMessage>> {
    let session = state
        .store
        .get(session_id)
        .ok_or_else(|| AppError::NotFound(format!("session '{}' not found", session_id)))?;

    let messages = {
        let mut guard = session.write().await;
        guard.touch();
        guard.full_messages()
    };

    let chat_messages = messages
        .iter()
        .filter_map(message_to_chat_message)
        .collect::<Vec<_>>();

    if chat_messages.is_empty() {
        return Err(AppError::Conflict(
            "no textual messages available for demo chat completion".to_string(),
        ));
    }

    Ok(chat_messages)
}

fn message_to_chat_message(message: &Message) -> Option<ChatMessage> {
    let role = match message.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let content = message.content_text().trim().to_string();
    let has_tool_calls = message.has_tool_calls();
    if content.is_empty() && !has_tool_calls {
        return None;
    }

    Some(ChatMessage {
        role: role.to_string(),
        content: if content.is_empty() {
            None
        } else {
            Some(content)
        },
        reasoning_content: message.reasoning_content.clone(),
        tool_calls: message.tool_calls.clone(),
        tool_call_id: message.tool_call_id.clone(),
        name: message.name.clone(),
    })
}

fn assistant_text(response: &ChatMessageResponse) -> AppResult<String> {
    response
        .content
        .as_ref()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| AppError::Upstream("empty completion response".to_string()))
}

fn assistant_response_to_message(response: ChatMessageResponse) -> Message {
    Message {
        role: Role::Assistant,
        content: response
            .content
            .filter(|content| !content.trim().is_empty())
            .map(MessageContent::Text),
        reasoning_content: response
            .reasoning_content
            .filter(|content| !content.trim().is_empty()),
        tool_calls: response.tool_calls.filter(|calls| !calls.is_empty()),
        tool_call_id: None,
        name: None,
    }
}

fn pending_tool_call_ids(messages: &[ChatMessage]) -> Vec<String> {
    let resolved = messages
        .iter()
        .filter(|message| message.role == "tool")
        .filter_map(|message| message.tool_call_id.as_deref())
        .collect::<std::collections::HashSet<_>>();

    messages
        .iter()
        .filter(|message| message.role == "assistant")
        .filter_map(|message| message.tool_calls.as_ref())
        .flat_map(|tool_calls| tool_calls.iter())
        .filter(|tool_call| !resolved.contains(tool_call.id.as_str()))
        .map(|tool_call| tool_call.id.clone())
        .collect()
}

async fn push_trace(
    state: &AppState,
    session_id: &str,
    kind: SessionTraceKind,
    message: impl Into<String>,
) {
    let Some(session) = state.store.get(session_id) else {
        return;
    };

    let mut guard = session.write().await;
    guard.touch();
    guard.push_trace(kind, message);
}
