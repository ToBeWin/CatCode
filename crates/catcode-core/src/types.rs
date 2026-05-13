use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Add;

// === Role ===

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// [`Role`]
pub enum Role {
/// [`System`].
    System,
/// [`User`].
    User,
/// [`Assistant`].
    Assistant,
/// [`Tool`].
    Tool,
}

// === Message ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`Message`]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

/// Assistant.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

/// Assistant with tool calls.
    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

/// Tool result.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
        }
    }

/// System.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
}

// === ToolCall ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ToolCall`]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

// === ContentBlock ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// [`ContentBlock`]
pub enum ContentBlock {
/// [`Text`].
    Text {
        text: String,
    },
/// [`ToolCall`].
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
/// [`Thinking`].
    Thinking {
        text: String,
    },
}

impl ContentBlock {
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

/// Check if tool call.
    pub fn is_tool_call(&self) -> bool {
        matches!(self, Self::ToolCall { .. })
    }

/// Check if thinking.
    pub fn is_thinking(&self) -> bool {
        matches!(self, Self::Thinking { .. })
    }

/// Text content.
    pub fn text_content(&self) -> Option<&str> {
        match self {
            Self::Text { text } | Self::Thinking { text } => Some(text),
            _ => None,
        }
    }
}

// === TokenUsage ===

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
/// [`TokenUsage`]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

/// Cache savings ratio.
    pub fn cache_savings_ratio(&self) -> f64 {
        let total_input = self.input_tokens + self.cache_read_tokens;
        if total_input == 0 {
            0.0
        } else {
            self.cache_read_tokens as f64 / total_input as f64
        }
    }
}

impl Add for TokenUsage {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            input_tokens: self.input_tokens + rhs.input_tokens,
            output_tokens: self.output_tokens + rhs.output_tokens,
            cache_read_tokens: self.cache_read_tokens + rhs.cache_read_tokens,
            cache_creation_tokens: self.cache_creation_tokens + rhs.cache_creation_tokens,
        }
    }
}

// === StopReason ===

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// [`StopReason`]
pub enum StopReason {
/// [`EndTurn`].
    EndTurn,
/// [`MaxTokens`].
    MaxTokens,
/// [`ToolUse`].
    ToolUse,
/// [`StopSequence`].
    StopSequence,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndTurn => write!(f, "end_turn"),
            Self::MaxTokens => write!(f, "max_tokens"),
            Self::ToolUse => write!(f, "tool_use"),
            Self::StopSequence => write!(f, "stop_sequence"),
        }
    }
}

// === ChatRequest ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ChatRequest`]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub stream: bool,
}

// === ChatResponse ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ChatResponse`]
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub usage: TokenUsage,
    pub stop_reason: StopReason,
    pub model: String,
}

impl ChatResponse {
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.text_content())
            .collect::<Vec<_>>()
            .join("")
    }

/// Check if tool calls exists.
    pub fn has_tool_calls(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_call())
    }

