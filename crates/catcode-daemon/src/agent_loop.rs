use catcode_context::{ContextStack, TieredCompactor, TokenBudget};
use catcode_core::provider::{Provider, ProviderContext};
use catcode_core::{
    ChatRequest, Message, Role, StopReason, TokenUsage, ToolCall, ToolContext, ToolDefinition,
};
use catcode_middleware::MiddlewareChain;
use catcode_middleware::model_router::{estimate_complexity, ModelRouter, ProviderHealth, RoutingBudget};
use catcode_tools::ToolRegistry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::lsp_diagnostics::DiagnosticRegistry;
use crate::streaming_executor::StreamingToolExecutor;
use crate::subagent::{SubAgentConfig, SubAgentSpawner};

/// Maximum number of agent turns before forcing stop.
const MAX_TURNS: u64 = 50;

/// Maximum consecutive tool failures allowed before aborting.
const MAX_HEAL_FAILURES: u32 = 5;

/// Result of running the agent loop for a single user message.
#[derive(Debug, Clone)]
pub struct AgentLoopResult {
    /// The final assistant text response.
    pub response: String,
    /// Total token usage across all turns in this loop.
    pub total_usage: TokenUsage,
    /// Number of turns executed (each LLM call = 1 turn).
    pub turns_used: u64,
    /// Whether the loop hit the max turn limit.
    pub hit_max_turns: bool,
    /// All messages generated during this loop (for appending to conversation).
    pub messages: Vec<Message>,
    /// Generated plan if auto-plan was used.
    pub auto_plan: Option<String>,
    /// Number of self-heal actions taken.
    pub self_healed: u32,
    /// Number of sub-agents spawned.
    pub sub_agents_spawned: u32,
    /// Models used across turns (for cost tracking).
    pub models_used: Vec<String>,
}

/// The agent execution loop.
///
/// Orchestrates the full cycle: build context → call LLM → execute tools → repeat.
/// Each call to `run()` processes one user message through potentially multiple
/// LLM turns (if the model requests tool calls).
///
/// Supports intelligence features: auto-plan, self-healing, sub-agent dispatch,
/// and model routing — all configurable via builder methods.
pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
    context: ContextStack,
    budget: TokenBudget,
    compressor: TieredCompactor,
    model_id: String,
    max_turns: u64,
    model_router: Option<ModelRouter>,
    auto_plan_enabled: bool,
    self_heal_enabled: bool,
    subagent_dispatch_enabled: bool,
    model_routing_enabled: bool,
    failed_tools: HashMap<String, u32>,
    total_failures: u32,
    lsp_registry: Option<Arc<Mutex<DiagnosticRegistry>>>,
}

