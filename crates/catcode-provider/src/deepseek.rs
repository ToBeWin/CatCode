use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{
    ChatRequest, ChatResponse, ContentBlock, Message, Role, StopReason, TokenUsage,
};
use serde::{Deserialize, Serialize};

// === Request types (OpenAI-compatible) ===

#[derive(Debug, Serialize)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<DeepSeekTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeepSeekMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<DeepSeekToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeepSeekToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: DeepSeekFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeepSeekFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct DeepSeekTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: DeepSeekFunctionDef,
}

#[derive(Debug, Serialize)]
struct DeepSeekFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct DeepSeekResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: String,
    choices: Vec<DeepSeekChoice>,
    usage: DeepSeekUsage,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChoice {
    #[allow(dead_code)]
    index: usize,
    message: DeepSeekResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<DeepSeekResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: DeepSeekResponseFunction,
}

#[derive(Debug, Deserialize)]
struct DeepSeekResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct DeepSeekUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_miss_tokens: Option<u64>,
}

// === Conversion functions ===

/// Convert a catcode-core ChatRequest into a DeepSeek API request.
fn convert_request(req: &ChatRequest) -> Result<DeepSeekRequest, ProviderError> {
    let mut messages: Vec<DeepSeekMessage> = Vec::new();

    // If there's a system prompt, add it as a system message first.
    if let Some(ref system) = req.system {
        messages.push(DeepSeekMessage {
            role: "system".to_string(),
            content: Some(system.clone()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in &req.messages {
        messages.push(convert_message(msg)?);
    }

    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| DeepSeekTool {
                tool_type: "function".to_string(),
                function: DeepSeekFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    });

    Ok(DeepSeekRequest {
        model: req.model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stream: req.stream,
        tools,
    })
}

fn convert_message(msg: &Message) -> Result<DeepSeekMessage, ProviderError> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let tool_calls = msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| DeepSeekToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: DeepSeekFunction {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.args).unwrap_or_default(),
                },
            })
            .collect()
    });

    Ok(DeepSeekMessage {
        role: role.to_string(),
        content: if msg.content.is_empty() {
            None
        } else {
            Some(msg.content.clone())
        },
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    })
}

/// Convert a DeepSeek API response into a catcode-core ChatResponse.
fn convert_response(resp: DeepSeekResponse, model: &str) -> Result<ChatResponse, ProviderError> {
    let choice = resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::RequestFailed("No choices in response".to_string()))?;

    let mut content = Vec::new();

    if let Some(text) = choice.message.content
        && !text.is_empty()
    {
        content.push(ContentBlock::Text { text });
    }

    if let Some(tool_calls) = choice.message.tool_calls {
        for tc in tool_calls {
            let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            content.push(ContentBlock::ToolCall {
                id: tc.id,
                name: tc.function.name,
                args,
            });
        }
    }

    let stop_reason = match choice.finish_reason.as_deref() {
        Some("stop") => StopReason::EndTurn,
        Some("length") => StopReason::MaxTokens,
        Some("tool_calls") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };

    let cache_read = resp.usage.prompt_cache_hit_tokens.unwrap_or(0);
    let cache_creation = resp.usage.prompt_cache_miss_tokens.unwrap_or(0);

    Ok(ChatResponse {
        content,
        usage: TokenUsage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
        },
        stop_reason,
        model: model.to_string(),
    })
}

// === DeepSeek Provider ===

/// DeepSeek provider implementation using the OpenAI-compatible API.
pub struct DeepSeekProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl DeepSeekProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Build the full API URL for chat completions.
    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

/// A simple tokenizer that estimates ~4 chars per token for DeepSeek.
pub struct DeepSeekTokenCounter;

impl TokenCounter for DeepSeekTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        // Rough estimate: ~4 characters per token
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
impl Provider for DeepSeekProvider {
    fn id(&self) -> &str {
        "deepseek"
    }

