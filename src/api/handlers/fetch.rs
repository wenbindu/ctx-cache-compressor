use std::sync::atomic::Ordering;

use axum::{
    extract::{Path, State},
    Json,
};
use tracing::instrument;

use crate::{
    api::{dto::FetchContextResponse, AppState},
    error::{AppError, AppResult},
    session::types::{SessionTraceKind, CONTEXT_SUMMARY_PREFIX},
};

pub async fn fetch_context_response(
    state: &AppState,
    session_id: &str,
) -> AppResult<FetchContextResponse> {
    let session = state
        .store
        .get(session_id)
        .ok_or_else(|| AppError::NotFound(format!("session '{}' not found", session_id)))?;

    let (
        messages,
        traces,
        is_compressing,
        turn_count,
        compressed_turns,
        next_compress_at,
        stable_message_count,
        pending_message_count,
        last_compression_evaluation,
    ) = {
        let mut guard = session.write().await;
        guard.touch();
        let messages = guard.full_messages();

        (
            messages,
            guard.traces.clone(),
            guard.is_compressing.load(Ordering::Relaxed),
            guard.turn_count,
            guard.compressed_turns,
            guard.next_compress_at,
            guard.stable.len(),
            guard.pending.len(),
            guard.last_compression_evaluation.clone(),
        )
    };

    let token_estimate = state.estimate_tokens(&messages);
    let summary_messages: Vec<String> = messages
        .iter()
        .filter_map(|message| {
            let text = message.content_text();
            text.starts_with(CONTEXT_SUMMARY_PREFIX).then_some(text)
        })
        .collect();
    let latest_summary_preview = summary_messages
        .last()
        .map(|text| text.chars().take(240).collect::<String>());
    let last_compression_triggered_at = traces
        .iter()
        .rev()
        .find(|event| event.kind == SessionTraceKind::CompressionTriggered)
        .map(|event| event.at);
    let last_compression_finished_at = traces
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event.kind,
                SessionTraceKind::CompressionSucceeded | SessionTraceKind::CompressionFailed
            )
        })
        .map(|event| event.at);

    Ok(FetchContextResponse {
        session_id: session_id.to_string(),
        messages,
        turn_count,
        is_compressing,
        compressed_turns,
        next_compress_at,
        turns_until_compression: next_compress_at.saturating_sub(turn_count),
        token_estimate,
        stable_message_count,
        pending_message_count,
        summary_message_count: summary_messages.len(),
        latest_summary_preview,
        last_compression_evaluation,
        last_compression_triggered_at,
        last_compression_finished_at,
        traces,
    })
}

#[instrument(skip(state))]
pub async fn fetch_context(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> AppResult<Json<FetchContextResponse>> {
    Ok(Json(fetch_context_response(&state, &session_id).await?))
}
