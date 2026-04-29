use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

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
    active_sessions: AtomicUsize,
}

impl SessionStore {
    pub fn new(max_sessions: usize, ttl_seconds: u64) -> Self {
        Self {
            sessions: DashMap::new(),
            max_sessions,
            ttl_seconds,
            active_sessions: AtomicUsize::new(0),
        }
    }

    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        every_n_turns: u32,
        system_prompt: Option<String>,
    ) -> AppResult<(String, Arc<RwLock<Session>>)> {
        loop {
            self.try_reserve_slot()?;

            let session_id = Uuid::new_v4().to_string();
            let session = Arc::new(RwLock::new(Session::new(
                session_id.clone(),
                every_n_turns.max(1),
                system_prompt.clone(),
            )));

            match self.sessions.entry(session_id.clone()) {
                Entry::Occupied(_) => self.release_slot(),
                Entry::Vacant(entry) => {
                    entry.insert(session.clone());
                    return Ok((session_id, session));
                }
            }
        }
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

        self.try_reserve_slot()?;

        let candidate = Arc::new(RwLock::new(Session::new(
            session_id.to_string(),
            every_n_turns.max(1),
            None,
        )));

        match self.sessions.entry(session_id.to_string()) {
            Entry::Occupied(entry) => {
                self.release_slot();
                Ok(entry.get().clone())
            }
            Entry::Vacant(entry) => {
                entry.insert(candidate.clone());
                Ok(candidate)
            }
        }
    }

    #[instrument(skip(self))]
    pub fn delete(&self, session_id: &str) -> bool {
        let removed = self.sessions.remove(session_id).is_some();
        if removed {
            self.release_slot();
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    fn try_reserve_slot(&self) -> AppResult<()> {
        let mut current = self.active_sessions.load(Ordering::Acquire);
        loop {
            if current >= self.max_sessions {
                return Err(AppError::TooManySessions);
            }

            match self.active_sessions.compare_exchange(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(next) => current = next,
            }
        }
    }

    fn release_slot(&self) {
        let mut current = self.active_sessions.load(Ordering::Acquire);
        while current > 0 {
            match self.active_sessions.compare_exchange(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
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
                    if self.sessions.remove(&session_id).is_some() {
                        self.release_slot();
                    }
                }
            }
        });
    }
}