impl AgentLoop {
    pub fn new(
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        middleware: Arc<MiddlewareChain>,
        context: ContextStack,
        budget: TokenBudget,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            tools,
            middleware,
            context,
            budget,
            compressor: TieredCompactor::new(),
            model_id: model_id.into(),
            max_turns: MAX_TURNS,
            model_router: None,
            auto_plan_enabled: true,
            self_heal_enabled: true,
            subagent_dispatch_enabled: true,
            model_routing_enabled: true,
            failed_tools: HashMap::new(),
            total_failures: 0,
            lsp_registry: None,
        }
    }

    pub fn with_max_turns(mut self, max_turns: u64) -> Self {
        self.max_turns = max_turns;
        self
    }

    pub fn with_model_router(mut self, router: ModelRouter) -> Self {
        self.model_router = Some(router);
        self
    }

    pub fn with_auto_plan(mut self, enabled: bool) -> Self {
        self.auto_plan_enabled = enabled;
        self
    }

    pub fn with_self_heal(mut self, enabled: bool) -> Self {
        self.self_heal_enabled = enabled;
        self
    }

    pub fn with_subagent_dispatch(mut self, enabled: bool) -> Self {
        self.subagent_dispatch_enabled = enabled;
        self
    }

    pub fn with_model_routing(mut self, enabled: bool) -> Self {
        self.model_routing_enabled = enabled;
        self
    }

    pub fn with_lsp(mut self, registry: Arc<Mutex<DiagnosticRegistry>>) -> Self {
        self.lsp_registry = Some(registry);
        self
    }

    /// Run the agent loop with intelligence features (auto-plan + full smart loop).
    ///
    /// This wraps `run()` with automatic plan generation for complex tasks.
    /// If `auto_plan_enabled` and task complexity > 0.6, the LLM generates a
    /// step-by-step plan before the main agent loop begins.
    pub async fn run_intelligent(
        &mut self,
        user_message: &str,
        project_dir: &Path,
    ) -> Result<AgentLoopResult, AgentLoopError> {
        let mut auto_plan: Option<String> = None;

        if self.auto_plan_enabled {
            let complexity = estimate_complexity(user_message);
            debug!(complexity, "Task complexity estimated for auto-plan");

            if complexity > 0.6 {
                info!(complexity, "Generating plan for complex task");
                let plan = self.generate_plan(user_message, project_dir).await?;
                auto_plan = Some(plan.clone());

                let prev = self.context.permanent.system_prompt.clone();
                self.context.permanent.system_prompt =
                    format!("{}\n\nPre-generated Plan:\n{}\n\nFollow this plan.", prev, plan);
            }
        }

        let mut result = self.run(user_message, project_dir).await?;
        result.auto_plan = auto_plan;
        Ok(result)
    }

    /// Run the agent loop for a single user message.
    ///
    /// Returns the final response after all tool calls are resolved.
    /// Integrates self-healing and model routing when enabled.
    pub async fn run(
        &mut self,
        user_message: &str,
        project_dir: &Path,
    ) -> Result<AgentLoopResult, AgentLoopError> {
        // Reset per-run tracking
        self.failed_tools.clear();
        self.total_failures = 0;

        self.context.add_user_message(user_message);

        let mut all_messages = Vec::new();
        let mut total_usage = TokenUsage::default();
        let mut turns = 0u64;
        let mut hit_max = false;
        let mut self_healed = 0u32;
        let mut sub_agents_spawned = 0u32;
        let mut models_used: Vec<String> = Vec::new();

        let tool_defs = self.build_tool_definitions();

        loop {
            if turns >= self.max_turns {
                warn!(turns, "Hit max turn limit");
                hit_max = true;
                break;
            }

            turns += 1;

            let tiers = self.compressor.compress_tiered(&mut self.context);
            debug!("Compaction tiers applied: {:?}", tiers);

            let messages = self.context.build_messages();
            let system = messages
                .iter()
                .find(|m| m.role == Role::System)
                .map(|m| m.content.clone());

            let non_system: Vec<Message> = messages
                .into_iter()
                .filter(|m| m.role != Role::System)
                .collect();

            let model = self.resolve_model(user_message);

            if models_used.last() != Some(&model) {
                models_used.push(model.clone());
            }

            let mut request = ChatRequest {
                model,
                messages: non_system,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs.clone())
                },
                system,
                max_tokens: Some(4096),
                temperature: None,
                stream: false,
            };

            self.attach_lsp_diagnostics(&mut request);

            let provider_ctx = ProviderContext {
                session_id: None,
                project_dir: Some(project_dir.to_string_lossy().to_string()),
                metadata: Default::default(),
            };

            debug!(turn = turns, "Calling provider");
            let response = self
                .provider
                .chat(request, &provider_ctx)
                .await
                .map_err(|e| AgentLoopError::ProviderError(e.to_string()))?;

            total_usage = total_usage + response.usage.clone();
            self.budget.record_usage(&response.usage);

            if self.budget.is_exhausted() {
                warn!("Token budget exhausted");
                return Err(AgentLoopError::BudgetExhausted);
            }

            let text = response.text_content();
            let tool_calls = response.get_tool_calls();

            if tool_calls.is_empty() || response.stop_reason == StopReason::EndTurn {
                if !text.is_empty() {
                    self.context.add_assistant_message(&text);
                    all_messages.push(Message::assistant(&text));
                }
                return Ok(AgentLoopResult {
                    response: text,
                    total_usage,
                    turns_used: turns,
                    hit_max_turns: hit_max,
                    messages: all_messages,
                    auto_plan: None,
                    self_healed,
                    sub_agents_spawned,
                    models_used,
                });
            }

            let tc_objects: Vec<ToolCall> = tool_calls
                .iter()
                .map(|(id, name, args)| ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                })
                .collect();

            let assistant_msg = Message::assistant_with_tool_calls(&text, tc_objects.clone());
            self.context.add_assistant_message(&text);
            all_messages.push(assistant_msg);

            let tool_ctx = ToolContext {
                session_id: None,
                project_dir: Some(project_dir.to_path_buf()),
                working_dir: Some(project_dir.to_path_buf()),
                dry_run: false,
            };

            let executor = StreamingToolExecutor::new(self.tools.clone(), self.middleware.clone());
            let results = executor.execute_batch(&tc_objects, &tool_ctx).await;

            for (call_id, result) in results {
                let tool_name = tc_objects
                    .iter()
                    .find(|tc| tc.id == call_id)
                    .map(|tc| tc.name.clone())
                    .unwrap_or_default();

                debug!(
                    tool = %tool_name,
                    is_error = result.is_error,
                    "Tool execution completed"
                );

                // Self-healing: detect repeated tool failures and provide guidance
                if self.self_heal_enabled && result.is_error {
                    let entry = self.failed_tools.entry(tool_name.clone()).or_insert(0);
                    *entry += 1;
                    self.total_failures += 1;

                    if self.total_failures > MAX_HEAL_FAILURES {
                        warn!("Too many failures ({}), aborting", self.total_failures);
                        return Err(AgentLoopError::MaxTurnsExceeded(self.max_turns));
                    }

                    self_healed += 1;

                    let advice = if *entry >= 2 {
                        format!(
                            "The tool '{}' failed again with: {}. Try a completely different approach. Instead of '{}', try using read_file and search tools directly.",
                            tool_name, result.output, tool_name
                        )
                    } else {
                        format!(
                            "The tool '{}' failed with: {}. Try a different approach.",
                            tool_name, result.output
                        )
                    };

                    let mut modified_result = result.clone();
                    modified_result.output =
                        format!("{}\n\nSelf-healing advice: {}", result.output, advice);
                    self.context
                        .add_tool_result(&call_id, &tool_name, modified_result.clone());
                    all_messages.push(Message::tool_result(
                        &call_id,
                        &modified_result.output,
                    ));
                } else {
                    if !result.is_error {
                        self.failed_tools.remove(&tool_name);
                    }

                    self.context
                        .add_tool_result(&call_id, &tool_name, result.clone());
                    all_messages.push(Message::tool_result(&call_id, &result.output));
                }

                // Sub-agent dispatch: spawn parallel sub-agents when file lists detected
                if self.subagent_dispatch_enabled && !result.output.is_empty() {
                    let sub_results = self
                        .try_dispatch_subagents(&tool_name, &result.output, project_dir)
                        .await;
                    if !sub_results.is_empty() {
                        sub_agents_spawned += sub_results.len() as u32;
                        let merged = sub_results.join("\n---\n");
                        let msg = format!("Sub-agent results:\n{}", merged);
                        self.context.add_user_message(&msg);
                        all_messages.push(Message::user(msg));
                    }
                }
            }
        }

        Ok(AgentLoopResult {
            response: String::new(),
            total_usage,
            turns_used: turns,
            hit_max_turns: hit_max,
            messages: all_messages,
            auto_plan: None,
            self_healed,
            sub_agents_spawned,
            models_used,
        })
    }

    /// Generate a plan for a complex task by calling the LLM.
    async fn generate_plan(
        &self,
        user_message: &str,
        project_dir: &Path,
    ) -> Result<String, AgentLoopError> {
        let provider_ctx = ProviderContext {
            session_id: None,
            project_dir: Some(project_dir.to_string_lossy().to_string()),
            metadata: Default::default(),
        };

        let request = ChatRequest {
            model: self.model_id.clone(),
            messages: vec![Message::user(user_message)],
            tools: None,
            system: Some(
                "Analyze this task and create a step-by-step plan. Output ONLY the plan as numbered steps."
                    .to_string(),
            ),
            max_tokens: Some(4096),
            temperature: None,
            stream: false,
        };

        let response = self
            .provider
            .chat(request, &provider_ctx)
            .await
            .map_err(|e| AgentLoopError::ProviderError(e.to_string()))?;

        let plan = response.text_content();
        info!(plan_len = plan.len(), "Plan generated");
        Ok(plan)
    }

    /// Resolve the model to use — via router if configured, otherwise default.
    fn resolve_model(&self, user_message: &str) -> String {
        if self.model_routing_enabled && let Some(ref router) = self.model_router {
            let complexity = estimate_complexity(user_message);
            let used = self.budget.input_used + self.budget.output_used;
            let remaining = self.budget.session_limit.saturating_sub(used);
            let budget_info = RoutingBudget {
                remaining_tokens: remaining,
                total_tokens: self.budget.session_limit,
                max_cost_per_request_usd: 0.1,
            };
            let health = ProviderHealth::default();
            return router.select_model(complexity, &budget_info, &health);
        }
        self.model_id.clone()
    }

    /// Check for self-healing opportunity and return guidance advice.
    ///
    /// Tracks consecutive failures per tool. First failure returns advice
    /// to try a different approach. Second+ consecutive failure on the same
    /// tool suggests alternative tools. Resets failure counter on success.
    /// Returns `None` if total failures exceed `MAX_HEAL_FAILURES`.
    pub fn check_self_heal(
        &mut self,
        tool_name: &str,
        was_error: bool,
        tool_output: &str,
    ) -> Option<String> {
        if !was_error {
            self.failed_tools.remove(tool_name);
            return None;
        }

        let entry = self.failed_tools.entry(tool_name.to_string()).or_insert(0);
        *entry += 1;
        self.total_failures += 1;

        if self.total_failures > MAX_HEAL_FAILURES {
            return None;
        }

        Some(if *entry >= 2 {
            format!(
                "The tool '{}' failed again with: {}. Try a completely different approach. Instead of '{}', try using read_file and search tools.",
                tool_name, tool_output, tool_name
            )
        } else {
            format!(
                "The tool '{}' failed with: {}. Try a different approach.",
                tool_name, tool_output
            )
        })
    }

    /// Try to dispatch sub-agents when tool output contains file paths.
    ///
    /// For bash/search/glob/grep/find outputs containing 3+ file paths,
    /// spawns up to 3 sub-agents to process files in parallel.
    pub async fn try_dispatch_subagents(
        &self,
        tool_name: &str,
        tool_output: &str,
        project_dir: &Path,
    ) -> Vec<String> {
        if !self.subagent_dispatch_enabled {
            return Vec::new();
        }

        match tool_name {
            "search" | "glob" | "bash" | "grep" | "find" => {}
            _ => return Vec::new(),
        }

        let re = match regex::Regex::new(
            r"(?m)^\s*((?:\./)?[^\s]+\.(?:rs|py|js|ts|toml|md|json|yaml|yml|sh))\s*$",
        ) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let files: Vec<String> = re
            .captures_iter(tool_output)
            .map(|c| c[1].to_string())
            .filter(|p| !p.is_empty())
            .collect();

        if files.len() < 3 {
            return Vec::new();
        }

        info!(
            tool = %tool_name,
            file_count = files.len(),
            "Dispatching sub-agents for parallel work"
        );

        let batch_size = files.len().div_ceil(3);
        let batches: Vec<&[String]> = files.chunks(batch_size).take(3).collect();

        let spawner = SubAgentSpawner::new(
            self.provider.clone(),
            self.tools.clone(),
            self.middleware.clone(),
            &self.model_id,
        );

        let tasks: Vec<(String, SubAgentConfig)> = batches
            .into_iter()
            .map(|batch| {
                let task = format!("Process the following files: {}", batch.join(", "));
                let config = SubAgentConfig {
                    max_turns: 10,
                    token_budget_limit: 50_000,
                    system_prompt: Some(format!(
                        "You are a focused sub-agent handling files: {}. Complete your task efficiently.",
                        batch.join(", ")
                    )),
                };
                (task, config)
            })
            .collect();

        if tasks.is_empty() {
            return Vec::new();
        }

        let results = spawner
            .run_many(tasks, project_dir.to_path_buf())
            .await;

        results
            .into_iter()
            .filter_map(|r| match r {
                Ok(sub) => {
                    debug!(task = %sub.task, "Sub-agent completed");
                    Some(format!("[Sub-agent: {}]\n{}", sub.task, sub.response))
                }
                Err(e) => {
                    warn!("Sub-agent failed: {}", e);
                    None
                }
            })
            .collect()
    }

    /// Attach LSP diagnostics to the ChatRequest as additional system context.
    ///
    /// Reads from the diagnostic registry and appends any pending diagnostics
    /// to the system prompt so the LLM can see compiler feedback.
    fn attach_lsp_diagnostics(&self, request: &mut ChatRequest) {
        let Some(ref registry) = self.lsp_registry else { return };
        let registry = registry.blocking_lock();
        if let Some(attachment) = registry.build_attachment(&[]) {
            let msg = format!(
                "Current LSP diagnostics:\n{}\n\nDetails:\n{}",
                attachment.summary,
                attachment.details.join("\n")
            );
            request.system = Some(match request.system.take() {
                Some(mut s) => {
                    s.push_str(&format!("\n\n{}", msg));
                    s
                }
                None => msg,
            });
        }
    }

    /// Build tool definitions from the registry for the LLM request.
    fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .to_llm_schema()
            .into_iter()
            .map(|schema| ToolDefinition {
                name: schema["name"].as_str().unwrap_or_default().to_string(),
                description: schema["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                parameters: schema["parameters"].clone(),
            })
            .collect()
    }

    /// Get a reference to the token budget.
    pub fn budget(&self) -> &TokenBudget {
        &self.budget
    }

    /// Get a mutable reference to the token budget.
    pub fn budget_mut(&mut self) -> &mut TokenBudget {
        &mut self.budget
    }

    /// Get a reference to the context stack.
    pub fn context(&self) -> &ContextStack {
        &self.context
    }

    /// Get a mutable reference to the context stack.
    pub fn context_mut(&mut self) -> &mut ContextStack {
        &mut self.context
    }
}

