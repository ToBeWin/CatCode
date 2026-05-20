use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::info;

use crate::session::{Session, SessionId, SessionState, SessionSummary};
use crate::session_manager::SessionManager;

/// Messages sent from the UI/API to a running agent session.
#[derive(Debug, Clone)]
pub enum SessionCommand {
    /// Send a user message to the agent.
    /// [`SendMessage`].
    SendMessage(String),
    /// Pause the agent.
    /// [`Pause`].
    Pause,
    /// Resume a paused agent.
    /// [`Resume`].
    Resume,
    /// Cancel the agent.
    /// [`Cancel`].
    Cancel,
}

/// Events emitted by a running agent session.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Agent produced text output.
    /// [`AgentMessage`].
    AgentMessage(String),
    /// Agent is calling a tool.
    /// [`ToolCall`].
    ToolCall {
        tool: String,
        args: serde_json::Value,
    },
    /// Tool execution completed.
    /// [`ToolResult`].
    ToolResult {
        tool: String,
        output: String,
        is_error: bool,
    },
    /// Agent finished processing.
    /// [`Completed`].
    Completed { response: String },
    /// Agent encountered an error.
    /// [`Error`].
    Error(String),
    /// Token usage update.
    /// [`TokenUpdate`].
    TokenUpdate { input: u64, output: u64, cache: u64 },
    /// Agent is waiting for input.
    /// [`WaitingForInput`].
    WaitingForInput,
}

/// Stored handle for a running session task.
struct RunningSession {
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    join_handle: tokio::task::JoinHandle<()>,
}

/// Receiver handle returned to the caller of spawn_session.
pub struct SessionHandle {
    /// Send commands to the agent.
    pub cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    /// Receive events from the agent.
    pub event_rx: mpsc::UnboundedReceiver<SessionEvent>,
    /// Session ID.
    pub session_id: SessionId,
}

/// Concurrent session manager that can run multiple agents in parallel.
///
/// Wraps the basic SessionManager and adds async task spawning.
pub struct ConcurrentSessionManager {
    /// Underlying session metadata manager.
    inner: Arc<RwLock<SessionManager>>,
    /// Handles to running sessions.
    running: Arc<RwLock<HashMap<SessionId, RunningSession>>>,
}

impl ConcurrentSessionManager {
    /// Create a new concurrent session manager.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SessionManager::new(max_concurrent))),
            running: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session and register it.
    ///
    /// Does NOT spawn the agent task — call `spawn()` for that.
    pub async fn create_session(
        &self,
        name: impl Into<String>,
        project_dir: std::path::PathBuf,
        model_id: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Result<SessionId, String> {
        let mut mgr = self.inner.write().await;
        mgr.create_session(name, project_dir, model_id, provider_id)
    }

    /// Spawn a session as an async task.
    ///
    /// Returns a SessionHandle with channels to communicate with the agent.
    pub async fn spawn_session<F, Fut>(
        &self,
        session_id: &str,
        agent_fn: F,
    ) -> Result<SessionHandle, String>
    where
        F: FnOnce(
                mpsc::UnboundedReceiver<SessionCommand>,
                mpsc::UnboundedSender<SessionEvent>,
            ) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Verify session exists
        {
            let mgr = self.inner.read().await;
            if mgr.get(session_id).is_none() {
                return Err(format!("Session not found: {}", session_id));
            }
        }

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let id = session_id.to_string();
        let join_handle = tokio::spawn(async move {
            agent_fn(cmd_rx, event_tx).await;
        });

        // Store the running session
        self.running.write().await.insert(
            id.clone(),
            RunningSession {
                cmd_tx: cmd_tx.clone(),
                join_handle,
            },
        );

        info!(session_id = %id, "Spawned session task");

        Ok(SessionHandle {
            cmd_tx,
            event_rx,
            session_id: id,
        })
    }

    /// Send a command to a running session.
    pub async fn send_command(&self, session_id: &str, cmd: SessionCommand) -> Result<(), String> {
        let running = self.running.read().await;
        let handle = running
            .get(session_id)
            .ok_or_else(|| format!("Session not running: {}", session_id))?;
        handle
            .cmd_tx
            .send(cmd)
            .map_err(|e| format!("Failed to send command: {}", e))
    }

    /// Get a session's metadata.
    pub async fn get_session(&self, id: &str) -> Option<Session> {
        let mgr = self.inner.read().await;
        mgr.get(id).cloned()
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let mgr = self.inner.read().await;
        mgr.list()
    }

    /// Update a session's state.
    pub async fn update_state(&self, id: &str, state: SessionState) -> Result<(), String> {
        let mut mgr = self.inner.write().await;
        mgr.update_state(id, state)
    }

