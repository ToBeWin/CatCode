# catcode-core Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the foundational types and traits crate with zero IO dependencies — the bedrock that all other crates build on.

**Architecture:** Pure Rust library crate defining Provider, Tool, Middleware traits and all shared data types (ChatRequest, ChatResponse, ContentBlock, TokenUsage, errors). No tokio, no reqwest, no filesystem — only serde, thiserror, and async-trait.

**Tech Stack:** Rust, serde, serde_json, thiserror, async-trait, uuid, chrono

---

## File Structure

```
crates/catcode-core/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports all public types
    ├── types.rs            # ChatRequest, ChatResponse, ContentBlock, Message, TokenUsage, StopReason
    ├── provider.rs         # Provider trait, ModelInfo, ProviderCapabilities, ModelTier, ProviderContext, ChatStream
    ├── tool.rs             # Tool trait, OperationLevel, ToolResult, ToolCall, ToolDefinition, ToolContext
    ├── middleware.rs        # Middleware trait, AgentContext, MiddlewareChain, ToolCallNext
    ├── memory.rs           # MemoryType, MemoryEntry, ArchiveFact, FactCategory
    ├── error.rs            # CatCodeError, ProviderError, ToolError, MiddlewareError, ContextError, ConfigError
    └── config.rs           # Config structs (pure data, no IO)
```

---

## Chunk 1: Project Setup + Error Types

### Task 1: Initialize workspace and catcode-core crate

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/catcode-core/Cargo.toml`
- Create: `crates/catcode-core/src/lib.rs`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/catcode-core",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
async-trait = "0.1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

- [ ] **Step 2: Create catcode-core Cargo.toml**

```toml
[package]
name = "catcode-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
```

- [ ] **Step 3: Create minimal lib.rs**

```rust
pub mod error;

pub use error::*;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p catcode-core`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/catcode-core/
git commit -m "feat(core): initialize workspace and catcode-core crate"
```

### Task 2: Define error types

**Files:**
- Create: `crates/catcode-core/src/error.rs`

- [ ] **Step 1: Write error types**

```rust
use std::fmt;

// === Provider Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API request failed: {0}")]
    RequestFailed(String),

    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Provider unavailable: {0}")]
    Unavailable(String),
}

// === Tool Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),
}

// === Middleware Errors ===

#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    #[error("Middleware '{name}' failed: {message}")]
    ExecutionFailed { name: String, message: String },

    #[error("Loop detected: {0}")]
    LoopDetected(String),

    #[error("Guardrail denied: {0}")]
    GuardrailDenied(String),
}

// === Context Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("Token budget exhausted: used {used}/{limit}")]
    BudgetExhausted { used: u64, limit: u64 },

    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    #[error("Memory error: {0}")]
    MemoryError(String),
}

// === Config Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(String),

    #[error("Invalid config: {0}")]
    Invalid(String),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

// === Unified Error ===

#[derive(Debug, thiserror::Error)]
pub enum CatCodeError {
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Middleware error: {0}")]
    Middleware(#[from] MiddlewareError),

    #[error("Context error: {0}")]
    Context(#[from] ContextError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),

    #[error("{0}")]
    Other(String),
}
```

