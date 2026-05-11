use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Type alias for the tool execution function passed to the chain.
pub type ToolFn =
    Arc<dyn Fn(&ToolCall) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync>;

/// An opaque Send+Sync wrapper around a raw pointer.
///
/// # Safety
/// The address must remain valid for the duration of its use, and the caller must
/// ensure no concurrent mutable access occurs. In the middleware chain, execution
/// is sequential (each middleware awaits the next), so concurrent access is impossible.
#[derive(Clone, Copy)]
struct SendPtr(usize);

// SAFETY: The middleware chain executes sequentially. Each middleware awaits the
// next before completing, so no concurrent mutable access occurs.
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

/// A chain of middleware that wraps tool call execution.
///
/// Execution flow for middlewares [A, B, C]:
///   A.before -> B.before -> C.before -> tool_fn -> C.after -> B.after -> A.after
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn add(&mut self, middleware: impl Middleware + 'static) {
        self.middlewares.push(Arc::new(middleware));
    }

    pub fn add_arc(&mut self, middleware: Arc<dyn Middleware>) {
        self.middlewares.push(middleware);
    }

    /// Execute a tool call through the middleware chain.
    pub async fn execute_tool(
        &self,
        ctx: &mut AgentContext,
        call: &ToolCall,
        tool_fn: ToolFn,
    ) -> ToolResult {
        if self.middlewares.is_empty() {
            return tool_fn(call).await;
        }

        // SAFETY: ctx_ptr is valid for the duration of this async fn.
        // The chain executes sequentially, so no concurrent mutable access occurs.
        let ctx_ptr = SendPtr(ctx as *mut AgentContext as usize);

        build_and_run(&self.middlewares, ctx_ptr, call, &tool_fn, 0).await
    }
}

/// Build a ToolCallNext for middleware at `index`, then execute it.
fn build_and_run<'a>(
    middlewares: &'a [Arc<dyn Middleware>],
    ctx_ptr: SendPtr,
    call: &'a ToolCall,
    tool_fn: &'a ToolFn,
    index: usize,
) -> Pin<Box<dyn Future<Output = ToolResult> + Send + 'a>> {
    Box::pin(async move {
        if index >= middlewares.len() {
            return tool_fn(call).await;
        }

        let middleware = middlewares[index].clone();

        // Build a ToolCallNext that continues the chain from the next middleware.
        let remaining: Vec<Arc<dyn Middleware>> = middlewares[index + 1..].to_vec();
        let tool_fn_clone = tool_fn.clone();

        let next = ToolCallNext::new(move |next_call: &ToolCall| {
            let remaining = remaining.clone();
            let tool_fn = tool_fn_clone.clone();
            let ptr = ctx_ptr; // Copy the SendPtr
            let next_call = next_call.clone();
            Box::pin(async move { build_and_run(&remaining, ptr, &next_call, &tool_fn, 0).await })
        });

        // SAFETY: ctx_ptr was created from a valid &mut AgentContext that lives
        // for the duration of execute_tool(). The chain executes sequentially.
        let ctx: &mut AgentContext = unsafe { &mut *(ctx_ptr.0 as *mut AgentContext) };
        middleware.wrap_tool_call(ctx, call, next).await
    })
}

