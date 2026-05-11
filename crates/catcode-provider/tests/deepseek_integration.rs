//! Integration tests for DeepSeek provider.
//! Requires DEEPSEEK_API_KEY environment variable.
//! Run: cargo test -p catcode-provider --test deepseek_integration -- --ignored

use catcode_core::provider::{Provider, ProviderContext};
use catcode_core::types::{ChatRequest, Message};
use catcode_provider::deepseek::DeepSeekProvider;

fn make_provider() -> Option<DeepSeekProvider> {
    let api_key = std::env::var("DEEPSEEK_API_KEY").ok()?;
    Some(DeepSeekProvider::new(
        api_key,
        "https://api.deepseek.com".to_string(),
    ))
}

#[tokio::test]
#[ignore]
async fn test_deepseek_chat_basic() {
    let provider = match make_provider() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: DEEPSEEK_API_KEY not set");
            return;
        }
    };

    let req = ChatRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![Message::user("Say exactly: hello world")],
        tools: None,
        system: None,
        max_tokens: Some(100),
        temperature: Some(0.0),
        stream: false,
    };
    let ctx = ProviderContext::default();

    let resp = provider.chat(req, &ctx).await.unwrap();
    let text = resp.text_content();
    assert!(!text.is_empty(), "Response should not be empty");
    assert_eq!(resp.model, "deepseek-chat");
    assert!(resp.usage.input_tokens > 0);
    assert!(resp.usage.output_tokens > 0);
}

#[tokio::test]
#[ignore]
async fn test_deepseek_health_check() {
    let provider = match make_provider() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: DEEPSEEK_API_KEY not set");
            return;
        }
    };

    assert!(provider.health_check().await.is_ok());
}
