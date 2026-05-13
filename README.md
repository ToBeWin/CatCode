# CatCode

**Model-agnostic open-source AI coding agent** — Rust, TUI-first, daemon architecture, 849 tests.

## Overview

CatCode is an AI coding agent that works with **11 model providers** through a unified interface. It uses harness engineering to compensate for model capability differences, rather than relying on the model itself.

## Quick Start (30 seconds)

```bash
# 1. Build everything
cargo build --release

# 2. Launch TUI (interactive) — daemon auto-starts in background
./catcode.sh

# Or run components separately:
#   ./target/release/catcode-daemon    # daemon (background API server)
#   ./target/release/catcode-tui       # TUI (interactive terminal UI)
#   ./target/release/catcode run "..." # CLI mode
```

Set your API key:
```bash
export DEEPSEEK_API_KEY="sk-your-key-here"
# or: export ANTHROPIC_API_KEY="..."
# or: export OPENAI_API_KEY="..."
# see full list of providers below
```

Type a message and press Enter.

---

### Key Features

- **11 Model Providers** — Anthropic, DeepSeek, OpenAI, Qwen, Google Gemini, MiniMax, GLM, Ollama, OpenRouter, Volcengine + Mock
- **13 Built-in Tools** — read, write, patch, git, bash, search, web_fetch, code_analysis, delete, glob, list_dir
- **Harness Engineering** — 8-layer middleware: circuit breaker, retry, timeout, loop detection, output validation, model routing, sandbox gate, token usage tracking
- **Thinking Mode** — Real-time reasoning/chain-of-thought display in TUI (DeepSeek reasoning_content)
- **Code Review** — Pattern-based (8 detectors) + LLM-deep review with structured findings
- **Security Check** — Secret detection (11 regex patterns), code injection scan, dependency CVE check, config audit
- **SWE-Bench Harness** — Full evaluation framework: parallel instance execution, git lifecycle, test runner, detailed reporting
- **Context Engineering** — 3-layer context (Permanent/Session/Working), smart compression, token budget management, prompt cache optimization
- **Sandbox Isolation** — Native + Docker backends, operation safety classification (Safe/Sensitive/Dangerous), approval gates
- **Plan/Act/Auto/Goal Modes** — Plan mode for analysis, Act for execution, Auto for planned execution, Goal for autonomous task pursuit
- **Extensible** — Skills (TOML), Plugins (Rust/WASM), MCP (Model Context Protocol, JSON-RPC over stdio)
- **Daemon Architecture** — Background multi-agent concurrency, REST/SSE/WebSocket API, SQLite persistence, audit log
- **Cat Mascot** — ASCII art cat with 5 state-based animations
- **Benchmark** — Built-in evaluation for provider+model combinations

## Architecture

```
 Client Layer
 ┌─────────────────────────────────────────────────────┐
 │  catcode-tui (ratatui)  │  catcode-cli (CLI)       │
 │  catcode-daemon (daemon) │  third-party via API     │
 └──────────────────────┬──────────────────────────────┘
                        │
                        ▼
 catcode-api (axum REST + SSE + WebSocket + Auth)
                        │
                        ▼
 catcode-daemon ──────────────────────────────────────────
 │  Session Manager + Concurrent Sessions + SubAgents     │
 │  Agent Loop (LLM → Tool → LLM cycle)                   │
 │  CodeReview + SecurityCheck + SWE-Bench Harness        │
 │  SQLite Persistence + Checkpoint + Audit Log           │
 ───────────┬──────────────────────────────────────────────
            │
     ┌──────┼──────┬──────┬──────┬──────┬──────┐
     ▼      ▼      ▼      ▼      ▼      ▼      ▼
  catcode  catcode  catcode catcode catcode catcode catcode
 -core    -provider -middleware -context -tools  -sandbox -plugin
 (no IO)  11provs  8-layer   3-layer  13tools  Docker   Skill/
          +Mock    safety    +budget          +Native  MCP/WASM
```

## Crates

