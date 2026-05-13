use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{
    ChatRequest, ChatResponse, ContentBlock, Message, Role, StopReason, TokenUsage,
};
use serde::{Deserialize, Serialize};

// === Request types (OpenAI Chat Completions API) ===

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>, // can be string or array of content parts
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunctionDef,
}

#[derive(Debug, Serialize)]
struct OpenAIFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: String,
    choices: Vec<OpenAIChoice>,
    usage: OpenAIUsage,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    #[allow(dead_code)]
    index: usize,
    message: OpenAIResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: OpenAIResponseFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: Option<u64>,
    /// OpenAI returns cached token info in prompt_tokens_details.
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<OpenAIRequest, ProviderError> {
    let mut messages: Vec<OpenAIMessage> = Vec::new();

    // System prompt as a system message
    if let Some(ref system) = req.system {
        messages.push(OpenAIMessage {
            role: "system".to_string(),
            content: Some(serde_json::Value::String(system.clone())),
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
            .map(|t| OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    });

    Ok(OpenAIRequest {
        model: req.model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stream: req.stream,
        tools,
    })
}

fn convert_message(msg: &Message) -> Result<OpenAIMessage, ProviderError> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let tool_calls = msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| OpenAIToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: OpenAIFunction {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.args).unwrap_or_default(),
                },
            })
            .collect()
    });

    let content = if msg.content.is_empty() {
        None
    } else {
        Some(serde_json::Value::String(msg.content.clone()))
    };

    Ok(OpenAIMessage {
        role: role.to_string(),
        content,
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    })
}

fn convert_response(resp: OpenAIResponse, model: &str) -> Result<ChatResponse, ProviderError> {
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

    let cache_read = resp
        .usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);

    Ok(ChatResponse {
        content,
        usage: TokenUsage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
            cache_read_tokens: cache_read,
            cache_creation_tokens: 0,
        },
        stop_reason,
        model: model.to_string(),
    })
}

// === OpenAI Provider ===

/// OpenAI provider implementation using the Chat Completions API.
pub struct OpenAIProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIProvider {
/// Create a new OpenAI-compatible provider.
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

/// Token counter for OpenAI models (~4 chars per token estimate).
pub struct OpenAITokenCounter;

impl TokenCounter for OpenAITokenCounter {
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
impl Provider for OpenAIProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn display_name(&self) -> &str {
        "OpenAI"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            // GPT-4.1 family (latest)
            ModelInfo {
                id: "gpt-4.1".to_string(),
                display_name: "GPT-4.1".to_string(),
                input_price_per_mtok: 2.00,
                output_price_per_mtok: 8.00,
                context_window: 1_047_576,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "gpt-4.1-mini".to_string(),
                display_name: "GPT-4.1 Mini".to_string(),
                input_price_per_mtok: 0.40,
                output_price_per_mtok: 1.60,
                context_window: 1_047_576,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "gpt-4.1-nano".to_string(),
                display_name: "GPT-4.1 Nano".to_string(),
                input_price_per_mtok: 0.10,
                output_price_per_mtok: 0.40,
                context_window: 1_047_576,
                tier: ModelTier::Fast,
            },
            // GPT-4o family
            ModelInfo {
                id: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                input_price_per_mtok: 2.50,
                output_price_per_mtok: 10.00,
                context_window: 128_000,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "gpt-4o-mini".to_string(),
                display_name: "GPT-4o Mini".to_string(),
                input_price_per_mtok: 0.15,
                output_price_per_mtok: 0.60,
                context_window: 128_000,
                tier: ModelTier::Fast,
            },
            // o3 family (reasoning)
            ModelInfo {
                id: "o3".to_string(),
                display_name: "o3".to_string(),
                input_price_per_mtok: 10.00,
                output_price_per_mtok: 40.00,
                context_window: 200_000,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "o3-mini".to_string(),
                display_name: "o3 Mini".to_string(),
                input_price_per_mtok: 1.10,
                output_price_per_mtok: 4.40,
                context_window: 200_000,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "o4-mini".to_string(),
                display_name: "o4 Mini".to_string(),
                input_price_per_mtok: 1.10,
                output_price_per_mtok: 4.40,
                context_window: 200_000,
                tier: ModelTier::Balanced,
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
        let openai_req = convert_request(&request)?;
        let url = self.chat_url();

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&openai_req)
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

