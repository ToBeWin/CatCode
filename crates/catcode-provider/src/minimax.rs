use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{ChatRequest, ChatResponse, Message};
use serde::{Deserialize, Serialize};

// === Request types (OpenAI-compatible) ===

#[derive(Debug, Serialize)]
struct MiniMaxRequest {
    model: String,
    messages: Vec<MiniMaxMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<MiniMaxTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MiniMaxMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<MiniMaxToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MiniMaxToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: MiniMaxFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct MiniMaxFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct MiniMaxTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: MiniMaxFunctionDef,
}

#[derive(Debug, Serialize)]
struct MiniMaxFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct MiniMaxResponse {
    #[allow(dead_code)]
    id: String,
    choices: Vec<MiniMaxChoice>,
    usage: MiniMaxUsage,
}

#[derive(Debug, Deserialize)]
struct MiniMaxChoice {
    #[allow(dead_code)]
    index: usize,
    message: MiniMaxResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<MiniMaxResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: MiniMaxResponseFunction,
}

#[derive(Debug, Deserialize)]
struct MiniMaxResponseFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct MiniMaxUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<MiniMaxRequest, ProviderError> {
    let mut messages: Vec<MiniMaxMessage> = Vec::new();

    if let Some(ref system) = req.system {
        messages.push(MiniMaxMessage {
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
            .map(|t| MiniMaxTool {
                tool_type: "function".to_string(),
                function: MiniMaxFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect()
    });

    Ok(MiniMaxRequest {
        model: req.model.clone(),
        messages,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stream: req.stream,
        tools,
    })
}

fn convert_message(msg: &Message) -> Result<MiniMaxMessage, ProviderError> {
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
            .map(|tc| MiniMaxToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: MiniMaxFunction {
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

    Ok(MiniMaxMessage {
        role: role.to_string(),
        content,
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    })
}

fn convert_response(resp: MiniMaxResponse, model: &str) -> Result<ChatResponse, ProviderError> {
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

// === MiniMax Provider ===

/// MiniMax provider using OpenAI-compatible API.
pub struct MiniMaxProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl MiniMaxProvider {
/// Create a new MiniMax provider.
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

/// [`MiniMaxTokenCounter`]
pub struct MiniMaxTokenCounter;

impl TokenCounter for MiniMaxTokenCounter {
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
impl Provider for MiniMaxProvider {
    fn id(&self) -> &str {
        "minimax"
    }

    fn display_name(&self) -> &str {
        "MiniMax"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "MiniMax-M1".to_string(),
                display_name: "MiniMax M1".to_string(),
                input_price_per_mtok: 0.50,
                output_price_per_mtok: 2.00,
                context_window: 1_000_000,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "MiniMax-Text-01".to_string(),
                display_name: "MiniMax Text 01".to_string(),
                input_price_per_mtok: 0.20,
                output_price_per_mtok: 0.80,
                context_window: 1_000_000,
                tier: ModelTier::Balanced,
            },
        ]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 1_000_000,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let mm_req = convert_request(&request)?;
        let url = self.chat_url();

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&mm_req)
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

        let mm_resp: MiniMaxResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(mm_resp, &request.model)
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
        Box::new(MiniMaxTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::StopReason;

    #[test]
    fn test_serialize_request_basic() {
        let req = MiniMaxRequest {
            model: "MiniMax-M1".to_string(),
            messages: vec![MiniMaxMessage {
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
        assert_eq!(json["model"], "MiniMax-M1");
    }

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let resp: MiniMaxResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    }

    #[test]
    fn test_convert_response_text() {
        let resp = MiniMaxResponse {
            id: "chatcmpl-123".to_string(),
            choices: vec![MiniMaxChoice {
                index: 0,
                message: MiniMaxResponseMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: MiniMaxUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: Some(15),
            },
        };

        let chat_resp = convert_response(resp, "MiniMax-M1").unwrap();
        assert_eq!(chat_resp.text_content(), "Hello!");
        assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
    }

    fn create_test_provider() -> MiniMaxProvider {
        MiniMaxProvider::new(
            "test-key".to_string(),
            "https://api.minimax.chat/v1".to_string(),
        )
    }

    #[test]
    fn test_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "minimax");
        assert_eq!(provider.display_name(), "MiniMax");
        assert_eq!(provider.supported_models().len(), 2);
    }

    #[test]
    fn test_provider_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_context_tokens, 1_000_000);
    }
}
