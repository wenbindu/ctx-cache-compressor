use std::sync::atomic::Ordering;

use axum::{
    extract::{Path, State},
    Json,
};
use tracing::instrument;

use crate::{
    api::{
        dto::{AppendMessageRequest, AppendMessageResponse},
        AppState,
    },
    compression::trigger::should_trigger_compression,
    error::AppResult,
    session::{
        turn::is_at_turn_boundary,
        types::{Message, Role, SessionTraceKind},
        validator::validate_append,
    },
};

pub async fn append_message_to_session(
    state: &AppState,
    session_id: &str,
    incoming: Message,
) -> AppResult<AppendMessageResponse> {
    let every_n_turns = state.config.compression.every_n_turns.max(1);
    let session = state
        .store
        .get_or_create_with_id(session_id, every_n_turns)?;
    let mut snapshot_to_compress: Option<Vec<Message>> = None;

    let response = {
        let mut guard = session.write().await;
        guard.touch();

        let existing = guard.full_messages();
        validate_append(&existing, &incoming)?;

        let incoming_is_assistant_final = incoming.is_assistant_final();
        let trace_kind = match incoming.role {
            Role::System => SessionTraceKind::SystemMessageAppended,
            Role::User => SessionTraceKind::UserMessageAppended,
            Role::Assistant => SessionTraceKind::AssistantMessageAppended,
            Role::Tool => SessionTraceKind::ToolMessageAppended,
        };
        let trace_message = if guard.is_compressing.load(Ordering::Relaxed) {
            "message appended into pending buffer"
        } else {
            "message appended into stable buffer"
        };

        if guard.is_compressing.load(Ordering::Relaxed) {
            guard.pending.push(incoming);
        } else {
            guard.stable.push(incoming);
        }
        guard.push_trace(trace_kind, trace_message);

        let merged_view = guard.full_messages();
        let at_turn_boundary = is_at_turn_boundary(&merged_view);
        let turn_completed = incoming_is_assistant_final && at_turn_boundary;
        if turn_completed {
            guard.turn_count = guard.turn_count.saturating_add(1);
        }

        let mut compression_triggered = false;
        if should_trigger_compression(
            turn_completed,
            at_turn_boundary,
            guard.turn_count,
            guard.next_compress_at,
        ) && guard
            .is_compressing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            compression_triggered = true;
            let trace_message = format!(
                "compression triggered at turn {} with {} stable messages",
                guard.turn_count,
                guard.stable.len()
            );
            guard.push_trace(SessionTraceKind::CompressionTriggered, trace_message);
            snapshot_to_compress = Some(guard.stable.clone());
        }

        AppendMessageResponse {
            turn_count: guard.turn_count,
            message_count: guard.message_count(),
            compression_triggered,
        }
    };

    if let Some(snapshot) = snapshot_to_compress {
        state.scheduler.schedule(session.clone(), snapshot);
    }

    Ok(response)
}

#[instrument(skip(state, payload), fields(session_id = %session_id))]
pub async fn append_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<AppendMessageRequest>,
) -> AppResult<Json<AppendMessageResponse>> {
    let incoming: Message = payload.into();
    Ok(Json(
        append_message_to_session(&state, &session_id, incoming).await?,
    ))
}
