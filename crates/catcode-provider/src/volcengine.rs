use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{
    ChatRequest, ChatResponse, ContentBlock, Message, Role, StopReason, TokenUsage,
};
use serde::{Deserialize, Serialize};

// === Volcengine model list ===

/// Built-in Volcengine model identifiers for reference.
pub const VOLCENGINE_MODELS: &[(&str, &str, f64, f64, u64, ModelTier)] = &[
    (
        "doubao-1.5-pro-256k",
        "Doubao 1.5 Pro 256K",
        0.8,
        0.8,
        256_000,
        ModelTier::Powerful,
    ),
    (
        "doubao-1.5-pro-32k",
        "Doubao 1.5 Pro 32K",
        0.35,
        0.35,
        32_000,
        ModelTier::Powerful,
    ),
    (
        "doubao-1.5-lite-32k",
        "Doubao 1.5 Lite 32K",
        0.1,
        0.1,
        128_000,
        ModelTier::Fast,
    ),
    (
        "deepseek-r1-250120",
        "DeepSeek R1",
        0.55,
        2.19,
        128_000,
        ModelTier::Powerful,
    ),
    (
        "deepseek-v3-241226",
        "DeepSeek V3",
        0.5,
        0.5,
        128_000,
        ModelTier::Balanced,
    ),
];

// === Request types (OpenAI-compatible) ===

#[derive(Debug, Serialize)]
struct VolcengineRequest {
    model: String,
    messages: Vec<VolcengineMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<VolcengineTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VolcengineMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<VolcengineToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VolcengineToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: VolcengineFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct VolcengineFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct VolcengineTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: VolcengineFunctionDef,
}

#[derive(Debug, Serialize)]
struct VolcengineFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct VolcengineResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: String,
    choices: Vec<VolcengineChoice>,
    usage: VolcengineUsage,
}

#[derive(Debug, Deserialize)]
struct VolcengineChoice {
    #[allow(dead_code)]
    index: usize,
    message: VolcengineResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VolcengineResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<VolcengineResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct VolcengineResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: VolcengineResponseFunction,
}

#[derive(Debug, Deserialize)]
struct VolcengineResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct VolcengineUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<VolcengineRequest, ProviderError> {
    let mut messages: Vec<VolcengineMessage> = Vec::new();

