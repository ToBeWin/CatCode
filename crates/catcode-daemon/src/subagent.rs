use catcode_context::{ContextStack, TokenBudget};
use catcode_core::Provider;
use catcode_middleware::MiddlewareChain;
use catcode_tools::ToolRegistry;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::agent_loop::{AgentLoop, AgentLoopError};

/// Configuration for a sub-agent.
#[derive(Debug, Clone)]
pub struct SubAgentConfig {
    /// Maximum turns for the sub-agent.
    pub max_turns: u64,
    /// Token budget for the sub-agent (0 = inherit from parent).
    pub token_budget_limit: u64,
    /// System prompt override for the sub-agent.
    pub system_prompt: Option<String>,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 20,
            token_budget_limit: 100_000,
            system_prompt: None,
        }
    }
}

/// Result from a sub-agent execution.
#[derive(Debug, Clone)]
pub struct SubAgentResult {
    /// The sub-agent's text response.
    pub response: String,
    /// Token usage by the sub-agent.
    pub usage: catcode_core::TokenUsage,
    /// Turns used.
    pub turns: u64,
    /// Whether it hit max turns.
    pub hit_max_turns: bool,
    /// Sub-agent name/task for identification.
    pub task: String,
}

/// Spawns sub-agents that can run concurrently.
///
/// Sub-agents share the parent's provider, tools, and middleware,
/// but have their own context stack and token budget.
pub struct SubAgentSpawner {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
    model_id: String,
}

impl SubAgentSpawner {
    pub fn new(
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        middleware: Arc<MiddlewareChain>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            tools,
            middleware,
            model_id: model_id.into(),
        }
    }

    /// Spawn a sub-agent for a specific task.
    ///
    /// Returns a JoinHandle that resolves to the sub-agent's result.
    pub fn spawn(
        &self,
        task: &str,
        config: SubAgentConfig,
        project_dir: std::path::PathBuf,
    ) -> tokio::task::JoinHandle<Result<SubAgentResult, AgentLoopError>> {
        let provider = self.provider.clone();
        let tools = self.tools.clone();
        let middleware = self.middleware.clone();
        let model_id = self.model_id.clone();
        let task = task.to_string();

        let system = config.system_prompt.clone().unwrap_or_else(|| {
            "You are a focused sub-agent. Complete your assigned task efficiently.".to_string()
        });

        tokio::spawn(async move {
            let context = ContextStack::new(&system, "");
            let budget = TokenBudget::new(
                config.token_budget_limit,
                config.token_budget_limit / 5,
                0.80,
            );

            let mut agent = AgentLoop::new(provider, tools, middleware, context, budget, &model_id)
                .with_max_turns(config.max_turns);

            debug!(task = %task, "Sub-agent starting");

            let result = agent.run(&task, &project_dir).await?;

            info!(
                task = %task,
                turns = result.turns_used,
                "Sub-agent completed"
            );

            Ok(SubAgentResult {
                response: result.response,
                usage: result.total_usage,
                turns: result.turns_used,
                hit_max_turns: result.hit_max_turns,
                task,
            })
        })
    }

    /// Spawn multiple sub-agents concurrently and collect all results.
    pub fn spawn_many(
        &self,
        tasks: Vec<(String, SubAgentConfig)>,
        project_dir: std::path::PathBuf,
    ) -> Vec<tokio::task::JoinHandle<Result<SubAgentResult, AgentLoopError>>> {
        tasks
            .into_iter()
            .map(|(task, config)| self.spawn(&task, config, project_dir.clone()))
            .collect()
    }

    /// Spawn and await all sub-agents, returning all results.
    pub async fn run_many(
        &self,
        tasks: Vec<(String, SubAgentConfig)>,
        project_dir: std::path::PathBuf,
    ) -> Vec<Result<SubAgentResult, AgentLoopError>> {
        let handles = self.spawn_many(tasks, project_dir);
        let mut results = Vec::with_capacity(handles.len());

        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    warn!("Sub-agent task panicked: {}", e);
                    results.push(Err(AgentLoopError::ProviderError(format!(
                        "Sub-agent panicked: {}",
                        e
                    ))));
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_provider::mock::MockProvider;

    fn make_spawner(response: &str) -> SubAgentSpawner {
        let provider = Arc::new(MockProvider::with_text_response(response));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());
        SubAgentSpawner::new(provider, tools, middleware, "test-model")
    }

    #[tokio::test]
    async fn test_spawn_subagent() {
        let spawner = make_spawner("Sub-task done!");
        let handle = spawner.spawn(
            "Fix the bug",
            SubAgentConfig::default(),
            std::path::PathBuf::from("/tmp"),
        );

        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.response, "Sub-task done!");
        assert_eq!(result.task, "Fix the bug");
        assert!(!result.hit_max_turns);
    }

    #[tokio::test]
    async fn test_spawn_with_custom_config() {
        let spawner = make_spawner("Done");
        let config = SubAgentConfig {
            max_turns: 5,
            token_budget_limit: 50_000,
            system_prompt: Some("You are a test agent.".to_string()),
        };

        let handle = spawner.spawn("Test task", config, std::path::PathBuf::from("/tmp"));

        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.response, "Done");
    }

    #[tokio::test]
    async fn test_spawn_many() {
        let spawner = make_spawner("Result");
        let tasks = vec![
            ("Task 1".to_string(), SubAgentConfig::default()),
            ("Task 2".to_string(), SubAgentConfig::default()),
            ("Task 3".to_string(), SubAgentConfig::default()),
        ];

        let results = spawner
            .run_many(tasks, std::path::PathBuf::from("/tmp"))
            .await;

        assert_eq!(results.len(), 3);
        for result in results {
            assert!(result.is_ok());
            assert_eq!(result.unwrap().response, "Result");
        }
    }

    #[tokio::test]
    async fn test_subagent_tracks_usage() {
        let spawner = make_spawner("Done with usage");
        let handle = spawner.spawn(
            "Track my tokens",
            SubAgentConfig::default(),
            std::path::PathBuf::from("/tmp"),
        );

        let result = handle.await.unwrap().unwrap();
        // Mock provider returns some usage
        assert!(result.turns > 0);
    }
}
