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
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OllamaFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OllamaFunctionDef,
}

#[derive(Debug, Serialize)]
struct OllamaFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: String,
    choices: Vec<OllamaChoice>,
    usage: OllamaUsage,
}

#[derive(Debug, Deserialize)]
struct OllamaChoice {
    #[allow(dead_code)]
    index: usize,
    message: OllamaResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<OllamaResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: OllamaResponseFunction,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OllamaUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<OllamaRequest, ProviderError> {
    let mut messages = Vec::new();

    if let Some(ref system) = req.system {
        messages.push(OllamaMessage {
            role: "system".to_string(),
            content: Some(system.clone()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in &req.messages {
        messages.push(convert_message(msg));
    }

    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| OllamaTool {
                tool_type: "function".to_string(),
                function: OllamaFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    });

    Ok(OllamaRequest {
        model: req.model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stream: req.stream,
        tools,
    })
}

fn convert_message(msg: &Message) -> OllamaMessage {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let tool_calls = msg.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|tc| OllamaToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: OllamaFunction {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.args).unwrap_or_default(),
                },
            })
            .collect()
    });

    OllamaMessage {
        role: role.to_string(),
        content: if msg.content.is_empty() {
            None
        } else {
            Some(msg.content.clone())
        },
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    }
}

fn convert_response(resp: OllamaResponse, model: &str) -> Result<ChatResponse, ProviderError> {
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

// === Ollama Provider ===

/// Ollama provider implementation using the OpenAI-compatible API.
///
/// Connects to a local Ollama instance. No API key required.
pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Create a new Ollama provider.
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}

/// Token counter for Ollama models (rough estimate ~4 chars per token).
pub struct OllamaTokenCounter;

