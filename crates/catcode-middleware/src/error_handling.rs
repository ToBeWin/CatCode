use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Middleware that catches panics during tool execution,
/// converting them into `ToolResult::error` instead of propagating.
///
/// Uses `std::panic::catch_unwind` on the tool execution future. Panics that
/// occur during `Future::poll` are caught and converted to error results.
#[derive(Debug, Default)]
pub struct ToolErrorHandlingMiddleware;

impl ToolErrorHandlingMiddleware {
    pub fn new() -> Self {
        Self
    }
}

/// A future wrapper that catches panics during polling.
struct CatchUnwind<F> {
    inner: F,
}

impl<F: Future> Future for CatchUnwind<F>
where
    F: std::panic::UnwindSafe,
{
    type Output = std::result::Result<F::Output, String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We're not moving `inner` - we're just polling it.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        match catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(poll_result) => match poll_result {
                Poll::Ready(value) => Poll::Ready(Ok(value)),
                Poll::Pending => Poll::Pending,
            },
            Err(panic_err) => {
                let msg = extract_panic_message(panic_err);
                Poll::Ready(Err(msg))
            }
        }
    }
}

/// Extract a human-readable message from a panic payload.
fn extract_panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic (no message)".to_string()
    }
}

#[async_trait]
impl Middleware for ToolErrorHandlingMiddleware {
    fn name(&self) -> &str {
        "tool_error_handling"
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let future = next.execute(call);
        let wrapped = CatchUnwind {
            inner: AssertUnwindSafe(future),
        };

        match wrapped.await {
            Ok(result) => result,
            Err(panic_msg) => {
                tracing::error!(
                    tool = %call.name,
                    error = %panic_msg,
                    "Tool execution panicked"
                );
                ToolResult::error(format!(
                    "[tool_error_handling] Tool '{}' panicked: {}",
                    call.name, panic_msg
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
    async fn test_passes_through_on_success() {
        let mw = ToolErrorHandlingMiddleware::new();
        let mut ctx = AgentContext::new("test");
        let call = make_call("read_file");

        let handler = |_call: &ToolCall| -> Pin<Box<dyn Future<Output = ToolResult> + Send>> {
            Box::pin(async { ToolResult::success("file content") })
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "file content");
    }

    #[tokio::test]
    async fn test_passes_through_on_tool_error() {
        let mw = ToolErrorHandlingMiddleware::new();
        let mut ctx = AgentContext::new("test");
        let call = make_call("read_file");

        let handler = |_call: &ToolCall| -> Pin<Box<dyn Future<Output = ToolResult> + Send>> {
            Box::pin(async { ToolResult::error("file not found") })
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert_eq!(result.output, "file not found");
    }

    #[tokio::test]
    async fn test_catches_panic_in_async() {
        let mw = ToolErrorHandlingMiddleware::new();
        let mut ctx = AgentContext::new("test");
        let call = make_call("crashing_tool");

        let handler = |_call: &ToolCall| -> Pin<Box<dyn Future<Output = ToolResult> + Send>> {
            Box::pin(async {
                panic!("intentional panic for testing");
            })
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert!(result.output.contains("panicked"));
        assert!(result.output.contains("intentional panic for testing"));
    }

    #[tokio::test]
    async fn test_catches_panic_with_string() {
        let mw = ToolErrorHandlingMiddleware::new();
        let mut ctx = AgentContext::new("test");
        let call = make_call("another_tool");

        let handler = |_call: &ToolCall| -> Pin<Box<dyn Future<Output = ToolResult> + Send>> {
            Box::pin(async {
                let msg = String::from("something went wrong");
                panic!("{}", msg);
            })
        };
        let next = ToolCallNext::new(handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert!(result.output.contains("something went wrong"));
    }
}