/// Get the tool calls.
    pub fn get_tool_calls(&self) -> Vec<(String, String, serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|b| {
                if let ContentBlock::ToolCall { id, name, args } = b {
                    Some((id.clone(), name.clone(), args.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

// === ToolDefinition ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ToolDefinition`]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Hello");
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("I'll help");
        assert_eq!(msg.role, Role::Assistant);
    }

    #[test]
    fn test_message_with_tool_calls() {
        let msg = Message::assistant_with_tool_calls(
            "Let me read",
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                args: serde_json::json!({"path": "src/main.rs"}),
            }],
        );
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.tool_calls.is_some());
        assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_message_tool_result() {
        let msg = Message::tool_result("call_1", "file content");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn test_message_system() {
        let msg = Message::system("You are helpful");
        assert_eq!(msg.role, Role::System);
    }

    #[test]
    fn test_content_block_text() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
        };
        assert!(block.is_text());
        assert!(!block.is_tool_call());
        assert_eq!(block.text_content(), Some("hello"));
    }

    #[test]
    fn test_content_block_tool_call() {
        let block = ContentBlock::ToolCall {
            id: "c1".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({}),
        };
        assert!(!block.is_text());
        assert!(block.is_tool_call());
        assert!(block.text_content().is_none());
    }

    #[test]
    fn test_content_block_thinking() {
        let block = ContentBlock::Thinking {
            text: "reasoning".to_string(),
        };
        assert!(block.is_thinking());
        assert_eq!(block.text_content(), Some("reasoning"));
    }

    #[test]
    fn test_token_usage_add() {
        let a = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };
        let b = TokenUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_tokens: 50,
            cache_creation_tokens: 25,
        };
        let total = a + b;
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 150);
        assert_eq!(total.cache_read_tokens, 50);
        assert_eq!(total.cache_creation_tokens, 25);
    }

    #[test]
    fn test_token_usage_total() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 30,
            cache_creation_tokens: 10,
        };
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn test_token_usage_cache_savings() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 100,
            cache_creation_tokens: 0,
        };
        assert!((usage.cache_savings_ratio() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_token_usage_cache_savings_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.cache_savings_ratio(), 0.0);
    }

    #[test]
    fn test_stop_reason_display() {
        assert_eq!(StopReason::EndTurn.to_string(), "end_turn");
        assert_eq!(StopReason::ToolUse.to_string(), "tool_use");
        assert_eq!(StopReason::MaxTokens.to_string(), "max_tokens");
    }

    #[test]
    fn test_chat_response_text_content() {
        let resp = ChatResponse {
            content: vec![
                ContentBlock::Text {
                    text: "Hello ".to_string(),
                },
                ContentBlock::Text {
                    text: "world".to_string(),
                },
            ],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
            model: "deepseek-chat".to_string(),
        };
        assert_eq!(resp.text_content(), "Hello world");
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn test_chat_response_has_tool_calls() {
        let resp = ChatResponse {
            content: vec![
                ContentBlock::Text {
                    text: "Let me read".to_string(),
                },
                ContentBlock::ToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    args: serde_json::json!({"path": "src/main.rs"}),
                },
            ],
            usage: TokenUsage::default(),
            stop_reason: StopReason::ToolUse,
            model: "deepseek-chat".to_string(),
        };
        assert!(resp.has_tool_calls());
        let calls = resp.get_tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![Message::user("Hello")],
            tools: None,
            system: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            stream: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("deepseek-chat"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_content_block_empty_text() {
        let block = ContentBlock::Text { text: String::new() };
        assert!(block.is_text());
        assert_eq!(block.text_content(), Some(""));
    }

    #[test]
    fn test_content_block_tool_call_empty_fields() {
        let block = ContentBlock::ToolCall {
            id: String::new(),
            name: String::new(),
            args: serde_json::Value::Null,
        };
        assert!(block.is_tool_call());
        assert!(block.text_content().is_none());
    }

    #[test]
    fn test_content_block_thinking_empty() {
        let block = ContentBlock::Thinking { text: String::new() };
        assert!(block.is_thinking());
        assert_eq!(block.text_content(), Some(""));
    }

    #[test]
    fn test_message_with_name() {
        let mut msg = Message::assistant("result");
        msg.name = Some("get_weather".to_string());
        assert_eq!(msg.name.as_deref(), Some("get_weather"));
    }

    #[test]
    fn test_message_all_optional_fields() {
        let msg = Message {
            role: Role::Assistant,
            content: "calling tools".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                name: "search".to_string(),
                args: serde_json::json!({"q": "test"}),
            }]),
            tool_call_id: Some("call_1".to_string()),
            name: Some("search_tool".to_string()),
        };
        assert!(msg.tool_calls.is_some());
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(msg.name.as_deref(), Some("search_tool"));
    }

    #[test]
    fn test_chat_response_mixed_content() {
        let resp = ChatResponse {
            content: vec![
                ContentBlock::Thinking {
                    text: "let me think".to_string(),
                },
                ContentBlock::Text {
                    text: "Here is the answer".to_string(),
                },
                ContentBlock::ToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    args: serde_json::json!({"path": "test.txt"}),
                },
            ],
            usage: TokenUsage::default(),
            stop_reason: StopReason::ToolUse,
            model: "deepseek-reasoner".to_string(),
        };
        assert!(resp.has_tool_calls());
        assert_eq!(resp.text_content(), "let me thinkHere is the answer");
        let calls = resp.get_tool_calls();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn test_token_usage_very_large_values() {
        let usage = TokenUsage {
            input_tokens: 1_000_000_000,
            output_tokens: 2_000_000_000,
            cache_read_tokens: 500_000_000,
            cache_creation_tokens: 100_000_000,
        };
        assert_eq!(usage.total(), 3_000_000_000);
    }

    #[test]
    fn test_stop_reason_stop_sequence() {
        assert_eq!(StopReason::StopSequence.to_string(), "stop_sequence");
    }

    #[test]
    fn test_stop_reason_serialization() {
        let json = serde_json::to_string(&StopReason::EndTurn).unwrap();
        assert_eq!(json, "\"end_turn\"");
        let deserialized: StopReason = serde_json::from_str("\"max_tokens\"").unwrap();
        assert_eq!(deserialized, StopReason::MaxTokens);
    }

    #[test]
    fn test_role_variants() {
        assert_eq!(format!("{:?}", Role::System), "System");
        assert_eq!(format!("{:?}", Role::User), "User");
        assert_eq!(format!("{:?}", Role::Assistant), "Assistant");
        assert_eq!(format!("{:?}", Role::Tool), "Tool");
    }

    #[test]
    fn test_role_serialization() {
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
        let deserialized: Role = serde_json::from_str("\"tool\"").unwrap();
        assert_eq!(deserialized, Role::Tool);
    }

    #[test]
    fn test_tool_definition_creation() {
        let def = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        };
        assert_eq!(def.name, "read_file");
        assert!(def.parameters["properties"]["path"]["type"].as_str() == Some("string"));
    }

    #[test]
    fn test_message_content_empty_string() {
        let msg = Message::user("");
        assert_eq!(msg.content, "");
    }

    #[test]
    fn test_chat_request_with_tools() {
        let req = ChatRequest {
            model: "test".to_string(),
            messages: vec![],
            tools: Some(vec![ToolDefinition {
                name: "tool1".to_string(),
                description: "desc".to_string(),
                parameters: serde_json::json!({}),
            }]),
            system: Some("system prompt".to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("tool1"));
        assert!(json.contains("system prompt"));
    }
}
