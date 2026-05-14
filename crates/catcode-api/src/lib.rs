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

/// Backend that executes a user message for a session.
#[async_trait]
pub trait MessageRunner: Send + Sync {
    async fn run_message(
        &self,
        session: ApiSession,
        message: String,
    ) -> anyhow::Result<RunMessageResult>;
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
    async fn record_token_usage(
        &self,
        session: &ApiSession,
        input_tokens: u64,
        output_tokens: u64,
        cache_tokens: u64,
    ) -> anyhow::Result<()>;
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
