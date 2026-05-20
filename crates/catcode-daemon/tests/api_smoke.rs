use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use tokio_tungstenite::tungstenite::Message;

struct DaemonProcess {
    child: Child,
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
async fn daemon_api_persists_mock_session_flow() {
    let temp = tempfile::TempDir::new().unwrap();
    let port = free_port();
    write_mock_config(temp.path(), port);

    let mut daemon = spawn_daemon(temp.path());
    wait_for_health(port).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{port}");
    let sse_response = client
        .get(format!("{base_url}/api/v1/events"))
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut sse_stream = sse_response.bytes_stream();

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/api/v1/ws"))
        .await
        .unwrap();
    ws.send(Message::Text(r#"{"type":"subscribe"}"#.into()))
        .await
        .unwrap();

    let create: serde_json::Value = client
        .post(format!("{base_url}/api/v1/sessions"))
        .json(&json!({
            "name": "smoke",
            "project_dir": temp.path(),
            "provider_id": "mock",
            "model_id": "mock-model"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(create["ok"], true);
    let session_id = create["data"]["id"].as_str().unwrap().to_string();
    assert_sse_contains(&mut sse_stream, "session_created").await;
    assert_ws_contains(&mut ws, "session_created").await;

    let message: serde_json::Value = client
        .post(format!("{base_url}/api/v1/sessions/{session_id}/message"))
        .json(&json!({"content": "hello smoke"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(message["ok"], true);
    assert_eq!(message["data"]["status"], "completed");
    assert!(
        message["data"]["response"]
            .as_str()
            .unwrap()
            .contains("Mock provider response")
    );

    let audit: serde_json::Value = client
        .get(format!("{base_url}/api/v1/sessions/{session_id}/audit"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(audit["ok"], true);
    assert!(audit["data"].as_array().unwrap().is_empty());

    let messages: serde_json::Value = client
        .get(format!("{base_url}/api/v1/sessions/{session_id}/messages"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(messages["ok"], true);
    let messages = messages["data"].as_array().unwrap();
    assert!(messages.iter().any(|entry| entry["role"] == "user"));
    assert!(messages.iter().any(|entry| entry["role"] == "assistant"));

    let usage: serde_json::Value = client
        .get(format!("{base_url}/api/v1/sessions/{session_id}/usage"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(usage["ok"], true);
    assert!(usage["data"]["input_tokens"].as_i64().unwrap() > 0);
    assert!(usage["data"]["total_tokens"].as_i64().unwrap() > 0);

    let recovery: serde_json::Value = client
        .get(format!("{base_url}/api/v1/sessions/{session_id}/recovery"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(recovery["ok"], true);
    assert_eq!(recovery["data"]["state"], "running");
    assert!(
        recovery["data"]["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("Continue"))
    );

    assert_sqlite_rows(temp.path(), &session_id).await;

    let _ = daemon.child.kill();
}

async fn assert_sse_contains<S, B>(stream: &mut S, needle: &str)
where
    S: futures::Stream<Item = reqwest::Result<B>> + Unpin,
    B: AsRef<[u8]>,
{
    let mut buffer = String::new();
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            buffer.push_str(&String::from_utf8_lossy(chunk.as_ref()));
            if buffer.contains(needle) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    assert!(found, "SSE stream did not contain {needle}; got {buffer:?}");
}

async fn assert_ws_contains(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    needle: &str,
) {
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(message) = ws.next().await {
            let message = message.unwrap();
            if message.to_text().unwrap_or_default().contains(needle) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    assert!(found, "WebSocket stream did not contain {needle}");
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_mock_config(project_dir: &std::path::Path, port: u16) {
    std::fs::write(
        project_dir.join("catcode.toml"),
        format!(
            r#"
[daemon]
host = "127.0.0.1"
port = {port}
auto_start = true
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[defaults]
provider = "mock"
model = "mock-model"
sandbox = false

[budget]
session_limit_tokens = 500000
per_request_limit_tokens = 50000
warning_threshold = 0.80

[context]
compression_enabled = true
dedup_tool_outputs = true
max_file_content_tokens = 8000

[observability]
log_level = "info"
log_format = "text"
"#
        ),
    )
    .unwrap();
}

fn spawn_daemon(project_dir: &std::path::Path) -> DaemonProcess {
    let bin = env!("CARGO_BIN_EXE_catcode-daemon");
    let child = Command::new(bin)
        .current_dir(project_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    DaemonProcess { child }
}

async fn wait_for_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/api/v1/health");
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("daemon health endpoint did not become ready");
}

async fn assert_sqlite_rows(project_dir: &std::path::Path, session_id: &str) {
    let db_path = project_dir.join(".catcode/catcode.db");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite:{}", db_path.display()))
        .await
        .unwrap();

    let sessions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE id = ?1")
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions.0, 1);

    let messages: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages WHERE session_id = ?1")
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(messages.0 >= 2);

    let usage: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM token_usage WHERE session_id = ?1")
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(usage.0 >= 1);
}
