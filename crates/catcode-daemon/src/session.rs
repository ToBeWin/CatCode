use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for a session.
/// [`SessionId`]
pub type SessionId = String;

/// Current state of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Current state of a session in its lifecycle.
pub enum SessionState {
    /// Agent is actively running.
    /// [`Running`].
    Running,
    /// Agent is paused, waiting for user input.
    /// [`Paused`].
    Paused,
    /// Task completed successfully.
    /// [`Completed`].
    Completed,
    /// Session failed with an error message.
    /// [`Failed`].
    Failed(String),
}

/// A single agent session.
///
/// Contains all metadata about a session: its identity, state, configuration,
/// and the project directory it operates in. The actual agent execution
/// state (messages, tool results) is managed separately by `AgentLoop`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub state: SessionState,
    pub project_dir: PathBuf,
    pub model_id: String,
    pub provider_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Total turns executed in this session.
    pub turn_count: u64,
}

impl Session {
    /// Create a new session with the given configuration.
    pub fn new(
        name: impl Into<String>,
        project_dir: PathBuf,
        model_id: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            state: SessionState::Running,
            project_dir,
            model_id: model_id.into(),
            provider_id: provider_id.into(),
            created_at: now,
            updated_at: now,
            turn_count: 0,
        }
    }

    /// Update the session state and timestamp.
    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
        self.updated_at = Utc::now();
    }

    /// Increment the turn counter.
    pub fn increment_turn(&mut self) {
        self.turn_count += 1;
        self.updated_at = Utc::now();
    }

    /// Check if the session is in a terminal state (Completed or Failed).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            SessionState::Completed | SessionState::Failed(_)
        )
    }
}

/// Summary of a session for listing purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub name: String,
    pub state: SessionState,
    pub model_id: String,
    pub turn_count: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Session> for SessionSummary {
    fn from(s: &Session) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            state: s.state.clone(),
            model_id: s.model_id.clone(),
            turn_count: s.turn_count,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session() -> Session {
        Session::new(
            "test-session",
            PathBuf::from("/tmp/project"),
            "deepseek-chat",
            "deepseek",
        )
    }

    #[test]
    fn test_session_new() {
        let s = make_session();
        assert!(!s.id.is_empty());
        assert_eq!(s.name, "test-session");
        assert_eq!(s.state, SessionState::Running);
        assert_eq!(s.project_dir, PathBuf::from("/tmp/project"));
        assert_eq!(s.model_id, "deepseek-chat");
        assert_eq!(s.provider_id, "deepseek");
        assert_eq!(s.turn_count, 0);
    }

    #[test]
    fn test_session_set_state() {
        let mut s = make_session();
        s.set_state(SessionState::Paused);
        assert_eq!(s.state, SessionState::Paused);

        s.set_state(SessionState::Completed);
        assert_eq!(s.state, SessionState::Completed);
    }

    #[test]
    fn test_session_increment_turn() {
        let mut s = make_session();
        assert_eq!(s.turn_count, 0);
        s.increment_turn();
        assert_eq!(s.turn_count, 1);
        s.increment_turn();
        assert_eq!(s.turn_count, 2);
    }

    #[test]
    fn test_session_is_terminal() {
        let mut s = make_session();
        assert!(!s.is_terminal());

        s.set_state(SessionState::Paused);
        assert!(!s.is_terminal());

        s.set_state(SessionState::Completed);
        assert!(s.is_terminal());

        s.set_state(SessionState::Failed("error".to_string()));
        assert!(s.is_terminal());
    }

    #[test]
    fn test_session_summary_from() {
        let s = make_session();
        let summary = SessionSummary::from(&s);
        assert_eq!(summary.id, s.id);
        assert_eq!(summary.name, "test-session");
        assert_eq!(summary.state, SessionState::Running);
    }

    #[test]
    fn test_session_serialization() {
        let s = make_session();
        let json = serde_json::to_string(&s).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, s.id);
        assert_eq!(parsed.name, s.name);
    }
}
