use catcode_context::{ContextCompressor, ContextStack, TokenBudget};
use catcode_core::middleware::AgentContext;
use catcode_core::provider::{Provider, ProviderContext};
use catcode_core::{
    ChatRequest, Message, Role, StopReason, TokenUsage, ToolCall, ToolContext, ToolDefinition,
    ToolResult,
};
use catcode_middleware::MiddlewareChain;
use catcode_tools::ToolRegistry;
use std::sync::Arc;
use tracing::{debug, warn};

/// Maximum number of agent turns before forcing stop.
const MAX_TURNS: u64 = 50;

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
}

/// The agent execution loop.
///
/// Orchestrates the full cycle: build context → call LLM → execute tools → repeat.
/// Each call to `run()` processes one user message through potentially multiple
/// LLM turns (if the model requests tool calls).
pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
    context: ContextStack,
    budget: TokenBudget,
    compressor: ContextCompressor,
    model_id: String,
    max_turns: u64,
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
            compressor: ContextCompressor::new(),
            model_id: model_id.into(),
            max_turns: MAX_TURNS,
        }
    }

    pub fn with_max_turns(mut self, max_turns: u64) -> Self {
        self.max_turns = max_turns;
        self
    }

    /// Run the agent loop for a single user message.
    ///
    /// Returns the final response after all tool calls are resolved.
    pub async fn run(
        &mut self,
        user_message: &str,
        project_dir: &std::path::Path,
    ) -> Result<AgentLoopResult, AgentLoopError> {
        // Add user message to context
        self.context.add_user_message(user_message);

        let mut all_messages = Vec::new();
        let mut total_usage = TokenUsage::default();
        let mut turns = 0u64;
        let mut hit_max = false;

        // Build tool definitions from registry
        let tool_defs = self.build_tool_definitions();

        loop {
            if turns >= self.max_turns {
                warn!(turns, "Hit max turn limit");
                hit_max = true;
                break;
            }

            turns += 1;

            // Compress context if needed
            self.compressor.compress(&mut self.context);

            // Build the request
            let messages = self.context.build_messages();
            let system = messages
                .iter()
                .find(|m| m.role == Role::System)
                .map(|m| m.content.clone());

            let non_system: Vec<Message> = messages
                .into_iter()
                .filter(|m| m.role != Role::System)
                .collect();

            let request = ChatRequest {
                model: self.model_id.clone(),
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

            // Call the provider
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

            // Record usage
            total_usage = total_usage + response.usage.clone();
            self.budget.record_usage(&response.usage);

            // Check budget
            if self.budget.is_exhausted() {
                warn!("Token budget exhausted");
                return Err(AgentLoopError::BudgetExhausted);
            }

            // Extract text and tool calls
            let text = response.text_content();
            let tool_calls = response.get_tool_calls();

            // If no tool calls, we're done
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
                });
            }

            // Has tool calls — add assistant message with tool calls
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

            // Execute each tool call through the middleware chain
            let mut agent_ctx = AgentContext::new("agent-loop");

            for tc in &tc_objects {
                let call_id = tc.id.clone();
                let tool_name = tc.name.clone();
                let tools = self.tools.clone();
                let tc_ref = tc.clone();
                let tool_ctx = ToolContext {
                    session_id: None,
                    project_dir: Some(project_dir.to_path_buf()),
                    working_dir: Some(project_dir.to_path_buf()),
                    dry_run: false,
                };

                // Build the tool function
                let tool_fn: catcode_middleware::chain::ToolFn = Arc::new(move |call| {
                    let tools = tools.clone();
                    let tool_ctx = tool_ctx.clone();
                    let call = call.clone();
                    Box::pin(async move {
                        tools
                            .dispatch(&call.name, call.args.clone(), &tool_ctx)
                            .await
                            .unwrap_or_else(|e| ToolResult::error(e.to_string()))
                    })
                });

                // Execute through middleware
                let result = self
                    .middleware
                    .execute_tool(&mut agent_ctx, &tc_ref, tool_fn)
                    .await;

                debug!(
                    tool = %tool_name,
                    is_error = result.is_error,
                    "Tool execution completed"
                );

                // Add tool result to context
                self.context
                    .add_tool_result(&call_id, &tool_name, result.clone());
                all_messages.push(Message::tool_result(&call_id, &result.output));
            }
        }

        Ok(AgentLoopResult {
            response: String::new(),
            total_usage,
            turns_used: turns,
            hit_max_turns: hit_max,
            messages: all_messages,
        })
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

        AgentLoop::new(
            provider,
            tools,
            middleware,
            context,
            budget,
            "deepseek-chat",
        )
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
    }

    #[tokio::test]
    async fn test_budget_tracking() {
        let mut agent = make_loop("Response");
        let result = agent
            .run("Question", std::path::Path::new("/tmp"))
            .await
            .unwrap();

        // Budget should have recorded usage
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
        assert_eq!(defs.len(), 6); // 6 built-in tools
        assert!(defs.iter().any(|d| d.name == "read_file"));
        assert!(defs.iter().any(|d| d.name == "bash"));
    }
}
