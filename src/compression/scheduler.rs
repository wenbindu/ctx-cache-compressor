use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

use crate::{
    compression::compressor::{CompressionEvaluation, CompressionOutcome, Compressor},
    error::AppError,
    session::types::{CompressionEvaluationSnapshot, Message, Session, SessionTraceKind},
};

#[derive(Debug, Clone)]
pub struct CompressionSnapshot {
    pub messages: Vec<Message>,
    pub stable_revision: u64,
}

#[derive(Clone)]
pub struct CompressionScheduler {
    compressor: Arc<Compressor>,
    every_n_turns: u32,
    keep_recent_turns: u32,
    llm_timeout_seconds: u64,
    max_retries: u32,
    warn_on_failure: bool,
}

impl CompressionScheduler {
    pub fn new(
        compressor: Arc<Compressor>,
        every_n_turns: u32,
        keep_recent_turns: u32,
        llm_timeout_seconds: u64,
        max_retries: u32,
        warn_on_failure: bool,
    ) -> Self {
        Self {
            compressor,
            every_n_turns: every_n_turns.max(1),
            keep_recent_turns,
            llm_timeout_seconds,
            max_retries,
            warn_on_failure,
        }
    }

    pub fn schedule(&self, session: Arc<RwLock<Session>>, snapshot: CompressionSnapshot) {
        let cloned = self.clone();
        tokio::spawn(async move {
            cloned.compress_task(session, snapshot).await;
        });
    }

    #[instrument(skip(self, session, snapshot))]
    async fn compress_task(&self, session: Arc<RwLock<Session>>, snapshot: CompressionSnapshot) {
        let mut attempt = 0u32;
        let total_attempts = self.max_retries + 1;
        let timeout = Duration::from_secs(self.llm_timeout_seconds);
        let mut success: Option<CompressionOutcome> = None;
        let mut last_error: Option<AppError> = None;

        while attempt < total_attempts {
            attempt += 1;

            let result = tokio::time::timeout(
                timeout,
                self.compressor
                    .compress_snapshot(&snapshot.messages, self.keep_recent_turns),
            )
            .await;

            match result {
                Ok(Ok(outcome)) => {
                    success = Some(outcome);
                    break;
                }
                Ok(Err(err)) => {
                    last_error = Some(err);
                }
                Err(_) => {
                    last_error = Some(AppError::Timeout(format!(
                        "compression timeout after {} seconds",
                        self.llm_timeout_seconds
                    )));
                }
            }
        }

        let mut guard = session.write().await;

        if let Some(outcome) = success {
            if guard.stable_revision != snapshot.stable_revision {
                let drained_pending: Vec<_> = guard.pending.drain(..).collect();
                if !drained_pending.is_empty() {
                    guard.stable.extend(drained_pending);
                    guard.stable_revision = guard.stable_revision.saturating_add(1);
                }
                guard.next_compress_at = guard.turn_count.saturating_add(self.every_n_turns);
                guard.is_compressing.store(false, Ordering::SeqCst);
                guard.touch();
                guard.push_trace(
                    SessionTraceKind::CompressionFailed,
                    "compression result discarded because stable messages changed during compression",
                );
                info!(
                    session_id = %guard.id,
                    "compression result discarded because stable revision changed"
                );
                return;
            }

            let compressed_turns_delta = outcome.compressed_turns_delta;
            let evaluation = outcome.evaluation.clone();
            guard.last_compression_evaluation = Some(evaluation_snapshot(&evaluation));
            guard.stable = outcome.new_stable;
            let drained_pending: Vec<_> = guard.pending.drain(..).collect();
            guard.stable.extend(drained_pending);
            guard.stable_revision = guard.stable_revision.saturating_add(1);
            guard.compressed_turns = guard
                .compressed_turns
                .saturating_add(compressed_turns_delta);
            guard.next_compress_at = guard.turn_count.saturating_add(self.every_n_turns);
            guard.is_compressing.store(false, Ordering::SeqCst);
            guard.touch();
            guard.push_trace(
                SessionTraceKind::CompressionSucceeded,
                format!(
                    "compression succeeded; compressed {compressed_turns_delta} completed turns; prompt input {} messages/{} chars; compressed region {} messages/{} chars; stable output {} messages/{} chars; summary {} chars; retained {} recent messages; previous_summary={}",
                    evaluation.prompt_input_message_count,
                    evaluation.prompt_input_char_count,
                    evaluation.compressed_region_message_count,
                    evaluation.compressed_region_char_count,
                    evaluation.stable_output_message_count,
                    evaluation.stable_output_char_count,
                    evaluation.summary_char_count,
                    evaluation.retained_recent_message_count,
                    evaluation.previous_summary_included
                ),
            );
            info!(session_id = %guard.id, "compression succeeded");
            return;
        }

        let drained_pending: Vec<_> = guard.pending.drain(..).collect();
        if !drained_pending.is_empty() {
            guard.stable.extend(drained_pending);
            guard.stable_revision = guard.stable_revision.saturating_add(1);
        }
        guard.next_compress_at = guard.turn_count.saturating_add(self.every_n_turns);
        guard.is_compressing.store(false, Ordering::SeqCst);
        guard.touch();

        if let Some(err) = last_error {
            let err_message = err.to_string();
            guard.push_trace(
                SessionTraceKind::CompressionFailed,
                format!("compression failed with graceful degradation: {err_message}"),
            );
            if self.warn_on_failure {
                warn!(session_id = %guard.id, error = %err, "compression failed, degraded gracefully");
            } else {
                info!(session_id = %guard.id, error = %err, "compression failed with graceful degradation");
            }
        }
    }
}

fn evaluation_snapshot(evaluation: &CompressionEvaluation) -> CompressionEvaluationSnapshot {
    CompressionEvaluationSnapshot {
        prompt_input_message_count: evaluation.prompt_input_message_count,
        prompt_input_char_count: evaluation.prompt_input_char_count,
        compressed_region_message_count: evaluation.compressed_region_message_count,
        compressed_region_char_count: evaluation.compressed_region_char_count,
        stable_output_message_count: evaluation.stable_output_message_count,
        stable_output_char_count: evaluation.stable_output_char_count,
        summary_char_count: evaluation.summary_char_count,
        retained_recent_message_count: evaluation.retained_recent_message_count,
        previous_summary_included: evaluation.previous_summary_included,
    }
}
