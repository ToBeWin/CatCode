use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use md5::{Digest, Md5};
use std::collections::VecDeque;

/// Middleware that detects repeated tool calls (loops) within a sliding window.
///
/// Uses md5 hash of (tool_name + sorted args) to identify duplicate calls.
/// When the same call appears `warn_threshold` times, a warning is injected.
/// When it reaches `hard_limit`, the call is blocked with an error result.
#[derive(Debug)]
pub struct LoopDetectionMiddleware {
    warn_threshold: u32,
    hard_limit: u32,
    window_size: usize,
}

impl Default for LoopDetectionMiddleware {
    fn default() -> Self {
        Self {
            warn_threshold: 3,
            hard_limit: 5,
            window_size: 20,
        }
    }
}

impl LoopDetectionMiddleware {
    pub fn new(warn_threshold: u32, hard_limit: u32, window_size: usize) -> Self {
        Self {
            warn_threshold,
            hard_limit,
            window_size,
        }
    }

    /// Compute a deterministic hash for a tool call: md5(tool_name + sorted args).
    fn compute_hash(call: &ToolCall) -> String {
        let mut hasher = Md5::new();
        hasher.update(call.name.as_bytes());
        // Sort object keys for deterministic serialization
        let args_str = if call.args.is_object() {
            let sorted = sort_json_keys(&call.args);
            serde_json::to_string(&sorted).unwrap_or_else(|_| call.args.to_string())
        } else {
            call.args.to_string()
        };
        hasher.update(args_str.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Recursively sort JSON object keys for deterministic hashing.
fn sort_json_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted_map = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                sorted_map.insert(key.clone(), sort_json_keys(&map[key]));
            }
            serde_json::Value::Object(sorted_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sort_json_keys).collect())
        }
        other => other.clone(),
    }
}

