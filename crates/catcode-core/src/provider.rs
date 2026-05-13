use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ProviderError;
use crate::types::{ChatRequest, ChatResponse, TokenUsage};

// === ModelTier ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModelTier {
    Fast,
    Balanced,
    Powerful,
}

// === ModelInfo ===

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct ProviderContext {
    pub session_id: Option<String>,
    pub project_dir: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

// === ChatStream ===

pub type ChatStream = std::pin::Pin<
    Box<dyn futures_core::Stream<Item = Result<ChatStreamChunk, ProviderError>> + Send>,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    pub content: Option<String>,
    pub thinking: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub usage: Option<TokenUsage>,
    pub stop_reason: Option<crate::types::StopReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub args_delta: Option<String>,
}

// === TokenCounter ===

pub trait TokenCounter: Send + Sync {
    fn count_text(&self, text: &str) -> usize;
    fn count_messages(&self, messages: &[crate::types::Message]) -> usize;
}

// === Provider Trait ===

#[async_trait]
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
}
