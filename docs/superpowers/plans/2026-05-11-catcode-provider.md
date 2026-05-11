# catcode-provider Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement DeepSeek and Mock providers with ProviderRegistry

**Architecture:** OpenAI-compatible HTTP client for DeepSeek, configurable mock for testing

**Tech Stack:** Rust, reqwest, serde, async-trait, tokio

---

## Prerequisites

- catcode-core crate must be implemented first (provides Provider trait, ChatRequest, ChatResponse, etc.)
- Run `cargo check -p catcode-core` to verify it compiles before starting

---

## File Structure

```
crates/catcode-provider/
├── Cargo.toml
└── src/
    ├── lib.rs              # ProviderRegistry + re-exports
    ├── deepseek.rs         # DeepSeek provider implementation
    └── mock.rs             # Mock provider for testing
```

---

## Chunk 1: Project Setup

### Task 1: Create catcode-provider crate

**Files:**
- Modify: `Cargo.toml` (add to workspace members)
- Create: `crates/catcode-provider/Cargo.toml`
- Create: `crates/catcode-provider/src/lib.rs`

- [ ] **Step 1: Add catcode-provider to workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/catcode-core",
    "crates/catcode-provider",
]
```

- [ ] **Step 2: Create catcode-provider Cargo.toml**

```toml
[package]
name = "catcode-provider"
version.workspace = true
edition.workspace = true

[dependencies]
catcode-core = { path = "../catcode-core" }
async-trait = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio = { workspace = true }
tracing = "0.1"
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
```

- [ ] **Step 3: Create minimal lib.rs**

```rust
pub mod deepseek;
pub mod mock;

pub use catcode_core::provider::*;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p catcode-provider`
Expected: Compiles successfully (deepseek.rs and mock.rs don't exist yet, so create stub files)

Create stub `crates/catcode-provider/src/deepseek.rs`:
```rust
// DeepSeek provider implementation
```

Create stub `crates/catcode-provider/src/mock.rs`:
```rust
// Mock provider for testing
```

Run: `cargo check -p catcode-provider`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/catcode-provider/
git commit -m "feat(provider): initialize catcode-provider crate"
```

---

## Chunk 2: Mock Provider (Test First)

### Task 2: Implement MockProvider with configurable responses

**Files:**
- Create: `crates/catcode-provider/src/mock.rs`

We implement Mock first because it enables testing DeepSeek without hitting the real API.

- [ ] **Step 1: Write MockProvider tests**

Add to `crates/catcode-provider/src/mock.rs`:

```rust
use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ChatStream, ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{ChatRequest, ChatResponse, ContentBlock, StopReason, TokenUsage};
use std::sync::{Arc, Mutex};

/// A mock provider for testing. Returns pre-configured responses.
pub struct MockProvider {
    responses: Arc<Mutex<Vec<ChatResponse>>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockProvider {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    pub fn with_text_response(text: &str) -> Self {
        Self::new(vec![ChatResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
            stop_reason: StopReason::EndTurn,
            model: "mock-model".to_string(),
        }])
    }

    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider_returns_configured_response() {
        let mock = MockProvider::with_text_response("Hello from mock");
        let req = ChatRequest {
            model: "mock-model".to_string(),
            messages: vec![catcode_core::types::Message::user("Hi")],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let ctx = ProviderContext::default();

        let resp = mock.chat(req, &ctx).await.unwrap();
        assert_eq!(resp.text_content(), "Hello from mock");
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_provider_cycles_responses() {
        let mock = MockProvider::new(vec![
            ChatResponse {
                content: vec![ContentBlock::Text { text: "first".to_string() }],
                usage: TokenUsage::default(),
                stop_reason: StopReason::EndTurn,
                model: "mock".to_string(),
            },
            ChatResponse {
                content: vec![ContentBlock::Text { text: "second".to_string() }],
                usage: TokenUsage::default(),
                stop_reason: StopReason::EndTurn,
                model: "mock".to_string(),
            },
        ]);
        let req = ChatRequest {
            model: "mock".to_string(),
            messages: vec![],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let ctx = ProviderContext::default();

        let r1 = mock.chat(req.clone(), &ctx).await.unwrap();
        assert_eq!(r1.text_content(), "first");

        let r2 = mock.chat(req.clone(), &ctx).await.unwrap();
        assert_eq!(r2.text_content(), "second");

        // Cycles back to first
        let r3 = mock.chat(req, &ctx).await.unwrap();
        assert_eq!(r3.text_content(), "first");
    }

    #[tokio::test]
    async fn test_mock_provider_metadata() {
        let mock = MockProvider::with_text_response("test");
        assert_eq!(mock.id(), "mock");
        assert_eq!(mock.display_name(), "Mock Provider");
        assert!(!mock.supported_models().is_empty());
    }

    #[tokio::test]
    async fn test_mock_provider_health_check() {
        let mock = MockProvider::with_text_response("test");
        assert!(mock.health_check().await.is_ok());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-provider --lib mock`
