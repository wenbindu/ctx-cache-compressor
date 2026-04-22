use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

use crate::{
    compression::compressor::{CompressionOutcome, Compressor},
    error::AppError,
    runtime::DemoRuntimeConfig,
    session::types::{Session, SessionTraceKind},
};

#[derive(Clone)]
pub struct CompressionScheduler {
    runtime: Arc<RwLock<DemoRuntimeConfig>>,
    compressor: Arc<Compressor>,
    keep_recent_turns: u32,
    llm_timeout_seconds: u64,
    max_retries: u32,
    warn_on_failure: bool,
}

impl CompressionScheduler {
    pub fn new(
        runtime: Arc<RwLock<DemoRuntimeConfig>>,
        compressor: Arc<Compressor>,
        keep_recent_turns: u32,
        llm_timeout_seconds: u64,
        max_retries: u32,
        warn_on_failure: bool,
    ) -> Self {
        Self {
            runtime,
            compressor,
            keep_recent_turns,
            llm_timeout_seconds,
            max_retries,
            warn_on_failure,
        }
    }

    pub fn schedule(
        &self,
        session: Arc<RwLock<Session>>,
        snapshot: Vec<crate::session::types::Message>,
    ) {
        let cloned = self.clone();
        tokio::spawn(async move {
            cloned.compress_task(session, snapshot).await;
        });
    }

    #[instrument(skip(self, session, snapshot))]
    async fn compress_task(
        &self,
        session: Arc<RwLock<Session>>,
        snapshot: Vec<crate::session::types::Message>,
    ) {
        let mut attempt = 0u32;
        let total_attempts = self.max_retries + 1;
        let timeout = Duration::from_secs(self.llm_timeout_seconds);
        let mut success: Option<CompressionOutcome> = None;
        let mut last_error: Option<AppError> = None;
        let next_every_n_turns = {
            let guard = self.runtime.read().await;
            guard.compression_every_n_turns.max(1)
        };

        while attempt < total_attempts {
            attempt += 1;

            let result = tokio::time::timeout(
                timeout,
                self.compressor
                    .compress_snapshot(&snapshot, self.keep_recent_turns),
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
            let compressed_turns_delta = outcome.compressed_turns_delta;
            guard.stable = outcome.new_stable;
            let drained_pending: Vec<_> = guard.pending.drain(..).collect();
            guard.stable.extend(drained_pending);
            guard.compressed_turns = guard
                .compressed_turns
                .saturating_add(compressed_turns_delta);
            guard.next_compress_at = guard.turn_count.saturating_add(next_every_n_turns);
            guard.is_compressing.store(false, Ordering::SeqCst);
            guard.touch();
            guard.push_trace(
                SessionTraceKind::CompressionSucceeded,
                format!(
                    "compression succeeded; compressed {compressed_turns_delta} completed turns"
                ),
            );
            info!(session_id = %guard.id, "compression succeeded");
            return;
        }

        let drained_pending: Vec<_> = guard.pending.drain(..).collect();
        guard.stable.extend(drained_pending);
        guard.next_compress_at = guard.turn_count.saturating_add(next_every_n_turns);
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
