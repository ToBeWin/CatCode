use async_trait::async_trait;
use catcode_daemon::{AgentRuntime, AgentRuntimeOptions, Config, Database, load_config};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

struct LocalMessageRunner {
    db: Database,
}

#[async_trait]
impl catcode_api::MessageRunner for LocalMessageRunner {
    async fn run_message(
        &self,
        session: catcode_api::ApiSession,
        message: String,
    ) -> anyhow::Result<catcode_api::RunMessageResult> {
        let project_dir = PathBuf::from(&session.project_dir);
        let result = AgentRuntime::new()
            .run_once(
                &message,
                &project_dir,
                AgentRuntimeOptions {
                    provider_id: Some(session.provider_id),
                    model_id: Some(session.model_id),
                    session_id: Some(session.id),
                    audit_db: Some(self.db.clone()),
                    system_prompt: "You are CatCode, a concise coding agent served by the local daemon. Use tools when needed and keep responses focused.".to_string(),
                },
            )
            .await?;
        Ok(catcode_api::RunMessageResult {
            response: result.response,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
            cache_tokens: result.total_usage.cache_read_tokens,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let project_dir = std::env::current_dir()?;
    let config = load_config(&project_dir)?;
    let db_path = Config::db_path(&project_dir);
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let db = Database::new(&db_path.to_string_lossy()).await?;
    let (tx, _rx) = broadcast::channel(100);

    let api_state = catcode_api::AppState::new(
        tx,
        catcode_api::auth::AuthConfig {
            mode: catcode_api::auth::AuthMode::LocalOnly,
            token: None,
        },
    )
    .with_store(Arc::new(db.clone()))
    .with_runner(Arc::new(LocalMessageRunner { db }));

    let addr: SocketAddr = format!("{}:{}", config.daemon.host, config.daemon.port).parse()?;
    tracing::info!("CatCode daemon starting on {}", addr);

    let server = catcode_api::serve(addr, api_state);

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                tracing::error!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down...");
        }
    }

    Ok(())
}
