use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};

use crate::error::Result;
use crate::tool::ToolResult;
use crate::types::{ChatRequest, ChatResponse, Message, TokenUsage, ToolCall};

// === AgentContext ===

#[derive(Debug)]
/// [`AgentContext`]
pub struct AgentContext {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub tool_outputs: VecDeque<ToolOutput>,
    pub metadata: HashMap<String, serde_json::Value>,
    usage_history: Vec<TokenUsage>,
}

#[derive(Debug, Clone)]
/// [`ToolOutput`]
pub struct ToolOutput {
    pub call_id: String,
    pub tool_name: String,
    pub result: ToolResult,
}

impl AgentContext {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            messages: Vec::new(),
            tool_outputs: VecDeque::new(),
            metadata: HashMap::new(),
            usage_history: Vec::new(),
        }
    }

/// Add message.
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

/// Add tool output.
    pub fn add_tool_output(&mut self, call_id: String, tool_name: String, result: ToolResult) {
        self.tool_outputs.push_back(ToolOutput {
            call_id,
            tool_name,
            result,
        });
    }

/// Record usage.
    pub fn record_usage(&mut self, usage: TokenUsage) {
        self.usage_history.push(usage);
    }

/// Total usage.
    pub fn total_usage(&self) -> TokenUsage {
        self.usage_history
            .iter()
            .fold(TokenUsage::default(), |acc, u| acc + u.clone())
    }

/// Set the metadata.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.metadata.insert(key.into(), value);
    }

/// Get the metadata.
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }
}

// === ToolCallNext (for middleware chain) ===

type ToolCallHandler<'a> = Box<
    dyn Fn(
            &ToolCall,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send + 'a>>
        + Send
        + Sync
        + 'a,
>;

/// [`ToolCallNext`]
pub struct ToolCallNext<'a> {
    inner: std::sync::Arc<ToolCallHandler<'a>>,
}

impl<'a> Clone for ToolCallNext<'a> {
    fn clone(&self) -> Self {
        Self {
            inner: std::sync::Arc::clone(&self.inner),
        }
    }
}

impl<'a> ToolCallNext<'a> {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn(&ToolCall) -> Fut + Send + Sync + 'a,
        Fut: std::future::Future<Output = ToolResult> + Send + 'a,
    {
        Self {
            inner: std::sync::Arc::new(Box::new(move |call| Box::pin(f(call)))),
        }
    }

/// Execute.
    pub async fn execute(&self, call: &ToolCall) -> ToolResult {
        (self.inner)(call).await
    }
}

// === Middleware Trait ===

#[async_trait]
/// [`Middleware`]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;

    async fn before_agent(&self, _ctx: &mut AgentContext) -> Result<()> {
        Ok(())
    }

    async fn after_agent(&self, _ctx: &mut AgentContext) -> Result<()> {
        Ok(())
    }

    async fn before_model(
        &self,
        _ctx: &mut AgentContext,
        _request: &mut ChatRequest,
    ) -> Result<()> {
        Ok(())
    }

    async fn after_model(&self, _ctx: &mut AgentContext, _response: &ChatResponse) -> Result<()> {
        Ok(())
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        next.execute(call).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_context_creation() {
        let ctx = AgentContext::new("session_123");
        assert_eq!(ctx.session_id, "session_123");
        assert!(ctx.messages.is_empty());
        assert!(ctx.tool_outputs.is_empty());
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn test_agent_context_add_message() {
        let mut ctx = AgentContext::new("session_123");
        ctx.add_message(Message::user("Hello"));
        assert_eq!(ctx.messages.len(), 1);
    }

    #[test]
    fn test_agent_context_token_usage() {
        let mut ctx = AgentContext::new("session_123");
        ctx.record_usage(TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        });
        ctx.record_usage(TokenUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_tokens: 50,
            cache_creation_tokens: 25,
        });
        let total = ctx.total_usage();
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 150);
        assert_eq!(total.cache_read_tokens, 50);
    }

    #[test]
    fn test_agent_context_metadata() {
        let mut ctx = AgentContext::new("session_123");
        ctx.set_metadata("model", serde_json::json!("deepseek-chat"));
        assert_eq!(
            ctx.get_metadata("model"),
            Some(&serde_json::json!("deepseek-chat"))
        );
        assert!(ctx.get_metadata("nonexistent").is_none());
    }

    #[test]
    fn test_agent_context_tool_output() {
        let mut ctx = AgentContext::new("session_123");
        ctx.add_tool_output(
            "call_1".to_string(),
            "read_file".to_string(),
            ToolResult::success("content"),
        );
        assert_eq!(ctx.tool_outputs.len(), 1);
        assert_eq!(ctx.tool_outputs[0].tool_name, "read_file");
        assert_eq!(ctx.tool_outputs[0].call_id, "call_1");
    }

    #[test]
    fn test_agent_context_multiple_messages() {
        let mut ctx = AgentContext::new("session_1");
        ctx.add_message(Message::system("be helpful"));
        ctx.add_message(Message::user("hello"));
        ctx.add_message(Message::assistant("hi!"));
        assert_eq!(ctx.messages.len(), 3);
    }

    #[test]
    fn test_agent_context_tool_output_with_error() {
        let mut ctx = AgentContext::new("session_1");
        ctx.add_tool_output(
            "call_err".to_string(),
            "bash".to_string(),
            ToolResult::error("command failed"),
        );
        assert_eq!(ctx.tool_outputs.len(), 1);
        assert!(ctx.tool_outputs[0].result.is_error);
    }

    #[test]
    fn test_agent_context_record_usage_zero() {
        let mut ctx = AgentContext::new("session_1");
        ctx.record_usage(TokenUsage::default());
        let total = ctx.total_usage();
        assert_eq!(total.input_tokens, 0);
        assert_eq!(total.output_tokens, 0);
    }

    #[test]
    fn test_agent_context_total_usage_empty() {
        let ctx = AgentContext::new("session_1");
        let total = ctx.total_usage();
        assert_eq!(total, TokenUsage::default());
    }

    #[test]
    fn test_agent_context_metadata_overwrite() {
        let mut ctx = AgentContext::new("session_1");
        ctx.set_metadata("key", serde_json::json!("first"));
        assert_eq!(ctx.get_metadata("key"), Some(&serde_json::json!("first")));
        ctx.set_metadata("key", serde_json::json!("second"));
        assert_eq!(ctx.get_metadata("key"), Some(&serde_json::json!("second")));
    }

    #[test]
    fn test_tool_output_creation() {
        let output = ToolOutput {
            call_id: "call_1".to_string(),
            tool_name: "read_file".to_string(),
            result: ToolResult::success("data"),
        };
        assert_eq!(output.call_id, "call_1");
        assert_eq!(output.tool_name, "read_file");
        assert!(!output.result.is_error);
    }

    #[test]
    fn test_agent_context_multiple_recordings() {
        let mut ctx = AgentContext::new("session_1");
        for _ in 0..5 {
            ctx.record_usage(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            });
        }
        let total = ctx.total_usage();
        assert_eq!(total.input_tokens, 50);
        assert_eq!(total.output_tokens, 25);
    }
}