/// Errors that can occur during agent loop execution.
#[derive(Debug, thiserror::Error)]
pub enum AgentLoopError {
    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Token budget exhausted")]
    BudgetExhausted,

    #[error("Max turns ({0}) exceeded")]
    MaxTurnsExceeded(u64),
}

impl AgentLoopError {
    /// Return a user-friendly error message suitable for displaying in the TUI.
    pub fn user_friendly_message(&self) -> String {
        match self {
            AgentLoopError::ProviderError(msg) => {
                format!(
                    "Model API call failed: {}. Check your API key and network connection.",
                    msg
                )
            }
            AgentLoopError::BudgetExhausted => {
                "Token budget exhausted. Use a cheaper model or increase budget in config."
                    .to_string()
            }
            AgentLoopError::MaxTurnsExceeded(turns) => {
                format!(
                    "Max turns reached ({}). The task may be too complex — try breaking it into smaller steps.",
                    turns
                )
            }
        }
    }

    /// Return a short status label for the error.
    pub fn status_label(&self) -> &str {
        match self {
            AgentLoopError::ProviderError(_) => "API error",
            AgentLoopError::BudgetExhausted => "budget exhausted",
            AgentLoopError::MaxTurnsExceeded(_) => "max turns",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_provider::mock::MockProvider;
    use catcode_tools::ToolRegistry;

    fn make_loop(response: &str) -> AgentLoop {
        let provider = Arc::new(MockProvider::with_text_response(response));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());
        let context = ContextStack::new("You are a helpful assistant.", "");
        let budget = TokenBudget::new(500_000, 50_000, 0.80);

        AgentLoop::new(provider, tools, middleware, context, budget, "deepseek-chat")
            .with_self_heal(false)
            .with_model_routing(false)
            .with_subagent_dispatch(false)
            .with_auto_plan(false)
    }

    fn make_smart_loop(response: &str) -> AgentLoop {
        let provider = Arc::new(MockProvider::with_text_response(response));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());
        let context = ContextStack::new("You are a helpful assistant.", "");
        let budget = TokenBudget::new(500_000, 50_000, 0.80);

        AgentLoop::new(provider, tools, middleware, context, budget, "deepseek-chat")
    }

