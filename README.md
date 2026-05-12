# CatCode

**Model-agnostic open-source AI coding agent** — Rust, TUI-first, daemon architecture.

## Overview

CatCode is an AI coding agent that works with any mainstream model (domestic and international) through a unified interface. It uses harness engineering to compensate for model capability differences, rather than relying on the model itself.

### Key Features

- **Model Agnostic** — Anthropic, DeepSeek, Ollama, OpenAI, Qwen, Google Gemini, MiniMax, GLM
- **Harness Engineering** — Circuit breaker, retry, output validation, model routing
- **Context Engineering** — Layered context, smart compression, token budget management
- **Daemon Architecture** — Background multi-agent concurrency, remote control via API
- **Sandbox Isolation** — Operation safety classification, approval gates, WASM plugin sandbox
- **Extensible** — Skills (TOML), Plugins (Rust/WASM), MCP (Model Context Protocol)
- **Plan/Act Mode** — Plan mode for analysis, Act mode for execution, Auto mode for planning then executing
- **Goal Mode** — Autonomous goal-driven execution with token budget tracking
- **Benchmark** — Built-in evaluation system for provider+model combinations
- **Cat Mascot** — ASCII art cat with state-based animations

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
    ├─► catcode-provider (Anthropic + DeepSeek + Ollama + OpenAI + Qwen + Google + MiniMax + GLM)
    ├─► catcode-tools (read_file + write_file + bash + search)
    ├─► catcode-sandbox (Docker + Operation Classification)
    └─► catcode-plugin (Skills + Plugins + MCP + WASM Sandbox)
```

## Crate Structure

| Crate | Description |
|-------|-------------|
| `catcode-core` | Core types, traits, error definitions (no IO) |
| `catcode-provider` | Model provider abstraction + implementations |
| `catcode-middleware` | Middleware chain, circuit breaker, retry, model router |
| `catcode-context` | Context engineering, token budget, compression |
| `catcode-daemon` | Daemon process, session management, persistence, benchmark |
| `catcode-tools` | Built-in tool implementations |
| `catcode-sandbox` | Sandbox isolation and operation classification |
| `catcode-api` | HTTP/WebSocket API for remote control |
| `catcode-plugin` | Skill/Plugin/MCP extension system + WASM sandbox |
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

## TUI Commands

| Command | Description |
|---------|-------------|
| `/new <name>` | Create a new session |
| `/sessions` | List all sessions |
| `/switch <n\|name>` | Switch to session |
| `/close` | Close current session |
| `/clear` | Clear messages |
| `/model <name>` | Set/view model |
| `/usage` | Show token usage |
| `/plan` | Enter plan mode (no tool execution) |
| `/act` | Enter act mode (default, tools available) |
| `/auto` | Plan first, then execute after approval |
| `/goal <objective>` | Create autonomous goal |
| `/goal status\|pause\|resume\|clear` | Manage goals |
| `/benchmark list\|results\|clear` | Benchmark evaluation |
| `/cat on\|off` | Toggle cat mascot |
| `/quit` | Exit CatCode |

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Enter` | Send message |
| `/` or `Tab` | Enter command mode |
| `Ctrl+P` | Toggle plan/act mode |
| `Ctrl+N` | New session |
| `Ctrl+W` | Close session |
| `Ctrl+K` | Clear messages |
| `Ctrl+L` | Clear input |
| `Ctrl+1-9` | Switch to session N |
| `Ctrl+Left/Right` | Cycle sessions |
| `PageUp/Down` | Scroll history |
| `Home/End` | Scroll to top/bottom |

## Supported Providers

| Provider | Models | Status |
|----------|--------|--------|
| Anthropic | claude-sonnet-4, claude-opus-4, claude-haiku-4.5 | Implemented |
| OpenAI | gpt-4.1, gpt-4.1-mini, gpt-4.1-nano, gpt-4o, o3, o3-mini, o4-mini | Implemented |
| DeepSeek | deepseek-chat, deepseek-reasoner | Implemented |
| Qwen (DashScope) | qwen3, qwen3-coder, qwen3-moe, qwen2.5 | Implemented |
| Google | gemini-2.5-pro, gemini-2.5-flash, gemini-2.0-flash | Implemented |
| MiniMax | MiniMax-M1, MiniMax-Text-01 | Implemented |
| GLM (Zhipu) | glm-4-plus, glm-4-flash, glm-4-long, glm-z1-air | Implemented |
| Ollama | llama3.1, qwen2.5, codellama (local) | Implemented |

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
