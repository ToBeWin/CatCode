use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{ChatRequest, ChatResponse, Message, Role};
use serde::{Deserialize, Serialize};

// === Request types (Google Generative AI API) ===

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "systemInstruction")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "generationConfig")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none", rename = "maxOutputTokens")]
    max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

// === Response types ===

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
    #[serde(rename = "totalTokenCount")]
    #[allow(dead_code)]
    total_token_count: Option<u64>,
}

// === Conversion functions ===

fn convert_request(req: &ChatRequest) -> Result<GeminiRequest, ProviderError> {
    let mut contents: Vec<GeminiContent> = Vec::new();

    for msg in &req.messages {
        contents.push(convert_message(msg)?);
    }

    let system_instruction = req.system.as_ref().map(|s| GeminiContent {
        role: "user".to_string(),
        parts: vec![GeminiPart::Text { text: s.clone() }],
    });

    let tools = req.tools.as_ref().map(|tools| {
        vec![GeminiTool {
            function_declarations: tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                })
                .collect(),
        }]
    });

    let generation_config = Some(GeminiGenerationConfig {
        max_output_tokens: req.max_tokens,
        temperature: req.temperature,
    });

    Ok(GeminiRequest {
        contents,
        system_instruction,
        tools,
        generation_config,
    })
}

fn convert_message(msg: &Message) -> Result<GeminiContent, ProviderError> {
    let role = match msg.role {
        Role::User | Role::System => "user",
        Role::Assistant => "model",
        Role::Tool => "user", // Tool results sent as user messages in Gemini
    };

    let mut parts: Vec<GeminiPart> = Vec::new();

    // Add tool calls first (from assistant) — Google expects function calls before text
    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            parts.push(GeminiPart::FunctionCall {
                function_call: GeminiFunctionCall {
                    name: tc.name.clone(),
                    args: Some(tc.args.clone()),
                },
            });
        }
    }

    // Add text content if present
    if !msg.content.is_empty() {
        parts.push(GeminiPart::Text {
            text: msg.content.clone(),
        });
    }

    // Add tool result
    if let Some(ref tool_call_id) = msg.tool_call_id {
        // In Gemini, tool results are function responses
        // We need to find the function name from the tool call id
        // For now, use a placeholder name based on the content
        parts.push(GeminiPart::FunctionResponse {
            function_response: GeminiFunctionResponse {
                name: format!("tool_{}", tool_call_id),
                response: serde_json::json!({
                    "output": msg.content
                }),
            },
        });
    }

    if parts.is_empty() {
        parts.push(GeminiPart::Text {
            text: String::new(),
        });
    }

    Ok(GeminiContent {
        role: role.to_string(),
        parts,
    })
}

fn convert_response(resp: GeminiResponse, model: &str) -> Result<ChatResponse, ProviderError> {
    use catcode_core::types::{ContentBlock, StopReason, TokenUsage};

    let candidate = resp
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::RequestFailed("No candidates in response".to_string()))?;

    let mut content = Vec::new();

    for part in &candidate.content.parts {
        match part {
            GeminiPart::Text { text } => {
                if !text.is_empty() {
                    content.push(ContentBlock::Text { text: text.clone() });
                }
            }
            GeminiPart::FunctionCall { function_call } => {
                content.push(ContentBlock::ToolCall {
                    id: format!("call_{}", function_call.name),
                    name: function_call.name.clone(),
                    args: function_call
                        .args
                        .clone()
                        .unwrap_or(serde_json::Value::Object(Default::default())),
                });
            }
            _ => {}
        }
    }

    let stop_reason = match candidate.finish_reason.as_deref() {
        Some("STOP") => StopReason::EndTurn,
        Some("MAX_TOKENS") => StopReason::MaxTokens,
        Some("TOOL_CALLS") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };

    let usage = resp.usage_metadata.unwrap_or(GeminiUsageMetadata {
        prompt_token_count: Some(0),
        candidates_token_count: Some(0),
        total_token_count: Some(0),
    });

    Ok(ChatResponse {
        content,
        usage: TokenUsage {
            input_tokens: usage.prompt_token_count.unwrap_or(0),
            output_tokens: usage.candidates_token_count.unwrap_or(0),
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        },
        stop_reason,
        model: model.to_string(),
    })
}

// === Google Provider ===

/// Google Gemini provider using the Generative AI API.
pub struct GoogleProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl GoogleProvider {
    /// Create a new Google (Gemini) provider.
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn chat_url(&self, model: &str) -> String {
        format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, model, self.api_key
        )
    }
}

/// [`GoogleTokenCounter`]
pub struct GoogleTokenCounter;

impl TokenCounter for GoogleTokenCounter {
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
impl Provider for GoogleProvider {
    fn id(&self) -> &str {
        "google"
    }