impl std::fmt::Debug for MiddlewareChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareChain")
            .field(
                "middlewares",
                &self
                    .middlewares
                    .iter()
                    .map(|m| m.name())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
    use catcode_core::tool::{ToolCall, ToolResult};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug)]
    struct TrackingMiddleware {
        name: String,
        order: Arc<AtomicU32>,
        log: Arc<Mutex<Vec<(String, u32)>>>,
    }

    impl TrackingMiddleware {
        fn new(name: &str, order: Arc<AtomicU32>, log: Arc<Mutex<Vec<(String, u32)>>>) -> Self {
            Self {
                name: name.to_string(),
                order,
                log,
            }
        }
    }

    #[async_trait::async_trait]
    impl Middleware for TrackingMiddleware {
        fn name(&self) -> &str {
            &self.name
        }

        async fn wrap_tool_call(
            &self,
            _ctx: &mut AgentContext,
            call: &ToolCall,
            next: ToolCallNext<'_>,
        ) -> ToolResult {
            let o = self.order.fetch_add(1, Ordering::SeqCst);
            self.log
                .lock()
                .unwrap()
                .push((format!("{}_before", self.name), o));
            let result = next.execute(call).await;
            let o = self.order.fetch_add(1, Ordering::SeqCst);
            self.log
                .lock()
                .unwrap()
                .push((format!("{}_after", self.name), o));
            result
        }
    }

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            id: "test".to_string(),
            name: name.to_string(),
            args: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn test_empty_chain_calls_tool() {
        let chain = MiddlewareChain::new();
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");
        let tool_fn: ToolFn = Arc::new(|_call| Box::pin(async { ToolResult::success("result") }));
        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "result");
    }

    #[tokio::test]
    async fn test_single_middleware_wraps_tool() {
        let mut chain = MiddlewareChain::new();
        let order = Arc::new(AtomicU32::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        chain.add(TrackingMiddleware::new("A", order, log.clone()));

        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");
        let tool_fn: ToolFn = Arc::new(|_call| Box::pin(async { ToolResult::success("result") }));
        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].0, "A_before");
        assert_eq!(log[1].0, "A_after");
    }

    #[tokio::test]
    async fn test_multiple_middlewares_execute_in_order() {
        let mut chain = MiddlewareChain::new();
        let order = Arc::new(AtomicU32::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        chain.add(TrackingMiddleware::new("A", order.clone(), log.clone()));
        chain.add(TrackingMiddleware::new("B", order.clone(), log.clone()));
        chain.add(TrackingMiddleware::new("C", order.clone(), log.clone()));

        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");
        let tool_fn: ToolFn = Arc::new(|_call| Box::pin(async { ToolResult::success("result") }));
        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 6);
        assert_eq!(log[0].0, "A_before");
        assert_eq!(log[1].0, "B_before");
        assert_eq!(log[2].0, "C_before");
        assert_eq!(log[3].0, "C_after");
        assert_eq!(log[4].0, "B_after");
        assert_eq!(log[5].0, "A_after");
    }

    #[tokio::test]
    async fn test_middleware_can_short_circuit() {
        #[derive(Debug)]
        struct BlockAll;

        #[async_trait::async_trait]
        impl Middleware for BlockAll {
            fn name(&self) -> &str {
                "block_all"
            }

            async fn wrap_tool_call(
                &self,
                _ctx: &mut AgentContext,
                _call: &ToolCall,
                _next: ToolCallNext<'_>,
            ) -> ToolResult {
                ToolResult::error("blocked by middleware")
            }
        }

        let mut chain = MiddlewareChain::new();
        chain.add(BlockAll);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");
        let tool_fn: ToolFn =
            Arc::new(|_call| Box::pin(async { ToolResult::success("should not reach here") }));
        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert!(result.is_error);
        assert_eq!(result.output, "blocked by middleware");
    }

    #[tokio::test]
    async fn test_middleware_can_modify_result() {
        #[derive(Debug)]
        struct Modifier;

        #[async_trait::async_trait]
        impl Middleware for Modifier {
            fn name(&self) -> &str {
                "modifier"
            }

            async fn wrap_tool_call(
                &self,
                _ctx: &mut AgentContext,
                call: &ToolCall,
                next: ToolCallNext<'_>,
            ) -> ToolResult {
                let mut result = next.execute(call).await;
                result.output = format!("modified: {}", result.output);
                result
            }
        }

        let mut chain = MiddlewareChain::new();
        chain.add(Modifier);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");
        let tool_fn: ToolFn = Arc::new(|_call| Box::pin(async { ToolResult::success("original") }));
        let result = chain.execute_tool(&mut ctx, &call, tool_fn).await;
        assert_eq!(result.output, "modified: original");
    }

    #[test]
    fn test_debug_format() {
        let chain = MiddlewareChain::new();
        let debug = format!("{:?}", chain);
        assert!(debug.contains("MiddlewareChain"));
    }
}
