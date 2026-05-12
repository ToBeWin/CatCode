use std::net::SocketAddr;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let config = catcode_daemon::Config::default();
    let (tx, _rx) = broadcast::channel(100);

    let api_state = catcode_api::AppState {
        event_tx: tx,
        auth: catcode_api::auth::AuthConfig {
            mode: catcode_api::auth::AuthMode::LocalOnly,
            token: None,
        },
    };

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