impl TokenCounter for OllamaTokenCounter {
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
impl Provider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn display_name(&self) -> &str {
        "Ollama"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "llama3.1".to_string(),
                display_name: "Llama 3.1".to_string(),
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                context_window: 128_000,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "qwen2.5".to_string(),
                display_name: "Qwen 2.5".to_string(),
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                context_window: 128_000,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "deepseek-coder-v2".to_string(),
                display_name: "DeepSeek Coder V2".to_string(),
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                context_window: 128_000,
                tier: ModelTier::Balanced,
            },
            ModelInfo {
                id: "codellama".to_string(),
                display_name: "Code Llama".to_string(),
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
                context_window: 16_000,
                tier: ModelTier::Fast,
            },
        ]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 128_000,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let ollama_req = convert_request(&request)?;
        let url = self.chat_url();

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&ollama_req)
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
                404 => Err(ProviderError::ModelNotFound(format!(
                    "Model not found: {body}"
                ))),
                500..=599 => Err(ProviderError::Unavailable(format!(
                    "Server error {status}: {body}"
                ))),
                _ => Err(ProviderError::RequestFailed(format!(
                    "HTTP {status}: {body}"
                ))),
            };
        }

        let ollama_resp: OllamaResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(ollama_resp, &request.model)
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(format!("Health check failed: {e}")))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ProviderError::Unavailable(format!(
                "Ollama not reachable at {} (status: {})",
                self.base_url,
                resp.status()
            )))
        }
    }

    fn token_counter(&self) -> Box<dyn TokenCounter> {
        Box::new(OllamaTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::ToolCall;

    // === Request serialization tests ===

    #[test]
    fn test_serialize_request_basic() {
        let req = OllamaRequest {
            model: "llama3.1".to_string(),
            messages: vec![OllamaMessage {
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
        assert_eq!(json["model"], "llama3.1");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["stream"], false);
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn test_serialize_request_with_tools() {
        let req = OllamaRequest {
            model: "llama3.1".to_string(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            stream: false,
            tools: Some(vec![OllamaTool {
                tool_type: "function".to_string(),
                function: OllamaFunctionDef {
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

    // === Response deserialization tests ===

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "model": "llama3.1",
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

        let resp: OllamaResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello! How can I help?")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn test_deserialize_tool_call_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-456",
            "model": "llama3.1",
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

        let resp: OllamaResponse = serde_json::from_value(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let calls = msg.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "read_file");
    }

    // === Conversion tests ===

    #[test]
    fn test_convert_chat_request() {
        let req = ChatRequest {
            model: "llama3.1".to_string(),
            messages: vec![
                Message::system("You are helpful"),
                Message::user("Hello"),
                Message::assistant("Hi!"),
            ],
            tools: None,
            system: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: false,
        };

        let ollama_req = convert_request(&req).unwrap();
        assert_eq!(ollama_req.model, "llama3.1");
        assert_eq!(ollama_req.messages.len(), 3);
        assert_eq!(ollama_req.messages[0].role, "system");
        assert_eq!(ollama_req.messages[1].role, "user");
        assert_eq!(ollama_req.messages[2].role, "assistant");
    }

    #[test]
    fn test_convert_request_with_system_field() {
        let req = ChatRequest {
            model: "llama3.1".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: Some("You are a helpful assistant".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let ollama_req = convert_request(&req).unwrap();
        assert_eq!(ollama_req.messages.len(), 2);
        assert_eq!(ollama_req.messages[0].role, "system");
        assert_eq!(
            ollama_req.messages[0].content.as_deref(),
            Some("You are a helpful assistant")
        );
    }

    #[test]
    fn test_convert_request_with_tool_calls() {
        let req = ChatRequest {
            model: "llama3.1".to_string(),
            messages: vec![Message::assistant_with_tool_calls(
                "Let me read it",
                vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    args: serde_json::json!({"path": "src/main.rs"}),
                }],
            )],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };

        let ollama_req = convert_request(&req).unwrap();
        assert!(ollama_req.messages[0].tool_calls.is_some());
    }

    #[test]
    fn test_convert_response_to_chat_response() {
        let ollama_resp = OllamaResponse {
            id: "chatcmpl-123".to_string(),
            model: "llama3.1".to_string(),
            choices: vec![OllamaChoice {
                index: 0,
                message: OllamaResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: OllamaUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: Some(15),
            },
        };

        let resp = convert_response(ollama_resp, "llama3.1").unwrap();
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.text_content(), "Hello!");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn test_convert_response_with_tool_calls() {
        let ollama_resp = OllamaResponse {
            id: "chatcmpl-456".to_string(),
            model: "llama3.1".to_string(),
            choices: vec![OllamaChoice {
                index: 0,
                message: OllamaResponseMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![OllamaResponseToolCall {
                        id: "call_abc".to_string(),
                        call_type: Some("function".to_string()),
                        function: OllamaResponseFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: OllamaUsage {
                prompt_tokens: 20,
                completion_tokens: 15,
                total_tokens: Some(35),
            },
        };

        let resp = convert_response(ollama_resp, "llama3.1").unwrap();
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

    fn create_test_provider() -> OllamaProvider {
        OllamaProvider::new("http://localhost:11434".to_string())
    }

    #[test]
    fn test_ollama_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "ollama");
        assert_eq!(provider.display_name(), "Ollama");
        assert!(!provider.supported_models().is_empty());
    }

    #[test]
    fn test_ollama_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(!caps.supports_vision);
        assert!(!caps.supports_prompt_cache);
        assert_eq!(caps.max_context_tokens, 128_000);
    }

    #[test]
    fn test_ollama_models_are_free() {
        let provider = create_test_provider();
        for model in provider.supported_models() {
            assert_eq!(model.input_price_per_mtok, 0.0);
            assert_eq!(model.output_price_per_mtok, 0.0);
        }
    }

    #[test]
    fn test_ollama_url_construction() {
        let provider = OllamaProvider::new("http://localhost:11434/".to_string());
        assert_eq!(
            provider.chat_url(),
            "http://localhost:11434/v1/chat/completions"
        );
    }
}