Expected: FAIL (MockProvider not implemented)

- [ ] **Step 3: Implement MockProvider**

Replace contents of `crates/catcode-provider/src/mock.rs`:

```rust
use async_trait::async_trait;
use catcode_core::error::ProviderError;
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};
use catcode_core::types::{ChatRequest, ChatResponse, Message, TokenUsage};
use std::sync::{Arc, Mutex};

/// A mock provider for testing. Returns pre-configured responses in sequence,
/// cycling back to the start when exhausted.
pub struct MockProvider {
    responses: Arc<Mutex<Vec<ChatResponse>>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockProvider {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a mock provider that always returns the given text.
    pub fn with_text_response(text: &str) -> Self {
        Self::new(vec![ChatResponse {
            content: vec![catcode_core::types::ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
            stop_reason: catcode_core::types::StopReason::EndTurn,
            model: "mock-model".to_string(),
        }])
    }

    /// Number of times `chat` has been called.
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

/// A simple token counter that counts words (split by whitespace).
pub struct MockTokenCounter;

impl TokenCounter for MockTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|m| self.count_text(&m.content))
            .sum()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn display_name(&self) -> &str {
        "Mock Provider"
    }

    fn supported_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "mock-model".to_string(),
            display_name: "Mock Model".to_string(),
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            context_window: 4096,
            tier: ModelTier::Fast,
        }]
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_tool_call: true,
            supports_vision: false,
            supports_prompt_cache: false,
            max_context_tokens: 4096,
            supports_streaming: false,
        }
    }

    async fn chat(
        &self,
        _request: ChatRequest,
        _ctx: &ProviderContext,
    ) -> Result<ChatResponse, ProviderError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();

        if responses.is_empty() {
            return Err(ProviderError::Unavailable(
                "No mock responses configured".to_string(),
            ));
        }

        let idx = *count % responses.len();
        *count += 1;
        Ok(responses[idx].clone())
    }

    async fn health_check(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    fn token_counter(&self) -> Box<dyn TokenCounter> {
        Box::new(MockTokenCounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider_returns_configured_response() {
        let mock = MockProvider::with_text_response("Hello from mock");
        let req = ChatRequest {
            model: "mock-model".to_string(),
            messages: vec![Message::user("Hi")],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let ctx = ProviderContext::default();

        let resp = mock.chat(req, &ctx).await.unwrap();
        assert_eq!(resp.text_content(), "Hello from mock");
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_provider_cycles_responses() {
        let mock = MockProvider::new(vec![
            ChatResponse {
                content: vec![catcode_core::types::ContentBlock::Text {
                    text: "first".to_string(),
                }],
                usage: TokenUsage::default(),
                stop_reason: catcode_core::types::StopReason::EndTurn,
                model: "mock".to_string(),
            },
            ChatResponse {
                content: vec![catcode_core::types::ContentBlock::Text {
                    text: "second".to_string(),
                }],
                usage: TokenUsage::default(),
                stop_reason: catcode_core::types::StopReason::EndTurn,
                model: "mock".to_string(),
            },
        ]);
        let req = ChatRequest {
            model: "mock".to_string(),
            messages: vec![],
            tools: None,
            system: None,
            max_tokens: None,
            temperature: None,
            stream: false,
        };
        let ctx = ProviderContext::default();

        let r1 = mock.chat(req.clone(), &ctx).await.unwrap();
        assert_eq!(r1.text_content(), "first");

        let r2 = mock.chat(req.clone(), &ctx).await.unwrap();
        assert_eq!(r2.text_content(), "second");

        // Cycles back to first
        let r3 = mock.chat(req, &ctx).await.unwrap();
        assert_eq!(r3.text_content(), "first");
    }

    #[tokio::test]
    async fn test_mock_provider_metadata() {
        let mock = MockProvider::with_text_response("test");
        assert_eq!(mock.id(), "mock");
        assert_eq!(mock.display_name(), "Mock Provider");
        assert!(!mock.supported_models().is_empty());
    }

    #[tokio::test]
    async fn test_mock_provider_health_check() {
        let mock = MockProvider::with_text_response("test");
        assert!(mock.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_mock_token_counter() {
        let counter = MockTokenCounter;
        assert_eq!(counter.count_text("hello world"), 2);
        assert_eq!(counter.count_text(""), 0);
        assert_eq!(
            counter.count_messages(&[
                Message::user("hello world"),
                Message::assistant("foo bar baz"),
            ]),
            5
        );
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-provider --lib mock`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-provider/src/mock.rs
git commit -m "feat(provider): add MockProvider with configurable responses"
```

---

## Chunk 3: DeepSeek Provider — Request Serialization

### Task 3: DeepSeek request types (OpenAI-compatible format)

**Files:**
- Create: `crates/catcode-provider/src/deepseek.rs`

DeepSeek uses the OpenAI chat completions API format. We need serde types for the request body.

- [ ] **Step 1: Write request serialization tests**

Add to `crates/catcode-provider/src/deepseek.rs`:

```rust
use serde::{Deserialize, Serialize};