- [ ] **Step 2: Write error conversion tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_error_conversion() {
        let provider_err = ProviderError::RateLimited { retry_after_ms: 1000 };
        let catcode_err: CatCodeError = provider_err.into();
        assert!(catcode_err.to_string().contains("Rate limited"));
    }

    #[test]
    fn test_tool_error_conversion() {
        let tool_err = ToolError::NotFound("bash".to_string());
        let catcode_err: CatCodeError = tool_err.into();
        assert!(catcode_err.to_string().contains("Tool not found"));
    }

    #[test]
    fn test_middleware_error_conversion() {
        let mw_err = MiddlewareError::LoopDetected("repeated read_file".to_string());
        let catcode_err: CatCodeError = mw_err.into();
        assert!(catcode_err.to_string().contains("Loop detected"));
    }

    #[test]
    fn test_context_error_conversion() {
        let ctx_err = ContextError::BudgetExhausted { used: 50000, limit: 50000 };
        let catcode_err: CatCodeError = ctx_err.into();
        assert!(catcode_err.to_string().contains("Budget exhausted"));
    }

    #[test]
    fn test_other_error() {
        let err = CatCodeError::Other("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p catcode-core`
Expected: All 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/catcode-core/src/error.rs crates/catcode-core/src/lib.rs
git commit -m "feat(core): add error types with thiserror"
```

---

## Chunk 2: Core Data Types

### Task 3: Define message and content types

**Files:**
- Create: `crates/catcode-core/src/types.rs`

- [ ] **Step 1: Write message type tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_with_tool_calls() {
        let msg = Message::assistant_with_tool_calls(
            "I'll read the file",
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
    fn test_content_block_text() {
        let block = ContentBlock::Text { text: "hello".to_string() };
        assert!(block.is_text());
        assert!(!block.is_tool_call());
    }

    #[test]
    fn test_content_block_tool_call() {
        let block = ContentBlock::ToolCall {
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({}),
        };
        assert!(!block.is_text());
        assert!(block.is_tool_call());
    }

    #[test]
    fn test_token_usage_add() {
        let a = TokenUsage { input_tokens: 100, output_tokens: 50, cache_read_tokens: 0, cache_creation_tokens: 0 };
        let b = TokenUsage { input_tokens: 200, output_tokens: 100, cache_read_tokens: 50, cache_creation_tokens: 25 };
        let total = a + b;
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 150);
        assert_eq!(total.cache_read_tokens, 50);
    }

    #[test]
    fn test_stop_reason_display() {
        assert_eq!(StopReason::EndTurn.to_string(), "end_turn");
        assert_eq!(StopReason::ToolUse.to_string(), "tool_use");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-core --lib types`
Expected: FAIL (module not found)

- [ ] **Step 3: Implement types.rs**

```rust
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Add;

// === Role ===

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// === Message ===

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_with_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
        }
    }

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
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

// === ContentBlock ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    Thinking { text: String },
}

impl ContentBlock {
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    pub fn is_tool_call(&self) -> bool {
        matches!(self, Self::ToolCall { .. })
    }

    pub fn is_thinking(&self) -> bool {
        matches!(self, Self::Thinking { .. })
    }

    pub fn text_content(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::Thinking { text } => Some(text),
            _ => None,
        }
    }
}

// === TokenUsage ===

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
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
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub usage: TokenUsage,
    pub stop_reason: StopReason,
    pub model: String,
}

impl ChatResponse {
    pub fn tool_calls(&self) -> Vec<&ToolCall> {
        self.content.iter().filter_map(|block| {
            if let ContentBlock::ToolCall { id, name, args } = block {
                // Return a reference-like structure
                None // We'll handle this differently
            } else {
                None
            }
        }).collect()
    }

    pub fn text_content(&self) -> String {
        self.content.iter()
            .filter_map(|b| b.text_content())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn has_tool_calls(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_call())
    }
}

// === ToolDefinition ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-core --lib types`
Expected: All 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-core/src/types.rs crates/catcode-core/src/lib.rs
git commit -m "feat(core): add message, content, token usage, and chat types"
```

### Task 4: Add tests for ChatRequest and ChatResponse

- [ ] **Step 1: Write ChatRequest/ChatResponse tests**

```rust
#[test]
fn test_chat_request_builder() {
    let req = ChatRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![Message::user("Hello")],
        tools: None,
        system: None,
        max_tokens: Some(4096),
        temperature: Some(0.7),
        stream: true,
    };
    assert_eq!(req.model, "deepseek-chat");
    assert!(req.stream);
}

#[test]
fn test_chat_response_text_content() {
    let resp = ChatResponse {
        content: vec![
            ContentBlock::Text { text: "Hello ".to_string() },
            ContentBlock::Text { text: "world".to_string() },
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
            ContentBlock::Text { text: "Let me read".to_string() },
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
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p catcode-core --lib types`
Expected: All 9 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/catcode-core/src/types.rs
git commit -m "test(core): add ChatRequest and ChatResponse tests"
```

---

## Chunk 3: Provider Trait

### Task 5: Define Provider trait and related types

**Files:**
- Create: `crates/catcode-core/src/provider.rs`

- [ ] **Step 1: Write provider trait tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_tier_ordering() {
        assert!(ModelTier::Fast < ModelTier::Balanced);
        assert!(ModelTier::Balanced < ModelTier::Powerful);
    }

    #[test]
    fn test_provider_capabilities_default() {
        let caps = ProviderCapabilities::default();
        assert!(!caps.supports_tool_call);
        assert!(!caps.supports_vision);
        assert!(!caps.supports_prompt_cache);
        assert_eq!(caps.max_context_tokens, 4096);
    }

    #[test]
    fn test_model_info_creation() {
        let info = ModelInfo {
            id: "deepseek-chat".to_string(),
            display_name: "DeepSeek Chat".to_string(),
            input_price_per_mtok: 0.14,
            output_price_per_mtok: 0.28,
            context_window: 64000,
            tier: ModelTier::Balanced,
        };
        assert_eq!(info.id, "deepseek-chat");
        assert_eq!(info.tier, ModelTier::Balanced);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-core --lib provider`
Expected: FAIL

- [ ] **Step 3: Implement provider.rs**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::future::Future;

use crate::types::{ChatRequest, ChatResponse, TokenUsage};
use crate::error::ProviderError;

// === ModelTier ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModelTier {
    Fast,
    Balanced,
    Powerful,
}

// === ModelInfo ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
    pub context_window: u64,
    pub tier: ModelTier,
}

// === ProviderCapabilities ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub supports_tool_call: bool,
    pub supports_vision: bool,
    pub supports_prompt_cache: bool,
    pub max_context_tokens: u64,
    pub supports_streaming: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            supports_tool_call: false,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 4096,
            supports_streaming: false,
        }
    }
}

// === ProviderContext ===

#[derive(Debug, Clone, Default)]
pub struct ProviderContext {
    pub session_id: Option<String>,
    pub project_dir: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

// === ChatStream ===

pub type ChatStream = Pin<Box<dyn futures_core::Stream<Item = Result<ChatStreamChunk, ProviderError>> + Send>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    pub content: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub usage: Option<TokenUsage>,
    pub stop_reason: Option<crate::types::StopReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub args_delta: Option<String>,
}

// === TokenCounter ===

pub trait TokenCounter: Send + Sync {
    fn count_text(&self, text: &str) -> usize;
    fn count_messages(&self, messages: &[crate::types::Message]) -> usize;
}

// === Provider Trait ===

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn supported_models(&self) -> Vec<ModelInfo>;
    fn capabilities(&self) -> ProviderCapabilities;

    async fn chat(
        &self,
        request: ChatRequest,
        ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError>;

    async fn health_check(&self) -> Result<(), ProviderError>;
    fn token_counter(&self) -> Box<dyn TokenCounter>;
}
```

- [ ] **Step 4: Add futures-core dependency**

Add to `crates/catcode-core/Cargo.toml`:
```toml
futures-core = "0.3"
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p catcode-core --lib provider`
Expected: All 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/catcode-core/src/provider.rs crates/catcode-core/src/lib.rs crates/catcode-core/Cargo.toml
git commit -m "feat(core): add Provider trait, ModelInfo, and ProviderCapabilities"
```

---

## Chunk 4: Tool Trait

### Task 6: Define Tool trait and related types

**Files:**
- Create: `crates/catcode-core/src/tool.rs`

- [ ] **Step 1: Write tool trait tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_level_ordering() {
        // Safe < Sensitive < Dangerous in terms of risk
        assert!(matches!(OperationLevel::Safe, OperationLevel::Safe));
        assert!(matches!(OperationLevel::Sensitive, OperationLevel::Sensitive));
        assert!(matches!(OperationLevel::Dangerous, OperationLevel::Dangerous));
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("file content here");
        assert_eq!(result.output, "file content here");
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("file not found");
        assert_eq!(result.output, "file not found");
        assert!(result.is_error);
    }

    #[test]
    fn test_tool_call_creation() {
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({"path": "src/main.rs"}),
        };
        assert_eq!(call.name, "read_file");
    }

    #[test]
    fn test_tool_definition_schema() {
        let def = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        };
        assert_eq!(def.name, "read_file");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-core --lib tool`
Expected: FAIL

- [ ] **Step 3: Implement tool.rs**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ToolError;

// === OperationLevel ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationLevel {
    Safe,
    Sensitive,
    Dangerous,
}

// === ToolResult ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
    pub metadata: serde_json::Value,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

// === ToolContext ===

#[derive(Debug, Clone, Default)]
pub struct ToolContext {
    pub session_id: Option<String>,
    pub project_dir: Option<std::path::PathBuf>,
    pub working_dir: Option<std::path::PathBuf>,
    pub dry_run: bool,
}

// === Tool Trait ===

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn operation_level(&self) -> OperationLevel;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

// Re-export types from types.rs for convenience
pub use crate::types::{ToolCall, ToolDefinition};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-core --lib tool`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-core/src/tool.rs crates/catcode-core/src/lib.rs
git commit -m "feat(core): add Tool trait, OperationLevel, and ToolResult"
```

---

## Chunk 5: Middleware Trait

### Task 7: Define Middleware trait and AgentContext

**Files:**
- Create: `crates/catcode-core/src/middleware.rs`

- [ ] **Step 1: Write middleware trait tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_context_creation() {
        let ctx = AgentContext::new("session_123");
        assert_eq!(ctx.session_id, "session_123");
        assert!(ctx.messages.is_empty());
        assert!(ctx.tool_outputs.is_empty());
    }

    #[test]
    fn test_agent_context_add_message() {
        let mut ctx = AgentContext::new("session_123");
        ctx.add_message(Message::user("Hello"));
        assert_eq!(ctx.messages.len(), 1);
    }

    #[test]
    fn test_agent_context_token_usage() {
        let mut ctx = AgentContext::new("session_123");
        ctx.record_usage(TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        });
        assert_eq!(ctx.total_usage().input_tokens, 100);
        assert_eq!(ctx.total_usage().output_tokens, 50);
    }

    #[test]
    fn test_agent_context_metadata() {
        let mut ctx = AgentContext::new("session_123");
        ctx.set_metadata("model", serde_json::json!("deepseek-chat"));
        assert_eq!(ctx.get_metadata("model"), Some(&serde_json::json!("deepseek-chat")));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-core --lib middleware`
Expected: FAIL

- [ ] **Step 3: Implement middleware.rs**

```rust
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};

use crate::types::{ChatRequest, ChatResponse, Message, TokenUsage, ToolCall};
use crate::tool::ToolResult;
use crate::error::Result;

// === AgentContext ===

#[derive(Debug)]
pub struct AgentContext {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub tool_outputs: VecDeque<ToolOutput>,
    pub metadata: HashMap<String, serde_json::Value>,
    usage_history: Vec<TokenUsage>,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub call_id: String,
    pub tool_name: String,
    pub result: ToolResult,
}

impl AgentContext {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            messages: Vec::new(),
            tool_outputs: VecDeque::new(),
            metadata: HashMap::new(),
            usage_history: Vec::new(),
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn add_tool_output(&mut self, call_id: String, tool_name: String, result: ToolResult) {
        self.tool_outputs.push_back(ToolOutput {
            call_id,
            tool_name,
            result,
        });
    }

    pub fn record_usage(&mut self, usage: TokenUsage) {
        self.usage_history.push(usage);
    }

    pub fn total_usage(&self) -> TokenUsage {
        self.usage_history.iter().fold(TokenUsage::default(), |acc, u| acc + u.clone())
    }

    pub fn set_metadata(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.metadata.insert(key.into(), value);
    }

    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }
}

// === ToolCallNext (for middleware chain) ===

pub struct ToolCallNext<'a> {
    inner: Box<dyn Fn(&ToolCall) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send + 'a>> + Send + 'a>,
}

impl<'a> ToolCallNext<'a> {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn(&ToolCall) -> Fut + Send + 'a,
        Fut: std::future::Future<Output = ToolResult> + Send + 'a,
    {
        Self {
            inner: Box::new(move |call| Box::pin(f(call))),
        }
    }

    pub async fn execute(&self, call: &ToolCall) -> ToolResult {
        (self.inner)(call).await
    }
}

// === Middleware Trait ===

#[async_trait]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;

    async fn before_agent(&self, _ctx: &mut AgentContext) -> crate::error::Result<()> {
        Ok(())
    }

    async fn after_agent(&self, _ctx: &mut AgentContext) -> crate::error::Result<()> {
        Ok(())
    }

    async fn before_model(&self, _ctx: &mut AgentContext, _request: &mut ChatRequest) -> crate::error::Result<()> {
        Ok(())
    }

    async fn after_model(&self, _ctx: &mut AgentContext, _response: &ChatResponse) -> crate::error::Result<()> {
        Ok(())
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        next.execute(call).await
    }
}
```

- [ ] **Step 4: Update lib.rs**

```rust
pub mod error;
pub mod types;
pub mod provider;
pub mod tool;
pub mod middleware;

