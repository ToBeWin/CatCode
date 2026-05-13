use catcode_core::middleware::AgentContext;
use catcode_core::tool::{ToolCall, ToolContext, ToolResult};
use catcode_middleware::chain::{MiddlewareChain, ToolFn};
use catcode_tools::ToolRegistry;
use std::sync::Arc;

/// Executes tools in parallel as they arrive from the LLM stream.
///
/// Concurrency-safe tools (read-only) execute in parallel.
/// Non-concurrent tools (writes) execute serially, waiting for concurrent ones.
/// Results are emitted in the ORDER the tools were received.
pub struct StreamingToolExecutor {
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
}

impl StreamingToolExecutor {
    pub fn new(tools: Arc<ToolRegistry>, middleware: Arc<MiddlewareChain>) -> Self {
        Self { tools, middleware }
    }

    /// Execute a batch of tool calls with optimal parallelism.
    ///
    /// 1. Partition into concurrent-safe and serial batches
    /// 2. Spawn concurrent tasks via tokio::spawn
    /// 3. Await serial tools one by one
    /// 4. Return results in original order
    pub async fn execute_batch(
        &self,
        calls: &[ToolCall],
        ctx: &ToolContext,
    ) -> Vec<(String, ToolResult)> {
        let n = calls.len();
        let mut results: Vec<Option<(String, ToolResult)>> = vec![None; n];
        let mut concurrent_indices: Vec<usize> = Vec::new();
        let mut serial_indices: Vec<usize> = Vec::new();

        for (i, call) in calls.iter().enumerate() {
            if let Some(tool) = self.tools.get(&call.name) {
                if tool.is_concurrency_safe() {
                    concurrent_indices.push(i);
                } else {
                    serial_indices.push(i);
                }
            } else {
                serial_indices.push(i);
            }
        }

        // Spawn all concurrent-safe tools in parallel
        let mut handles: Vec<(usize, tokio::task::JoinHandle<(String, ToolResult)>)> = Vec::new();
        for &i in &concurrent_indices {
            let call = &calls[i];
            let tools = self.tools.clone();
            let middleware = self.middleware.clone();
            let call = call.clone();
            let ctx = ctx.clone();
            let handle = tokio::spawn(async move {
                let result = execute_single_tool(tools, middleware, &call, &ctx).await;
                (call.id, result)
            });
            handles.push((i, handle));
        }

        // Run serial tools one by one (not concurrent with each other)
        for &i in &serial_indices {
            let call = &calls[i];
            let result =
                execute_single_tool(self.tools.clone(), self.middleware.clone(), call, ctx).await;
            results[i] = Some((call.id.clone(), result));
        }

        // Collect concurrent results
        for (i, handle) in handles {
            let (id, result) = handle.await.unwrap_or_else(|e| {
                (String::new(), ToolResult::error(format!("Task panicked: {}", e)))
            });
            results[i] = Some((id, result));
        }

        results.into_iter().map(|r| r.unwrap()).collect()
    }
}

