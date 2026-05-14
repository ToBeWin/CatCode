use axum::Router;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::AppState;

/// SSE routes.
pub fn sse_routes() -> Router<AppState> {
    Router::new().route("/api/v1/events", get(sse_handler))
}

/// SSE endpoint — streams real-time events to clients.
async fn sse_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            let json = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(json)))
        }
        Err(_) => None,
    });

    // Add a heartbeat every 15 seconds
    let stream = stream.chain(futures_util::stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_secs(15)).await;
        Some((Ok(Event::default().event("heartbeat").data("ping")), ()))
    }));

    Sse::new(stream).keep_alive(KeepAlive::default())
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
    async fn test_sse_endpoint_exists() {
        let app = crate::build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/events")
            .header("Accept", "text/event-stream")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
