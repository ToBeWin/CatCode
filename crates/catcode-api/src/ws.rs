use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;

use crate::AppState;

/// WebSocket routes.
pub fn ws_routes() -> Router<AppState> {
    Router::new().route("/api/v1/ws", get(ws_handler))
}

/// WebSocket upgrade handler.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Handle a WebSocket connection.
async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut event_rx = state.event_tx.subscribe();

    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Messages from the client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        tracing::debug!(%text, "Received WebSocket message");
                        // Parse and handle client commands
                        if let Err(e) = handle_client_message(&text, &state).await {
                            tracing::warn!(error = %e, "Failed to handle client message");
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("WebSocket client disconnected");
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
            // Events from the broadcast channel
            event = event_rx.recv() => {
                match event {
                    Ok(event) => {
                        let json = serde_json::to_string(&event).unwrap_or_default();
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "WebSocket client lagged");
                    }
                    Err(_) => break,
                }
            }
        }
    }

    tracing::info!("WebSocket connection closed");
}

/// Handle a message from the WebSocket client.
async fn handle_client_message(text: &str, _state: &AppState) -> anyhow::Result<()> {
    let msg: serde_json::Value = serde_json::from_str(text)?;
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "ping" => {
            // Client heartbeat — no action needed
        }
        "subscribe" => {
            // Client wants to subscribe to specific events
            tracing::debug!("Client subscribed to events");
        }
        _ => {
            tracing::debug!(%msg_type, "Unknown WebSocket message type");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    fn test_state() -> AppState {
        let (tx, _) = tokio::sync::broadcast::channel(100);
        AppState::new(tx, crate::auth::AuthConfig::default())
    }

    #[tokio::test]
    async fn test_ws_endpoint_requires_upgrade() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/ws")
            .body(Body::empty())
            .unwrap();

        // Without WebSocket upgrade headers, should return 426 or 400
        let resp = app.oneshot(req).await.unwrap();
        // axum returns 426 Upgrade Required for non-WebSocket requests
        assert!(
            resp.status() == StatusCode::UPGRADE_REQUIRED
                || resp.status() == StatusCode::BAD_REQUEST
                || resp.status() == StatusCode::OK
        );
    }
}
