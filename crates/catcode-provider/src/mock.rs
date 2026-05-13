use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ChatStream, ChatStreamChunk, ModelInfo, ModelTier, Provider, ProviderCapabilities,
    ProviderContext, TokenCounter, ToolCallDelta,
};
use catcode_core::types::{ChatRequest, ChatResponse, Message, TokenUsage};
use futures::stream;
use std::sync::{Arc, Mutex};

/// A mock provider for testing. Returns pre-configured responses in sequence,
/// cycling back to the start when exhausted.
pub struct MockProvider {
    responses: Arc<Mutex<Vec<ChatResponse>>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockProvider {
/// Create a mock provider with no responses.
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a mock provider that always returns the given text.
    pub fn with_text_response(text: &str) -> Self {
        Self::new(vec![ChatResponse {
            content: vec![catcode_core::types::ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
            stop_reason: catcode_core::types::StopReason::EndTurn,
            model: "mock-model".to_string(),
        }])
    }

    /// Number of times `chat` has been called.
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

/// A simple token counter that counts words (split by whitespace).
pub struct MockTokenCounter;

impl TokenCounter for MockTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| self.count_text(&m.content)).sum()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn display_name(&self) -> &str {
        "Mock Provider"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "mock-model".to_string(),
            display_name: "Mock Model".to_string(),
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            context_window: 4096,
            tier: ModelTier::Fast,
        }]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 4096,
            supports_streaming: false,
        }
    }

    async fn chat(
        &self,
        _request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();

        if responses.is_empty() {
            return Err(ProviderError::Unavailable(
                "No mock responses configured".to_string(),
            ));
        }

        let idx = *count % responses.len();
        *count += 1;
        Ok(responses[idx].clone())
    }

    async fn stream_chat(
        &self,
        _request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatStream, ProviderError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();

        if responses.is_empty() {
            return Err(ProviderError::Unavailable(
                "No mock responses configured".to_string(),
            ));
        }

        let idx = *count % responses.len();
        *count += 1;
        let response = responses[idx].clone();

        let mut chunks: Vec<Result<ChatStreamChunk, ProviderError>> = Vec::new();

        for block in &response.content {
            match block {
                catcode_core::types::ContentBlock::Text { text } => {
                    chunks.push(Ok(ChatStreamChunk {
                        content: Some(text.clone()),
                        thinking: None,
                        tool_call_delta: None,
                        usage: None,
                        stop_reason: None,
                    }));
                }
                catcode_core::types::ContentBlock::Thinking { text } => {
                    chunks.push(Ok(ChatStreamChunk {
                        content: None,
                        thinking: Some(text.clone()),
                        tool_call_delta: None,
                        usage: None,
                        stop_reason: None,
                    }));
                }
                catcode_core::types::ContentBlock::ToolCall { id, name, args } => {
                    chunks.push(Ok(ChatStreamChunk {
                        content: None,
                        thinking: None,
                        tool_call_delta: Some(ToolCallDelta {
                            id: Some(id.clone()),
                            name: Some(name.clone()),
                            args_delta: Some(serde_json::to_string(args).unwrap_or_default()),
                        }),
                        usage: None,
                        stop_reason: None,
                    }));
                }
            }
        }

        chunks.push(Ok(ChatStreamChunk {
            content: None,
            thinking: None,
            tool_call_delta: None,
            usage: Some(response.usage.clone()),
            stop_reason: Some(response.stop_reason.clone()),
        }));

        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    fn token_counter(&self) -> Box<dyn TokenCounter> {
        Box::new(MockTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider_returns_configured_response() {
        let mock = MockProvider::with_text_response("Hello from mock");
        let req = ChatRequest {
            model: "mock-model".to_string(),
            messages: vec![Message::user("Hi")],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let ctx = ProviderContext::default();

        let resp = mock.chat(req, &ctx).await.unwrap();
        assert_eq!(resp.text_content(), "Hello from mock");
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_provider_cycles_responses() {
        let mock = MockProvider::new(vec![
            ChatResponse {
                content: vec![catcode_core::types::ContentBlock::Text {
                    text: "first".to_string(),
                }],
                usage: TokenUsage::default(),
                stop_reason: catcode_core::types::StopReason::EndTurn,
                model: "mock".to_string(),
            },
            ChatResponse {
                content: vec![catcode_core::types::ContentBlock::Text {
                    text: "second".to_string(),
                }],
                usage: TokenUsage::default(),
                stop_reason: catcode_core::types::StopReason::EndTurn,
                model: "mock".to_string(),
            },
        ]);
        let req = ChatRequest {
            model: "mock".to_string(),
            messages: vec![],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let ctx = ProviderContext::default();

        let r1 = mock.chat(req.clone(), &ctx).await.unwrap();
        assert_eq!(r1.text_content(), "first");

        let r2 = mock.chat(req.clone(), &ctx).await.unwrap();
        assert_eq!(r2.text_content(), "second");

        // Cycles back to first
        let r3 = mock.chat(req, &ctx).await.unwrap();
        assert_eq!(r3.text_content(), "first");
    }

    #[tokio::test]
    async fn test_mock_provider_metadata() {
        let mock = MockProvider::with_text_response("test");
        assert_eq!(mock.id(), "mock");
        assert_eq!(mock.display_name(), "Mock Provider");
        assert!(!mock.supported_models().is_empty());
    }

    #[tokio::test]
    async fn test_mock_provider_health_check() {
        let mock = MockProvider::with_text_response("test");
        assert!(mock.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_mock_token_counter() {
        let counter = MockTokenCounter;
        assert_eq!(counter.count_text("hello world"), 2);
        assert_eq!(counter.count_text(""), 0);
        assert_eq!(
            counter.count_messages(&[
                Message::user("hello world"),
                Message::assistant("foo bar baz"),
            ]),
            5
        );
    }
}