    if let Some(ref system) = req.system {
        messages.push(VolcengineMessage {
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
            .map(|t| VolcengineTool {
                tool_type: "function".to_string(),
                function: VolcengineFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    });

    Ok(VolcengineRequest {
        model: req.model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stream: req.stream,
        tools,
    })
}

fn convert_message(msg: &Message) -> Result<VolcengineMessage, ProviderError> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let tool_calls = msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| VolcengineToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: VolcengineFunction {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.args).unwrap_or_default(),
                },
            })
            .collect()
    });

    Ok(VolcengineMessage {
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

fn convert_response(resp: VolcengineResponse, model: &str) -> Result<ChatResponse, ProviderError> {
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

    Ok(ChatResponse {
        content,
        usage: TokenUsage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        },
        stop_reason,
        model: model.to_string(),
    })
}

// === Volcengine Provider ===

/// Volcengine (ByteDance) LLM provider.
pub struct VolcengineProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl VolcengineProvider {
    /// Create a new Volcengine provider.
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

/// [`VolcengineTokenCounter`]
pub struct VolcengineTokenCounter;

impl TokenCounter for VolcengineTokenCounter {
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
impl Provider for VolcengineProvider {
    fn id(&self) -> &str {
        "volcengine"
    }

    fn display_name(&self) -> &str {
        "Volcengine"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        VOLCENGINE_MODELS
            .iter()
            .map(
                |(id, name, input_price, output_price, ctx, tier)| ModelInfo {
                    id: id.to_string(),
                    display_name: name.to_string(),
                    input_price_per_mtok: *input_price,
                    output_price_per_mtok: *output_price,
                    context_window: *ctx,
                    tier: *tier,
                },
            )
            .collect()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 256_000,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let vc_req = convert_request(&request)?;
        let url = self.chat_url();

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&vc_req)
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

        let vc_resp: VolcengineResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(vc_resp, &request.model)
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
        Box::new(VolcengineTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::ToolCall;

    // === Request serialization tests ===

    #[test]
    fn test_serialize_request_basic() {
        let req = VolcengineRequest {
            model: "doubao-1.5-pro-32k".to_string(),
            messages: vec![VolcengineMessage {
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
        assert_eq!(json["model"], "doubao-1.5-pro-32k");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["stream"], false);
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn test_serialize_request_with_tools() {
        let req = VolcengineRequest {
            model: "doubao-1.5-pro-32k".to_string(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            stream: false,
            tools: Some(vec![VolcengineTool {
                tool_type: "function".to_string(),
                function: VolcengineFunctionDef {
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
        let msg = VolcengineMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![VolcengineToolCall {
                id: "call_123".to_string(),
                call_type: "function".to_string(),
                function: VolcengineFunction {
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
        let msg = VolcengineMessage {
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
            "model": "doubao-1.5-pro-32k",
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

        let resp: VolcengineResponse = serde_json::from_value(json).unwrap();
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
            "model": "doubao-1.5-pro-32k",
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

        let resp: VolcengineResponse = serde_json::from_value(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let calls = msg.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "read_file");
    }

    // === Conversion tests ===

    #[test]
    fn test_convert_chat_request_to_volcengine() {
        let req = ChatRequest {
            model: "doubao-1.5-pro-32k".to_string(),
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

        let vc_req = convert_request(&req).unwrap();
        assert_eq!(vc_req.model, "doubao-1.5-pro-32k");
        assert_eq!(vc_req.messages.len(), 6);
        assert_eq!(vc_req.messages[0].role, "system");
        assert_eq!(vc_req.messages[1].role, "user");
        assert_eq!(vc_req.messages[2].role, "assistant");
        assert_eq!(vc_req.messages[3].role, "user");
        assert_eq!(vc_req.messages[4].role, "assistant");
        assert!(vc_req.messages[4].tool_calls.is_some());
        assert_eq!(vc_req.messages[5].role, "tool");
        assert_eq!(vc_req.messages[5].tool_call_id.as_deref(), Some("call_1"));
        assert!(vc_req.tools.is_some());
        assert_eq!(vc_req.tools.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_convert_request_with_system_field() {
        let req = ChatRequest {
            model: "doubao-1.5-pro-32k".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: Some("You are a helpful assistant".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let vc_req = convert_request(&req).unwrap();
        assert_eq!(vc_req.messages.len(), 2);
        assert_eq!(vc_req.messages[0].role, "system");
        assert_eq!(
            vc_req.messages[0].content.as_deref(),
            Some("You are a helpful assistant")
        );
        assert_eq!(vc_req.messages[1].role, "user");
    }

    #[test]
    fn test_convert_response_to_chat_response() {
        let vc_resp = VolcengineResponse {
            id: "chatcmpl-123".to_string(),
            model: "doubao-1.5-pro-32k".to_string(),
            choices: vec![VolcengineChoice {
                index: 0,
                message: VolcengineResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: VolcengineUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: Some(15),
            },
        };

        let resp = convert_response(vc_resp, "doubao-1.5-pro-32k").unwrap();
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.text_content(), "Hello!");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn test_convert_response_with_tool_calls() {
        let vc_resp = VolcengineResponse {
            id: "chatcmpl-456".to_string(),
            model: "doubao-1.5-pro-32k".to_string(),
            choices: vec![VolcengineChoice {
                index: 0,
                message: VolcengineResponseMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![VolcengineResponseToolCall {
                        id: "call_abc".to_string(),
                        call_type: Some("function".to_string()),
                        function: VolcengineResponseFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: VolcengineUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                total_tokens: Some(35),
            },
        };

        let resp = convert_response(vc_resp, "doubao-1.5-pro-32k").unwrap();
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

    fn create_test_provider() -> VolcengineProvider {
        VolcengineProvider::new(
            "test-key".to_string(),
            "https://ark.cn-beijing.volces.com/api/v3".to_string(),
        )
    }

    #[test]
    fn test_volcengine_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "volcengine");
        assert_eq!(provider.display_name(), "Volcengine");
        assert_eq!(provider.supported_models().len(), 5);
    }

    #[test]
    fn test_volcengine_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_context_tokens, 256_000);
    }

    #[test]
    fn test_chat_url() {
        let provider = create_test_provider();
        assert_eq!(
            provider.chat_url(),
            "https://ark.cn-beijing.volces.com/api/v3/chat/completions"
        );
    }

    #[test]
    fn test_token_counter() {
        let counter = VolcengineTokenCounter;
        assert_eq!(counter.count_text("hello"), 2);
        assert_eq!(counter.count_text("x".repeat(100).as_str()), 25);
    }

    #[test]
    fn test_volcengine_models_have_tiers() {
        let provider = create_test_provider();
        let models = provider.supported_models();

        let doubao_pro = models
            .iter()
            .find(|m| m.id == "doubao-1.5-pro-32k")
            .unwrap();
        assert_eq!(doubao_pro.tier, ModelTier::Powerful);

        let lite = models
            .iter()
            .find(|m| m.id == "doubao-1.5-lite-32k")
            .unwrap();
        assert_eq!(lite.tier, ModelTier::Fast);

        let ds_v3 = models
            .iter()
            .find(|m| m.id == "deepseek-v3-241226")
            .unwrap();
        assert_eq!(ds_v3.tier, ModelTier::Balanced);
    }
}
