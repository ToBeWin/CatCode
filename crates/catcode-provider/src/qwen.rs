use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{ChatRequest, ChatResponse, Message};
use serde::{Deserialize, Serialize};

// === Request types (OpenAI-compatible via DashScope) ===

#[derive(Debug, Serialize)]
struct QwenRequest {
    model: String,
    messages: Vec<QwenMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<QwenTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QwenMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<QwenToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct QwenToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: QwenFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct QwenFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct QwenTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: QwenFunctionDef,
}

#[derive(Debug, Serialize)]
struct QwenFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct QwenResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: String,
    choices: Vec<QwenChoice>,
    usage: QwenUsage,
}

#[derive(Debug, Deserialize)]
struct QwenChoice {
    #[allow(dead_code)]
    index: usize,
    message: QwenResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QwenResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<QwenResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct QwenResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: QwenResponseFunction,
}

#[derive(Debug, Deserialize)]
struct QwenResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct QwenUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<QwenRequest, ProviderError> {
    let mut messages: Vec<QwenMessage> = Vec::new();

    if let Some(ref system) = req.system {
        messages.push(QwenMessage {
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
            .map(|t| QwenTool {
                tool_type: "function".to_string(),
                function: QwenFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    });

    Ok(QwenRequest {
        model: req.model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stream: req.stream,
        tools,
    })
}

fn convert_message(msg: &Message) -> Result<QwenMessage, ProviderError> {
    use catcode_core::types::Role;

    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let tool_calls = msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| QwenToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: QwenFunction {
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

    Ok(QwenMessage {
        role: role.to_string(),
        content,
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    })
}

fn convert_response(resp: QwenResponse, model: &str) -> Result<ChatResponse, ProviderError> {
    use catcode_core::types::{ContentBlock, StopReason, TokenUsage};

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

// === Qwen Provider ===

/// Qwen (DashScope) provider using OpenAI-compatible API.
pub struct QwenProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl QwenProvider {
/// Create a new Qwen provider.
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

/// Token counter for Qwen models (~4 chars per token estimate).
pub struct QwenTokenCounter;

impl TokenCounter for QwenTokenCounter {
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
impl Provider for QwenProvider {
    fn id(&self) -> &str {
        "qwen"
    }

    fn display_name(&self) -> &str {
        "Qwen (DashScope)"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "qwen3".to_string(),
                display_name: "Qwen3".to_string(),
                input_price_per_mtok: 0.40,
                output_price_per_mtok: 1.20,
                context_window: 131_072,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "qwen3-coder".to_string(),
                display_name: "Qwen3 Coder".to_string(),
                input_price_per_mtok: 0.40,
                output_price_per_mtok: 1.20,
                context_window: 131_072,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "qwen3-moe".to_string(),
                display_name: "Qwen3 MoE".to_string(),
                input_price_per_mtok: 0.10,
                output_price_per_mtok: 0.30,
                context_window: 131_072,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "qwen2.5".to_string(),
                display_name: "Qwen2.5".to_string(),
                input_price_per_mtok: 0.20,
                output_price_per_mtok: 0.60,
                context_window: 131_072,
                tier: ModelTier::Balanced,
            },
        ]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 131_072,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let qwen_req = convert_request(&request)?;
        let url = self.chat_url();

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&qwen_req)
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

        let qwen_resp: QwenResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(qwen_resp, &request.model)
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
        Box::new(QwenTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::{ContentBlock, StopReason, ToolCall};

    #[test]
    fn test_serialize_request_basic() {
        let req = QwenRequest {
            model: "qwen3".to_string(),
            messages: vec![QwenMessage {
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
        assert_eq!(json["model"], "qwen3");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_serialize_request_with_tools() {
        let req = QwenRequest {
            model: "qwen3".to_string(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            stream: false,
            tools: Some(vec![QwenTool {
                tool_type: "function".to_string(),
                function: QwenFunctionDef {
                    name: "bash".to_string(),
                    description: "Run shell command".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"}
                        }
                    }),
                },
            }]),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["tools"][0]["function"]["name"], "bash");
    }

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "model": "qwen3",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let resp: QwenResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    }

    #[test]
    fn test_deserialize_tool_call_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-456",
            "model": "qwen3",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "bash",
                            "arguments": "{\"command\":\"ls\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 15, "total_tokens": 35}
        });

        let resp: QwenResponse = serde_json::from_value(json).unwrap();
        let calls = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].function.name, "bash");
    }

    #[test]
    fn test_convert_request_with_system() {
        let req = ChatRequest {
            model: "qwen3".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: Some("You are helpful".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let qwen_req = convert_request(&req).unwrap();
        assert_eq!(qwen_req.messages.len(), 2);
        assert_eq!(qwen_req.messages[0].role, "system");
    }

    #[test]
    fn test_convert_request_with_tool_calls() {
        let req = ChatRequest {
            model: "qwen3".to_string(),
            messages: vec![Message::assistant_with_tool_calls(
                "Let me check",
                vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "bash".to_string(),
                    args: serde_json::json!({"command": "ls"}),
                }],
            )],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let qwen_req = convert_request(&req).unwrap();
        assert!(qwen_req.messages[0].tool_calls.is_some());
    }

    #[test]
    fn test_convert_response_text() {
        let resp = QwenResponse {
            id: "chatcmpl-123".to_string(),
            model: "qwen3".to_string(),
            choices: vec![QwenChoice {
                index: 0,
                message: QwenResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: QwenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: Some(15),
            },
        };

        let chat_resp = convert_response(resp, "qwen3").unwrap();
        assert_eq!(chat_resp.text_content(), "Hello!");
        assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn test_convert_response_tool_calls() {
        let resp = QwenResponse {
            id: "chatcmpl-456".to_string(),
            model: "qwen3".to_string(),
            choices: vec![QwenChoice {
                index: 0,
                message: QwenResponseMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![QwenResponseToolCall {
                        id: "call_abc".to_string(),
                        call_type: Some("function".to_string()),
                        function: QwenResponseFunction {
                            name: "bash".to_string(),
                            arguments: r#"{"command":"ls"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: QwenUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                total_tokens: Some(35),
            },
        };

        let chat_resp = convert_response(resp, "qwen3").unwrap();
        assert!(chat_resp.has_tool_calls());
        assert_eq!(chat_resp.stop_reason, StopReason::ToolUse);
        match &chat_resp.content[0] {
            ContentBlock::ToolCall { id, name, .. } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "bash");
            }
            _ => panic!("Expected tool call"),
        }
    }

    fn create_test_provider() -> QwenProvider {
        QwenProvider::new(
            "test-key".to_string(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
        )
    }

    #[test]
    fn test_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "qwen");
        assert_eq!(provider.display_name(), "Qwen (DashScope)");
        assert_eq!(provider.supported_models().len(), 4);
    }

    #[test]
    fn test_provider_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_streaming);
        assert!(!caps.supports_vision);
        assert!(!caps.supports_prompt_cache);
    }

    #[test]
    fn test_model_tiers() {
        let provider = create_test_provider();
        let models = provider.supported_models();

        let qwen3 = models.iter().find(|m| m.id == "qwen3").unwrap();
        assert_eq!(qwen3.tier, ModelTier::Powerful);

        let moe = models.iter().find(|m| m.id == "qwen3-moe").unwrap();
        assert_eq!(moe.tier, ModelTier::Balanced);
    }

    #[test]
    fn test_chat_url() {
        let provider = create_test_provider();
        assert_eq!(
            provider.chat_url(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
    }

    #[test]
    fn test_token_counter() {
        let counter = QwenTokenCounter;
        assert_eq!(counter.count_text("hello"), 2);
    }
}
