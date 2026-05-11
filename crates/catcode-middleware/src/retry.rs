use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};

/// Middleware that retries tool calls on error with exponential backoff.
///
/// Only retries if the tool result has `is_error == true`.
/// Uses exponential backoff: `base_delay_ms * 2^attempt`, capped at `max_delay_ms`.
#[derive(Debug)]
pub struct RetryMiddleware {
    max_attempts: u32,
    base_delay_ms: u64,
    max_delay_ms: u64,
}

impl Default for RetryMiddleware {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30000,
        }
    }
}

impl RetryMiddleware {
    pub fn new(max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            max_attempts,
            base_delay_ms,
            max_delay_ms,
        }
    }

    /// Compute the delay for a given attempt (0-indexed).
    fn delay_for_attempt(&self, attempt: u32) -> u64 {
        let delay = self.base_delay_ms.saturating_mul(1u64 << attempt);
        delay.min(self.max_delay_ms)
    }
}

#[async_trait]
impl Middleware for RetryMiddleware {
    fn name(&self) -> &str {
        "retry"
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let mut last_result = None;

        for attempt in 0..self.max_attempts {
            let result = next.execute(call).await;

            if !result.is_error {
                return result;
            }

            tracing::warn!(
                tool = %call.name,
                attempt = attempt + 1,
                max_attempts = self.max_attempts,
                error = %result.output,
                "Tool call failed, retrying"
            );

            // If this is not the last attempt, wait before retrying
            if attempt + 1 < self.max_attempts {
                let delay = self.delay_for_attempt(attempt);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }

            last_result = Some(result);
        }

        // All attempts exhausted, return the last error
        last_result.unwrap_or_else(|| {
            ToolResult::error("[retry] All retry attempts exhausted with no result")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::AgentContext;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            id: "test_call".to_string(),
            name: name.to_string(),
            args: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn test_no_retry_on_success() {
        let mw = RetryMiddleware::new(3, 10, 100);
        let mut ctx = AgentContext::new("test");
        let call = make_call("read_file");
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = move |_call: &ToolCall| {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                ToolResult::success("ok")
            }
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retries_on_error_then_succeeds() {
        let mw = RetryMiddleware::new(3, 10, 100);
        let mut ctx = AgentContext::new("test");
        let call = make_call("flaky_tool");
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = move |_call: &ToolCall| {
            let c = counter_clone.clone();
            async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    ToolResult::error(format!("failure on attempt {}", attempt))
                } else {
                    ToolResult::success("finally worked")
                }
            }
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "finally worked");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_returns_last_error_after_all_retries_exhausted() {
        let mw = RetryMiddleware::new(3, 10, 100);
        let mut ctx = AgentContext::new("test");
        let call = make_call("always_fail");
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = move |_call: &ToolCall| {
            let c = counter_clone.clone();
            async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                ToolResult::error(format!("failure on attempt {}", attempt))
            }
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert!(result.output.contains("failure on attempt 2"));
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_delay_for_attempt_exponential() {
        let mw = RetryMiddleware::new(5, 100, 5000);
        assert_eq!(mw.delay_for_attempt(0), 100); // 100 * 2^0 = 100
        assert_eq!(mw.delay_for_attempt(1), 200); // 100 * 2^1 = 200
        assert_eq!(mw.delay_for_attempt(2), 400); // 100 * 2^2 = 400
        assert_eq!(mw.delay_for_attempt(3), 800); // 100 * 2^3 = 800
        assert_eq!(mw.delay_for_attempt(4), 1600); // 100 * 2^4 = 1600
    }

    #[test]
    fn test_delay_capped_at_max() {
        let mw = RetryMiddleware::new(10, 1000, 5000);
        // 1000 * 2^3 = 8000 > 5000, so should be capped
        assert_eq!(mw.delay_for_attempt(3), 5000);
        assert_eq!(mw.delay_for_attempt(10), 5000);
    }
}
