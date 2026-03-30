use std::collections::HashMap;
use std::path::PathBuf;

use atn_core::agent::{AgentConfig, AgentId};
use atn_core::error::{AtnError, Result};

use crate::session::PtySession;

/// Manages the lifecycle of multiple PTY agent sessions.
pub struct SessionManager {
    sessions: HashMap<AgentId, PtySession>,
    log_dir: Option<PathBuf>,
}

impl SessionManager {
    /// Create a new session manager.
    ///
    /// If `log_dir` is provided, each agent's transcript will be written to
    /// `{log_dir}/{agent_id}/`.
    pub fn new(log_dir: Option<PathBuf>) -> Self {
        Self {
            sessions: HashMap::new(),
            log_dir,
        }
    }

    /// Spawn a new agent session and return its ID.
    pub async fn spawn_agent(&mut self, config: AgentConfig) -> Result<AgentId> {
        let id = config.id.clone();
        let session = PtySession::spawn(&config, self.log_dir.clone()).await?;
        self.sessions.insert(id.clone(), session);
        Ok(id)
    }

    /// Get a reference to an agent's session.
    pub fn get_session(&self, id: &AgentId) -> Result<&PtySession> {
        self.sessions
            .get(id)
            .ok_or_else(|| AtnError::AgentNotFound(id.clone()))
    }

    /// Get a mutable reference to an agent's session.
    pub fn get_session_mut(&mut self, id: &AgentId) -> Result<&mut PtySession> {
        self.sessions
            .get_mut(id)
            .ok_or_else(|| AtnError::AgentNotFound(id.clone()))
    }

    /// Shut down a specific agent session and remove it from management.
    pub async fn shutdown_agent(&mut self, id: &AgentId) -> Result<()> {
        let mut session = self
            .sessions
            .remove(id)
            .ok_or_else(|| AtnError::AgentNotFound(id.clone()))?;
        session.shutdown().await
    }

    /// Shut down all agent sessions.
    pub async fn shutdown_all(&mut self) -> Result<()> {
        let ids: Vec<AgentId> = self.sessions.keys().cloned().collect();
        for id in ids {
            if let Some(mut session) = self.sessions.remove(&id) {
                let _ = session.shutdown().await;
            }
        }
        Ok(())
    }

    /// List all active agent IDs.
    pub fn agent_ids(&self) -> Vec<&AgentId> {
        self.sessions.keys().collect()
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether there are no active sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}