        let openai_resp: OpenAIResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(openai_resp, &request.model)
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
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
        Box::new(OpenAITokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::ToolCall;

    // === Request serialization tests ===

    #[test]
    fn test_serialize_request_basic() {
        let req = OpenAIRequest {
            model: "gpt-4o".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some(serde_json::Value::String("Hello".to_string())),
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: false,
            tools: None,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["stream"], false);
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn test_serialize_request_with_tools() {
        let req = OpenAIRequest {
            model: "gpt-4o".to_string(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            stream: false,
            tools: Some(vec![OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIFunctionDef {
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
        let msg = OpenAIMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_123".to_string(),
                call_type: "function".to_string(),
                function: OpenAIFunction {
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
        let msg = OpenAIMessage {
            role: "tool".to_string(),
            content: Some(serde_json::Value::String(
                "file contents here".to_string(),
            )),
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
            "model": "gpt-4o",
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

        let resp: OpenAIResponse = serde_json::from_value(json).unwrap();
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
            "model": "gpt-4o",
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

        let resp: OpenAIResponse = serde_json::from_value(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let calls = msg.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "read_file");
    }

    #[test]
    fn test_deserialize_with_cached_tokens() {
        let json = serde_json::json!({
            "id": "chatcmpl-789",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let resp: OpenAIResponse = serde_json::from_value(json).unwrap();
        assert_eq!(
            resp.usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens),
            Some(80)
        );
    }

    // === Conversion tests ===

    #[test]
    fn test_convert_chat_request_to_openai() {
        let req = ChatRequest {
            model: "gpt-4o".to_string(),
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

        let openai_req = convert_request(&req).unwrap();
        assert_eq!(openai_req.model, "gpt-4o");
        assert_eq!(openai_req.messages.len(), 6);
        assert_eq!(openai_req.messages[0].role, "system");
        assert_eq!(openai_req.messages[1].role, "user");
        assert_eq!(openai_req.messages[4].role, "assistant");
        assert!(openai_req.messages[4].tool_calls.is_some());
        assert_eq!(openai_req.messages[5].role, "tool");
        assert!(openai_req.tools.is_some());
    }

    #[test]
    fn test_convert_request_with_system_field() {
        let req = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: Some("You are a helpful assistant".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let openai_req = convert_request(&req).unwrap();
        assert_eq!(openai_req.messages.len(), 2);
        assert_eq!(openai_req.messages[0].role, "system");
    }

    #[test]
    fn test_convert_response_to_chat_response() {
        let resp = OpenAIResponse {
            id: "chatcmpl-123".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: Some(15),
                prompt_tokens_details: None,
            },
        };

        let chat_resp = convert_response(resp, "gpt-4o").unwrap();
        assert_eq!(chat_resp.content.len(), 1);
        assert_eq!(chat_resp.text_content(), "Hello!");
        assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
        assert_eq!(chat_resp.usage.input_tokens, 10);
        assert_eq!(chat_resp.usage.output_tokens, 5);
        assert_eq!(chat_resp.usage.cache_read_tokens, 0);
    }

    #[test]
    fn test_convert_response_with_cached_tokens() {
        let resp = OpenAIResponse {
            id: "chatcmpl-456".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("ok".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: OpenAIUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: Some(150),
                prompt_tokens_details: Some(PromptTokensDetails {
                    cached_tokens: Some(80),
                }),
            },
        };

        let chat_resp = convert_response(resp, "gpt-4o").unwrap();
        assert_eq!(chat_resp.usage.cache_read_tokens, 80);
    }

    #[test]
    fn test_convert_response_with_tool_calls() {
        let resp = OpenAIResponse {
            id: "chatcmpl-789".to_string(),
            model: "gpt-4o".to_string(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIResponseMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OpenAIResponseToolCall {
                        id: "call_abc".to_string(),
                        call_type: Some("function".to_string()),
                        function: OpenAIResponseFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: OpenAIUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                total_tokens: Some(35),
                prompt_tokens_details: None,
            },
        };

        let chat_resp = convert_response(resp, "gpt-4o").unwrap();
        assert!(chat_resp.has_tool_calls());
        assert_eq!(chat_resp.stop_reason, StopReason::ToolUse);
    }

    // === Provider metadata tests ===

    fn create_test_provider() -> OpenAIProvider {
        OpenAIProvider::new(
            "test-key".to_string(),
            "https://api.openai.com/v1".to_string(),
        )
    }

    #[test]
    fn test_openai_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "openai");
        assert_eq!(provider.display_name(), "OpenAI");
        assert_eq!(provider.supported_models().len(), 8);
    }

    #[test]
    fn test_openai_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_vision);
        assert!(caps.supports_prompt_cache);
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_context_tokens, 200_000);
    }

    #[test]
    fn test_openai_model_tiers() {
        let provider = create_test_provider();
        let models = provider.supported_models();

        let gpt4o = models.iter().find(|m| m.id == "gpt-4o").unwrap();
        assert_eq!(gpt4o.tier, ModelTier::Powerful);

        let mini = models.iter().find(|m| m.id == "gpt-4o-mini").unwrap();
        assert_eq!(mini.tier, ModelTier::Fast);

        let o3 = models.iter().find(|m| m.id == "o3").unwrap();
        assert_eq!(o3.tier, ModelTier::Powerful);
    }

    #[test]
    fn test_chat_url() {
        let provider = create_test_provider();
        assert_eq!(
            provider.chat_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_token_counter() {
        let counter = OpenAITokenCounter;
        assert_eq!(counter.count_text("hello"), 2); // 5 chars / 4 = 2
        assert_eq!(counter.count_text("x".repeat(100).as_str()), 25);
    }
}
