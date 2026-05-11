use catcode_core::{ContextConfig, Message, ToolResult};
use std::collections::VecDeque;

/// Permanent context layer — included in every request, kept minimal.
///
/// Contains system prompt, project rules, and user preferences that form
/// the stable foundation of the conversation context.
#[derive(Debug, Clone)]
pub struct PermanentLayer {
    /// The system prompt defining the agent's behavior.
    pub system_prompt: String,
    /// Project-specific rules (from AGENTS.md or similar).
    pub project_rules: String,
    /// User preference hints for the model.
    pub user_preferences: String,
}

/// Session context layer — captures the current task and progress.
///
/// Summarizes what has happened in the session so far: the task description,
/// completed steps, and key decisions made during the conversation.
#[derive(Debug, Clone)]
pub struct SessionLayer {
    /// High-level description of the current task.
    pub task_description: String,
    /// Summary of completed steps.
    pub completed_steps: Vec<String>,
    /// Important decisions made during the session.
    pub key_decisions: Vec<String>,
}

/// Working context layer — the active working set that changes every turn.
///
/// Contains files currently being worked on, recent tool outputs, and
/// active errors. This layer is subject to compression.
#[derive(Debug, Clone)]
pub struct WorkingLayer {
    /// Files currently open or being edited.
    pub current_files: Vec<String>,
    /// Recent tool outputs: (tool_name, result).
    pub recent_tool_outputs: VecDeque<(String, ToolResult)>,
    /// Errors currently being resolved.
    pub current_errors: Vec<String>,
}

impl WorkingLayer {
    /// Deduplicate tool outputs, keeping only the latest output per tool name.
    ///
    /// When the same tool (e.g. `read_file`) has been called multiple times,
    /// only the most recent result is retained. This saves context space by
    /// removing stale data.
    pub fn dedup_tool_outputs(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut deduped = VecDeque::new();
        // Iterate in reverse so the latest entry per tool name wins
        for (name, result) in self.recent_tool_outputs.iter().rev() {
            if seen.insert(name.clone()) {
                deduped.push_front((name.clone(), result.clone()));
            }
        }
        self.recent_tool_outputs = deduped;
    }

    /// Truncate tool outputs exceeding `max_chars` characters.
    ///
    /// Large outputs (e.g. from bash commands) are truncated with a
    /// `[truncated, N chars total]` marker to preserve context budget
    /// while keeping the output recognizable.
    pub fn truncate_outputs(&mut self, max_chars: usize) {
        for (_name, result) in self.recent_tool_outputs.iter_mut() {
            if result.output.len() > max_chars {
                let truncated: String = result.output.chars().take(max_chars).collect();
                let original_len = result.output.len();
                result.output = format!("{truncated}\n[truncated, {original_len} chars total]");
            }
        }
    }
}

/// Layered context stack that assembles messages for LLM requests.
///
/// The context stack follows a four-layer model (though only three are
/// mutable at runtime):
///
/// 1. **Permanent layer** — system prompt, project rules, user preferences
/// 2. **Session layer** — task description, completed steps, key decisions
/// 3. **Working layer** — current files, recent tool outputs, active errors
/// 4. **Configuration** — context engineering settings
///
/// `build_messages()` assembles all layers into a `Vec<Message>` suitable
/// for inclusion in a `ChatRequest`.
#[derive(Debug, Clone)]
pub struct ContextStack {
    pub permanent: PermanentLayer,
    pub session: SessionLayer,
    pub working: WorkingLayer,
    pub config: ContextConfig,
}

