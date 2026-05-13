use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;
use crate::tokenizer::Tokenizer;
use crate::types::{ChatRequest, ChatResponse, TokenUsage};

// === ModelTier ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// [`ModelTier`]
pub enum ModelTier {
/// [`Fast`].
    Fast,
/// [`Balanced`].
    Balanced,
/// [`Powerful`].
    Powerful,
}

// === ModelInfo ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ModelInfo`]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
    pub context_window: u64,
    pub tier: ModelTier,
}

// === ProviderCapabilities ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ProviderCapabilities`]
pub struct ProviderCapabilities {
    pub supports_tool_call: bool,
    pub supports_vision: bool,
    pub supports_prompt_cache: bool,
    pub max_context_tokens: u64,
    pub supports_streaming: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            supports_tool_call: false,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 4096,
            supports_streaming: false,
        }
    }
}

// === ProviderContext ===

#[derive(Debug, Clone, Default)]
/// [`ProviderContext`]
pub struct ProviderContext {
    pub session_id: Option<String>,
    pub project_dir: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

// === ChatStream ===

/// [`ChatStream`]
/// [`ChatStream`]
pub type ChatStream = std::pin::Pin<
    Box<dyn futures_core::Stream<Item = Result<ChatStreamChunk, ProviderError>> + Send>,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ChatStreamChunk`]
pub struct ChatStreamChunk {
    pub content: Option<String>,
    pub thinking: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub usage: Option<TokenUsage>,
    pub stop_reason: Option<crate::types::StopReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ToolCallDelta`]
pub struct ToolCallDelta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub args_delta: Option<String>,
}

// === TokenCounter ===

/// [`TokenCounter`]
pub trait TokenCounter: Send + Sync {
    fn count_text(&self, text: &str) -> usize;
    fn count_messages(&self, messages: &[crate::types::Message]) -> usize;
}

/// A TokenCounter implementation that uses the Tokenizer.
///
/// Delegates counting to `Tokenizer`, which uses tiktoken-rs when the
/// `tokenizer` feature is enabled and a heuristic fallback otherwise.
pub struct DefaultTokenCounter {
    tokenizer: Tokenizer,
}

impl DefaultTokenCounter {
    pub fn new() -> Self {
        Self {
            tokenizer: Tokenizer::new(),
        }
    }
}

impl Default for DefaultTokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCounter for DefaultTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        self.tokenizer.count(text)
    }

    fn count_messages(&self, messages: &[crate::types::Message]) -> usize {
        self.tokenizer.count_messages(messages)
    }
}

// === Provider Trait ===

