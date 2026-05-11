use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use std::time::Duration;

/// Middleware that enforces a timeout on tool execution.
///
/// If a tool call takes longer than `timeout_secs` seconds, it is cancelled
/// and an error ToolResult is returned.
#[derive(Debug)]
pub struct TimeoutMiddleware {
    timeout_secs: u64,
}

impl Default for TimeoutMiddleware {
    fn default() -> Self {
        Self { timeout_secs: 120 }
    }
}

impl TimeoutMiddleware {
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

#[async_trait]
impl Middleware for TimeoutMiddleware {
    fn name(&self) -> &str {
        "timeout"
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let timeout_duration = Duration::from_secs(self.timeout_secs);

        match tokio::time::timeout(timeout_duration, next.execute(call)).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    tool = %call.name,
                    timeout_secs = self.timeout_secs,
                    "Tool execution timed out"
                );
                ToolResult::error(format!(
                    "[timeout] Tool '{}' execution timed out after {} seconds",
                    call.name, self.timeout_secs
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::AgentContext;

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            id: "test_call".to_string(),
            name: name.to_string(),
            args: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn test_completes_within_timeout() {
        let mw = TimeoutMiddleware::new(5);
        let mut ctx = AgentContext::new("test");
        let call = make_call("fast_tool");

        let handler = |_call: &ToolCall| -> std::pin::Pin<
            Box<dyn std::future::Future<Output = ToolResult> + Send>,
        > {
            Box::pin(async {
                // Simulate a fast tool
                ToolResult::success("done quickly")
            })
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "done quickly");
    }

    #[tokio::test]
    async fn test_times_out_on_slow_tool() {
        let mw = TimeoutMiddleware::new(1); // 1 second timeout
        let mut ctx = AgentContext::new("test");
        let call = make_call("slow_tool");

        let handler = |_call: &ToolCall| -> std::pin::Pin<
            Box<dyn std::future::Future<Output = ToolResult> + Send>,
        > {
            Box::pin(async {
                // Simulate a slow tool that takes 10 seconds
                tokio::time::sleep(Duration::from_secs(10)).await;
                ToolResult::success("done slowly")
            })
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert!(result.output.contains("timed out"));
        assert!(result.output.contains("slow_tool"));
    }

    #[tokio::test]
    async fn test_passes_through_tool_error() {
        let mw = TimeoutMiddleware::new(5);
        let mut ctx = AgentContext::new("test");
        let call = make_call("error_tool");

        let handler = |_call: &ToolCall| -> std::pin::Pin<
            Box<dyn std::future::Future<Output = ToolResult> + Send>,
        > { Box::pin(async { ToolResult::error("something failed") }) };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        // Timeout middleware should pass through tool errors
        assert!(result.is_error);
        assert_eq!(result.output, "something failed");
    }

    #[test]
    fn test_default_timeout() {
        let mw = TimeoutMiddleware::default();
        assert_eq!(mw.timeout_secs, 120);
    }
}
