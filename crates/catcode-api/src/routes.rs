use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::{ApiSession, AppState};

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
/// JSON response body for session operations.
pub struct SessionResponse {
    pub id: String,
    pub name: String,
    pub state: String,
    pub model_id: String,
    pub provider_id: String,
    pub turn_count: u64,
}

#[derive(Deserialize, Serialize)]
/// Request body for creating a new session.
pub struct CreateSessionRequest {
    pub name: String,
    pub project_dir: String,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
/// Request body for sending a message to a session.
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Serialize)]
/// Generic JSON API response wrapper.
pub struct ApiResponse<T: Serialize> {
    pub ok: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a success API response.
    pub fn success(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    /// Create an error API response.
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
async fn list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(store) = state.store.clone() {
        return match store.list_sessions().await {
            Ok(list) => {
                let mut list: Vec<SessionResponse> =
                    list.iter().map(SessionResponse::from).collect();
                list.sort_by(|a, b| a.name.cmp(&b.name));
                Json(ApiResponse::<Vec<SessionResponse>>::success(list))
            }
            Err(err) => Json(ApiResponse::<Vec<SessionResponse>>::error(format!(
                "Failed to list sessions: {}",
                err
            ))),
        };
    }

    let sessions = state.sessions.read().await;
    let mut list: Vec<SessionResponse> = sessions.values().map(SessionResponse::from).collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Json(ApiResponse::<Vec<SessionResponse>>::success(list))
}

/// Create a new session.
async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let session_id = uuid::Uuid::new_v4().to_string();
    let model = req.model_id.unwrap_or_else(|| "deepseek-chat".to_string());
    let provider = req.provider_id.unwrap_or_else(|| "deepseek".to_string());

    let session = ApiSession {
        id: session_id.clone(),
        name: req.name.clone(),
        state: "running".to_string(),
        project_dir: req.project_dir,
        model_id: model,
        provider_id: provider,
        turn_count: 0,
    };

    let resp = SessionResponse::from(&session);
    state
        .sessions
        .write()
        .await
        .insert(session_id.clone(), session.clone());
    if let Some(store) = state.store.clone()
        && let Err(err) = store.upsert_session(session).await
    {
        tracing::warn!(error = %err, "Failed to persist created session");
    }

    // Broadcast event
    let _ = state.event_tx.send(crate::ApiEvent::SessionCreated {
        session_id: session_id.clone(),
        name: resp.name.clone(),
    });

    (StatusCode::CREATED, Json(ApiResponse::success(resp)))
}

/// Get a session by ID.
async fn get_session(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(store) = state.store.clone() {
        return match store.get_session(&id).await {
            Ok(Some(session)) => Json(ApiResponse::success(SessionResponse::from(&session))),
            Ok(None) => Json(ApiResponse::<SessionResponse>::error(format!(
                "Session not found: {}",
                id
            ))),
            Err(err) => Json(ApiResponse::<SessionResponse>::error(format!(
                "Failed to get session: {}",
                err
            ))),
        };
    }

    let sessions = state.sessions.read().await;
    match sessions.get(&id) {
        Some(session) => Json(ApiResponse::success(SessionResponse::from(session))),
        None => Json(ApiResponse::<SessionResponse>::error(format!(
            "Session not found: {}",
            id
        ))),
    }
}

/// Delete a session.
async fn delete_session(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let removed = state.sessions.write().await.remove(&id);
    let persisted = if let Some(store) = state.store.clone() {
        match store.delete_session(&id).await {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(error = %err, session_id = %id, "Failed to delete persisted session");
                false
            }
        }
    } else {
        false
    };

    if removed.is_some() || persisted {
        let _ = state.event_tx.send(crate::ApiEvent::SessionState {
            session_id: id,
            state: "deleted".to_string(),
        });
        Json(ApiResponse::<()>::success(()))
    } else {
        Json(ApiResponse::<()>::error("Session not found"))
    }
}

