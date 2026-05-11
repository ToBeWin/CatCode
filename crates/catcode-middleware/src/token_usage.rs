use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware};
use catcode_core::types::ChatResponse;

/// Middleware that extracts token usage from ChatResponse and records it
/// to the AgentContext after each model call.
#[derive(Debug, Default)]
pub struct TokenUsageMiddleware;

impl TokenUsageMiddleware {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Middleware for TokenUsageMiddleware {
    fn name(&self) -> &str {
        "token_usage"
    }

    async fn after_model(
        &self,
        ctx: &mut AgentContext,
        response: &ChatResponse,
    ) -> catcode_core::error::Result<()> {
        ctx.record_usage(response.usage.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::{ContentBlock, StopReason, TokenUsage};

    fn make_response(usage: TokenUsage) -> ChatResponse {
        ChatResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            usage,
            stop_reason: StopReason::EndTurn,
            model: "test-model".to_string(),
        }
    }

    #[tokio::test]
    async fn test_records_usage_from_response() {
        let mw = TokenUsageMiddleware::new();
        let mut ctx = AgentContext::new("test");

        let response = make_response(TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 20,
            cache_creation_tokens: 10,
        });

        mw.after_model(&mut ctx, &response).await.unwrap();

        let total = ctx.total_usage();
        assert_eq!(total.input_tokens, 100);
        assert_eq!(total.output_tokens, 50);
        assert_eq!(total.cache_read_tokens, 20);
        assert_eq!(total.cache_creation_tokens, 10);
    }

    #[tokio::test]
    async fn test_accumulates_multiple_responses() {
        let mw = TokenUsageMiddleware::new();
        let mut ctx = AgentContext::new("test");

        let response1 = make_response(TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        });
        let response2 = make_response(TokenUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_tokens: 50,
            cache_creation_tokens: 25,
        });

        mw.after_model(&mut ctx, &response1).await.unwrap();
        mw.after_model(&mut ctx, &response2).await.unwrap();

        let total = ctx.total_usage();
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 150);
        assert_eq!(total.cache_read_tokens, 50);
        assert_eq!(total.cache_creation_tokens, 25);
    }
}