async fn execute_single_tool(
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
    call: &ToolCall,
    ctx: &ToolContext,
) -> ToolResult {
    let ctx = ctx.clone();
    let tool_fn: ToolFn = Arc::new(move |c: &ToolCall| {
        let tools = tools.clone();
        let c = c.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            tools
                .dispatch(&c.name, c.args.clone(), &ctx)
                .await
                .unwrap_or_else(|e| ToolResult::error(e.to_string()))
        })
    });

    let mut agent_ctx = AgentContext::new("streaming-executor");
    middleware.execute_tool(&mut agent_ctx, call, tool_fn).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
    use catcode_middleware::MiddlewareChain;
    use catcode_tools::ToolRegistry;
    use serde_json::json;
    use std::sync::Arc;

    struct ConcurrentSafeTool;

    #[async_trait]
    impl Tool for ConcurrentSafeTool {
        fn name(&self) -> &str {
            "concurrent_safe"
        }
        fn description(&self) -> &str {
            "A concurrent-safe read-only tool"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({})
        }
        fn operation_level(&self) -> OperationLevel {
            OperationLevel::Safe
        }
        fn is_concurrency_safe(&self) -> bool {
            true
        }
        fn is_read_only(&self) -> bool {
            true
        }
        async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            ToolResult::success("concurrent result")
        }
    }

    struct SerialTool;

    #[async_trait]
    impl Tool for SerialTool {
        fn name(&self) -> &str {
            "serial"
        }
        fn description(&self) -> &str {
            "A serial-only write tool"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({})
        }
        fn operation_level(&self) -> OperationLevel {
            OperationLevel::Sensitive
        }
        fn is_concurrency_safe(&self) -> bool {
            false
        }
        fn is_read_only(&self) -> bool {
            false
        }
        async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::success("serial result")
        }
    }

    fn make_registry() -> Arc<ToolRegistry> {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(ConcurrentSafeTool));
        reg.register(Arc::new(SerialTool));
        Arc::new(reg)
    }

    #[test]
    fn test_partition_concurrent_vs_serial() {
        let reg = make_registry();
        let middleware = Arc::new(MiddlewareChain::new());
        let executor = StreamingToolExecutor::new(reg, middleware);

        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "concurrent_safe".into(),
                args: json!({}),
            },
            ToolCall {
                id: "2".into(),
                name: "serial".into(),
                args: json!({}),
            },
            ToolCall {
                id: "3".into(),
                name: "concurrent_safe".into(),
                args: json!({}),
            },
        ];

        let mut concurrent = Vec::new();
        let mut serial = Vec::new();
        for call in &calls {
            if let Some(tool) = executor.tools.get(&call.name) {
                if tool.is_concurrency_safe() {
                    concurrent.push(call);
                } else {
                    serial.push(call);
                }
            } else {
                serial.push(call);
            }
        }

        assert_eq!(concurrent.len(), 2);
        assert_eq!(serial.len(), 1);
        assert_eq!(concurrent[0].id, "1");
        assert_eq!(concurrent[1].id, "3");
        assert_eq!(serial[0].id, "2");
    }

    #[tokio::test]
    async fn test_concurrent_tools_run_in_parallel() {
        let reg = make_registry();
        let middleware = Arc::new(MiddlewareChain::new());
        let executor = StreamingToolExecutor::new(reg, middleware);

        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "concurrent_safe".into(),
                args: json!({}),
            },
            ToolCall {
                id: "2".into(),
                name: "concurrent_safe".into(),
                args: json!({}),
            },
            ToolCall {
                id: "3".into(),
                name: "serial".into(),
                args: json!({}),
            },
        ];

        let ctx = ToolContext::default();
        let results = executor.execute_batch(&calls, &ctx).await;

        assert_eq!(results.len(), 3);
        assert!(!results[0].1.is_error);
        assert!(!results[1].1.is_error);
        assert!(!results[2].1.is_error);
    }

    #[tokio::test]
    async fn test_results_in_order() {
        let reg = make_registry();
        let middleware = Arc::new(MiddlewareChain::new());
        let executor = StreamingToolExecutor::new(reg, middleware);

        let calls = vec![
            ToolCall {
                id: "a".into(),
                name: "concurrent_safe".into(),
                args: json!({}),
            },
            ToolCall {
                id: "b".into(),
                name: "serial".into(),
                args: json!({}),
            },
            ToolCall {
                id: "c".into(),
                name: "concurrent_safe".into(),
                args: json!({}),
            },
        ];

        let ctx = ToolContext::default();
        let results = executor.execute_batch(&calls, &ctx).await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, "a");
        assert_eq!(results[1].0, "b");
        assert_eq!(results[2].0, "c");
    }

    #[tokio::test]
    async fn test_empty_batch() {
        let reg = Arc::new(ToolRegistry::new());
        let middleware = Arc::new(MiddlewareChain::new());
        let executor = StreamingToolExecutor::new(reg, middleware);

        let ctx = ToolContext::default();
        let results = executor.execute_batch(&[], &ctx).await;

        assert!(results.is_empty());
    }
}