#[async_trait]
/// [`Provider`]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn supported_models(&self) -> Vec<ModelInfo>;
    fn capabilities(&self) -> ProviderCapabilities;

    async fn chat(
        &self,
        request: ChatRequest,
        ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError>;

    async fn stream_chat(
        &self,
        _request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatStream, ProviderError> {
        Err(ProviderError::RequestFailed(
            "Streaming not supported by this provider".to_string(),
        ))
    }

    async fn health_check(&self) -> Result<(), ProviderError>;
    fn token_counter(&self) -> Box<dyn TokenCounter>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_tier_ordering() {
        assert!(ModelTier::Fast < ModelTier::Balanced);
        assert!(ModelTier::Balanced < ModelTier::Powerful);
    }

    #[test]
    fn test_provider_capabilities_default() {
        let caps = ProviderCapabilities::default();
        assert!(!caps.supports_tool_call);
        assert!(!caps.supports_vision);
        assert!(!caps.supports_prompt_cache);
        assert_eq!(caps.max_context_tokens, 4096);
    }

    #[test]
    fn test_model_info_creation() {
        let info = ModelInfo {
            id: "deepseek-chat".to_string(),
            display_name: "DeepSeek Chat".to_string(),
            input_price_per_mtok: 0.14,
            output_price_per_mtok: 0.28,
            context_window: 64000,
            tier: ModelTier::Balanced,
        };
        assert_eq!(info.id, "deepseek-chat");
        assert_eq!(info.tier, ModelTier::Balanced);
    }

    #[test]
    fn test_provider_context_default() {
        let ctx = ProviderContext::default();
        assert!(ctx.session_id.is_none());
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn test_model_info_all_fields() {
        let info = ModelInfo {
            id: "claude-opus-4".to_string(),
            display_name: "Claude Opus 4".to_string(),
            input_price_per_mtok: 15.0,
            output_price_per_mtok: 75.0,
            context_window: 200000,
            tier: ModelTier::Powerful,
        };
        assert_eq!(info.id, "claude-opus-4");
        assert!((info.input_price_per_mtok - 15.0).abs() < f64::EPSILON);
        assert_eq!(info.context_window, 200000);
        assert_eq!(info.tier, ModelTier::Powerful);
    }

    #[test]
    fn test_provider_capabilities_all_fields() {
        let caps = ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: true,
            supports_prompt_cache: true,
            max_context_tokens: 128000,
            supports_streaming: true,
        };
        assert!(caps.supports_tool_call);
        assert!(caps.supports_vision);
        assert!(caps.supports_prompt_cache);
        assert_eq!(caps.max_context_tokens, 128000);
        assert!(caps.supports_streaming);
    }

    #[test]
    fn test_model_tier_exhaustive_ordering() {
        assert!(ModelTier::Fast < ModelTier::Balanced);
        assert!(ModelTier::Fast < ModelTier::Powerful);
        assert!(ModelTier::Balanced < ModelTier::Powerful);
        assert!(ModelTier::Balanced > ModelTier::Fast);
        assert!(ModelTier::Powerful > ModelTier::Fast);
        assert!(ModelTier::Powerful > ModelTier::Balanced);
        assert_eq!(ModelTier::Fast, ModelTier::Fast);
        assert_ne!(ModelTier::Fast, ModelTier::Powerful);
    }

    #[test]
    fn test_model_tier_display() {
        assert_eq!(format!("{:?}", ModelTier::Fast), "Fast");
        assert_eq!(format!("{:?}", ModelTier::Balanced), "Balanced");
        assert_eq!(format!("{:?}", ModelTier::Powerful), "Powerful");
    }

    #[test]
    fn test_provider_context_with_metadata() {
        let ctx = ProviderContext {
            session_id: Some("session_001".to_string()),
            project_dir: Some("/home/user/project".to_string()),
            metadata: std::collections::HashMap::from([("key1".to_string(), "val1".to_string())]),
        };
        assert_eq!(ctx.session_id.as_deref(), Some("session_001"));
        assert_eq!(ctx.metadata.get("key1").map(|s| s.as_str()), Some("val1"));
    }

    #[test]
    fn test_chat_stream_chunk_with_thinking() {
        let chunk = ChatStreamChunk {
            content: Some("Hello".to_string()),
            thinking: Some("reasoning".to_string()),
            tool_call_delta: None,
            usage: None,
            stop_reason: None,
        };
        assert_eq!(chunk.content.as_deref(), Some("Hello"));
        assert_eq!(chunk.thinking.as_deref(), Some("reasoning"));
    }

    #[test]
    fn test_chat_stream_chunk_empty() {
        let chunk = ChatStreamChunk {
            content: None,
            thinking: None,
            tool_call_delta: None,
            usage: None,
            stop_reason: None,
        };
        assert!(chunk.content.is_none());
        assert!(chunk.thinking.is_none());
    }

    #[test]
    fn test_tool_call_delta_creation() {
        let delta = ToolCallDelta {
            id: Some("call_1".to_string()),
            name: Some("read_file".to_string()),
            args_delta: Some("{\"path\":\"".to_string()),
        };
        assert_eq!(delta.id.as_deref(), Some("call_1"));
        assert!(delta.args_delta.as_ref().unwrap().contains("path"));
    }

    #[test]
    fn test_provider_capabilities_serialization() {
        let caps = ProviderCapabilities::default();
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("supports_tool_call"));
        let deserialized: ProviderCapabilities = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.supports_vision);
    }
}
