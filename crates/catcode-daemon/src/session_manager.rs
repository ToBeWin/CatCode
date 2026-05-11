use crate::session::{Session, SessionId, SessionState, SessionSummary};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

/// Manages multiple concurrent sessions.
///
/// The SessionManager is the central hub for creating, pausing, resuming,
/// and cancelling sessions. It enforces a maximum concurrent session limit
/// and provides session lookup by ID.
#[derive(Debug)]
pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
    max_concurrent: usize,
}

impl SessionManager {
    /// Create a new session manager with the given concurrency limit.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            max_concurrent,
        }
    }

    /// Create a new session.
    ///
    /// Returns an error if the maximum number of concurrent sessions
    /// (Running + Paused) has been reached.
    pub fn create_session(
        &mut self,
        name: impl Into<String>,
        project_dir: PathBuf,
        model_id: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Result<SessionId, String> {
        let active_count = self.sessions.values().filter(|s| !s.is_terminal()).count();

        if active_count >= self.max_concurrent {
            return Err(format!(
                "Maximum concurrent sessions ({}) reached. Close or complete existing sessions first.",
                self.max_concurrent
            ));
        }

        let session = Session::new(name, project_dir, model_id, provider_id);
        let id = session.id.clone();
        info!(session_id = %id, "Created new session");
        self.sessions.insert(id.clone(), session);
        Ok(id)
    }

    /// Get a reference to a session by ID.
    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    /// Get a mutable reference to a session by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    /// Update a session's state.
    pub fn update_state(&mut self, id: &str, state: SessionState) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| format!("Session not found: {}", id))?;
        debug!(session_id = %id, ?state, "Updating session state");
        session.set_state(state);
        Ok(())
    }

    /// Remove a session. Only terminal sessions can be removed.
    pub fn remove_session(&mut self, id: &str) -> Result<(), String> {
        let session = self
            .sessions
            .get(id)
            .ok_or_else(|| format!("Session not found: {}", id))?;

        if !session.is_terminal() {
            return Err(format!(
                "Cannot remove active session '{}'. Pause or complete it first.",
                id
            ));
        }

        self.sessions.remove(id);
        info!(session_id = %id, "Removed session");
        Ok(())
    }

    /// Force remove a session regardless of state.
    pub fn force_remove(&mut self, id: &str) -> Option<Session> {
        self.sessions.remove(id)
    }

    /// List summaries of all sessions.
    pub fn list(&self) -> Vec<SessionSummary> {
        self.sessions.values().map(SessionSummary::from).collect()
    }

    /// List only active (non-terminal) sessions.
    pub fn list_active(&self) -> Vec<SessionSummary> {
        self.sessions
            .values()
            .filter(|s| !s.is_terminal())
            .map(SessionSummary::from)
            .collect()
    }

    /// Get the number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.values().filter(|s| !s.is_terminal()).count()
    }

    /// Get the total number of sessions (including completed/failed).
    pub fn total_count(&self) -> usize {
        self.sessions.len()
    }

    /// Check if more sessions can be created.
    pub fn can_create(&self) -> bool {
        self.active_count() < self.max_concurrent
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> SessionManager {
        SessionManager::new(3)
    }

    fn project_dir() -> PathBuf {
        PathBuf::from("/tmp/test-project")
    }

    #[test]
    fn test_create_session() {
        let mut mgr = make_manager();
        let id = mgr
            .create_session("test", project_dir(), "model", "provider")
            .unwrap();
        assert!(!id.is_empty());
        assert_eq!(mgr.total_count(), 1);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_create_max_concurrent() {
        let mut mgr = make_manager();
        for i in 0..3 {
            mgr.create_session(format!("s{i}"), project_dir(), "m", "p")
                .unwrap();
        }

        let result = mgr.create_session("overflow", project_dir(), "m", "p");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum concurrent"));
    }

    #[test]
    fn test_create_after_completion() {
        let mut mgr = SessionManager::new(1);
        let id = mgr.create_session("s1", project_dir(), "m", "p").unwrap();

        // Complete the session
        mgr.update_state(&id, SessionState::Completed).unwrap();
        assert_eq!(mgr.active_count(), 0);

        // Now we can create a new one
        let id2 = mgr.create_session("s2", project_dir(), "m", "p").unwrap();
        assert_eq!(mgr.total_count(), 2);
        assert_eq!(mgr.active_count(), 1);
        assert_ne!(id, id2);
    }

    #[test]
    fn test_update_state() {
        let mut mgr = make_manager();
        let id = mgr.create_session("test", project_dir(), "m", "p").unwrap();

        mgr.update_state(&id, SessionState::Paused).unwrap();
        let session = mgr.get(&id).unwrap();
        assert_eq!(session.state, SessionState::Paused);
    }

    #[test]
    fn test_update_state_not_found() {
        let mut mgr = make_manager();
        let result = mgr.update_state("nonexistent", SessionState::Paused);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_terminal_session() {
        let mut mgr = make_manager();
        let id = mgr.create_session("test", project_dir(), "m", "p").unwrap();
        mgr.update_state(&id, SessionState::Completed).unwrap();

        mgr.remove_session(&id).unwrap();
        assert_eq!(mgr.total_count(), 0);
    }

    #[test]
    fn test_remove_active_session_fails() {
        let mut mgr = make_manager();
        let id = mgr.create_session("test", project_dir(), "m", "p").unwrap();

        let result = mgr.remove_session(&id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot remove active"));
    }

    #[test]
    fn test_force_remove() {
        let mut mgr = make_manager();
        let id = mgr.create_session("test", project_dir(), "m", "p").unwrap();

        let removed = mgr.force_remove(&id);
        assert!(removed.is_some());
        assert_eq!(mgr.total_count(), 0);
    }

    #[test]
    fn test_list_and_list_active() {
        let mut mgr = make_manager();
        let id1 = mgr.create_session("s1", project_dir(), "m", "p").unwrap();
        let _id2 = mgr.create_session("s2", project_dir(), "m", "p").unwrap();
        mgr.update_state(&id1, SessionState::Completed).unwrap();

        assert_eq!(mgr.list().len(), 2);
        assert_eq!(mgr.list_active().len(), 1);
    }

    #[test]
    fn test_can_create() {
        let mut mgr = SessionManager::new(1);
        assert!(mgr.can_create());
        mgr.create_session("s", project_dir(), "m", "p").unwrap();
        assert!(!mgr.can_create());
    }

    #[test]
    fn test_get_and_get_mut() {
        let mut mgr = make_manager();
        let id = mgr.create_session("test", project_dir(), "m", "p").unwrap();

        assert!(mgr.get(&id).is_some());
        assert!(mgr.get("nonexistent").is_none());

        let session = mgr.get_mut(&id).unwrap();
        session.increment_turn();
        assert_eq!(mgr.get(&id).unwrap().turn_count, 1);
    }
}
