use std::{sync::Arc, time::Duration};

use chrono::Utc;
use dashmap::{mapref::entry::Entry, DashMap};
use tokio::sync::RwLock;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    session::types::Session,
};

#[derive(Debug)]
pub struct SessionStore {
    pub sessions: DashMap<String, Arc<RwLock<Session>>>,
    max_sessions: usize,
    ttl_seconds: u64,
}

impl SessionStore {
    pub fn new(max_sessions: usize, ttl_seconds: u64) -> Self {
        Self {
            sessions: DashMap::new(),
            max_sessions,
            ttl_seconds,
        }
    }

    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        every_n_turns: u32,
        system_prompt: Option<String>,
    ) -> AppResult<(String, Arc<RwLock<Session>>)> {
        if self.sessions.len() >= self.max_sessions {
            return Err(AppError::TooManySessions);
        }

        let session_id = Uuid::new_v4().to_string();
        let session = Arc::new(RwLock::new(Session::new(
            session_id.clone(),
            every_n_turns.max(1),
            system_prompt,
        )));
        self.sessions.insert(session_id.clone(), session.clone());

        Ok((session_id, session))
    }

    #[instrument(skip(self))]
    pub fn get(&self, session_id: &str) -> Option<Arc<RwLock<Session>>> {
        self.sessions
            .get(session_id)
            .map(|entry| entry.value().clone())
    }

    #[instrument(skip(self))]
    pub fn get_or_create_with_id(
        &self,
        session_id: &str,
        every_n_turns: u32,
    ) -> AppResult<Arc<RwLock<Session>>> {
        if let Some(existing) = self.sessions.get(session_id) {
            return Ok(existing.value().clone());
        }

        if self.sessions.len() >= self.max_sessions {
            return Err(AppError::TooManySessions);
        }

        let candidate = Arc::new(RwLock::new(Session::new(
            session_id.to_string(),
            every_n_turns.max(1),
            None,
        )));

        match self.sessions.entry(session_id.to_string()) {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => {
                entry.insert(candidate.clone());
                Ok(candidate)
            }
        }
    }

    #[instrument(skip(self))]
    pub fn delete(&self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn spawn_ttl_cleanup(self: Arc<Self>) {
        self.spawn_ttl_cleanup_with_interval(60);
    }

    pub fn spawn_ttl_cleanup_with_interval(self: Arc<Self>, interval_seconds: u64) {
        let ttl_seconds = self.ttl_seconds;
        let interval_seconds = interval_seconds.max(1);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));
            loop {
                interval.tick().await;

                let now = Utc::now();
                let snapshot: Vec<(String, Arc<RwLock<Session>>)> = self
                    .sessions
                    .iter()
                    .map(|entry| (entry.key().clone(), entry.value().clone()))
                    .collect();

                let mut expired = Vec::new();
                for (session_id, session) in snapshot {
                    let guard = session.read().await;
                    let idle_seconds = now.signed_duration_since(guard.last_accessed).num_seconds();

                    if idle_seconds >= ttl_seconds as i64 {
                        expired.push(session_id);
                    }
                }

                for session_id in expired {
                    self.sessions.remove(&session_id);
                }
            }
        });
    }
}
