use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::atomic::Ordering;
use tracing::instrument;

use crate::{
    api::{
        dto::{
            CreateSessionRequest, CreateSessionResponse, ListSessionsResponse,
            SessionListItemResponse,
        },
        AppState,
    },
    error::AppResult,
};

#[instrument(skip(state, payload))]
pub async fn create_session(
    State(state): State<AppState>,
    payload: Option<Json<CreateSessionRequest>>,
) -> AppResult<Json<CreateSessionResponse>> {
    let system_prompt = payload.and_then(|json| json.0.system_prompt);
    let every_n_turns = {
        let guard = state.runtime.read().await;
        guard.compression_every_n_turns.max(1)
    };
    let (session_id, session) = state.store.create_session(every_n_turns, system_prompt)?;

    let created_at = {
        let guard = session.read().await;
        guard.created_at
    };

    Ok(Json(CreateSessionResponse {
        session_id,
        created_at,
    }))
}

#[instrument(skip(state))]
pub async fn list_sessions(State(state): State<AppState>) -> AppResult<Json<ListSessionsResponse>> {
    let snapshot = state
        .store
        .sessions
        .iter()
        .map(|entry| (entry.key().clone(), entry.value().clone()))
        .collect::<Vec<_>>();

    let mut sessions = Vec::with_capacity(snapshot.len());
    for (session_id, session) in snapshot {
        let guard = session.read().await;
        sessions.push(SessionListItemResponse {
            session_id,
            created_at: guard.created_at,
            last_accessed: guard.last_accessed,
            turn_count: guard.turn_count,
            message_count: guard.message_count(),
            is_compressing: guard.is_compressing.load(Ordering::Relaxed),
        });
    }

    sessions.sort_by(|left, right| {
        right
            .last_accessed
            .cmp(&left.last_accessed)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });

    Ok(Json(ListSessionsResponse { sessions }))
}

#[instrument(skip(state))]
pub async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> StatusCode {
    state.store.delete(&session_id);
    StatusCode::NO_CONTENT
}