    fn display_name(&self) -> &str {
        "Google"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gemini-2.5-pro".to_string(),
                display_name: "Gemini 2.5 Pro".to_string(),
                input_price_per_mtok: 1.25,
                output_price_per_mtok: 10.00,
                context_window: 1_048_576,
                tier: ModelTier::Powerful,
            },
            ModelInfo {
                id: "gemini-2.5-flash".to_string(),
                display_name: "Gemini 2.5 Flash".to_string(),
                input_price_per_mtok: 0.15,
                output_price_per_mtok: 0.60,
                context_window: 1_048_576,
                tier: ModelTier::Fast,
            },
            ModelInfo {
                id: "gemini-2.0-flash".to_string(),
                display_name: "Gemini 2.0 Flash".to_string(),
                input_price_per_mtok: 0.10,
                output_price_per_mtok: 0.40,
                context_window: 1_048_576,
                tier: ModelTier::Fast,
            },
        ]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: true,
            supports_prompt_cache: false,
            max_context_tokens: 1_048_576,
            supports_streaming: true,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let gemini_req = convert_request(&request)?;
        let url = self.chat_url(&request.model);

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&gemini_req)
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
                400 => Err(ProviderError::RequestFailed(format!("Bad request: {body}"))),
                401 | 403 => Err(ProviderError::AuthFailed(body)),
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

        let gemini_resp: GeminiResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {e}")))?;

        convert_response(gemini_resp, &request.model)
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        let url = format!("{}/models?key={}", self.base_url, self.api_key);
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
                "Health check returned {}",
                resp.status()
            )))
        }
    }

    fn token_counter(&self) -> Box<dyn TokenCounter> {
        Box::new(GoogleTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::{ContentBlock, StopReason, ToolCall};

    #[test]
    fn test_serialize_request_basic() {
        let req = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::Text {
                    text: "Hello".to_string(),
                }],
            }],
            system_instruction: None,
            tools: None,
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: Some(4096),
                temperature: Some(0.7),
            }),
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["contents"][0]["role"], "user");
        assert_eq!(json["contents"][0]["parts"][0]["text"], "Hello");
        assert_eq!(json["generationConfig"]["maxOutputTokens"], 4096);
    }

    #[test]
    fn test_serialize_with_tools() {
        let req = GeminiRequest {
            contents: vec![],
            system_instruction: None,
            tools: Some(vec![GeminiTool {
                function_declarations: vec![GeminiFunctionDeclaration {
                    name: "read_file".to_string(),
                    description: "Read a file".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
            }]),
            generation_config: None,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json["tools"][0]["functionDeclarations"][0]["name"],
            "read_file"
        );
    }

    #[test]
    fn test_deserialize_text_response() {
        let json = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello!"}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15
            }
        });

        let resp: GeminiResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        match &resp.candidates[0].content.parts[0] {
            GeminiPart::Text { text } => assert_eq!(text, "Hello!"),
            _ => panic!("Expected text"),
        }
    }

    #[test]
    fn test_deserialize_tool_call_response() {
        let json = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "read_file",
                            "args": {"path": "src/main.rs"}
                        }
                    }]
                },
                "finishReason": "TOOL_CALLS"
            }],
            "usageMetadata": {
                "promptTokenCount": 20,
                "candidatesTokenCount": 15,
                "totalTokenCount": 35
            }
        });

        let resp: GeminiResponse = serde_json::from_value(json).unwrap();
        match &resp.candidates[0].content.parts[0] {
            GeminiPart::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "read_file");
            }
            _ => panic!("Expected function call"),
        }
    }

    #[test]
    fn test_convert_request_with_system() {
        let req = ChatRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: Some("You are helpful".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: false,
        };

        let gemini_req = convert_request(&req).unwrap();
        assert!(gemini_req.system_instruction.is_some());
        assert_eq!(gemini_req.contents.len(), 1);
    }

    #[test]
    fn test_convert_request_with_tool_calls() {
        let req = ChatRequest {
            model: "gemini-2.5-pro".to_string(),
            messages: vec![Message::assistant_with_tool_calls(
                "Let me check",
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

        let gemini_req = convert_request(&req).unwrap();
        match &gemini_req.contents[0].parts[0] {
            GeminiPart::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "read_file");
            }
            _ => panic!("Expected function call"),
        }
    }

    #[test]
    fn test_convert_response_text() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hello!".to_string(),
                    }],
                },
                finish_reason: Some("STOP".to_string()),
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(5),
                total_token_count: Some(15),
            }),
        };

        let chat_resp = convert_response(resp, "gemini-2.5-pro").unwrap();
        assert_eq!(chat_resp.text_content(), "Hello!");
        assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
        assert_eq!(chat_resp.usage.input_tokens, 10);
        assert_eq!(chat_resp.usage.output_tokens, 5);
    }

    #[test]
    fn test_convert_response_tool_call() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "bash".to_string(),
                            args: Some(serde_json::json!({"command": "ls"})),
                        },
                    }],
                },
                finish_reason: Some("TOOL_CALLS".to_string()),
            }],
            usage_metadata: None,
        };

        let chat_resp = convert_response(resp, "gemini-2.5-pro").unwrap();
        assert!(chat_resp.has_tool_calls());
        assert_eq!(chat_resp.stop_reason, StopReason::ToolUse);
        match &chat_resp.content[0] {
            ContentBlock::ToolCall { name, .. } => assert_eq!(name, "bash"),
            _ => panic!("Expected tool call"),
        }
    }

    fn create_test_provider() -> GoogleProvider {
        GoogleProvider::new(
            "test-key".to_string(),
            "https://generativelanguage.googleapis.com/v1beta".to_string(),
        )
    }

    #[test]
    fn test_provider_metadata() {
        let provider = create_test_provider();
        assert_eq!(provider.id(), "google");
        assert_eq!(provider.display_name(), "Google");
        assert_eq!(provider.supported_models().len(), 3);
    }

    #[test]
    fn test_provider_capabilities() {
        let provider = create_test_provider();
        let caps = provider.capabilities();
        assert!(caps.supports_tool_call);
        assert!(caps.supports_vision);
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_context_tokens, 1_048_576);
    }

    #[test]
    fn test_chat_url() {
        let provider = create_test_provider();
        assert_eq!(
            provider.chat_url("gemini-2.5-pro"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent?key=test-key"
        );
    }

    #[test]
    fn test_token_counter() {
        let counter = GoogleTokenCounter;
        assert_eq!(counter.count_text("hello"), 2);
    }
}