/// Load the sliding window from context metadata.
fn load_window(ctx: &AgentContext) -> VecDeque<String> {
    ctx.get_metadata("_loop_detection_window")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Save the sliding window back to context metadata.
fn save_window(ctx: &mut AgentContext, window: &VecDeque<String>) {
    if let Ok(val) = serde_json::to_value(window) {
        ctx.set_metadata("_loop_detection_window", val);
    }
}

#[async_trait]
impl Middleware for LoopDetectionMiddleware {
    fn name(&self) -> &str {
        "loop_detection"
    }

    async fn wrap_tool_call(
        &self,
        ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let hash = Self::compute_hash(call);
        let mut window = load_window(ctx);

        // Count occurrences of this hash in the current window
        let count = window.iter().filter(|h| *h == &hash).count() as u32;

        // Check hard limit BEFORE executing
        if count >= self.hard_limit {
            return ToolResult::error(format!(
                "[loop_detection] Hard limit reached: tool '{}' with identical args was called {} times in the last {} calls. Blocking execution to prevent infinite loop.",
                call.name, count, self.window_size
            ));
        }

        // Execute the tool
        let result = next.execute(call).await;

        // Add hash to window (after execution, so we count the current call too)
        window.push_back(hash);
        if window.len() > self.window_size {
            window.pop_front();
        }
        save_window(ctx, &window);

        // Check warn threshold (count includes the call we just made)
        let new_count = window
            .iter()
            .filter(|h| *h == &Self::compute_hash(call))
            .count() as u32;
        if new_count >= self.warn_threshold && new_count < self.hard_limit {
            // Inject warning into context metadata
            let warnings = ctx
                .get_metadata("_loop_warnings")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            let mut warnings = warnings;
            warnings.push(serde_json::json!({
                "tool": call.name,
                "count": new_count,
                "message": format!(
                    "Warning: tool '{}' with identical args called {} times (threshold: {})",
                    call.name, new_count, self.warn_threshold
                )
            }));
            ctx.set_metadata("_loop_warnings", serde_json::Value::Array(warnings));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::AgentContext;
    use catcode_core::tool::ToolCall;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "test_call".to_string(),
            name: name.to_string(),
            args,
        }
    }

    fn make_ctx() -> AgentContext {
        AgentContext::new("test_session")
    }

    fn success_handler()
    -> impl Fn(&ToolCall) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
    {
        |_call: &ToolCall| Box::pin(async { ToolResult::success("ok") })
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let call = make_call("read_file", serde_json::json!({"path": "src/main.rs"}));
        let h1 = LoopDetectionMiddleware::compute_hash(&call);
        let h2 = LoopDetectionMiddleware::compute_hash(&call);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_hash_different_for_different_args() {
        let call1 = make_call("read_file", serde_json::json!({"path": "a.rs"}));
        let call2 = make_call("read_file", serde_json::json!({"path": "b.rs"}));
        assert_ne!(
            LoopDetectionMiddleware::compute_hash(&call1),
            LoopDetectionMiddleware::compute_hash(&call2)
        );
    }

    #[test]
    fn test_compute_hash_same_for_reordered_keys() {
        let call1 = make_call("bash", serde_json::json!({"command": "ls", "timeout": 30}));
        let call2 = make_call("bash", serde_json::json!({"timeout": 30, "command": "ls"}));
        assert_eq!(
            LoopDetectionMiddleware::compute_hash(&call1),
            LoopDetectionMiddleware::compute_hash(&call2)
        );
    }

    #[tokio::test]
    async fn test_loop_detection_no_issue_within_threshold() {
        let mw = LoopDetectionMiddleware::new(3, 5, 20);
        let mut ctx = make_ctx();
        let call = make_call("read_file", serde_json::json!({"path": "a.rs"}));
        let handler = success_handler();

        // First two calls should succeed without warnings
        for _ in 0..2 {
            let next = ToolCallNext::new(&handler);
            let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
            assert!(!result.is_error);
        }
    }

    #[tokio::test]
    async fn test_loop_detection_warns_at_threshold() {
        let mw = LoopDetectionMiddleware::new(3, 5, 20);
        let mut ctx = make_ctx();
        let call = make_call("read_file", serde_json::json!({"path": "a.rs"}));
        let handler = success_handler();

        // Call 3 times to hit warn threshold
        for _ in 0..3 {
            let next = ToolCallNext::new(&handler);
            let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
            assert!(!result.is_error);
        }

        // Should have a warning in metadata
        let warnings = ctx.get_metadata("_loop_warnings");
        assert!(warnings.is_some());
        let warnings = warnings.unwrap().as_array().unwrap();
        assert!(!warnings.is_empty());
    }

    #[tokio::test]
    async fn test_loop_detection_blocks_at_hard_limit() {
        let mw = LoopDetectionMiddleware::new(3, 5, 20);
        let mut ctx = make_ctx();
        let call = make_call("read_file", serde_json::json!({"path": "a.rs"}));
        let handler = success_handler();

        // Call 5 times to reach hard limit
        for _ in 0..5 {
            let next = ToolCallNext::new(&handler);
            mw.wrap_tool_call(&mut ctx, &call, next).await;
        }

        // 6th call should be blocked
        let next = ToolCallNext::new(&handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert!(result.output.contains("Hard limit reached"));
    }

    #[tokio::test]
    async fn test_loop_detection_window_slides() {
        let mw = LoopDetectionMiddleware::new(3, 3, 5);
        let mut ctx = make_ctx();
        let handler = success_handler();

        // Fill window with different calls (no loops)
        for i in 0..5 {
            let call = make_call(
                "read_file",
                serde_json::json!({"path": format!("file_{}.rs", i)}),
            );
            let next = ToolCallNext::new(&handler);
            mw.wrap_tool_call(&mut ctx, &call, next).await;
        }

        // Now call the same tool - should NOT be blocked because old calls slid out
        let call = make_call("read_file", serde_json::json!({"path": "file_0.rs"}));
        let next = ToolCallNext::new(&handler);
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
    }

    #[test]
    fn test_sort_json_keys_nested() {
        let val = serde_json::json!({
            "z": 1,
            "a": {
                "b": 2,
                "a": 1
            }
        });
        let sorted = sort_json_keys(&val);
        let s = serde_json::to_string(&sorted).unwrap();
        // Keys should be sorted: a before z, and nested a before b
        assert!(s.starts_with(r#"{"a":{"a":1,"b":2}"#));
    }
}
