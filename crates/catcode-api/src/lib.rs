//! # catcode-api
//!
//! HTTP/WebSocket API layer for the CatCode AI coding agent.
//!
//! Provides:
//! - REST API for session management, messaging, and system control
//! - SSE (Server-Sent Events) for real-time event streaming
//! - WebSocket for bidirectional communication
//! - Authentication middleware (local-only and token-based)

pub mod auth;
pub mod routes;
pub mod sse;
pub mod ws;

use axum::Router;
use std::net::SocketAddr;
use tokio::sync::broadcast;

/// Shared application state for the API server.
#[derive(Clone)]
pub struct AppState {
    /// Broadcast channel for SSE events.
    pub event_tx: broadcast::Sender<ApiEvent>,
    /// Authentication configuration.
    pub auth: auth::AuthConfig,
}

/// Events broadcast to SSE clients.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ApiEvent {
    #[serde(rename = "session_created")]
    SessionCreated { session_id: String, name: String },
    #[serde(rename = "session_state")]
    SessionState {
        session_id: String,
        state: String,
    },
    #[serde(rename = "agent_message")]
    AgentMessage {
        session_id: String,
        content: String,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        session_id: String,
        tool: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        session_id: String,
        tool: String,
        output: String,
        is_error: bool,
    },
    #[serde(rename = "token_usage")]
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
        AppState {
            event_tx: tx,
            auth: auth::AuthConfig {
                mode: auth::AuthMode::LocalOnly,
                token: None,
            },
        }
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
}