/// OpenAI-compatible request body for DeepSeek API.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: FAIL (types not defined)

- [ ] **Step 3: Verify tests pass after adding the struct definitions above**

The struct definitions are already in the code block above. Run:

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: All 4 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/catcode-provider/src/deepseek.rs
git commit -m "feat(provider): add DeepSeek request serialization types"
```

---

## Chunk 4: DeepSeek Provider — Response Deserialization

### Task 4: DeepSeek response types

**Files:**
- Modify: `crates/catcode-provider/src/deepseek.rs`

- [ ] **Step 1: Write response deserialization tests**

Add to the `tests` module in `crates/catcode-provider/src/deepseek.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: FAIL (response types not defined)

- [ ] **Step 3: Add response types**

Add these structs to `crates/catcode-provider/src/deepseek.rs` (after the request types):

```rust
/// OpenAI-compatible response body from DeepSeek API.
#[derive(Debug, Deserialize)]
struct DeepSeekResponse {
    id: String,
    model: String,
    choices: Vec<DeepSeekChoice>,
    usage: DeepSeekUsage,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChoice {
    index: usize,
    message: DeepSeekResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekResponseMessage {
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<DeepSeekResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekResponseToolCall {
    id: String,
    #[serde(rename = "type")]
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
    total_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
    #[serde(default)]
    prompt_cache_miss_tokens: Option<u64>,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: All 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-provider/src/deepseek.rs
git commit -m "feat(provider): add DeepSeek response deserialization types"
```

---

## Chunk 5: DeepSeek Provider — Conversion Layer

### Task 5: Convert between catcode-core types and DeepSeek API types

**Files:**
- Modify: `crates/catcode-provider/src/deepseek.rs`

- [ ] **Step 1: Write conversion tests**

Add to the `tests` module:

```rust
    #[test]
    fn test_convert_chat_request_to_deepseek() {
        use catcode_core::types::{Message, ToolDefinition};

        let req = ChatRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![
                Message::system("You are helpful"),
                Message::user("Hello"),
                Message::assistant("Hi!"),
                Message::user("Read the file"),
                Message::assistant_with_tool_calls(
                    "Let me read it",
                    vec![catcode_core::types::ToolCall {
                        id: "call_1".to_string(),
                        name: "read_file".to_string(),
                        args: serde_json::json!({"path": "src/main.rs"}),
                    }],
                ),
                Message::tool_result("call_1", "fn main() {}"),
            ],
            tools: Some(vec![ToolDefinition {
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
        assert_eq!(
            ds_req.messages[5].tool_call_id.as_deref(),
            Some("call_1")
        );
        assert!(ds_req.tools.is_some());
        assert_eq!(ds_req.tools.as_ref().unwrap().len(), 1);
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
            catcode_core::types::ContentBlock::ToolCall { id, name, args } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "read_file");
                assert_eq!(args["path"], "src/main.rs");
                true
            }
            _ => false,
        };
        assert!(tc);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: FAIL (conversion functions not defined)

- [ ] **Step 3: Implement conversion functions**

Add to `crates/catcode-provider/src/deepseek.rs` (after the response types):

```rust
use catcode_core::error::ProviderError;
use catcode_core::types::{
    ChatRequest, ChatResponse, ContentBlock, Message, Role, StopReason, ToolCall, TokenUsage,
};