impl ContextStack {
    /// Create a new context stack with the given system prompt and project rules.
    ///
    /// All other fields are initialized to empty/default values.
    pub fn new(system_prompt: impl Into<String>, project_rules: impl Into<String>) -> Self {
        Self {
            permanent: PermanentLayer {
                system_prompt: system_prompt.into(),
                project_rules: project_rules.into(),
                user_preferences: String::new(),
            },
            session: SessionLayer {
                task_description: String::new(),
                completed_steps: Vec::new(),
                key_decisions: Vec::new(),
            },
            working: WorkingLayer {
                current_files: Vec::new(),
                recent_tool_outputs: VecDeque::new(),
                current_errors: Vec::new(),
            },
            config: ContextConfig::default(),
        }
    }

    /// Build the full message list from all context layers.
    ///
    /// The output order is:
    /// 1. System message (permanent layer assembled)
    /// 2. Session context summary as a user message (if non-empty)
    /// 3. Working context as a user message (if non-empty)
    ///
    /// Note: actual user/assistant turn messages are typically managed
    /// by the agent loop and appended separately. This method builds
    /// the *contextual preamble* from the layered model.
    pub fn build_messages(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        // 1. System message from permanent layer
        let system_content = self.build_system_content();
        messages.push(Message::system(system_content));

        // 2. Session context summary
        let session_summary = self.build_session_summary();
        if !session_summary.is_empty() {
            messages.push(Message::user(session_summary));
        }

        // 3. Working context summary
        let working_summary = self.build_working_summary();
        if !working_summary.is_empty() {
            messages.push(Message::user(working_summary));
        }

        messages
    }

    /// Add a user message to the session's completed steps.
    ///
    /// This records the user's input as part of the session history.
    pub fn add_user_message(&mut self, content: &str) {
        self.session
            .completed_steps
            .push(format!("User: {content}"));
    }

    /// Add an assistant message to the session's completed steps.
    ///
    /// This records the assistant's response as part of the session history.
    pub fn add_assistant_message(&mut self, content: &str) {
        self.session
            .completed_steps
            .push(format!("Assistant: {content}"));
    }

    /// Record a tool result in the working layer.
    ///
    /// The tool output is appended to the recent tool outputs deque.
    /// If dedup is enabled in config, old outputs for the same tool
    /// can be cleaned up later via `ContextCompressor`.
    pub fn add_tool_result(&mut self, _call_id: &str, tool_name: &str, result: ToolResult) {
        self.working
            .recent_tool_outputs
            .push_back((tool_name.to_string(), result));
    }

    /// Build the system content string from the permanent layer.
    fn build_system_content(&self) -> String {
        let mut parts = Vec::new();
        parts.push(self.permanent.system_prompt.clone());
        if !self.permanent.project_rules.is_empty() {
            parts.push(format!("Project rules:\n{}", self.permanent.project_rules));
        }
        if !self.permanent.user_preferences.is_empty() {
            parts.push(format!(
                "User preferences:\n{}",
                self.permanent.user_preferences
            ));
        }
        parts.join("\n\n")
    }

