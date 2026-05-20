use crate::ContextStack;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tier of compaction to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactTier {
    /// Level 1: Replace old tool outputs with summaries in-place (no API)
    /// [`Micro`].
    Micro,
    /// Level 2: Remove stale/irrelevant messages (no API)
    /// [`Snip`].
    Snip,
    /// Level 3: Merge consecutive read results (no API)
    /// [`Collapse`].
    Collapse,
    /// Level 4: LLM-based full conversation summary (1 API call)
    /// [`Full`].
    Full,
}

/// Configuration for the tiered compactor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactConfig {
    /// Max tool outputs before micro-compact kicks in.
    pub micro_threshold: usize,
    /// Max tool output chars before micro-compact summarizes.
    pub micro_max_chars: usize,
    /// Max messages before snip-compact kicks in.
    pub snip_message_threshold: usize,
    /// Max consecutive reads before collapse.
    pub collapse_consecutive_reads: usize,
    /// Context window threshold for auto full-compact (0 = disabled).
    pub full_auto_threshold_tokens: u64,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            micro_threshold: 15,
            micro_max_chars: 500,
            snip_message_threshold: 50,
            collapse_consecutive_reads: 5,
            full_auto_threshold_tokens: 0,
        }
    }
}

/// Tiered compactor implementing a 4-level compaction pipeline.
///
/// | Level | Name         | What it does                                          | API call? |
/// |-------|--------------|--------------------------------------------------------|-----------|
/// | 1     | MicroCompact | Replace old tool outputs with short summaries in-place | No        |
/// | 2     | SnipCompact  | Remove user-ignored/irrelevant messages                | No        |
/// | 3     | Collapse     | Merge consecutive reads into combined descriptions     | No        |
/// | 4     | FullCompact  | LLM summarizes conversation history                    | Yes (1)   |
#[derive(Debug, Clone, Default)]
/// [`TieredCompactor`]
pub struct TieredCompactor {
    pub base: ContextCompressor,
    pub config: CompactConfig,
}

impl TieredCompactor {
    /// Create a tiered compactor with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a tiered compactor with custom config.
    pub fn with_config(config: CompactConfig) -> Self {
        Self {
            base: ContextCompressor::default(),
            config,
        }
    }

    /// Run the 4-level tiered compaction pipeline.
    ///
    /// Levels 1–3 are automatic (no API call needed).
    /// Level 4 (Full) must be triggered separately via [`full_compact`].
    /// Returns the list of compaction tiers that were applied.
    pub fn compress_tiered(&mut self, stack: &mut ContextStack) -> Vec<CompactTier> {
        let mut applied = Vec::new();

        // Level 1: MicroCompact — always runs
        self.micro_compact(stack);
        applied.push(CompactTier::Micro);

        // Level 2: SnipCompact — runs if message count is high
        if self.should_snip(stack) {
            self.snip_compact(stack);
            applied.push(CompactTier::Snip);
        }

        // Level 3: ContextCollapse — runs if consecutive reads detected
        if self.should_collapse(stack) {
            self.collapse_consecutive_reads(stack);
            applied.push(CompactTier::Collapse);
        }

        // Always-on base compression: dedup, truncate, prune
        self.base.compress(stack);

        applied
    }

    /// Level 1: Replace large/repeated tool outputs with in-place summaries.
    /// No API call needed.
    pub fn micro_compact(&self, stack: &mut ContextStack) {
        // Phase 1: Summarize individual large outputs
        for (tool_name, result) in stack.working.recent_tool_outputs.iter_mut() {
            if result.output.len() > self.config.micro_max_chars {
                let len = result.output.len();
                result.output = format!("[summarized: {tool_name} returned {len} chars]");
            }
        }

        // Phase 2: Summarize repeated tool calls (>2 per tool)
        let to_summarize = {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for (name, _) in &stack.working.recent_tool_outputs {
                *counts.entry(name.clone()).or_insert(0) += 1;
            }
            counts
                .into_iter()
                .filter(|(_, c)| *c > 2)
                .collect::<HashMap<String, usize>>()
        };

        if to_summarize.is_empty() {
            return;
        }

        let old = std::mem::take(&mut stack.working.recent_tool_outputs);
        let mut running_counts: HashMap<String, usize> = HashMap::new();
        let mut summaries: HashMap<String, String> = HashMap::new();

        for (name, result) in old.into_iter() {
            if let Some(&total) = to_summarize.get(name.as_str()) {
                let count = running_counts.entry(name.clone()).or_insert(0);
                *count += 1;

                if *count <= total - 2 {
                    summaries
                        .entry(name.clone())
                        .or_insert_with(|| format!("[summarized: {name} called {total} times]"));
                    continue;
                }
            }
            stack.working.recent_tool_outputs.push_back((name, result));
        }

        for (tool, summary) in summaries {
            stack
                .working
                .recent_tool_outputs
                .push_front((tool, catcode_core::ToolResult::success(summary)));
        }
    }

