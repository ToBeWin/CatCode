use crate::ContextStack;

/// Context compressor that reduces token usage by cleaning up the working layer.
///
/// The compressor runs a pipeline of operations:
/// 1. **Dedup tool outputs** — keep only the latest output per tool name
/// 2. **Truncate large outputs** — cap individual output size
/// 3. **Prune old outputs** — keep only the most recent N entries
///
/// # Example
///
/// ```
/// use catcode_context::{ContextStack, ContextCompressor};
/// use catcode_core::ToolResult;
///
/// let mut stack = ContextStack::new("System prompt", "Rules");
/// stack.add_tool_result("1", "read_file", ToolResult::success("old"));
/// stack.add_tool_result("2", "bash", ToolResult::success("output"));
/// stack.add_tool_result("3", "read_file", ToolResult::success("new"));
///
/// let compressor = ContextCompressor::new();
/// compressor.compress(&mut stack);
/// // Only "bash" and the latest "read_file" remain
/// ```
#[derive(Debug, Clone)]
pub struct ContextCompressor {
    /// Maximum characters per tool output before truncation.
    pub max_tool_output_chars: usize,
    /// Number of recent tool output entries to retain.
    pub keep_recent_turns: usize,
}

impl Default for ContextCompressor {
    fn default() -> Self {
        Self {
            max_tool_output_chars: 2000,
            keep_recent_turns: 10,
        }
    }
}

impl ContextCompressor {
    /// Create a compressor with default settings.
    ///
    /// Defaults: 2000 char output limit, keep 10 recent turns.
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the full compression pipeline on the given context stack.
    ///
    /// Applies all compression steps in order:
    /// 1. Deduplicates tool outputs (latest per tool name)
    /// 2. Truncates outputs exceeding `max_tool_output_chars`
    /// 3. Prunes old outputs, keeping only `keep_recent_turns` entries
    pub fn compress(&self, stack: &mut ContextStack) {
        // Step 1: Dedup tool outputs
        if stack.config.dedup_tool_outputs {
            stack.working.dedup_tool_outputs();
        }

        // Step 2: Truncate large outputs
        stack.working.truncate_outputs(self.max_tool_output_chars);

        // Step 3: Keep only recent N entries
        self.prune_old_outputs(stack);
    }

    /// Remove old tool outputs, keeping only the most recent entries.
    fn prune_old_outputs(&self, stack: &mut ContextStack) {
        let len = stack.working.recent_tool_outputs.len();
        if len > self.keep_recent_turns {
            let to_remove = len - self.keep_recent_turns;
            for _ in 0..to_remove {
                stack.working.recent_tool_outputs.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::ToolResult;

    fn make_compressor() -> ContextCompressor {
        ContextCompressor::new()
    }

    fn make_stack() -> ContextStack {
        ContextStack::new("System", "Rules")
    }

    #[test]
    fn test_default_config() {
        let c = ContextCompressor::default();
        assert_eq!(c.max_tool_output_chars, 2000);
        assert_eq!(c.keep_recent_turns, 10);
    }

    #[test]
    fn test_compress_empty() {
        let c = make_compressor();
        let mut stack = make_stack();
        c.compress(&mut stack);
        assert!(stack.working.recent_tool_outputs.is_empty());
    }

    #[test]
    fn test_compress_dedup() {
        let c = make_compressor();
        let mut stack = make_stack();

        stack.add_tool_result("1", "read_file", ToolResult::success("old content"));
        stack.add_tool_result("2", "bash", ToolResult::success("ls output"));
        stack.add_tool_result("3", "read_file", ToolResult::success("new content"));

        c.compress(&mut stack);

        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
        // Latest read_file should survive
        let rf = stack
            .working
            .recent_tool_outputs
            .iter()
            .find(|(n, _)| n == "read_file")
            .unwrap();
        assert_eq!(rf.1.output, "new content");
    }

    #[test]
    fn test_compress_truncates_large_outputs() {
        let c = make_compressor();
        let mut stack = make_stack();

        let big_output = "x".repeat(5000);
        stack.add_tool_result("1", "bash", ToolResult::success(&big_output));
        stack.add_tool_result("2", "read_file", ToolResult::success("small"));

        c.compress(&mut stack);

        let bash_out = &stack.working.recent_tool_outputs[0].1;
        assert!(bash_out.output.contains("[truncated"));
        assert!(bash_out.output.len() < big_output.len());

        let rf_out = &stack.working.recent_tool_outputs[1].1;
        assert_eq!(rf_out.output, "small");
    }

    #[test]
    fn test_compress_prunes_old_outputs() {
        let c = ContextCompressor {
            max_tool_output_chars: 2000,
            keep_recent_turns: 3,
        };
        let mut stack = make_stack();

        for i in 0..10 {
            stack.add_tool_result(
                &format!("call_{i}"),
                "bash",
                ToolResult::success(format!("output {i}")),
            );
        }

        c.compress(&mut stack);

        // After dedup (all same tool "bash" => 1 entry) + prune => 1 entry
        // Actually dedup keeps only latest for "bash"
        assert_eq!(stack.working.recent_tool_outputs.len(), 1);
        assert_eq!(stack.working.recent_tool_outputs[0].1.output, "output 9");
    }

    #[test]
    fn test_compress_prunes_different_tools() {
        let c = ContextCompressor {
            max_tool_output_chars: 2000,
            keep_recent_turns: 3,
        };
        let mut stack = make_stack();

        // 5 different tools, no dedup needed
        stack.add_tool_result("1", "tool_a", ToolResult::success("a1"));
        stack.add_tool_result("2", "tool_b", ToolResult::success("b1"));
        stack.add_tool_result("3", "tool_c", ToolResult::success("c1"));
        stack.add_tool_result("4", "tool_d", ToolResult::success("d1"));
        stack.add_tool_result("5", "tool_e", ToolResult::success("e1"));

        c.compress(&mut stack);

        // Should keep only last 3
        assert_eq!(stack.working.recent_tool_outputs.len(), 3);
        assert_eq!(stack.working.recent_tool_outputs[0].0, "tool_c");
        assert_eq!(stack.working.recent_tool_outputs[1].0, "tool_d");
        assert_eq!(stack.working.recent_tool_outputs[2].0, "tool_e");
    }

    #[test]
    fn test_compress_under_limit_no_prune() {
        let c = ContextCompressor {
            max_tool_output_chars: 2000,
            keep_recent_turns: 10,
        };
        let mut stack = make_stack();

        stack.add_tool_result("1", "a", ToolResult::success("1"));
        stack.add_tool_result("2", "b", ToolResult::success("2"));

        c.compress(&mut stack);
        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
    }

    #[test]
    fn test_compress_respects_dedup_config() {
        let c = make_compressor();
        let mut stack = make_stack();
        stack.config.dedup_tool_outputs = false;

        stack.add_tool_result("1", "read_file", ToolResult::success("old"));
        stack.add_tool_result("2", "read_file", ToolResult::success("new"));

        c.compress(&mut stack);

        // With dedup disabled, both entries should remain
        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
    }

    #[test]
    fn test_compress_error_outputs_preserved() {
        let c = make_compressor();
        let mut stack = make_stack();

        stack.add_tool_result("1", "bash", ToolResult::error("command failed"));
        stack.add_tool_result("2", "bash", ToolResult::success("ok"));

        c.compress(&mut stack);

        // After dedup, latest (success) wins
        assert_eq!(stack.working.recent_tool_outputs.len(), 1);
        assert_eq!(stack.working.recent_tool_outputs[0].1.output, "ok");
    }
}