/// Send a message to a session.
async fn send_message(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let Some(mut session) = load_session_for_update(&state, &id).await else {
        return Json(ApiResponse::<serde_json::Value>::error(format!(
            "Session not found: {}",
            id
        )));
    };
    session.turn_count += 1;
    persist_session_state(&state, session.clone()).await;
    if let Some(store) = state.store.clone()
        && let Err(err) = store.insert_message(&id, "user", &req.content, None).await
    {
        tracing::warn!(error = %err, session_id = %id, "Failed to persist user message");
    }

    // Broadcast the message event
    let _ = state.event_tx.send(crate::ApiEvent::AgentMessage {
        session_id: id.clone(),
        content: req.content.clone(),
    });

    if let Some(runner) = state.runner.clone() {
        let session_for_usage = session.clone();
        match runner.run_message(session, req.content.clone()).await {
            Ok(result) => {
                let _ = state.event_tx.send(crate::ApiEvent::AgentMessage {
                    session_id: id.clone(),
                    content: result.response.clone(),
                });
                let _ = state.event_tx.send(crate::ApiEvent::TokenUsage {
                    session_id: id.clone(),
                    input: result.input_tokens,
                    output: result.output_tokens,
                    cache: result.cache_tokens,
                    cost_usd: 0.0,
                });
                if let Some(store) = state.store.clone() {
                    if let Err(err) = store
                        .insert_message(&id, "assistant", &result.response, None)
                        .await
                    {
                        tracing::warn!(error = %err, session_id = %id, "Failed to persist assistant message");
                    }
                    if let Err(err) = store
                        .record_token_usage(
                            &session_for_usage,
                            result.input_tokens,
                            result.output_tokens,
                            result.cache_tokens,
                        )
                        .await
                    {
                        tracing::warn!(error = %err, session_id = %id, "Failed to persist token usage");
                    }
                }
                return Json(ApiResponse::success(serde_json::json!({
                    "session_id": id,
                    "message": req.content,
                    "status": "completed",
                    "response": result.response,
                    "usage": {
                        "input": result.input_tokens,
                        "output": result.output_tokens,
                        "cache": result.cache_tokens
                    }
                })));
            }
            Err(err) => {
                let _ = state.event_tx.send(crate::ApiEvent::Error {
                    session_id: Some(id.clone()),
                    error: err.to_string(),
                });
                return Json(ApiResponse::<serde_json::Value>::error(format!(
                    "agent execution failed: {}",
                    err
                )));
            }
        }
    }

    Json(ApiResponse::success(serde_json::json!({
        "session_id": id,
        "message": req.content,
        "status": "queued"
    })))
}

/// Pause a session.
async fn pause_session(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    update_session_state(state, id, "paused").await
}

/// Resume a session.
async fn resume_session(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    update_session_state(state, id, "running").await
}

async fn update_session_state(
    state: AppState,
    id: String,
    next_state: &'static str,
) -> Json<ApiResponse<()>> {
    if let Some(mut session) = load_session_for_update(&state, &id).await {
        session.state = next_state.to_string();
        persist_session_state(&state, session).await;
        let _ = state.event_tx.send(crate::ApiEvent::SessionState {
            session_id: id,
            state: next_state.to_string(),
        });
        Json(ApiResponse::<()>::success(()))
    } else {
        Json(ApiResponse::<()>::error("Session not found"))
    }
}

async fn load_session_for_update(state: &AppState, id: &str) -> Option<ApiSession> {
    if let Some(store) = state.store.clone() {
        match store.get_session(id).await {
            Ok(session) => return session,
            Err(err) => {
                tracing::warn!(error = %err, session_id = %id, "Failed to load persisted session");
            }
        }
    }
    state.sessions.read().await.get(id).cloned()
}

async fn persist_session_state(state: &AppState, session: ApiSession) {
    state
        .sessions
        .write()
        .await
        .insert(session.id.clone(), session.clone());

    if let Some(store) = state.store.clone()
        && let Err(err) = store.upsert_session(session).await
    {
        tracing::warn!(error = %err, "Failed to persist session");
    }
}

impl From<&ApiSession> for SessionResponse {
    fn from(session: &ApiSession) -> Self {
        Self {
            id: session.id.clone(),
            name: session.name.clone(),
            state: session.state.clone(),
            model_id: session.model_id.clone(),
            provider_id: session.provider_id.clone(),
            turn_count: session.turn_count,
        }
    }
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
    use async_trait::async_trait;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    fn test_state() -> AppState {
        let (tx, _) = tokio::sync::broadcast::channel(100);
        AppState::new(tx, crate::auth::AuthConfig::default())
    }

    struct TestRunner;

