//! # catcode-api
//!
//! HTTP/WebSocket API layer for the CatCode AI coding agent.
//!
//! Provides:
//! - REST API for session management, messaging, and system control
//! - SSE (Server-Sent Events) for real-time event streaming
//! - WebSocket for bidirectional communication
//! - Authentication middleware (local-only and token-based)

/// The `auth` module.
pub mod auth;
/// The `routes` module.
pub mod routes;
/// The `sse` module.
pub mod sse;
/// The `ws` module.
pub mod ws;

use async_trait::async_trait;
use axum::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

/// Shared application state for the API server.
#[derive(Clone)]
pub struct AppState {
    /// Broadcast channel for SSE events.
    pub event_tx: broadcast::Sender<ApiEvent>,
    /// Authentication configuration.
    pub auth: auth::AuthConfig,
    /// In-memory session state shared by API routes.
    pub sessions: SharedSessions,
    /// Optional message execution backend injected by the daemon.
    pub runner: Option<Arc<dyn MessageRunner>>,
    /// Optional persistent session store injected by the daemon.
    pub store: Option<Arc<dyn SessionStore>>,
    /// Optional repository harness planner injected by the daemon.
    pub harness_planner: Option<Arc<dyn HarnessPlanner>>,
    /// Optional workspace changes provider injected by the daemon.
    pub changes_provider: Option<Arc<dyn WorkspaceChangesProvider>>,
    /// Optional code review provider injected by the daemon.
    pub review_provider: Option<Arc<dyn CodeReviewProvider>>,
    /// Optional final handoff provider injected by the daemon.
    pub handoff_provider: Option<Arc<dyn HandoffProvider>>,
}

/// Shared session map used by REST routes.
pub type SharedSessions = Arc<RwLock<HashMap<String, ApiSession>>>;

/// API-visible session state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiSession {
    pub id: String,
    pub name: String,
    pub state: String,
    pub project_dir: String,
    pub model_id: String,
    pub provider_id: String,
    pub turn_count: u64,
}

/// Result produced by a message runner.
#[derive(Debug, Clone)]
pub struct RunMessageResult {
    pub response: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
}

/// API-visible audit log entry for mutating tool operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AuditLogEntry {
    pub id: i64,
    pub session_id: String,
    pub operation: String,
    pub tool: Option<String>,
    pub args: Option<String>,
    pub level: String,
    pub approved_by: Option<String>,
    pub result: String,
    pub created_at: String,
}

/// API-visible persisted message entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct MessageEntry {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub token_count: Option<i64>,
    pub created_at: String,
}

/// API-visible aggregated token usage for a session.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct UsageSummary {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
}

/// API-visible recovery plan for a session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RecoveryPlan {
    pub session_id: String,
    pub state: String,
    pub failure_reason: Option<String>,
    pub summary: String,
    pub next_steps: Vec<String>,
    pub recent_messages: Vec<MessageEntry>,
    pub usage: UsageSummary,
}

/// API-visible repository profile used by harness planning.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiRepoProfile {
    pub has_git: bool,
    pub language_stack: Vec<String>,
    pub package_managers: Vec<String>,
    pub test_commands: Vec<String>,
    pub important_files: Vec<String>,
}

/// API-visible coding harness plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiHarnessPlan {
    pub task_summary: String,
    pub phases: Vec<String>,
    pub repo: ApiRepoProfile,
    pub verification: ApiVerificationPlan,
    pub instructions: Vec<String>,
}

/// API-visible verification command selected by the harness.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiVerificationCommand {
    pub command: String,
    pub reason: String,
    pub auto_run: bool,
}

/// API-visible verification plan selected by the harness.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiVerificationPlan {
    pub commands: Vec<ApiVerificationCommand>,
    pub safety_note: String,
}

/// API-visible workspace changes summary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiWorkspaceChanges {
    pub project_dir: String,
    pub clean: bool,
    pub changed_files: Vec<String>,
    pub summary: String,
}

/// API-visible review finding.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiReviewFinding {
    pub severity: String,
    pub category: String,
    pub file: String,
    pub line: Option<u64>,
    pub title: String,
    pub description: String,
    pub suggestion: Option<String>,
}

/// API-visible code review result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiCodeReview {
    pub title: String,
    pub summary: String,
    pub files_reviewed: Vec<String>,
    pub findings: Vec<ApiReviewFinding>,
    pub positive_notes: Vec<String>,
    pub overall_score: u8,
}

/// API-visible verification run result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiVerificationRunResult {
    pub command: String,
    pub success: bool,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub diagnostics: Option<ApiVerificationDiagnostic>,
    pub repair_plan: Option<ApiVerificationRepairPlan>,
}