| Crate | Description | Lines | Tests |
|-------|-------------|-------|-------|
| `catcode-core` | Core types, traits, errors (zero IO) | 1,436 | 42 |
| `catcode-provider` | 11 model providers + Mock | 6,162 | 136 |
| `catcode-middleware` | 8-layer safety middleware chain | 2,918 | 86 |
| `catcode-context` | Context engineering, compression, budget | 2,694 | 88 |
| `catcode-daemon` | Agent loop, sessions, persistence, code_review, security_check, swe_bench | ~4,600 | 186 |
| `catcode-tools` | 13 built-in tools | 2,555 | 102 |
| `catcode-sandbox` | Native + Docker sandbox, classification, approvals | 963 | 39 |
| `catcode-plugin` | Skills (TOML), Plugins, MCP client, WASM sandbox | 2,123 | 59 |
| `catcode-api` | axum REST + SSE + WebSocket API + Auth | 747 | 13 |
| `catcode-tui` | ratatui TUI with cat mascot, thinking mode | 2,961 | 94 |
| `catcode-cli` | Non-interactive CLI binary | 333 | — |
| **Total** | **11 crates** | **33,562** | **849** |

## Built-in Tools

| Tool | Level | Description |
|------|-------|-------------|
| `read_file` | 🟢 Safe | Read file with line range support |
| `list_dir` | 🟢 Safe | List directory contents |
| `search_files` | 🟢 Safe | Search content via ripgrep |
| `glob` | 🟢 Safe | Glob pattern matching |
| `git_status` | 🟢 Safe | Check git status |
| `git_diff` | 🟢 Safe | Show git diff |
| `code_analysis` | 🟢 Safe | AST-like code stats (lines, functions, classes) |
| `write_file` | 🟡 Sensitive | Write/create file with parent dir creation |
| `patch_file` | 🟡 Sensitive | Find and replace text in file (single match) |
| `git_commit` | 🟡 Sensitive | Git add + commit with message |
| `bash` | 🔴 Dangerous | Shell command execution with timeout |
| `delete_file` | 🔴 Dangerous | Delete file or directory |
| `web_fetch` | 🔴 Dangerous | HTTP GET URL fetch |

## Middleware Chain (8 layers)

| Middleware | Purpose |
|-----------|---------|
| ErrorHandling | Catch panics via `catch_unwind` |
| Timeout | 60s per-tool timeout via `tokio::time::timeout` |
| Retry | Exponential backoff (3 attempts, 1s-30s) |
| LoopDetection | MD5 sliding window duplicate detection |
| Sandbox | OperationLevel check via ApprovalGate |
| OutputValidator | Length cap + safety patterns + bash injection detection |
| TokenUsage | Record ChatResponse usage into AgentContext |
| ModelRouter | Cost-aware routing: simple vs powerful model |

## Thinking Mode

CatCode supports real-time reasoning display (chain-of-thought):

- **DeepSeek** — `reasoning_content` parsed from streaming SSE chunks
- **TUI** — Thinking panel with yellow/italic "Thinking..." header box
- Per-message thinking content rendered before message body
- Works with any provider that supports reasoning in streaming responses

## Code Review

```rust
let mut reviewer = CodeReviewer::new();
reviewer
    .add_file("src/main.rs", content)
    .add_diff(diff_output);

// Pattern-based review (fast, no LLM)
let review = reviewer.review_patterns();

// LLM deep review
let review = reviewer.review_deep(provider, "claude-sonnet-4").await;

// Combined
let review = reviewer.review_full(provider, "claude-sonnet-4").await;
```

8 static detectors: TODOs, debug prints, secrets, unwrap(), large diffs, missing docs, long functions, deep nesting.

## Security Check

```rust
let scanner = SecurityScanner::new();
let report = scanner.scan_directory("/path/to/project");

println!("Critical: {}", report.summary.critical);
println!("Secrets found: {}",
    report.findings.iter().filter(|f| f.category == FindingCategory::Secret).count());
```

4 scanners: Secret detection (11 regex), Code injection, Dependency CVE (14 built-in), Config audit.

## SWE-Bench Harness

```rust
let harness = SweBenchHarness::new(config, provider, tools, middleware);
let instances = SweBenchHarness::load_dataset(path)?;
let report = harness.evaluate_all(&instances).await;

println!("{}", SweBenchHarness::format_summary(&report));
harness.save_results(&report, "/tmp/report").await?;
```

- Parallel instance execution (configurable concurrency)
- Git lifecycle: clone → checkout → branch → diff → apply patch → run tests
- Supports JSON array + JSONL datasets
- Outputs: JSON report, Markdown summary, per-instance patches
- 5 built-in sample instances for quick testing

## Supported Providers