    /// Level 2: Remove stale/irrelevant messages and completed steps.
    pub fn snip_compact(&self, stack: &mut ContextStack) {
        // Trim completed_steps if too many
        if stack.session.completed_steps.len() > 20 {
            let keep = stack
                .session
                .completed_steps
                .split_off(stack.session.completed_steps.len() - 10);
            stack.session.completed_steps = keep;
        }

        // Filter tool outputs by relevance to current_files (if non-empty)
        if !stack.working.current_files.is_empty() {
            let focus: Vec<String> = stack
                .working
                .current_files
                .iter()
                .map(|f| f.to_lowercase())
                .collect();

            stack
                .working
                .recent_tool_outputs
                .retain(|(tool_name, result)| {
                    let name_lower = tool_name.to_lowercase();
                    let output_lower = result.output.to_lowercase();
                    focus.iter().any(|keyword| {
                        name_lower.contains(keyword)
                            || output_lower.contains(keyword)
                            || keyword.contains(&name_lower)
                    })
                });
        }
    }

    /// Level 3: Merge consecutive read_file calls into one summary entry.
    pub fn collapse_consecutive_reads(&self, stack: &mut ContextStack) {
        let min_consecutive = self.config.collapse_consecutive_reads;

        let mut consecutive = 0usize;
        for (name, _) in stack.working.recent_tool_outputs.iter().rev() {
            if name == "read_file" {
                consecutive += 1;
            } else {
                break;
            }
        }

        if consecutive < min_consecutive {
            return;
        }

        let old = std::mem::take(&mut stack.working.recent_tool_outputs);
        let len = old.len();

        for (i, entry) in old.into_iter().enumerate() {
            if i >= len - consecutive {
                continue;
            }
            stack.working.recent_tool_outputs.push_back(entry);
        }

        let summary = format!("[collapsed: read {consecutive} files]");
        stack.working.recent_tool_outputs.push_back((
            "read_file".to_string(),
            catcode_core::ToolResult::success(summary),
        ));
    }

    /// Level 4: LLM-based conversation summary (requires 1 API call).
    ///
    /// Uses the provided `summarize` callback to condense session history.
    /// The callback receives the raw session text and returns a summary string.
    pub fn full_compact<F>(&self, stack: &mut ContextStack, summarize: F)
    where
        F: Fn(&str) -> String,
    {
        let session_text = stack.session.completed_steps.join("\n");
        if session_text.is_empty() {
            return;
        }

        let summary = summarize(&session_text);

        stack
            .session
            .key_decisions
            .push("Session fully compacted via LLM summary".to_string());
        stack.session.completed_steps.clear();
        stack
            .session
            .completed_steps
            .push(format!("[Session Summary]\n{summary}"));
    }

    fn should_snip(&self, stack: &ContextStack) -> bool {
        stack.session.completed_steps.len() > self.config.snip_message_threshold
            || stack.working.recent_tool_outputs.len() > self.config.snip_message_threshold
    }