/// API-visible actionable verification failure diagnostic.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiVerificationDiagnostic {
    pub summary: String,
    pub locations: Vec<String>,
    pub suggestions: Vec<String>,
}

/// API-visible verification repair plan for the next coding turn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiVerificationRepairPlan {
    pub summary: String,
    pub files_to_inspect: Vec<String>,
    pub steps: Vec<String>,
    pub verification_command: String,
}

/// API-visible final handoff report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ApiHandoffReport {
    pub project_dir: String,
    pub task_summary: String,
    pub changes: ApiWorkspaceChanges,
    pub review: ApiCodeReview,
    pub verification: Option<ApiVerificationRunResult>,
    pub ready: bool,
    pub blockers: Vec<String>,
    pub recommendations: Vec<String>,
}

/// Backend that executes a user message for a session.
#[async_trait]
pub trait MessageRunner: Send + Sync {
    async fn run_message(
        &self,
        session: ApiSession,
        message: String,
    ) -> anyhow::Result<RunMessageResult>;
}

/// Backend that builds a repository harness plan.
#[async_trait]
pub trait HarnessPlanner: Send + Sync {
    async fn build_harness_plan(
        &self,
        project_dir: &Path,
        task: &str,
    ) -> anyhow::Result<ApiHarnessPlan>;
}

/// Backend that summarizes current working tree changes.
#[async_trait]
pub trait WorkspaceChangesProvider: Send + Sync {
    async fn workspace_changes(&self, project_dir: &Path) -> anyhow::Result<ApiWorkspaceChanges>;
}

/// Backend that reviews current workspace changes.
#[async_trait]
pub trait CodeReviewProvider: Send + Sync {
    async fn review_workspace(&self, project_dir: &Path) -> anyhow::Result<ApiCodeReview>;
}

/// Backend that runs the final handoff gate for current changes.
#[async_trait]
pub trait HandoffProvider: Send + Sync {
    async fn run_handoff(&self, project_dir: &Path, task: &str)
    -> anyhow::Result<ApiHandoffReport>;
}

/// Persistence backend for API session state.
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn list_sessions(&self) -> anyhow::Result<Vec<ApiSession>>;
    async fn get_session(&self, id: &str) -> anyhow::Result<Option<ApiSession>>;
    async fn upsert_session(&self, session: ApiSession) -> anyhow::Result<()>;
    async fn delete_session(&self, id: &str) -> anyhow::Result<()>;
    async fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        token_count: Option<i64>,
    ) -> anyhow::Result<()>;
    async fn list_messages(&self, session_id: &str) -> anyhow::Result<Vec<MessageEntry>>;
    async fn record_token_usage(
        &self,
        session: &ApiSession,
        input_tokens: u64,
        output_tokens: u64,
        cache_tokens: u64,
    ) -> anyhow::Result<()>;
    async fn get_usage(&self, session_id: &str) -> anyhow::Result<UsageSummary>;
    async fn list_audit_log(&self, session_id: &str) -> anyhow::Result<Vec<AuditLogEntry>>;
}

impl AppState {
    /// Create app state with an empty in-memory session store.
    pub fn new(event_tx: broadcast::Sender<ApiEvent>, auth: auth::AuthConfig) -> Self {
        Self {
            event_tx,
            auth,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            runner: None,
            store: None,
            harness_planner: None,
            changes_provider: None,
            review_provider: None,
            handoff_provider: None,
        }
    }

    /// Attach a message runner to execute API messages.
    pub fn with_runner(mut self, runner: Arc<dyn MessageRunner>) -> Self {
        self.runner = Some(runner);
        self
    }

    /// Attach a persistent session store.
    pub fn with_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Attach a repository harness planner.
    pub fn with_harness_planner(mut self, harness_planner: Arc<dyn HarnessPlanner>) -> Self {
        self.harness_planner = Some(harness_planner);
        self
    }

    /// Attach a workspace changes provider.
    pub fn with_changes_provider(
        mut self,
        changes_provider: Arc<dyn WorkspaceChangesProvider>,
    ) -> Self {
        self.changes_provider = Some(changes_provider);
        self
    }

    /// Attach a code review provider.
    pub fn with_review_provider(mut self, review_provider: Arc<dyn CodeReviewProvider>) -> Self {
        self.review_provider = Some(review_provider);
        self
    }

    /// Attach a final handoff provider.
    pub fn with_handoff_provider(mut self, handoff_provider: Arc<dyn HandoffProvider>) -> Self {
        self.handoff_provider = Some(handoff_provider);
        self
    }
}