    fn display_name(&self) -> &str {
        "DeepSeek"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "deepseek-chat".to_string(),
                display_name: "DeepSeek Chat".to_string(),
                input_price_per_mtok: 0.14,
                output_price_per_mtok: 0.28,
                context_window: 64_000,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "deepseek-reasoner".to_string(),
                display_name: "DeepSeek Reasoner".to_string(),
                input_price_per_mtok: 0.55,
                output_price_per_mtok: 2.19,
                context_window: 64_000,
                tier: ModelTier::Powerful,
            },
        ]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: true,
            max_context_tokens: 64_000,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let ds_req = convert_request(&request)?;
        let url = self.chat_url();

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&ds_req)
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

        let ds_resp: DeepSeekResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(ds_resp, &request.model)
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        // A lightweight check: just verify we can reach the server
        let url = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(format!("Health check failed: {e}")))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ProviderError::Unavailable(format!(
                "Health check returned {}",
                resp.status()
            )))
        }
    }

    fn token_counter(&self) -> Box<dyn TokenCounter> {
        Box::new(DeepSeekTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::ToolCall;

    // === Request serialization tests ===

    #[test]
    fn test_serialize_request_basic() {
        let req = DeepSeekRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![DeepSeekMessage {
                role: "user".to_string(),
                content: Some("Hello".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: false,
            tools: None,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "deepseek-chat");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["stream"], false);
        // tools should be absent when None
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn test_serialize_request_with_tools() {
        let req = DeepSeekRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            stream: false,
            tools: Some(vec![DeepSeekTool {
                tool_type: "function".to_string(),
                function: DeepSeekFunctionDef {
                    name: "read_file".to_string(),
                    description: "Read a file".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        }
                    }),
                },
            }]),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["tools"][0]["type"], "function");
        assert_eq!(json["tools"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn test_serialize_tool_call_message() {
        let msg = DeepSeekMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![DeepSeekToolCall {
                id: "call_123".to_string(),
                call_type: "function".to_string(),
                function: DeepSeekFunction {
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"src/main.rs"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["tool_calls"][0]["id"], "call_123");
        assert_eq!(json["tool_calls"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn test_serialize_tool_result_message() {
        let msg = DeepSeekMessage {
            role: "tool".to_string(),
            content: Some("file contents here".to_string()),
            tool_calls: None,
            tool_call_id: Some("call_123".to_string()),
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "call_123");
        assert_eq!(json["content"], "file contents here");
    }

    // === Response deserialization tests ===

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        });

        let resp: DeepSeekResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello! How can I help?")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 8);
    }

    #[test]
    fn test_deserialize_tool_call_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-456",
            "object": "chat.completion",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"src/main.rs\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 15,
                "total_tokens": 35
            }
        });

        let resp: DeepSeekResponse = serde_json::from_value(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let calls = msg.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "read_file");
    }

    // === Conversion tests ===

    #[test]
    fn test_convert_chat_request_to_deepseek() {
        let req = ChatRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![
                Message::system("You are helpful"),
                Message::user("Hello"),
                Message::assistant("Hi!"),
                Message::user("Read the file"),
                Message::assistant_with_tool_calls(
                    "Let me read it",
                    vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "read_file".to_string(),
                        args: serde_json::json!({"path": "src/main.rs"}),
                    }],
                ),
                Message::tool_result("call_1", "fn main() {}"),
            ],
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
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: false,
        };

        let ds_req = convert_request(&req).unwrap();
        assert_eq!(ds_req.model, "deepseek-chat");
        assert_eq!(ds_req.messages.len(), 6);
        assert_eq!(ds_req.messages[0].role, "system");
        assert_eq!(ds_req.messages[1].role, "user");
        assert_eq!(ds_req.messages[2].role, "assistant");
        assert_eq!(ds_req.messages[3].role, "user");
        assert_eq!(ds_req.messages[4].role, "assistant");
        assert!(ds_req.messages[4].tool_calls.is_some());
        assert_eq!(ds_req.messages[5].role, "tool");
        assert_eq!(ds_req.messages[5].tool_call_id.as_deref(), Some("call_1"));
        assert!(ds_req.tools.is_some());
        assert_eq!(ds_req.tools.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_convert_request_with_system_field() {
        let req = ChatRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: Some("You are a helpful assistant".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let ds_req = convert_request(&req).unwrap();
        assert_eq!(ds_req.messages.len(), 2);
        assert_eq!(ds_req.messages[0].role, "system");
        assert_eq!(
            ds_req.messages[0].content.as_deref(),
            Some("You are a helpful assistant")
        );
        assert_eq!(ds_req.messages[1].role, "user");
    }

    #[test]
    fn test_convert_response_to_chat_response() {
        let ds_resp = DeepSeekResponse {
            id: "chatcmpl-123".to_string(),
            model: "deepseek-chat".to_string(),
            choices: vec![DeepSeekChoice {
                index: 0,
                message: DeepSeekResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: DeepSeekUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: Some(15),
                prompt_cache_hit_tokens: Some(0),
                prompt_cache_miss_tokens: Some(10),
            },
        };

        let resp = convert_response(ds_resp, "deepseek-chat").unwrap();
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.text_content(), "Hello!");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn test_convert_response_with_tool_calls() {
        let ds_resp = DeepSeekResponse {
            id: "chatcmpl-456".to_string(),
            model: "deepseek-chat".to_string(),
            choices: vec![DeepSeekChoice {
                index: 0,
                message: DeepSeekResponseMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![DeepSeekResponseToolCall {
                        id: "call_abc".to_string(),
                        call_type: Some("function".to_string()),
                        function: DeepSeekResponseFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: DeepSeekUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                total_tokens: Some(35),
                prompt_cache_hit_tokens: None,
                prompt_cache_miss_tokens: None,
            },
        };

        let resp = convert_response(ds_resp, "deepseek-chat").unwrap();
        assert!(resp.has_tool_calls());
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        let tc = match &resp.content[0] {
            ContentBlock::ToolCall { id, name, args } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "read_file");
                assert_eq!(args["path"], "src/main.rs");
                true
            }
            _ => false,
        };
        assert!(tc);
    }

    // === Provider metadata tests ===

    fn create_test_provider() -> DeepSeekProvider {
        DeepSeekProvider::new(
            "test-key".to_string(),
            "https://api.deepseek.com".to_string(),
        )
    }

    #[test]
    fn test_deepseek_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "deepseek");
        assert_eq!(provider.display_name(), "DeepSeek");
        assert!(!provider.supported_models().is_empty());
    }

    #[test]
    fn test_deepseek_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_streaming);
        assert!(caps.max_context_tokens > 0);
    }
}