    fn should_collapse(&self, stack: &ContextStack) -> bool {
        let mut consecutive = 0usize;
        for (name, _) in stack.working.recent_tool_outputs.iter().rev() {
            if name == "read_file" {
                consecutive += 1;
            } else {
                break;
            }
        }
        consecutive >= self.config.collapse_consecutive_reads
    }
}

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
    /// 4. Rolls session history when working layer is bloated
    /// 5. Filters tool outputs by relevance to current focus
    pub fn compress(&self, stack: &mut ContextStack) {
        // Step 1: Dedup tool outputs
        if stack.config.dedup_tool_outputs {
            stack.working.dedup_tool_outputs();
        }

        // Step 2: Truncate large outputs
        stack.working.truncate_outputs(self.max_tool_output_chars);

        // Step 3: Keep only recent N entries
        self.prune_old_outputs(stack);

        // Step 4: Roll session history if enabled and working layer is bloated
        if stack.config.roll_history_enabled {
            let threshold = self.keep_recent_turns * 2;
            if stack.working.recent_tool_outputs.len() > threshold {
                self.roll_session_history(stack);
            }
        }

        // Step 5: Filter by relevance if enabled (uses current_files as focus)
        if stack.config.filter_relevance_enabled {
            let current_focus: Vec<String> = stack.working.current_files.clone();
            if !current_focus.is_empty() {
                self.filter_by_relevance(stack, &current_focus);
            }
        }
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

    /// Roll session history when the working layer has too many tool outputs.
    ///
    /// Moves completed steps from session memory into a summary string and
    /// prunes older entries, keeping only the most critical completed steps.
    /// This reduces context bloat by replacing verbose step history with a
    /// compressed summary.
    pub fn roll_session_history(&self, stack: &mut ContextStack) {
        let completed_count = stack.session.completed_steps.len();
        if completed_count == 0 {
            return;
        }

        // Build a summary from completed steps
        let summary_lines: Vec<&str> = stack
            .session
            .completed_steps
            .iter()
            .map(|s| s.as_str())
            .collect();
        let summary = format!(
            "[Session rollover — {} completed steps summarized]\n{}",
            completed_count,
            summary_lines.join("\n")
        );

        // Mark rollover in key_decisions
        stack.session.key_decisions.push(format!(
            "Session history rolled over at {completed_count} steps"
        ));

        // Keep only the most critical completed steps (last 5 entries)
        if completed_count > 5 {
            let keep = stack.session.completed_steps.split_off(completed_count - 5);
            stack.session.completed_steps = keep;
        }

        // Save the full summary so it can be included in context
        // We store it as a note in key_decisions for reference
        stack
            .session
            .key_decisions
            .push(format!("Summarized steps:\n{summary}"));
    }

    /// Filter tool outputs by relevance to the current focus keywords/paths.
    ///
    /// Examines the working layer's recent tool outputs and removes entries
    /// whose tool name and output content don't overlap with any of the
    /// `current_focus` strings. Relevance is determined by simple substring
    /// matching: if the focus keyword appears in the tool name or output,
    /// the entry is kept.
    pub fn filter_by_relevance(&self, stack: &mut ContextStack, current_focus: &[String]) {
        if current_focus.is_empty() || stack.working.recent_tool_outputs.is_empty() {
            return;
        }

        let focus_lower: Vec<String> = current_focus.iter().map(|f| f.to_lowercase()).collect();

        stack
            .working
            .recent_tool_outputs
            .retain(|(tool_name, result)| {
                let tool_lower = tool_name.to_lowercase();
                let output_lower = result.output.to_lowercase();

                // Keep if any focus keyword matches tool name or output content
                focus_lower.iter().any(|keyword| {
                    tool_lower.contains(keyword)
                        || output_lower.contains(keyword)
                        || keyword.contains(&tool_lower)
                })
            });
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

    // === TieredCompactor tests ===

    fn make_tiered() -> TieredCompactor {
        TieredCompactor::new()
    }

    #[test]
    fn test_tiered_default_config() {
        let config = CompactConfig::default();
        assert_eq!(config.micro_threshold, 15);
        assert_eq!(config.micro_max_chars, 500);
        assert_eq!(config.snip_message_threshold, 50);
        assert_eq!(config.collapse_consecutive_reads, 5);
        assert_eq!(config.full_auto_threshold_tokens, 0);
    }

    #[test]
    fn test_micro_compact_truncates_large_outputs() {
        let comp = TieredCompactor {
            base: ContextCompressor::default(),
            config: CompactConfig {
                micro_threshold: 0,
                micro_max_chars: 50,
                snip_message_threshold: 50,
                collapse_consecutive_reads: 5,
                full_auto_threshold_tokens: 0,
            },
        };
        let mut stack = make_stack();

        stack.add_tool_result("1", "bash", ToolResult::success("x".repeat(100)));
        stack.add_tool_result("2", "read_file", ToolResult::success("short"));

        comp.micro_compact(&mut stack);

        let bash_out = &stack.working.recent_tool_outputs[0].1;
        assert!(bash_out.output.contains("[summarized:"));
        assert!(bash_out.output.contains("bash"));
        assert!(!bash_out.output.contains("x".repeat(100).as_str()));

        let rf_out = &stack.working.recent_tool_outputs[1].1;
        assert_eq!(rf_out.output, "short");
    }

    #[test]
    fn test_micro_compact_summarizes_repeated_tools() {
        let comp = TieredCompactor {
            base: ContextCompressor::default(),
            config: CompactConfig {
                micro_threshold: 0,
                micro_max_chars: 500,
                snip_message_threshold: 50,
                collapse_consecutive_reads: 5,
                full_auto_threshold_tokens: 0,
            },
        };
        let mut stack = make_stack();

        stack.add_tool_result("1", "read_file", ToolResult::success("a"));
        stack.add_tool_result("2", "read_file", ToolResult::success("b"));
        stack.add_tool_result("3", "read_file", ToolResult::success("c"));
        stack.add_tool_result("4", "read_file", ToolResult::success("d"));

        comp.micro_compact(&mut stack);

        // Should have: summary entry + last 2 read_file entries
        let has_summary = stack
            .working
            .recent_tool_outputs
            .iter()
            .any(|(n, r)| n == "read_file" && r.output.contains("[summarized:"));
        assert!(has_summary);

        let entries: Vec<_> = stack
            .working
            .recent_tool_outputs
            .iter()
            .filter(|(n, _)| n == "read_file")
            .collect();
        // Summary marker + last 2 kept = 3 read_file entries
        assert_eq!(entries.len(), 3);
        assert!(entries[0].1.output.contains("[summarized:"));
        assert_eq!(entries[1].1.output, "c");
        assert_eq!(entries[2].1.output, "d");
    }

    #[test]
    fn test_snip_compact_removes_old_steps() {
        let comp = make_tiered();
        let mut stack = make_stack();

        for i in 0..30 {
            stack.session.completed_steps.push(format!("Step {i}"));
        }

        comp.snip_compact(&mut stack);

        // Should keep only last 10
        assert_eq!(stack.session.completed_steps.len(), 10);
        assert_eq!(stack.session.completed_steps[0], "Step 20");
        assert_eq!(stack.session.completed_steps[9], "Step 29");
    }

    #[test]
    fn test_snip_compact_under_limit_no_trim() {
        let comp = make_tiered();
        let mut stack = make_stack();

        for i in 0..5 {
            stack.session.completed_steps.push(format!("Step {i}"));
        }

        comp.snip_compact(&mut stack);

        // Under 20 limit, should stay unchanged
        assert_eq!(stack.session.completed_steps.len(), 5);
    }

    #[test]
    fn test_collapse_consecutive_reads() {
        let comp = TieredCompactor {
            base: ContextCompressor::default(),
            config: CompactConfig {
                micro_threshold: 0,
                micro_max_chars: 500,
                snip_message_threshold: 50,
                collapse_consecutive_reads: 3,
                full_auto_threshold_tokens: 0,
            },
        };
        let mut stack = make_stack();

        stack.add_tool_result("1", "bash", ToolResult::success("ok"));
        stack.add_tool_result("2", "read_file", ToolResult::success("file a"));
        stack.add_tool_result("3", "read_file", ToolResult::success("file b"));
        stack.add_tool_result("4", "read_file", ToolResult::success("file c"));

        comp.collapse_consecutive_reads(&mut stack);

        // Should have: bash + collapsed read_file summary
        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
        assert_eq!(stack.working.recent_tool_outputs[0].0, "bash");
        assert_eq!(stack.working.recent_tool_outputs[1].0, "read_file");
        assert!(
            stack.working.recent_tool_outputs[1]
                .1
                .output
                .contains("[collapsed: read 3 files]")
        );
    }

    #[test]
    fn test_collapse_below_threshold_no_change() {
        let comp = TieredCompactor {
            base: ContextCompressor::default(),
            config: CompactConfig {
                micro_threshold: 0,
                micro_max_chars: 500,
                snip_message_threshold: 50,
                collapse_consecutive_reads: 5,
                full_auto_threshold_tokens: 0,
            },
        };
        let mut stack = make_stack();

        stack.add_tool_result("1", "read_file", ToolResult::success("a"));
        stack.add_tool_result("2", "read_file", ToolResult::success("b"));

        comp.collapse_consecutive_reads(&mut stack);

        // Only 2 consecutive reads, threshold is 5, no change
        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
    }

    #[test]
    fn test_tiered_pipeline_runs_micro_and_base() {
        // Micro always runs; Snip/Collapse are conditional
        let mut comp = TieredCompactor {
            base: ContextCompressor::default(),
            config: CompactConfig {
                micro_threshold: 0,
                micro_max_chars: 10,
                snip_message_threshold: 100,
                collapse_consecutive_reads: 100,
                full_auto_threshold_tokens: 0,
            },
        };
        let mut stack = make_stack();

        stack.add_tool_result(
            "1",
            "bash",
            ToolResult::success("very long output that exceeds limit"),
        );

        let applied = comp.compress_tiered(&mut stack);

        // Micro always applied
        assert!(applied.contains(&CompactTier::Micro));
        // Snip not applied (steps < 100, outputs < 100)
        assert!(!applied.contains(&CompactTier::Snip));
        // Collapse not applied (only 1 read_file)
        assert!(!applied.contains(&CompactTier::Collapse));
    }

    #[test]
    fn test_tiered_pipeline_triggers_snip_and_collapse() {
        let mut comp = TieredCompactor {
            base: ContextCompressor::default(),
            config: CompactConfig {
                micro_threshold: 0,
                micro_max_chars: 500,
                snip_message_threshold: 5,
                collapse_consecutive_reads: 2,
                full_auto_threshold_tokens: 0,
            },
        };
        let mut stack = make_stack();

        // Enough completed steps for snip
        for i in 0..10 {
            stack.session.completed_steps.push(format!("Step {i}"));
        }

        // Consecutive reads must be at the END for collapse detection
        stack.add_tool_result("1", "bash", ToolResult::success("ok"));
        stack.add_tool_result("2", "read_file", ToolResult::success("a"));
        stack.add_tool_result("3", "read_file", ToolResult::success("b"));

        let applied = comp.compress_tiered(&mut stack);

        assert!(applied.contains(&CompactTier::Micro));
        assert!(applied.contains(&CompactTier::Snip));
        // 2 consecutive reads at end >= 2 threshold
        assert!(applied.contains(&CompactTier::Collapse));
    }

    #[test]
    fn test_full_compact_no_api_stub() {
        let comp = make_tiered();
        let mut stack = make_stack();

        stack
            .session
            .completed_steps
            .push("Fixed auth bug".to_string());
        stack
            .session
            .completed_steps
            .push("Added JWT validation".to_string());
        stack
            .session
            .completed_steps
            .push("Wrote tests".to_string());

        comp.full_compact(&mut stack, |text| {
            format!("[LLM summary of {} chars]", text.len())
        });

        assert_eq!(stack.session.completed_steps.len(), 1);
        assert!(stack.session.completed_steps[0].contains("[Session Summary]"));
        assert!(stack.session.completed_steps[0].contains("LLM summary"));
        assert!(
            stack
                .session
                .key_decisions
                .iter()
                .any(|d| d.contains("fully compacted"))
        );
    }

    #[test]
    fn test_compress_backward_compatible() {
        // Legacy ContextCompressor::compress() still works unchanged
        let c = ContextCompressor::new();
        let mut stack = make_stack();

        stack.add_tool_result("1", "read_file", ToolResult::success("old"));
        stack.add_tool_result("2", "bash", ToolResult::success("ls"));
        stack.add_tool_result("3", "read_file", ToolResult::success("new"));

        c.compress(&mut stack);

        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
        let rf = stack
            .working
            .recent_tool_outputs
            .iter()
            .find(|(n, _)| n == "read_file")
            .unwrap();
        assert_eq!(rf.1.output, "new");
    }

    #[test]
    fn test_compact_tier_serde_roundtrip() {
        let tiers = vec![
            CompactTier::Micro,
            CompactTier::Snip,
            CompactTier::Collapse,
            CompactTier::Full,
        ];
        for tier in &tiers {
            let json = serde_json::to_string(tier).unwrap();
            let back: CompactTier = serde_json::from_str(&json).unwrap();
            assert_eq!(*tier, back);
        }
    }

    #[test]
    fn test_compact_config_serde() {
        let config = CompactConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: CompactConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.micro_threshold, config.micro_threshold);
        assert_eq!(back.micro_max_chars, config.micro_max_chars);
    }
}
