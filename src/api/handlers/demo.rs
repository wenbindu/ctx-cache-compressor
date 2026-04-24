use std::time::Instant;

use axum::{extract::State, Json};
use tracing::instrument;

use crate::{
    api::{
        dto::{DemoChatRequest, DemoChatResponse, DemoConfigResponse, UpdateDemoConfigRequest},
        handlers::{append::append_message_to_session, fetch::fetch_context_response},
        AppState,
    },
    error::{AppError, AppResult},
    llm::types::ChatMessage,
    runtime::DemoRuntimeConfig,
    session::types::{Message, Role, SessionTraceKind},
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
    let assistant_message = match state.chat_llm.complete(&chat_messages).await {
        Ok(message) => message,
        Err(err) => {
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

    let assistant_append = match append_message_to_session(
        &state,
        &session_id,
        Message::text(Role::Assistant, assistant_message.clone()),
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
        Role::Assistant if !message.has_tool_calls() => "assistant",
        Role::Assistant | Role::Tool => return None,
    };

    let content = message.content_text().trim().to_string();
    if content.is_empty() {
        return None;
    }

    Some(ChatMessage {
        role: role.to_string(),
        content,
    })
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