/// Convert a catcode-core ChatRequest into a DeepSeek API request.
fn convert_request(req: &ChatRequest) -> Result<DeepSeekRequest, ProviderError> {
    let messages: Vec<DeepSeekMessage> = req
        .messages
        .iter()
        .map(convert_message)
        .collect::<Result<Vec<_>, _>>()?;

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
fn convert_response(
    resp: DeepSeekResponse,
    model: &str,
) -> Result<ChatResponse, ProviderError> {
    let choice = resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::RequestFailed("No choices in response".to_string()))?;

    let mut content = Vec::new();

    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content.push(ContentBlock::Text { text });
        }
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: All 9 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-provider/src/deepseek.rs
git commit -m "feat(provider): add DeepSeek request/response conversion layer"
```

---

## Chunk 6: DeepSeek Provider — HTTP Client

### Task 6: Implement the Provider trait for DeepSeek

**Files:**
- Modify: `crates/catcode-provider/src/deepseek.rs`

- [ ] **Step 1: Write DeepSeek provider tests**

Add to the `tests` module:

```rust
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

    fn create_test_provider() -> DeepSeekProvider {
        DeepSeekProvider::new("test-key".to_string(), "https://api.deepseek.com".to_string())
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: FAIL (DeepSeekProvider not defined)

- [ ] **Step 3: Implement DeepSeekProvider**

Add to `crates/catcode-provider/src/deepseek.rs` (after the conversion functions):

```rust
use catcode_core::provider::{
    ModelInfo, ModelTier, Provider, ProviderCapabilities, ProviderContext, TokenCounter,
};

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
        (text.len() + 3) / 4
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| self.count_text(&m.content) + 4).sum()
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
                429 => {
                    // Try to parse retry-after header
                    let retry_ms = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(|s| s * 1000)
                        .unwrap_or(1000);
                    Err(ProviderError::RateLimited {
                        retry_after_ms: retry_ms,
                    })
                }
                500..=599 => Err(ProviderError::Unavailable(format!(
                    "Server error {status}: {body}"
                ))),
                _ => Err(ProviderError::RequestFailed(format!(
                    "HTTP {status}: {body}"
                ))),
            };
        }

        let ds_resp: DeepSeekResponse = resp.json().await.map_err(|e| {
            ProviderError::RequestFailed(format!("Failed to parse response: {e}"))
        })?;

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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-provider --lib deepseek`
Expected: All 11 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-provider/src/deepseek.rs
git commit -m "feat(provider): implement DeepSeek Provider trait"
```

---

## Chunk 7: ProviderRegistry

### Task 7: Implement ProviderRegistry

**Files:**
- Modify: `crates/catcode-provider/src/lib.rs`

- [ ] **Step 1: Write ProviderRegistry tests**

Replace contents of `crates/catcode-provider/src/lib.rs`:

```rust
pub mod deepseek;
pub mod mock;

pub use catcode_core::provider::*;

use catcode_core::provider::Provider;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for managing multiple providers.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a provider. Uses the provider's id() as the key.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    /// Get a provider by id.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).cloned()
    }

    /// List all registered provider ids.
    pub fn list_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// List providers that pass health check.
    pub async fn list_healthy(&self) -> Vec<Arc<dyn Provider>> {
        let mut healthy = Vec::new();
        for provider in self.providers.values() {
            if provider.health_check().await.is_ok() {
                healthy.push(provider.clone());
            }
        }
        healthy
    }

    /// Get all registered providers.
    pub fn all(&self) -> Vec<Arc<dyn Provider>> {
        self.providers.values().cloned().collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ProviderRegistry::new();
        let mock = Arc::new(MockProvider::with_text_response("hello"));
        registry.register(mock.clone());

        let got = registry.get("mock").unwrap();
        assert_eq!(got.id(), "mock");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list_ids() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::with_text_response("a")));
        let ids = registry.list_ids();
        assert!(ids.contains(&"mock".to_string()));
    }

    #[test]
    fn test_registry_default() {
        let registry = ProviderRegistry::default();
        assert!(registry.list_ids().is_empty());
    }

    #[tokio::test]
    async fn test_registry_list_healthy() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::with_text_response("hello")));

        let healthy = registry.list_healthy().await;
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id(), "mock");
    }

    #[tokio::test]
    async fn test_registry_all() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::with_text_response("a")));

        let all = registry.all();
        assert_eq!(all.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p catcode-provider --lib`
Expected: All 17+ tests pass (11 deepseek + 5 mock + 6 registry)

- [ ] **Step 3: Commit**

```bash
git add crates/catcode-provider/src/lib.rs
git commit -m "feat(provider): add ProviderRegistry"
```

---

## Chunk 8: DeepSeek Integration Test (requires network)

### Task 8: Integration test with real DeepSeek API (optional, skip if no API key)

**Files:**
- Create: `crates/catcode-provider/tests/deepseek_integration.rs`

- [ ] **Step 1: Create integration test file**

```rust
//! Integration tests for DeepSeek provider.
//! Requires DEEPSEEK_API_KEY environment variable.
//! Run: cargo test -p catcode-provider --test deepseek_integration -- --ignored

use catcode_core::provider::{Provider, ProviderContext};
use catcode_core::types::{ChatRequest, Message};
use catcode_provider::deepseek::DeepSeekProvider;

fn make_provider() -> Option<DeepSeekProvider> {
    let api_key = std::env::var("DEEPSEEK_API_KEY").ok()?;
    Some(DeepSeekProvider::new(
        api_key,
        "https://api.deepseek.com".to_string(),
    ))
}

#[tokio::test]
#[ignore]
async fn test_deepseek_chat_basic() {
    let provider = match make_provider() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: DEEPSEEK_API_KEY not set");
            return;
        }
    };

    let req = ChatRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![Message::user("Say exactly: hello world")],
        tools: None,
        system: None,
        max_tokens: Some(100),
        temperature: Some(0.0),
        stream: false,
    };
    let ctx = ProviderContext::default();

    let resp = provider.chat(req, &ctx).await.unwrap();
    let text = resp.text_content();
    assert!(!text.is_empty(), "Response should not be empty");
    assert_eq!(resp.model, "deepseek-chat");
    assert!(resp.usage.input_tokens > 0);
    assert!(resp.usage.output_tokens > 0);
}