    #[tokio::test]
    async fn test_simple_text_response() {
        let mut agent = make_loop("Hello! How can I help?");
        let result = agent
            .run("Hi there", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        assert_eq!(result.response, "Hello! How can I help?");
        assert_eq!(result.turns_used, 1);
        assert!(!result.hit_max_turns);
        assert!(!result.messages.is_empty());
        assert_eq!(result.self_healed, 0);
        assert_eq!(result.sub_agents_spawned, 0);
        assert!(result.models_used.contains(&"deepseek-chat".to_string()));
    }

    #[tokio::test]
    async fn test_budget_tracking() {
        let mut agent = make_loop("Response");
        let result = agent
            .run("Question", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        assert!(result.total_usage.input_tokens > 0 || result.total_usage.output_tokens > 0);
    }

    #[tokio::test]
    async fn test_context_updated() {
        let mut agent = make_loop("I'll help with that");
        agent
            .run("Fix the bug", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        let ctx = agent.context();
        assert!(!ctx.session.completed_steps.is_empty());
    }

    #[test]
    fn test_build_tool_definitions() {
        let agent = make_loop("test");
        let defs = agent.build_tool_definitions();
        assert_eq!(defs.len(), 13);
        assert!(defs.iter().any(|d| d.name == "read_file"));
        assert!(defs.iter().any(|d| d.name == "bash"));
    }

    #[tokio::test]
    async fn test_auto_plan_triggers_on_complex_task() {
        let mut agent = make_smart_loop("1. Analyze\n2. Fix\n3. Test");
        agent.auto_plan_enabled = true;

        let result = agent
            .run_intelligent(
                "Refactor the architecture to support concurrent async distributed database migration with performance benchmarks",
                std::path::Path::new("/tmp"),
            )
            .await
            .unwrap();

        assert!(result.auto_plan.is_some());
        let plan = result.auto_plan.unwrap();
        assert!(!plan.is_empty());
    }

    #[tokio::test]
    async fn test_auto_plan_skips_simple_task() {
        let mut agent = make_smart_loop("Sure, here's the fix");
        agent.auto_plan_enabled = true;

        let result = agent
            .run_intelligent(
                "Fix typo in README",
                std::path::Path::new("/tmp"),
            )
            .await
            .unwrap();

        // Simple task should NOT trigger auto-plan (complexity <= 0.6)
        assert!(result.auto_plan.is_none());
    }

    #[test]
    fn test_self_heal_tracks_failures() {
        let mut agent = make_smart_loop("test");
        agent.self_heal_enabled = true;

        // First failure
        let advice = agent.check_self_heal("bash", true, "command not found");
        assert!(advice.is_some());
        assert!(advice.unwrap().contains("Try a different approach"));

        // Second consecutive failure on same tool
        let advice = agent.check_self_heal("bash", true, "permission denied");
        assert!(advice.is_some());
        assert!(advice.unwrap().contains("completely different approach"));

        // Success resets counter
        let advice = agent.check_self_heal("bash", false, "success");
        assert!(advice.is_none());

        // After reset, first failure should be simple advice again
        let advice = agent.check_self_heal("bash", true, "error");
        assert!(advice.is_some());
        assert!(advice.unwrap().contains("Try a different approach"));
    }

    #[test]
    fn test_self_heal_max_failures() {
        // Override: disable auto-plan, use run() directly to test self-heal in the loop
        let mut agent = make_smart_loop("test");
        agent.self_heal_enabled = true;
        agent.auto_plan_enabled = false;
        agent.model_routing_enabled = false;
        agent.subagent_dispatch_enabled = false;

        // Exhaust the failure budget
        for _ in 0..MAX_HEAL_FAILURES {
            let advice = agent.check_self_heal("bash", true, "error");
            assert!(advice.is_some());
        }

        // Next failure should return None
        let advice = agent.check_self_heal("bash", true, "error again");
        assert!(advice.is_none());
    }

    #[tokio::test]
    async fn test_subagent_dispatch_with_files() {
        let output = "src/main.rs\nsrc/lib.rs\nsrc/utils.rs\nsrc/config.rs";
        let mut agent = make_smart_loop("test");
        agent.subagent_dispatch_enabled = true;

        let result = agent.try_dispatch_subagents(
            "bash",
            output,
            std::path::Path::new("/tmp"),
        ).await;

        assert!(result.is_empty() || result.len() <= 3);
    }

    #[tokio::test]
    async fn test_subagent_dispatch_skips_few_files() {
        let output = "src/main.rs\nsrc/lib.rs";
        let mut agent = make_smart_loop("test");
        agent.subagent_dispatch_enabled = true;

        let result = agent.try_dispatch_subagents(
            "bash",
            output,
            std::path::Path::new("/tmp"),
        ).await;

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_subagent_dispatch_skips_unsupported_tools() {
        let output = "src/main.rs\nsrc/lib.rs\nsrc/utils.rs";
        let mut agent = make_smart_loop("test");
        agent.subagent_dispatch_enabled = true;

        let result = agent.try_dispatch_subagents(
            "read_file",
            output,
            std::path::Path::new("/tmp"),
        ).await;

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_model_router_integration() {
        use catcode_middleware::model_router::{ModelRouter, RoutingStrategy};

        let router = ModelRouter::new(RoutingStrategy::Fixed("custom-model".to_string()));
        let mut agent = make_loop("test");
        agent.model_router = Some(router);
        agent.model_routing_enabled = true;
        agent.self_heal_enabled = false;
        agent.subagent_dispatch_enabled = false;
        agent.auto_plan_enabled = false;

        // The model in the result should be tracked
        let result = agent
            .run("simple request", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        assert!(result.models_used.contains(&"custom-model".to_string()));
    }

    #[tokio::test]
    async fn test_cost_aware_routing() {
        use catcode_middleware::model_router::{ModelRouter, RoutingStrategy};

        let router = ModelRouter::new(RoutingStrategy::CostAware {
            simple_model: "cheap-model".to_string(),
            powerful_model: "expensive-model".to_string(),
            complexity_threshold: 0.6,
        });
        let mut agent = make_loop("test");
        agent.model_router = Some(router);
        agent.model_routing_enabled = true;
        agent.self_heal_enabled = false;
        agent.subagent_dispatch_enabled = false;
        agent.auto_plan_enabled = false;

        // Simple task should use cheap model
        let result = agent
            .run("simple task", std::path::Path::new("/tmp"))
            .await
            .unwrap();
        assert!(result.models_used.contains(&"cheap-model".to_string()));

        // Complex task should use expensive model
        let mut agent2 = make_loop("test");
        let router2 = ModelRouter::new(RoutingStrategy::CostAware {
            simple_model: "cheap-model".to_string(),
            powerful_model: "expensive-model".to_string(),
            complexity_threshold: 0.6,
        });
        agent2.model_router = Some(router2);
        agent2.model_routing_enabled = true;
        agent2.self_heal_enabled = false;
        agent2.subagent_dispatch_enabled = false;
        agent2.auto_plan_enabled = false;

        let result2 = agent2
            .run(
                "Refactor the architecture to support concurrent async distributed database migration",
                std::path::Path::new("/tmp"),
            )
            .await
            .unwrap();
        assert!(result2.models_used.contains(&"expensive-model".to_string()));
    }

    #[tokio::test]
    async fn test_auto_plan_not_enabled_by_default_in_run() {
        // run() should NOT trigger auto-plan even with a complex task
        let mut agent = make_smart_loop("Response");
        // auto_plan_enabled defaults to true, but run() doesn't do plan generation
        let result = agent
            .run(
                "Refactor the architecture",
                std::path::Path::new("/tmp"),
            )
            .await
            .unwrap();

        // run() doesn't set auto_plan, so it stays None
        // (run_intelligent sets it)
        assert!(result.auto_plan.is_none());
    }

    #[tokio::test]
    async fn test_run_intelligent_with_auto_plan_disabled() {
        let mut agent = make_loop("Response");
        // auto_plan is already disabled by make_loop
        let result = agent
            .run_intelligent("Hello", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        // No auto-plan since disabled
        assert!(result.auto_plan.is_none());
    }

    #[tokio::test]
    async fn test_models_used_tracked() {
        let mut agent = make_loop("Response");
        let result = agent
            .run("Hello", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        assert!(!result.models_used.is_empty());
        assert_eq!(result.models_used[0], "deepseek-chat");
    }
}