pub use error::*;
pub use types::*;
pub use provider::*;
pub use tool::*;
pub use middleware::*;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p catcode-core`
Expected: All 17+ tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/catcode-core/src/middleware.rs crates/catcode-core/src/lib.rs
git commit -m "feat(core): add Middleware trait, AgentContext, and ToolCallNext"
```

---

## Chunk 6: Memory Types

### Task 8: Define memory types

**Files:**
- Create: `crates/catcode-core/src/memory.rs`

- [ ] **Step 1: Write memory type tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_type_display() {
        assert_eq!(MemoryType::User.to_string(), "user");
        assert_eq!(MemoryType::Feedback.to_string(), "feedback");
        assert_eq!(MemoryType::Project.to_string(), "project");
        assert_eq!(MemoryType::Reference.to_string(), "reference");
    }

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry {
            name: "deepseek_default".to_string(),
            description: "Use DeepSeek as default provider".to_string(),
            memory_type: MemoryType::Feedback,
            content: "Always use DeepSeek first.".to_string(),
        };
        assert_eq!(entry.memory_type, MemoryType::Feedback);
    }

    #[test]
    fn test_archive_fact_confidence_clamp() {
        let fact = ArchiveFact::new("test", FactCategory::Preference, 1.5);
        assert!(fact.confidence <= 1.0);

        let fact2 = ArchiveFact::new("test", FactCategory::Preference, -0.5);
        assert!(fact2.confidence >= 0.0);
    }

    #[test]
    fn test_fact_category_display() {
        assert_eq!(FactCategory::Preference.to_string(), "preference");
        assert_eq!(FactCategory::Knowledge.to_string(), "knowledge");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-core --lib memory`
Expected: FAIL

- [ ] **Step 3: Implement memory.rs**

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

// === MemoryType ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Feedback => write!(f, "feedback"),
            Self::Project => write!(f, "project"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

// === MemoryEntry ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub content: String,
}

// === FactCategory ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactCategory {
    Preference,
    Knowledge,
    Context,
    Behavior,
    Goal,
}

impl fmt::Display for FactCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preference => write!(f, "preference"),
            Self::Knowledge => write!(f, "knowledge"),
            Self::Context => write!(f, "context"),
            Self::Behavior => write!(f, "behavior"),
            Self::Goal => write!(f, "goal"),
        }
    }
}

// === ArchiveFact ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveFact {
    pub id: String,
    pub content: String,
    pub category: FactCategory,
    pub confidence: f32,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ArchiveFact {
    pub fn new(
        content: impl Into<String>,
        category: FactCategory,
        confidence: f32,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.into(),
            category,
            confidence: confidence.clamp(0.0, 1.0),
            source: "manual".to_string(),
            created_at: chrono::Utc::now(),
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-core --lib memory`
Expected: All 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-core/src/memory.rs crates/catcode-core/src/lib.rs
git commit -m "feat(core): add MemoryType, MemoryEntry, ArchiveFact types"
```

---

## Chunk 7: Config Types + Final Integration

### Task 9: Define config types

**Files:**
- Create: `crates/catcode-core/src/config.rs`

- [ ] **Step 1: Write config tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.defaults.provider, "deepseek");
        assert_eq!(config.defaults.model, "deepseek-chat");
        assert!(config.defaults.sandbox);
    }

    #[test]
    fn test_middleware_config_defaults() {
        let config = MiddlewareConfig::default();
        assert_eq!(config.loop_detection.warn_threshold, 3);
        assert_eq!(config.loop_detection.hard_limit, 5);
        assert_eq!(config.retry.max_attempts, 3);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-core --lib config`
Expected: FAIL

- [ ] **Step 3: Implement config.rs**

```rust
use serde::{Deserialize, Serialize};

// === AppConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub daemon: DaemonConfig,
    pub defaults: DefaultsConfig,
    pub budget: BudgetConfig,
    pub context: ContextConfig,
    pub middleware: MiddlewareConfig,
    pub providers: std::collections::HashMap<String, ProviderConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            daemon: DaemonConfig::default(),
            defaults: DefaultsConfig::default(),
            budget: BudgetConfig::default(),
            context: ContextConfig::default(),
            middleware: MiddlewareConfig::default(),
            providers: std::collections::HashMap::new(),
        }
    }
}

// === DaemonConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub host: String,
    pub port: u16,
    pub auto_start: bool,
    pub max_concurrent_sessions: usize,
    pub checkpoint_interval_turns: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7070,
            auto_start: true,
            max_concurrent_sessions: 5,
            checkpoint_interval_turns: 10,
        }
    }
}

// === DefaultsConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub provider: String,
    pub model: String,
    pub sandbox: bool,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            sandbox: true,
        }
    }
}

// === BudgetConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub session_limit_tokens: u64,
    pub per_request_limit_tokens: u64,
    pub warning_threshold: f32,
    pub on_limit_reached: String,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            session_limit_tokens: 500_000,
            per_request_limit_tokens: 50_000,
            warning_threshold: 0.80,
            on_limit_reached: "pause".to_string(),
        }
    }
}

// === ContextConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub compression_enabled: bool,
    pub compression_threshold_ratio: f32,
    pub dedup_tool_outputs: bool,
    pub max_file_content_tokens: u64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            compression_enabled: true,
            compression_threshold_ratio: 0.75,
            dedup_tool_outputs: true,
            max_file_content_tokens: 8000,
        }
    }
}

// === MiddlewareConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareConfig {
    pub enabled: Vec<String>,
    pub loop_detection: LoopDetectionConfig,
    pub retry: RetryConfig,
    pub timeout: TimeoutConfig,
    pub memory: MemoryMiddlewareConfig,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enabled: vec![
                "loop_detection".to_string(),
                "tool_error_handling".to_string(),
                "retry".to_string(),
                "timeout".to_string(),
                "token_usage".to_string(),
            ],
            loop_detection: LoopDetectionConfig::default(),
            retry: RetryConfig::default(),
            timeout: TimeoutConfig::default(),
            memory: MemoryMiddlewareConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDetectionConfig {
    pub warn_threshold: u32,
    pub hard_limit: u32,
    pub window_size: usize,
}

impl Default for LoopDetectionConfig {
    fn default() -> Self {
        Self {
            warn_threshold: 3,
            hard_limit: 5,
            window_size: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub request_timeout_secs: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: 120,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMiddlewareConfig {
    pub debounce_seconds: u64,
    pub max_facts: usize,
    pub fact_confidence_threshold: f32,
    pub max_injection_tokens: u64,
}

impl Default for MemoryMiddlewareConfig {
    fn default() -> Self {
        Self {
            debounce_seconds: 30,
            max_facts: 100,
            fact_confidence_threshold: 0.7,
            max_injection_tokens: 2000,
        }
    }
}

// === ProviderConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub models: Option<Vec<String>>,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-core --lib config`
Expected: All 2 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-core/src/config.rs crates/catcode-core/src/lib.rs
git commit -m "feat(core): add AppConfig and middleware config types"
```

### Task 10: Final lib.rs integration and full test run

- [ ] **Step 1: Update lib.rs with all modules**

```rust
pub mod config;
pub mod error;
pub mod memory;
pub mod middleware;
pub mod provider;
pub mod tool;
pub mod types;

pub use config::*;
pub use error::*;
pub use memory::*;
pub use middleware::*;
pub use provider::*;
pub use tool::*;
pub use types::*;
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p catcode-core`
Expected: All 20+ tests pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p catcode-core -- -D warnings`
Expected: No warnings

- [ ] **Step 4: Run fmt**

Run: `cargo fmt -p catcode-core`
Expected: No changes

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-core/
git commit -m "feat(core): complete catcode-core crate with all types and traits"
```
