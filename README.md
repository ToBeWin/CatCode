# CatCode

**Model-agnostic open-source AI coding agent** — Rust, TUI-first, daemon architecture.

## Overview

CatCode is an AI coding agent that works with any mainstream model (domestic and international) through a unified interface. It uses harness engineering to compensate for model capability differences, rather than relying on the model itself.

### Key Features

- **Model Agnostic** — Anthropic, DeepSeek, Ollama, OpenAI, Qwen, and more
- **Harness Engineering** — Circuit breaker, retry, output validation, model routing
- **Context Engineering** — Layered context, smart compression, token budget management
- **Daemon Architecture** — Background multi-agent concurrency, remote control via API
- **Sandbox Isolation** — Operation safety classification, approval gates
- **Extensible** — Skills (TOML), Plugins (Rust), MCP (Model Context Protocol)

## Architecture

```
Client Layer (TUI / CLI / Web / Mobile)
    │
    ▼
catcode-api (axum REST + SSE + WebSocket)
    │
    ▼
catcode-daemon (Session Manager + Agent Loop)
    │
    ├─► catcode-middleware (Circuit Breaker + Retry + Model Router)
    ├─► catcode-context (Layered Context + Token Budget + Prompt Cache)
    ├─► catcode-provider (Anthropic + DeepSeek + Ollama + OpenAI)
    ├─► catcode-tools (read_file + write_file + bash + search)
    ├─► catcode-sandbox (Docker + Operation Classification)
    └─► catcode-plugin (Skills + Plugins + MCP)
```

## Crate Structure

| Crate | Description |
|-------|-------------|
| `catcode-core` | Core types, traits, error definitions (no IO) |
| `catcode-provider` | Model provider abstraction + implementations |
| `catcode-middleware` | Middleware chain, circuit breaker, retry, model router |
| `catcode-context` | Context engineering, token budget, compression |
| `catcode-daemon` | Daemon process, session management, persistence |
| `catcode-tools` | Built-in tool implementations |
| `catcode-sandbox` | Sandbox isolation and operation classification |
| `catcode-api` | HTTP/WebSocket API for remote control |
| `catcode-plugin` | Skill/Plugin/MCP extension system |
| `catcode-tui` | Terminal UI (ratatui) |

## Quick Start

```bash
# Build
cargo build --release

# Run tests
cargo test --workspace

# Check code quality
cargo clippy --workspace
```

## Supported Providers

| Provider | Models | Status |
|----------|--------|--------|
| Anthropic | Claude Sonnet 4, Claude Opus 4, Claude Haiku 4.5 | Implemented |
| OpenAI | gpt-4o, gpt-4o-mini, o3, o3-mini | Implemented |
| DeepSeek | deepseek-chat, deepseek-reasoner | Implemented |
| Qwen (DashScope) | qwen3, qwen3-coder, qwen3-moe, qwen2.5 | Implemented |
| Ollama | llama3.1, qwen2.5, codellama (local) | Implemented |
| Google | gemini-2.5-pro | Planned |

## API Endpoints

```
GET    /api/v1/health          # Health check
GET    /api/v1/version         # Version info
GET    /api/v1/providers       # List providers

GET    /api/v1/sessions        # List sessions
POST   /api/v1/sessions        # Create session
GET    /api/v1/sessions/:id    # Get session
DELETE /api/v1/sessions/:id    # Delete session
POST   /api/v1/sessions/:id/message  # Send message
POST   /api/v1/sessions/:id/pause    # Pause session
POST   /api/v1/sessions/:id/resume   # Resume session

GET    /api/v1/events          # SSE event stream
GET    /api/v1/ws              # WebSocket
```

## Configuration

```toml
[daemon]
host = "127.0.0.1"
port = 7070

[defaults]
provider = "anthropic"
model = "claude-sonnet-4-20250514"

[budget]
session_limit_tokens = 500000
warning_threshold = 0.80

[routing]
strategy = "cost_aware"
simple_model = "deepseek-chat"
powerful_model = "claude-sonnet-4-20250514"
```

## License

MIT
