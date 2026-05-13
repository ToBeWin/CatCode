//! # catcode-context
//!
//! Context engineering and memory system for the CatCode AI coding agent.
//!
//! This crate provides the layered context model, token budget tracking,
//! context compression, and memory persistence for managing the agent's
//! conversation context efficiently.
//!
//! ## Architecture
//!
//! The context system has three main components:
//!
//! - **ContextStack** — layered context model (permanent, session, working)
//! - **TokenBudget** — token usage tracking and limit enforcement
//! - **ContextCompressor** — basic context compression pipeline
//! - **TieredCompactor** — 4-level tiered compaction (Micro, Snip, Collapse, Full)
//!
//! And two memory subsystems:
//!
//! - **SessionMemory** — file-based memory (markdown files with frontmatter)
//! - **ArchiveMemory** — in-memory structured fact store

/// The `archive_memory` module.
pub mod archive_memory;
/// The `compressor` module.
pub mod compressor;
/// The `context_stack` module.
pub mod context_stack;
/// The `prompt_cache` module.
pub mod prompt_cache;
/// The `session_memory` module.
pub mod session_memory;
/// The `token_budget` module.
pub mod token_budget;

pub use archive_memory::ArchiveMemory;
pub use compressor::{CompactConfig, CompactTier, ContextCompressor, TieredCompactor};
pub use context_stack::{ContextStack, PermanentLayer, SessionLayer, WorkingLayer};
pub use prompt_cache::{CachePlan, CacheStats, PromptCacheOptimizer};
pub use session_memory::SessionMemory;
pub use token_budget::TokenBudget;

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::ToolResult;
    use catcode_core::memory::{ArchiveFact, FactCategory, MemoryEntry, MemoryType};
    use tempfile::TempDir;

    // === Integration tests that span multiple modules ===

    #[test]
    fn test_full_context_workflow() {
        // Create a context stack
        let mut stack = ContextStack::new("You are a Rust expert.", "Use cargo clippy.");
        stack.session.task_description = "Fix auth module".to_string();

        // Add some tool results
        stack.add_tool_result(
            "call_1",
            "read_file",
            ToolResult::success("fn main() { println!(\"hello\"); }"),
        );
        stack.add_tool_result("call_2", "bash", ToolResult::success("cargo check passed"));
        stack.add_tool_result(
            "call_3",
            "read_file",
            ToolResult::success("fn auth() { /* updated */ }"),
        );

        // Build messages before compression
        let messages = stack.build_messages();
        assert!(!messages.is_empty());
        assert_eq!(messages[0].role, catcode_core::Role::System);

        // Compress
        let compressor = ContextCompressor::new();
        compressor.compress(&mut stack);

        // After dedup, should have 2 unique tools
        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
    }

    #[test]
    fn test_budget_with_context() {
        let mut budget = TokenBudget::new(500_000, 50_000, 0.80);
        let mut stack = ContextStack::new("System prompt", "Rules");

        // Simulate a conversation
        stack.add_user_message("Fix the auth bug");
        stack.add_assistant_message("I'll look at auth.rs");
        stack.add_tool_result("1", "read_file", ToolResult::success("content"));

        // Simulate token usage
        budget.record_usage(&catcode_core::TokenUsage {
            input_tokens: 10_000,
            output_tokens: 2_000,
            cache_read_tokens: 5_000,
            cache_creation_tokens: 0,
        });

        assert!(!budget.should_warn());
        assert!(!budget.is_exhausted());
        assert!((budget.remaining_ratio() - 0.976).abs() < 0.01);
    }

    #[test]
    fn test_memory_round_trip() {
        let tmp = TempDir::new().unwrap();
        let memory = SessionMemory::new(tmp.path().join("memory"));
        memory.init().unwrap();

        // Save entries of different types
        let entries = vec![
            MemoryEntry {
                name: "user-pref".to_string(),
                description: "Prefer concise responses".to_string(),
                memory_type: MemoryType::User,
                content: "Be concise.".to_string(),
            },
            MemoryEntry {
                name: "rust-style".to_string(),
                description: "Rust style guide".to_string(),
                memory_type: MemoryType::Project,
                content: "Use anyhow for errors.".to_string(),
            },
            MemoryEntry {
                name: "feedback-loop".to_string(),
                description: "Feedback on loops".to_string(),
                memory_type: MemoryType::Feedback,
                content: "Avoid nested loops.".to_string(),
            },
        ];

        for entry in &entries {
            memory.save_memory(entry).unwrap();
        }

        // Load all
        let loaded = memory.load_all().unwrap();
        assert_eq!(loaded.len(), 3);

        // Load by type
        let user_entries = memory.load_by_type(MemoryType::User).unwrap();
        assert_eq!(user_entries.len(), 1);
        assert_eq!(user_entries[0].name, "user-pref");

        // Index should contain all entries
        let index = memory.get_index_content().unwrap();
        assert!(index.contains("user-pref"));
        assert!(index.contains("rust-style"));
        assert!(index.contains("feedback-loop"));
    }

    #[test]
    fn test_archive_memory_workflow() {
        let mut archive = ArchiveMemory::new(10, 0.7);

        // Add facts
        archive
            .add_fact(ArchiveFact::new(
                "User prefers tabs over spaces",
                FactCategory::Preference,
                0.95,
            ))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new(
                "Project uses tokio for async",
                FactCategory::Knowledge,
                0.85,
            ))
            .unwrap();
        archive
            .add_fact(ArchiveFact::new(
                "Uncertain about deployment target",
                FactCategory::Context,
                0.4,
            ))
            .unwrap();

        // Get all
        let all = archive.get_facts(None);
        assert_eq!(all.len(), 3);

        // Get by category
        let prefs = archive.get_facts(Some(FactCategory::Preference));
        assert_eq!(prefs.len(), 1);

        // Prune removes low-confidence
        archive.prune();
        let remaining = archive.get_facts(None);
        assert_eq!(remaining.len(), 2); // 0.4 fact removed
    }

    #[test]
    fn test_context_stack_messages_structure() {
        let mut stack = ContextStack::new("System", "Rules");
        stack.permanent.user_preferences = "Be verbose".to_string();
        stack.session.task_description = "Refactor".to_string();
        stack
            .session
            .completed_steps
            .push("Step 1 done".to_string());
        stack.working.current_files.push("src/main.rs".to_string());
        stack.working.current_errors.push("E0308".to_string());

        let messages = stack.build_messages();

        // System message
        assert_eq!(messages[0].role, catcode_core::Role::System);
        assert!(messages[0].content.contains("System"));
        assert!(messages[0].content.contains("Rules"));
        assert!(messages[0].content.contains("Be verbose"));

        // Session summary
        assert_eq!(messages[1].role, catcode_core::Role::User);
        assert!(messages[1].content.contains("Refactor"));
        assert!(messages[1].content.contains("Step 1 done"));

        // Working summary
        assert_eq!(messages[2].role, catcode_core::Role::User);
        assert!(messages[2].content.contains("src/main.rs"));
        assert!(messages[2].content.contains("E0308"));
    }

    #[test]
    fn test_compressor_with_various_output_sizes() {
        let compressor = ContextCompressor {
            max_tool_output_chars: 100,
            keep_recent_turns: 2,
        };
        let mut stack = ContextStack::new("Sys", "Rules");

        // Small output — should not be truncated
        stack.add_tool_result("1", "tool_a", ToolResult::success("small"));
        // Large output — should be truncated
        stack.add_tool_result("2", "tool_b", ToolResult::success("x".repeat(500)));
        // Medium output — should be truncated
        stack.add_tool_result("3", "tool_c", ToolResult::success("y".repeat(150)));
        // Another tool — exceeds keep_recent_turns after dedup
        stack.add_tool_result("4", "tool_d", ToolResult::success("z"));

        compressor.compress(&mut stack);

        // After dedup (4 unique tools) + truncation + prune to 2 most recent
        assert_eq!(stack.working.recent_tool_outputs.len(), 2);
        // The 2 most recent should be tool_c and tool_d
        assert_eq!(stack.working.recent_tool_outputs[0].0, "tool_c");
        assert_eq!(stack.working.recent_tool_outputs[1].0, "tool_d");

        // tool_c output should be truncated (150 > 100)
        assert!(
            stack.working.recent_tool_outputs[0]
                .1
                .output
                .contains("[truncated")
        );
    }
}
