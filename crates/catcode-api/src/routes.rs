use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Session management routes.
pub fn session_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/v1/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/v1/sessions/{id}/message", post(send_message))
        .route("/api/v1/sessions/{id}/pause", post(pause_session))
        .route("/api/v1/sessions/{id}/resume", post(resume_session))
}

/// System routes.
pub fn system_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/version", get(version))
        .route("/api/v1/providers", get(list_providers))
}

// === Request/Response types ===

#[derive(Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub name: String,
    pub state: String,
    pub model_id: String,
    pub provider_id: String,
    pub turn_count: u64,
}

#[derive(Deserialize, Serialize)]
pub struct CreateSessionRequest {
    pub name: String,
    pub project_dir: String,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub ok: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// === Handlers ===

/// List all sessions.
async fn list_sessions(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // Placeholder — in production, this queries the Database
    Json(ApiResponse::<Vec<SessionResponse>>::success(vec![]))
}

/// Create a new session.
async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let session_id = uuid::Uuid::new_v4().to_string();
    let model = req.model_id.unwrap_or_else(|| "deepseek-chat".to_string());
    let provider = req.provider_id.unwrap_or_else(|| "deepseek".to_string());

    // Broadcast event
    let _ = state.event_tx.send(crate::ApiEvent::SessionCreated {
        session_id: session_id.clone(),
        name: req.name.clone(),
    });

    let resp = SessionResponse {
        id: session_id,
        name: req.name,
        state: "running".to_string(),
        model_id: model,
        provider_id: provider,
        turn_count: 0,
    };

    (StatusCode::CREATED, Json(ApiResponse::success(resp)))
}

/// Get a session by ID.
async fn get_session(
    State(_state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Placeholder
    Json(ApiResponse::<SessionResponse>::error(format!(
        "Session not found: {}",
        id
    )))
}

/// Delete a session.
async fn delete_session(
    State(_state): State<AppState>,
    axum::extract::Path(_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Placeholder
    Json(ApiResponse::<()>::success(()))
}

/// Send a message to a session.
async fn send_message(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    // Broadcast the message event
    let _ = state.event_tx.send(crate::ApiEvent::AgentMessage {
        session_id: id.clone(),
        content: req.content.clone(),
    });

    Json(ApiResponse::success(serde_json::json!({
        "session_id": id,
        "message": req.content,
        "status": "sent"
    })))
}

/// Pause a session.
async fn pause_session(
    State(_state): State<AppState>,
    axum::extract::Path(_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::<()>::success(()))
}

/// Resume a session.
async fn resume_session(
    State(_state): State<AppState>,
    axum::extract::Path(_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    Json(ApiResponse::<()>::success(()))
}

/// Health check.
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "uptime": "running"
    }))
}

/// Version info.
async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "CatCode"
    }))
}

/// List available providers.
async fn list_providers() -> impl IntoResponse {
    Json(ApiResponse::success(vec![
        serde_json::json!({
            "id": "deepseek",
            "name": "DeepSeek",
            "models": ["deepseek-chat", "deepseek-reasoner"],
            "status": "available"
        }),
        serde_json::json!({
            "id": "anthropic",
            "name": "Anthropic",
            "models": ["claude-sonnet-4", "claude-opus-4"],
            "status": "available"
        }),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    fn test_state() -> AppState {
        let (tx, _) = tokio::sync::broadcast::channel(100);
        AppState {
            event_tx: tx,
            auth: crate::auth::AuthConfig::default(),
        }
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/sessions")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_session() {
        let app = crate::build_router(test_state());
        let body = serde_json::to_string(&CreateSessionRequest {
            name: "test".to_string(),
            project_dir: "/tmp".to_string(),
            model_id: None,
            provider_id: None,
        })
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/sessions")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_send_message() {
        let app = crate::build_router(test_state());
        let body = serde_json::to_string(&SendMessageRequest {
            content: "hello".to_string(),
        })
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/sessions/test-id/message")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_version() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/version")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