#[tokio::test]
#[ignore]
async fn test_deepseek_health_check() {
    let provider = match make_provider() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: DEEPSEEK_API_KEY not set");
            return;
        }
    };

    assert!(provider.health_check().await.is_ok());
}
```

- [ ] **Step 2: Run integration tests (if API key available)**

Run: `DEEPSEEK_API_KEY=your-key cargo test -p catcode-provider --test deepseek_integration -- --ignored`
Expected: Tests pass (or skip if no key)

- [ ] **Step 3: Commit**

```bash
git add crates/catcode-provider/tests/
git commit -m "test(provider): add DeepSeek integration tests"
```

---

## Chunk 9: Final Polish

### Task 9: Clippy, fmt, and final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -p catcode-provider -- -D warnings`
Expected: No warnings. Fix any warnings found.

- [ ] **Step 2: Run fmt**

Run: `cargo fmt -p catcode-provider`
Expected: No changes (or apply changes)

- [ ] **Step 3: Run all tests**

Run: `cargo test -p catcode-provider`
Expected: All tests pass

- [ ] **Step 4: Update workspace Cargo.toml to include workspace.dependencies for reqwest**

Add to `[workspace.dependencies]` in root `Cargo.toml`:

```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
tracing = "0.1"
```

Update `crates/catcode-provider/Cargo.toml` to use workspace dep:

```toml
reqwest = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 5: Final check**

Run: `cargo check -p catcode-provider && cargo test -p catcode-provider`
Expected: Compiles and all tests pass

- [ ] **Step 6: Commit**

```bash
git add .
git commit -m "feat(provider): complete catcode-provider with DeepSeek, Mock, and Registry"
```

---

## Summary

| Chunk | Task | Tests | Description |
|-------|------|-------|-------------|
| 1 | Project Setup | 0 | Create crate structure |
| 2 | MockProvider | 5 | Configurable mock for testing |
| 3 | DeepSeek Request | 4 | OpenAI-compatible request serialization |
| 4 | DeepSeek Response | 2 | Response deserialization |
| 5 | DeepSeek Conversion | 3 | catcode-core <-> DeepSeek type conversion |
| 6 | DeepSeek Provider | 2 | Full Provider trait implementation |
| 7 | ProviderRegistry | 6 | Register, get, list providers |
| 8 | Integration Tests | 2 | Real API tests (optional) |
| 9 | Polish | 0 | Clippy, fmt, final verification |

**Total: ~24 tests, ~30 minutes of implementation time**