    #[async_trait]
    impl crate::MessageRunner for TestRunner {
        async fn run_message(
            &self,
            _session: ApiSession,
            message: String,
        ) -> anyhow::Result<crate::RunMessageResult> {
            Ok(crate::RunMessageResult {
                response: format!("ran: {message}"),
                input_tokens: 7,
                output_tokens: 3,
                cache_tokens: 1,
            })
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
    async fn test_session_lifecycle_uses_shared_state() {
        let app = crate::build_router(test_state());
        let body = serde_json::to_string(&CreateSessionRequest {
            name: "stateful".to_string(),
            project_dir: "/tmp/project".to_string(),
            model_id: Some("mock-model".to_string()),
            provider_id: Some("mock".to_string()),
        })
        .unwrap();

        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(create_resp.into_body(), 2048)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = json["data"]["id"].as_str().unwrap();

        let get_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(get_resp.into_body(), 2048)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["name"].as_str().unwrap(), "stateful");

        let pause_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/sessions/{id}/pause"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pause_resp.status(), StatusCode::OK);

        let get_paused = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(get_paused.into_body(), 2048)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["state"].as_str().unwrap(), "paused");

        let delete_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_resp.status(), StatusCode::OK);

        let missing_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(missing_resp.into_body(), 2048)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!json["ok"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_send_message() {
        let app = crate::build_router(test_state());
        let session_id = create_test_session(app.clone()).await;
        let body = serde_json::to_string(&SendMessageRequest {
            content: "hello".to_string(),
        })
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/sessions/{session_id}/message"))
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

    #[tokio::test]
    async fn test_get_session_not_found() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/sessions/nonexistent-id")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!json["ok"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_delete_session() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .method("DELETE")
            .uri("/api/v1/sessions/test-id")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_pause_session() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/sessions/test-id/pause")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_resume_session() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/sessions/test-id/resume")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_providers() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/providers")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_session_with_all_fields() {
        let app = crate::build_router(test_state());
        let body = serde_json::to_string(&CreateSessionRequest {
            name: "full-test".to_string(),
            project_dir: "/home/user/project".to_string(),
            model_id: Some("claude-opus-4".to_string()),
            provider_id: Some("anthropic".to_string()),
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
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["ok"].as_bool().unwrap());
        assert_eq!(json["data"]["name"].as_str().unwrap(), "full-test");
        assert_eq!(json["data"]["model_id"].as_str().unwrap(), "claude-opus-4");
    }

    #[tokio::test]
    async fn test_send_message_response_body() {
        let app = crate::build_router(test_state());
        let session_id = create_test_session(app.clone()).await;
        let body = serde_json::to_string(&SendMessageRequest {
            content: "test message".to_string(),
        })
        .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/sessions/{session_id}/message"))
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["ok"].as_bool().unwrap());
        assert_eq!(json["data"]["message"].as_str().unwrap(), "test message");
    }

    #[tokio::test]
    async fn test_send_message_uses_injected_runner() {
        let state = test_state().with_runner(std::sync::Arc::new(TestRunner));
        let app = crate::build_router(state);
        let session_id = create_test_session(app.clone()).await;
        let body = serde_json::to_string(&SendMessageRequest {
            content: "execute".to_string(),
        })
        .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/sessions/{session_id}/message"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 2048).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["status"].as_str().unwrap(), "completed");
        assert_eq!(json["data"]["response"].as_str().unwrap(), "ran: execute");
        assert_eq!(json["data"]["usage"]["input"].as_u64().unwrap(), 7);
    }

    async fn create_test_session(app: Router) -> String {
        let body = serde_json::to_string(&CreateSessionRequest {
            name: "message-target".to_string(),
            project_dir: "/tmp".to_string(),
            model_id: None,
            provider_id: None,
        })
        .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 2048).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        json["data"]["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn test_health_response_body() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"].as_str().unwrap(), "ok");
    }

    #[tokio::test]
    async fn test_version_response_body() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/version")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"].as_str().unwrap(), "CatCode");
    }

    #[tokio::test]
    async fn test_create_session_request_serialization() {
        let req = CreateSessionRequest {
            name: "test".to_string(),
            project_dir: "/tmp".to_string(),
            model_id: Some("gpt-4".to_string()),
            provider_id: Some("openai".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("gpt-4"));
        assert!(json.contains("openai"));
    }

    #[tokio::test]
    async fn test_send_message_request_serialization() {
        let req = SendMessageRequest {
            content: "hello world".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("hello world"));
    }
}
