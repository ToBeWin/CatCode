use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{
    ChatRequest, ChatResponse, ContentBlock, Message, Role, StopReason, TokenUsage,
};
use serde::{Deserialize, Serialize};

// === Request types (Anthropic Messages API) ===

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u64,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    #[allow(dead_code)]
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    resp_type: String,
    content: Vec<AnthropicResponseContent>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicResponseContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<AnthropicRequest, ProviderError> {
    let mut messages = Vec::new();

    for msg in &req.messages {
        // Skip system messages — they go in the top-level `system` field
        if msg.role == Role::System {
            continue;
        }
        messages.push(convert_message(msg)?);
    }

    // Merge system prompt from messages with the request-level system field
    let system = req.system.clone().or_else(|| {
        req.messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.clone())
    });

    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| AnthropicTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.parameters.clone(),
            })
            .collect()
    });

    Ok(AnthropicRequest {
        model: req.model.clone(),
        max_tokens: req.max_tokens.unwrap_or(4096),
        messages,
        system,
        temperature: req.temperature,
        tools,
        stream: if req.stream { Some(true) } else { None },
    })
}

fn convert_message(msg: &Message) -> Result<AnthropicMessage, ProviderError> {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "user", // tool results are sent as user messages in Anthropic API
        Role::System => {
            return Err(ProviderError::RequestFailed(
                "System messages should be handled separately".to_string(),
            ))
        }
    };

    // If the message has tool calls, build content blocks
    if let Some(ref tool_calls) = msg.tool_calls {
        let mut blocks = Vec::new();

        // Add text content if present
        if !msg.content.is_empty() {
            blocks.push(AnthropicContentBlock::Text {
                text: msg.content.clone(),
            });
        }

        for tc in tool_calls {
            blocks.push(AnthropicContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.args.clone(),
            });
        }

        return Ok(AnthropicMessage {
            role: role.to_string(),
            content: AnthropicContent::Blocks(blocks),
        });
    }

    // If this is a tool result message
    if msg.role == Role::Tool
        && let Some(ref tool_call_id) = msg.tool_call_id
    {
        return Ok(AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                tool_use_id: tool_call_id.clone(),
                content: msg.content.clone(),
            }]),
        });
    }

    // Simple text message
    Ok(AnthropicMessage {
        role: role.to_string(),
        content: AnthropicContent::Text(msg.content.clone()),
    })
}

fn convert_response(resp: AnthropicResponse, model: &str) -> Result<ChatResponse, ProviderError> {
    let mut content = Vec::new();

    for block in &resp.content {
        match block {
            AnthropicResponseContent::Text { text } => {
                if !text.is_empty() {
                    content.push(ContentBlock::Text {
                        text: text.clone(),
                    });
                }
            }
            AnthropicResponseContent::ToolUse { id, name, input } => {
                content.push(ContentBlock::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    args: input.clone(),
                });
            }
        }
    }

    let stop_reason = match resp.stop_reason.as_deref() {
        Some("end_turn") => StopReason::EndTurn,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("tool_use") => StopReason::ToolUse,
        Some("stop_sequence") => StopReason::StopSequence,
        _ => StopReason::EndTurn,
    };

    Ok(ChatResponse {
        content,
        usage: TokenUsage {
            input_tokens: resp.usage.input_tokens,
            output_tokens: resp.usage.output_tokens,
            cache_read_tokens: resp.usage.cache_read_input_tokens.unwrap_or(0),
            cache_creation_tokens: resp.usage.cache_creation_input_tokens.unwrap_or(0),
        },
        stop_reason,
        model: model.to_string(),
    })
}

// === Anthropic Provider ===

/// Anthropic provider implementation using the Messages API.
pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    api_version: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_version: "2023-06-01".to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

/// Token counter for Anthropic models (~3.5 chars per token).
pub struct AnthropicTokenCounter;