/// Events broadcast to SSE clients.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
/// Events broadcast to SSE/WebSocket clients.
pub enum ApiEvent {
    #[serde(rename = "session_created")]
    /// [`SessionCreated`].
    SessionCreated { session_id: String, name: String },
    #[serde(rename = "session_state")]
    /// [`SessionState`].
    SessionState { session_id: String, state: String },
    #[serde(rename = "agent_message")]
    /// [`AgentMessage`].
    AgentMessage { session_id: String, content: String },
    #[serde(rename = "tool_call")]
    /// [`ToolCall`].
    ToolCall {
        session_id: String,
        tool: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    /// [`ToolResult`].
    ToolResult {
        session_id: String,
        tool: String,
        output: String,
        is_error: bool,
    },
    #[serde(rename = "token_usage")]
    /// [`TokenUsage`].
    TokenUsage {
        session_id: String,
        input: u64,
        output: u64,
        cache: u64,
        cost_usd: f64,
    },
    #[serde(rename = "error")]
    Error {
        session_id: Option<String>,
        error: String,
    },
    #[serde(rename = "health")]
    Health { status: String, version: String },
}

/// Build the axum router with all API routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::session_routes())
        .merge(routes::system_routes())
        .merge(sse::sse_routes())
        .merge(ws::ws_routes())
        .with_state(state)
        .layer(tower_http::cors::CorsLayer::permissive())
}

/// Start the API server on the given address.
pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let app = build_router(state);

    tracing::info!(%addr, "Starting API server");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    fn test_state() -> AppState {
        let (tx, _) = broadcast::channel(100);
        AppState::new(
            tx,
            auth::AuthConfig {
                mode: auth::AuthMode::LocalOnly,
                token: None,
            },
        )
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_version_endpoint() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/version")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_api_event_session_created_serialization() {
        let event = ApiEvent::SessionCreated {
            session_id: "sess_001".to_string(),
            name: "test".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("session_created"));
        assert!(json.contains("sess_001"));
    }

    #[test]
    fn test_api_event_session_state_serialization() {
        let event = ApiEvent::SessionState {
            session_id: "sess_001".to_string(),
            state: "running".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("session_state"));
        assert!(json.contains("running"));
    }

    #[test]
    fn test_api_event_agent_message_serialization() {
        let event = ApiEvent::AgentMessage {
            session_id: "sess_001".to_string(),
            content: "hello".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("agent_message"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_api_event_tool_call_serialization() {
        let event = ApiEvent::ToolCall {
            session_id: "sess_001".to_string(),
            tool: "read_file".to_string(),
            args: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_call"));
        assert!(json.contains("read_file"));
        assert!(json.contains("tmp/test.txt"));
    }

    #[test]
    fn test_api_event_tool_result_serialization() {
        let event = ApiEvent::ToolResult {
            session_id: "sess_001".to_string(),
            tool: "bash".to_string(),
            output: "success".to_string(),
            is_error: false,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_result"));
        let deserialized: ApiEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            ApiEvent::ToolResult { is_error, .. } => assert!(!is_error),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_api_event_token_usage_serialization() {
        let event = ApiEvent::TokenUsage {
            session_id: "sess_001".to_string(),
            input: 100,
            output: 50,
            cache: 10,
            cost_usd: 0.002,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("token_usage"));
        assert!(json.contains("100"));
    }

    #[test]
    fn test_api_event_error_serialization() {
        let event = ApiEvent::Error {
            session_id: Some("sess_001".to_string()),
            error: "something went wrong".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn test_api_event_error_no_session_id() {
        let event = ApiEvent::Error {
            session_id: None,
            error: "generic error".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("generic error"));
    }

    #[test]
    fn test_api_event_health_serialization() {
        let event = ApiEvent::Health {
            status: "ok".to_string(),
            version: "1.0.0".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("health"));
        assert!(json.contains("1.0.0"));
    }

    #[test]
    fn test_api_event_deserialization_roundtrip() {
        let original = ApiEvent::SessionCreated {
            session_id: "sess_001".to_string(),
            name: "roundtrip".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ApiEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ApiEvent::SessionCreated { .. }));
    }

    #[test]
    fn test_app_state_creation() {
        let (tx, _rx) = broadcast::channel(10);
        let state = AppState::new(tx, auth::AuthConfig::default());
        assert_eq!(state.event_tx.receiver_count(), 1);
    }

    #[tokio::test]
    async fn test_build_router_with_token_auth() {
        let (tx, _) = broadcast::channel(100);
        let state = AppState::new(
            tx,
            auth::AuthConfig {
                mode: auth::AuthMode::Token,
                token: Some("test-token".to_string()),
            },
        );
        // Without auth middleware on routes, this should still work
        let app = build_router(state);
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
