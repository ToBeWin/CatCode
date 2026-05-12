//! # catcode-daemon
//!
//! Session management, agent execution loop, and checkpoint persistence
//! for the CatCode AI coding agent.
//!
//! This crate provides the core daemon infrastructure:
//!
//! - **Config** — TOML configuration loading with sensible defaults
//! - **Session** — Session state and metadata management
//! - **SessionManager** — Multi-session concurrent management
//! - **ConcurrentSessionManager** — Async multi-session with tokio task spawning
//! - **AgentLoop** — The main LLM → tool → LLM execution cycle
//! - **SubAgentSpawner** — Concurrent sub-agent spawning
//! - **CheckpointManager** — Session state persistence to disk

pub mod agent_loop;
pub mod benchmark;
pub mod checkpoint;
pub mod concurrent_session;
pub mod config;
pub mod persistence;
pub mod session;
pub mod session_manager;
pub mod subagent;

pub use agent_loop::{AgentLoop, AgentLoopError, AgentLoopResult};
pub use benchmark::{
    BenchmarkCase, BenchmarkReport, BenchmarkResult, default_benchmark_cases, format_report_table,
};
pub use checkpoint::{Checkpoint, CheckpointManager, CheckpointMeta};
pub use concurrent_session::{
    ConcurrentSessionManager, SessionCommand, SessionEvent, SessionHandle,
};
pub use config::Config;
pub use persistence::Database;
pub use session::{Session, SessionId, SessionState, SessionSummary};
pub use session_manager::SessionManager;
pub use subagent::{SubAgentConfig, SubAgentResult, SubAgentSpawner};

/// Create a default middleware chain with all built-in middlewares including sandbox.
///
/// This is the recommended chain for production use. For testing, use
/// `MiddlewareChain::new()` instead.
pub fn default_middleware_chain() -> catcode_middleware::MiddlewareChain {
    use catcode_middleware::*;

    let mut chain = MiddlewareChain::new();
    chain.add(ToolErrorHandlingMiddleware::new());
    chain.add(TimeoutMiddleware::new(60)); // 60s timeout
    chain.add(RetryMiddleware::new(3, 1000, 30000));
    chain.add(LoopDetectionMiddleware::new(5, 10, 20));
    chain.add(SandboxMiddleware::auto_approve()); // Default: auto-approve for local dev
    chain.add(TokenUsageMiddleware::new());
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_context::{ContextStack, TokenBudget};
    use catcode_middleware::MiddlewareChain;
    use catcode_provider::mock::MockProvider;
    use catcode_tools::ToolRegistry;
    use std::sync::Arc;

    #[test]
    fn test_session_lifecycle() {
        let mut mgr = SessionManager::new(5);
        let id = mgr
            .create_session(
                "test",
                std::path::PathBuf::from("/tmp"),
                "deepseek-chat",
                "deepseek",
            )
            .unwrap();

        let session = mgr.get(&id).unwrap();
        assert_eq!(session.state, SessionState::Running);

        mgr.update_state(&id, SessionState::Paused).unwrap();
        assert_eq!(mgr.get(&id).unwrap().state, SessionState::Paused);

        mgr.update_state(&id, SessionState::Completed).unwrap();
        assert!(mgr.get(&id).unwrap().is_terminal());

        mgr.remove_session(&id).unwrap();
        assert!(mgr.get(&id).is_none());
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = CheckpointManager::new(tmp.path().to_path_buf());

        let session = Session::new(
            "test",
            std::path::PathBuf::from("/tmp"),
            "model",
            "provider",
        );

        let meta = mgr
            .save(&session, r#"[{"role":"user","content":"hello"}]"#)
            .unwrap();

        let loaded = mgr.load(&meta.id).unwrap().unwrap();
        assert_eq!(loaded.session.id, session.id);
    }

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.daemon.port, 7070);
        assert_eq!(config.defaults.provider, "deepseek");
    }

    #[tokio::test]
    async fn test_agent_loop_integration() {
        let provider = Arc::new(MockProvider::with_text_response("Done!"));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());
        let context = ContextStack::new("System", "Rules");
        let budget = TokenBudget::new(500_000, 50_000, 0.80);

        let mut agent = AgentLoop::new(
            provider,
            tools,
            middleware,
            context,
            budget,
            "deepseek-chat",
        );
        let result = agent
            .run("Hello", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        assert_eq!(result.response, "Done!");
        assert_eq!(result.turns_used, 1);
    }
}