    /// Stop a running session task.
    pub async fn stop_session(&self, id: &str) -> Result<(), String> {
        let mut running = self.running.write().await;
        if let Some(handle) = running.remove(id) {
            handle.join_handle.abort();
            info!(session_id = %id, "Stopped session task");
            Ok(())
        } else {
            Err(format!("Session not running: {}", id))
        }
    }

    /// Remove a completed/failed session.
    pub async fn remove_session(&self, id: &str) -> Result<(), String> {
        // Stop the task first if running
        let _ = self.stop_session(id).await;
        let mut mgr = self.inner.write().await;
        mgr.remove_session(id)
    }

    /// Get the number of active sessions.
    pub async fn active_count(&self) -> usize {
        let mgr = self.inner.read().await;
        mgr.active_count()
    }

    /// Check if a session is currently running as a task.
    pub async fn is_running(&self, id: &str) -> bool {
        self.running.read().await.contains_key(id)
    }

    /// Stop all running sessions.
    pub async fn stop_all(&self) {
        let mut running = self.running.write().await;
        for (id, handle) in running.drain() {
            handle.join_handle.abort();
            info!(session_id = %id, "Stopped session task");
        }
    }
}

impl Default for ConcurrentSessionManager {
    fn default() -> Self {
        Self::new(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_dir() -> std::path::PathBuf {
        std::path::PathBuf::from("/tmp/test-project")
    }

    #[tokio::test]
    async fn test_create_session() {
        let mgr = ConcurrentSessionManager::new(5);
        let id = mgr
            .create_session("test", project_dir(), "model", "provider")
            .await
            .unwrap();
        assert!(!id.is_empty());
        assert!(mgr.get_session(&id).await.is_some());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let mgr = ConcurrentSessionManager::new(5);
        mgr.create_session("s1", project_dir(), "m", "p")
            .await
            .unwrap();
        mgr.create_session("s2", project_dir(), "m", "p")
            .await
            .unwrap();

        let sessions = mgr.list_sessions().await;
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_update_state() {
        let mgr = ConcurrentSessionManager::new(5);
        let id = mgr
            .create_session("test", project_dir(), "m", "p")
            .await
            .unwrap();

        mgr.update_state(&id, SessionState::Paused).await.unwrap();
        let session = mgr.get_session(&id).await.unwrap();
        assert_eq!(session.state, SessionState::Paused);
    }

    #[tokio::test]
    async fn test_spawn_and_send_command() {
        let mgr = ConcurrentSessionManager::new(5);
        let id = mgr
            .create_session("test", project_dir(), "m", "p")
            .await
            .unwrap();

        // Spawn a simple agent that echoes messages
        let handle = mgr
            .spawn_session(
                &id,
                |mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
                 event_tx: mpsc::UnboundedSender<SessionEvent>| async move {
                    while let Some(cmd) = cmd_rx.recv().await {
                        match cmd {
                            SessionCommand::SendMessage(msg) => {
                                let _ = event_tx
                                    .send(SessionEvent::AgentMessage(format!("Echo: {}", msg)));
                                let _ = event_tx.send(SessionEvent::WaitingForInput);
                            }
                            SessionCommand::Cancel => break,
                            _ => {}
                        }
                    }
                },
            )
            .await;

        assert!(handle.is_ok());
        assert!(mgr.is_running(&id).await);

        mgr.stop_session(&id).await.unwrap();
        assert!(!mgr.is_running(&id).await);
    }

    #[tokio::test]
    async fn test_stop_all() {
        let mgr = ConcurrentSessionManager::new(5);
        let id1 = mgr
            .create_session("s1", project_dir(), "m", "p")
            .await
            .unwrap();
        let id2 = mgr
            .create_session("s2", project_dir(), "m", "p")
            .await
            .unwrap();

        let noop = |_rx: mpsc::UnboundedReceiver<SessionCommand>,
                    _tx: mpsc::UnboundedSender<SessionEvent>| async {};

        let _ = mgr.spawn_session(&id1, noop).await;
        let _ = mgr.spawn_session(&id2, noop).await;

        mgr.stop_all().await;
        assert!(!mgr.is_running(&id1).await);
        assert!(!mgr.is_running(&id2).await);
    }

    #[tokio::test]
    async fn test_remove_session() {
        let mgr = ConcurrentSessionManager::new(5);
        let id = mgr
            .create_session("test", project_dir(), "m", "p")
            .await
            .unwrap();

        mgr.update_state(&id, SessionState::Completed)
            .await
            .unwrap();
        mgr.remove_session(&id).await.unwrap();
        assert!(mgr.get_session(&id).await.is_none());
    }
}
