//! Streaming agent execution events for real-time TUI/API progress display.

use serde::{Deserialize, Serialize};

/// Event emitted by the AgentLoop during execution for real-time progress display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStreamEvent {
    /// Status update (e.g. "Calling API...", "Executing tool: bash")
    Status(String),
    /// Thinking/reasoning content from LLM stream
    Thinking(String),
    /// Tool call started
    ToolCall { tool: String, args: String },
    /// Tool execution completed with result
    ToolResult {
        tool: String,
        output: String,
        is_error: bool,
    },
    /// Final text response from LLM
    TextDelta(String),
    /// Token usage update
    TokenUpdate { input: u64, output: u64, cache: u64 },
    /// Execution encountered an error
    Error(String),
    /// Execution completed successfully
    Completed,
}

/// Shared type for the event channel between daemon and TUI.
pub type AgentEventSender = tokio::sync::mpsc::UnboundedSender<AgentStreamEvent>;
