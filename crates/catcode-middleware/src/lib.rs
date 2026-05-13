//! # catcode-middleware
//!
//! Middleware chain implementation for the CatCode agent loop.
//!
//! Provides a middleware execution engine and several built-in middlewares:
//! - [`MiddlewareChain`] - the execution engine that chains middlewares together
//! - [`LoopDetectionMiddleware`] - detects repeated tool calls to prevent infinite loops
//! - [`ToolErrorHandlingMiddleware`] - catches panics during tool execution
//! - [`RetryMiddleware`] - retries failed tool calls with exponential backoff
//! - [`TimeoutMiddleware`] - enforces timeouts on tool execution
//! - [`TokenUsageMiddleware`] - tracks token usage from model responses

/// The `chain` module.
pub mod chain;
/// The `circuit_breaker` module.
pub mod circuit_breaker;
/// The `error_handling` module.
pub mod error_handling;
/// The `loop_detection` module.
pub mod loop_detection;
/// The `model_profile` module.
pub mod model_profile;
/// The `model_router` module.
pub mod model_router;
/// The `output_validator` module.
pub mod output_validator;
/// The `retry` module.
pub mod retry;
/// The `sandbox` module.
pub mod sandbox;
/// The `timeout` module.
pub mod timeout;
/// The `token_usage` module.
pub mod token_usage;

// Re-export all public types for convenience
pub use chain::{MiddlewareChain, ToolFn};
pub use circuit_breaker::{CircuitBreakerMiddleware, CircuitState};
pub use error_handling::ToolErrorHandlingMiddleware;
pub use loop_detection::LoopDetectionMiddleware;
pub use model_profile::{InstructionStyle, ModelProfile, ProfileRegistry};
pub use model_router::{ModelRouter, RoutingBudget, RoutingStrategy};
pub use output_validator::OutputValidatorMiddleware;
pub use retry::RetryMiddleware;
pub use sandbox::SandboxMiddleware;
pub use timeout::TimeoutMiddleware;
pub use token_usage::TokenUsageMiddleware;

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::AgentContext;
    use catcode_core::tool::{ToolCall, ToolResult};
    use std::sync::Arc;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "test_call".to_string(),
            name: name.to_string(),
            args,
        }
    }

    #[tokio::test]
    async fn test_full_middleware_chain_integration() {
        // Build a chain with all built-in middlewares
        let mut chain = MiddlewareChain::new();
        chain.add(ToolErrorHandlingMiddleware::new());
        chain.add(TimeoutMiddleware::new(10));
        chain.add(RetryMiddleware::new(2, 10, 100));

        let mut ctx = AgentContext::new("integration_test");
        let call = make_call("read_file", serde_json::json!({"path": "src/main.rs"}));

        let tool_fn: ToolFn =
            Arc::new(|_call| Box::pin(async { ToolResult::success("fn main() {}") }));

        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "fn main() {}");
    }

    #[tokio::test]
    async fn test_chain_with_loop_detection() {
        let mut chain = MiddlewareChain::new();
        chain.add(LoopDetectionMiddleware::new(2, 3, 10));

        let mut ctx = AgentContext::new("loop_test");
        let call = make_call("read_file", serde_json::json!({"path": "a.rs"}));

        let tool_fn: ToolFn = Arc::new(|_call| Box::pin(async { ToolResult::success("ok") }));

        // Calls 1-3 should succeed (hard_limit=3 means 3 prior occurrences block the 4th)
        for i in 0..3 {
            let result = chain.execute_tool(&mut ctx, &call, tool_fn.clone()).await;
            assert!(!result.is_error, "call {} should succeed", i + 1);
        }

        // Fourth call should be blocked (3 prior occurrences >= hard_limit of 3)
        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(result.is_error);
        assert!(result.output.contains("Hard limit reached"));
    }

    #[tokio::test]
    async fn test_chain_with_retry_on_failure() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let mut chain = MiddlewareChain::new();
        chain.add(RetryMiddleware::new(3, 10, 100));

        let mut ctx = AgentContext::new("retry_test");
        let call = make_call("flaky_tool", serde_json::json!({}));

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let tool_fn: ToolFn = Arc::new(move |_call| {
            let c = counter_clone.clone();
            Box::pin(async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    ToolResult::error("transient failure")
                } else {
                    ToolResult::success("recovered")
                }
            })
        });

        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "recovered");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_token_usage_middleware_records_usage() {
        use catcode_core::types::{ChatResponse, ContentBlock, StopReason, TokenUsage};

        let mw = TokenUsageMiddleware::new();
        let mut ctx = AgentContext::new("usage_test");

        let response = ChatResponse {
            content: vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
            usage: TokenUsage {
                input_tokens: 1000,
                output_tokens: 500,
                cache_read_tokens: 800,
                cache_creation_tokens: 200,
            },
            stop_reason: StopReason::EndTurn,
            model: "test-model".to_string(),
        };

        use catcode_core::middleware::Middleware;
        mw.after_model(&mut ctx, &response).await.unwrap();

        let total = ctx.total_usage();
        assert_eq!(total.input_tokens, 1000);
        assert_eq!(total.output_tokens, 500);
        assert_eq!(total.cache_read_tokens, 800);
    }
}