    /// Build a summary of the session layer for injection into context.
    fn build_session_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.session.task_description.is_empty() {
            parts.push(format!("Task: {}", self.session.task_description));
        }
        if !self.session.completed_steps.is_empty() {
            let steps = self.session.completed_steps.join("\n- ");
            parts.push(format!("Completed steps:\n- {steps}"));
        }
        if !self.session.key_decisions.is_empty() {
            let decisions = self.session.key_decisions.join("\n- ");
            parts.push(format!("Key decisions:\n- {decisions}"));
        }
        parts.join("\n\n")
    }

    /// Build a summary of the working layer for injection into context.
    fn build_working_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.working.current_files.is_empty() {
            let files = self.working.current_files.join(", ");
            parts.push(format!("Current files: {files}"));
        }
        if !self.working.current_errors.is_empty() {
            let errors = self.working.current_errors.join("\n- ");
            parts.push(format!("Active errors:\n- {errors}"));
        }
        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::Role;

    fn make_stack() -> ContextStack {
        ContextStack::new("You are a helpful assistant.", "Use Rust idioms.")
    }

    #[test]
    fn test_new_context_stack() {
        let stack = make_stack();
        assert_eq!(
            stack.permanent.system_prompt,
            "You are a helpful assistant."
        );
        assert_eq!(stack.permanent.project_rules, "Use Rust idioms.");
        assert!(stack.permanent.user_preferences.is_empty());
        assert!(stack.session.task_description.is_empty());
        assert!(stack.session.completed_steps.is_empty());
        assert!(stack.working.current_files.is_empty());
        assert!(stack.working.recent_tool_outputs.is_empty());
    }

    #[test]
    fn test_build_messages_system_only() {
        let stack = make_stack();
        let messages = stack.build_messages();
        // Should have exactly 1 system message when session/working are empty
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::System);
        assert!(messages[0].content.contains("You are a helpful assistant."));
        assert!(messages[0].content.contains("Use Rust idioms."));
    }

    #[test]
    fn test_build_messages_with_session() {
        let mut stack = make_stack();
        stack.session.task_description = "Fix auth bug".to_string();
        stack
            .session
            .completed_steps
            .push("Read auth.rs".to_string());
        stack.session.key_decisions.push("Use JWT".to_string());

        let messages = stack.build_messages();
        assert_eq!(messages.len(), 2); // system + session summary
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);
        assert!(messages[1].content.contains("Fix auth bug"));
        assert!(messages[1].content.contains("Read auth.rs"));
        assert!(messages[1].content.contains("Use JWT"));
    }

    #[test]
    fn test_build_messages_with_working() {
        let mut stack = make_stack();
        stack.working.current_files.push("src/main.rs".to_string());
        stack
            .working
            .current_errors
            .push("E0308 mismatch".to_string());

        let messages = stack.build_messages();
        assert_eq!(messages.len(), 2); // system + working summary
        assert!(messages[1].content.contains("src/main.rs"));
        assert!(messages[1].content.contains("E0308 mismatch"));
    }

    #[test]
    fn test_build_messages_full() {
        let mut stack = make_stack();
        stack.session.task_description = "Fix auth".to_string();
        stack.working.current_files.push("src/lib.rs".to_string());

        let messages = stack.build_messages();
        assert_eq!(messages.len(), 3); // system + session + working
    }

    #[test]
    fn test_add_user_message() {
        let mut stack = make_stack();
        stack.add_user_message("Hello, fix this bug");
        assert_eq!(stack.session.completed_steps.len(), 1);
        assert!(stack.session.completed_steps[0].contains("Hello, fix this bug"));
    }

    #[test]
    fn test_add_assistant_message() {
        let mut stack = make_stack();
        stack.add_assistant_message("I'll fix it now");
        assert_eq!(stack.session.completed_steps.len(), 1);
        assert!(stack.session.completed_steps[0].contains("I'll fix it now"));
    }

    #[test]
    fn test_add_tool_result() {
        let mut stack = make_stack();
        let result = ToolResult::success("file content here");
        stack.add_tool_result("call_1", "read_file", result);

        assert_eq!(stack.working.recent_tool_outputs.len(), 1);
        assert_eq!(stack.working.recent_tool_outputs[0].0, "read_file");
        assert_eq!(
            stack.working.recent_tool_outputs[0].1.output,
            "file content here"
        );
    }

    #[test]
    fn test_user_preferences_in_system() {
        let mut stack = make_stack();
        stack.permanent.user_preferences = "Be concise".to_string();

        let messages = stack.build_messages();
        assert!(messages[0].content.contains("User preferences:"));
        assert!(messages[0].content.contains("Be concise"));
    }

    #[test]
    fn test_working_layer_dedup_tool_outputs() {
        let mut layer = WorkingLayer {
            current_files: vec![],
            recent_tool_outputs: VecDeque::new(),
            current_errors: vec![],
        };

        layer
            .recent_tool_outputs
            .push_back(("read_file".to_string(), ToolResult::success("old content")));
        layer
            .recent_tool_outputs
            .push_back(("bash".to_string(), ToolResult::success("ls output")));
        layer
            .recent_tool_outputs
            .push_back(("read_file".to_string(), ToolResult::success("new content")));

        layer.dedup_tool_outputs();

        assert_eq!(layer.recent_tool_outputs.len(), 2);
        // read_file should have the latest content
        let read_file = layer
            .recent_tool_outputs
            .iter()
            .find(|(n, _)| n == "read_file")
            .unwrap();
        assert_eq!(read_file.1.output, "new content");
        // bash should be preserved
        let bash = layer
            .recent_tool_outputs
            .iter()
            .find(|(n, _)| n == "bash")
            .unwrap();
        assert_eq!(bash.1.output, "ls output");
    }

    #[test]
    fn test_working_layer_dedup_preserves_order() {
        let mut layer = WorkingLayer {
            current_files: vec![],
            recent_tool_outputs: VecDeque::new(),
            current_errors: vec![],
        };

        layer
            .recent_tool_outputs
            .push_back(("a".to_string(), ToolResult::success("1")));
        layer
            .recent_tool_outputs
            .push_back(("b".to_string(), ToolResult::success("2")));
        layer
            .recent_tool_outputs
            .push_back(("a".to_string(), ToolResult::success("3")));

        layer.dedup_tool_outputs();

        // Order should be: b, a (latest a)
        assert_eq!(layer.recent_tool_outputs[0].0, "b");
        assert_eq!(layer.recent_tool_outputs[1].0, "a");
        assert_eq!(layer.recent_tool_outputs[1].1.output, "3");
    }

    #[test]
    fn test_working_layer_truncate_outputs() {
        let mut layer = WorkingLayer {
            current_files: vec![],
            recent_tool_outputs: VecDeque::new(),
            current_errors: vec![],
        };

        let long_output = "x".repeat(5000);
        layer
            .recent_tool_outputs
            .push_back(("bash".to_string(), ToolResult::success(&long_output)));
        layer
            .recent_tool_outputs
            .push_back(("read_file".to_string(), ToolResult::success("short")));

        layer.truncate_outputs(2000);

        // Long output should be truncated
        let bash = &layer.recent_tool_outputs[0];
        assert!(bash.1.output.contains("[truncated"));
        assert!(bash.1.output.contains("5000 chars total"));

        // Short output should be unchanged
        let read = &layer.recent_tool_outputs[1];
        assert_eq!(read.1.output, "short");
    }

    #[test]
    fn test_working_layer_truncate_exact_boundary() {
        let mut layer = WorkingLayer {
            current_files: vec![],
            recent_tool_outputs: VecDeque::new(),
            current_errors: vec![],
        };

        let exact_output = "a".repeat(2000);
        layer
            .recent_tool_outputs
            .push_back(("bash".to_string(), ToolResult::success(&exact_output)));

        layer.truncate_outputs(2000);
        // Exactly at limit should NOT be truncated
        assert!(!layer.recent_tool_outputs[0].1.output.contains("[truncated"));
    }

    #[test]
    fn test_working_layer_dedup_empty() {
        let mut layer = WorkingLayer {
            current_files: vec![],
            recent_tool_outputs: VecDeque::new(),
            current_errors: vec![],
        };
        layer.dedup_tool_outputs();
        assert!(layer.recent_tool_outputs.is_empty());
    }

    #[test]
    fn test_working_layer_dedup_single_entry() {
        let mut layer = WorkingLayer {
            current_files: vec![],
            recent_tool_outputs: VecDeque::new(),
            current_errors: vec![],
        };
        layer
            .recent_tool_outputs
            .push_back(("read_file".to_string(), ToolResult::success("content")));
        layer.dedup_tool_outputs();
        assert_eq!(layer.recent_tool_outputs.len(), 1);
        assert_eq!(layer.recent_tool_outputs[0].1.output, "content");
    }
}