| Provider | Models | Auth |
|----------|--------|------|
| Anthropic | claude-sonnet-4, claude-opus-4, claude-haiku-4.5 | `ANTHROPIC_API_KEY` |
| OpenAI | gpt-4.1, gpt-4.1-mini, gpt-4.1-nano, gpt-4o, o3, o3-mini, o4-mini | `OPENAI_API_KEY` |
| DeepSeek | deepseek-chat, deepseek-reasoner | `DEEPSEEK_API_KEY` |
| Qwen | qwen3, qwen3-coder, qwen3-moe, qwen2.5 | `QWEN_API_KEY` |
| Google | gemini-2.5-pro, gemini-2.5-flash, gemini-2.0-flash | `GOOGLE_API_KEY` |
| MiniMax | MiniMax-M1, MiniMax-Text-01 | `MINIMAX_API_KEY` |
| GLM (Zhipu) | glm-4-plus, glm-4-flash, glm-4-long, glm-z1-air | `GLM_API_KEY` |
| OpenRouter | 10 models (claude, gpt-4o, gemini, deepseek, qwen, mistral, llama) | `OPENROUTER_API_KEY` |
| Volcengine | doubao-1.5-pro, doubao-1.5-lite, deepseek-r1, deepseek-v3 | `VOLCENGINE_API_KEY` |
| Ollama | llama3.1, qwen2.5, codellama (local) | None (local) |

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
| `/review <file\|diff>` | Run code review |
| `/security <path>` | Run security scan |
| `/thinking on\|off` | Toggle thinking mode panel |
| `/quit` | Exit CatCode |

## CLI Subcommands

```
catcode version              # Show version
catcode help                 # Show help
catcode daemon start         # Start daemon process
catcode daemon status        # Check daemon status
catcode session list         # List sessions
catcode session create <n>   # Create session
catcode run <message>        # Non-interactive agent run
```

## API Endpoints

```
GET    /api/v1/health            # Health check
GET    /api/v1/version           # Version info
GET    /api/v1/providers         # List providers

GET    /api/v1/sessions          # List sessions
POST   /api/v1/sessions          # Create session
GET    /api/v1/sessions/:id      # Get session
DELETE /api/v1/sessions/:id      # Delete session
POST   /api/v1/sessions/:id/message  # Send message
POST   /api/v1/sessions/:id/pause    # Pause session
POST   /api/v1/sessions/:id/resume   # Resume session

GET    /api/v1/events            # SSE event stream
GET    /api/v1/ws                # WebSocket
```

## Mobile / IM 集成

CatCode 通过 **cc-connect** 桥接到即时通讯平台，无需开发独立 App，无需公网服务器。

```
钉钉 / 飞书 / 企微 / Telegram / Discord / Slack / QQ / LINE
                ↓  出站 WebSocket/长轮询（无需公网IP）
          cc-connect (本地网桥)
                ↓  HTTP API
      catcode-daemon (REST API :7070)
```

### 快速开始

```bash
# 1. 安装 cc-connect
npm install -g cc-connect

# 2. 终端1：启动 CatCode daemon
cargo run -p catcode-daemon

# 3. 终端2：复制配置并启动 cc-connect
cp cc-connect.toml.example catcode-cc-connect.toml
# 编辑配置，填入 IM 平台凭证
cc-connect -config catcode-cc-connect.toml
```

### 支持的平台

| 平台 | 连接方式 | 公网IP | 配置指南 |
|------|---------|--------|---------|
| 飞书 | WebSocket | ❌ | 见 cc-connect.toml.example |
| 钉钉 | Stream Mode | ❌ | 见 cc-connect.toml.example |
| Telegram | Long Polling | ❌ | 见 cc-connect.toml.example |
| Discord | Gateway | ❌ | 见 cc-connect.toml.example |
| Slack | Socket Mode | ❌ | 见 cc-connect.toml.example |
| 企业微信 | WebSocket | ❌ | 见 cc-connect.toml.example |
| 个人微信 | ilink 长轮询 | ❌ | 见 cc-connect.toml.example |
| QQ | WebSocket | ❌ | 见 cc-connect.toml.example |

详细配置请参考 `cc-connect.toml.example` 和 `scripts/catcode-agent.sh`。

## Skills System

Skills are TOML-defined capability bundles that auto-inject system prompts, rules, and hooks. See `skills/` directory:

```toml
[skill]
name = "rust"
version = "1.0.0"
description = "Rust project development skill"

[rules]
always_run = ["cargo check", "cargo clippy"]
prefer_tools = ["code_analysis", "patch_file"]

[prompts]
system_suffix = "You are working on a Rust project..."

[hooks]
before_commit = "cargo test"
after_write = "cargo fmt"
```

## License

MIT