impl TokenCounter for AnthropicTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        text.len().div_ceil(4)
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|m| self.count_text(&m.content) + 4)
            .sum()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn display_name(&self) -> &str {
        "Anthropic"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "claude-sonnet-4-20250514".to_string(),
                display_name: "Claude Sonnet 4".to_string(),
                input_price_per_mtok: 3.0,
                output_price_per_mtok: 15.0,
                context_window: 200_000,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "claude-opus-4-20250514".to_string(),
                display_name: "Claude Opus 4".to_string(),
                input_price_per_mtok: 15.0,
                output_price_per_mtok: 75.0,
                context_window: 200_000,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "claude-haiku-4-5-20251001".to_string(),
                display_name: "Claude Haiku 4.5".to_string(),
                input_price_per_mtok: 0.80,
                output_price_per_mtok: 4.0,
                context_window: 200_000,
                tier: ModelTier::Fast,
            },
        ]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: true,
            supports_prompt_cache: true,
            max_context_tokens: 200_000,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let anthropic_req = convert_request(&request)?;
        let url = self.messages_url();

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .header("content-type", "application/json")
            .json(&anthropic_req)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "failed to read error body".to_string());
            return match status.as_u16() {
                401 => Err(ProviderError::AuthFailed(body)),
                429 => Err(ProviderError::RateLimited {
                    retry_after_ms: 1000,
                }),
                500..=599 => Err(ProviderError::Unavailable(format!(
                    "Server error {status}: {body}"
                ))),
                _ => Err(ProviderError::RequestFailed(format!(
                    "HTTP {status}: {body}"
                ))),
            };
        }

        let anthropic_resp: AnthropicResponse = resp.json().await.map_err(|e| {
            ProviderError::RequestFailed(format!("Failed to parse response: {e}"))
        })?;

        convert_response(anthropic_resp, &request.model)
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        // Anthropic doesn't have a dedicated health endpoint; just verify the API key is valid
        // by checking we can reach the server
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": "claude-haiku-4-5-20251001",
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(format!("Health check failed: {e}")))?;

        if resp.status().is_success() || resp.status().as_u16() == 400 {
            // 400 means the server is reachable (bad request is fine for health check)
            Ok(())
        } else if resp.status().as_u16() == 401 {
            Err(ProviderError::AuthFailed("Invalid API key".to_string()))
        } else {
            Err(ProviderError::Unavailable(format!(
                "Health check returned {}",
                resp.status()
            )))
        }
    }

    fn token_counter(&self) -> Box<dyn TokenCounter> {
        Box::new(AnthropicTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::ToolCall;

    // === Request conversion tests ===

    #[test]
    fn test_convert_basic_request() {
        let req = ChatRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: false,
        };

        let anthropic_req = convert_request(&req).unwrap();
        assert_eq!(anthropic_req.model, "claude-sonnet-4-20250514");
        assert_eq!(anthropic_req.max_tokens, 4096);
        assert_eq!(anthropic_req.messages.len(), 1);
    }

    #[test]
    fn test_convert_request_with_system() {
        let req = ChatRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::system("Be helpful"), Message::user("Hi")],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let anthropic_req = convert_request(&req).unwrap();
        assert_eq!(anthropic_req.system.as_deref(), Some("Be helpful"));
        // System message should be filtered out from messages
        assert_eq!(anthropic_req.messages.len(), 1);
        assert_eq!(anthropic_req.messages[0].role, "user");
    }

    #[test]
    fn test_convert_request_with_system_field() {
        let req = ChatRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("Hi")],
            tools: None,
            system: Some("You are helpful".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let anthropic_req = convert_request(&req).unwrap();
        assert_eq!(anthropic_req.system.as_deref(), Some("You are helpful"));
    }

    #[test]
    fn test_convert_request_with_tools() {
        let req = ChatRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("Read a file")],
            tools: Some(vec![catcode_core::types::ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    }
                }),
            }]),
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let anthropic_req = convert_request(&req).unwrap();
        let tools = anthropic_req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
    }

    // === Message conversion tests ===

    #[test]
    fn test_convert_user_message() {
        let msg = Message::user("Hello");
        let anthropic_msg = convert_message(&msg).unwrap();
        assert_eq!(anthropic_msg.role, "user");
    }

    #[test]
    fn test_convert_assistant_message() {
        let msg = Message::assistant("Hi there");
        let anthropic_msg = convert_message(&msg).unwrap();
        assert_eq!(anthropic_msg.role, "assistant");
    }

    #[test]
    fn test_convert_assistant_with_tool_calls() {
        let msg = Message::assistant_with_tool_calls(
            "Let me read it",
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                args: serde_json::json!({"path": "src/main.rs"}),
            }],
        );
        let anthropic_msg = convert_message(&msg).unwrap();
        assert_eq!(anthropic_msg.role, "assistant");
        match &anthropic_msg.content {
            AnthropicContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2); // text + tool_use
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_convert_tool_result_message() {
        let msg = Message::tool_result("call_1", "file contents");
        let anthropic_msg = convert_message(&msg).unwrap();
        assert_eq!(anthropic_msg.role, "user");
        match &anthropic_msg.content {
            AnthropicContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AnthropicContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => {
                        assert_eq!(tool_use_id, "call_1");
                        assert_eq!(content, "file contents");
                    }
                    _ => panic!("Expected tool_result block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_convert_system_message_returns_error() {
        let msg = Message::system("You are helpful");
        assert!(convert_message(&msg).is_err());
    }

    // === Response conversion tests ===

    #[test]
    fn test_convert_text_response() {
        let resp = AnthropicResponse {
            id: "msg_123".to_string(),
            resp_type: "message".to_string(),
            content: vec![AnthropicResponseContent::Text {
                text: "Hello!".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };

        let chat_resp = convert_response(resp, "claude-sonnet-4-20250514").unwrap();
        assert_eq!(chat_resp.text_content(), "Hello!");
        assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
        assert_eq!(chat_resp.usage.input_tokens, 10);
        assert_eq!(chat_resp.usage.output_tokens, 5);
    }

    #[test]
    fn test_convert_tool_use_response() {
        let resp = AnthropicResponse {
            id: "msg_456".to_string(),
            resp_type: "message".to_string(),
            content: vec![
                AnthropicResponseContent::Text {
                    text: "Let me read it".to_string(),
                },
                AnthropicResponseContent::ToolUse {
                    id: "toolu_abc".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
            ],
            stop_reason: Some("tool_use".to_string()),
            usage: AnthropicUsage {
                input_tokens: 20,
                output_tokens: 15,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(10),
            },
        };

        let chat_resp = convert_response(resp, "claude-sonnet-4-20250514").unwrap();
        assert!(chat_resp.has_tool_calls());
        assert_eq!(chat_resp.stop_reason, StopReason::ToolUse);
        assert_eq!(chat_resp.usage.cache_read_tokens, 10);
        let calls = chat_resp.get_tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");
    }

    #[test]
    fn test_convert_response_max_tokens() {
        let resp = AnthropicResponse {
            id: "msg_789".to_string(),
            resp_type: "message".to_string(),
            content: vec![AnthropicResponseContent::Text {
                text: "partial".to_string(),
            }],
            stop_reason: Some("max_tokens".to_string()),
            usage: AnthropicUsage {
                input_tokens: 100,
                output_tokens: 4096,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };

        let chat_resp = convert_response(resp, "claude-sonnet-4-20250514").unwrap();
        assert_eq!(chat_resp.stop_reason, StopReason::MaxTokens);
    }

    // === Provider metadata tests ===

    fn create_test_provider() -> AnthropicProvider {
        AnthropicProvider::new(
            "test-key".to_string(),
            "https://api.anthropic.com".to_string(),
        )
    }

    #[test]
    fn test_anthropic_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "anthropic");
        assert_eq!(provider.display_name(), "Anthropic");
        assert!(!provider.supported_models().is_empty());
    }

    #[test]
    fn test_anthropic_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_vision);
        assert!(caps.supports_prompt_cache);
        assert_eq!(caps.max_context_tokens, 200_000);
    }

    // === Serialization roundtrip tests ===

    #[test]
    fn test_anthropic_request_serialization() {
        let req = AnthropicRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("Hello".to_string()),
            }],
            system: Some("Be helpful".to_string()),
            temperature: Some(0.7),
            tools: None,
            stream: None,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "claude-sonnet-4-20250514");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["system"], "Be helpful");
        assert_eq!(json["messages"][0]["role"], "user");
        // tools and stream should be absent when None
        assert!(json.get("tools").is_none());
        assert!(json.get("stream").is_none());
    }

    #[test]
    fn test_anthropic_response_deserialization() {
        let json = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let resp: AnthropicResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.id, "msg_123");
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn test_anthropic_response_with_cache_tokens() {
        let json = serde_json::json!({
            "id": "msg_456",
            "type": "message",
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "cache_creation_input_tokens": 50,
                "cache_read_input_tokens": 80
            }
        });

        let resp: AnthropicResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.usage.cache_creation_input_tokens, Some(50));
        assert_eq!(resp.usage.cache_read_input_tokens, Some(80));
    }
}
