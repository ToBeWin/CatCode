# CatCode

**Model-agnostic open-source AI coding agent** — Rust, TUI-first, daemon architecture.

## Overview

CatCode is an AI coding agent that works with **10 real model providers plus a mock provider** through a unified runtime interface. It uses middleware and context engineering to compensate for model capability differences, rather than relying on the model itself.

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

Run the same local gates used by CI:

```bash
bash install.sh --check
cargo check --workspace
cargo clippy -p catcode-api -p catcode-daemon -p catcode-cli -p catcode-tui --all-targets --all-features -- -D warnings
cargo test -p catcode-api -p catcode-daemon -p catcode-cli -p catcode-tui
scripts/smoke-local.sh
cargo test -p catcode-daemon --test api_smoke -- --nocapture
```

---

### Key Features

- **10 Real Providers + Mock** — Anthropic, DeepSeek, OpenAI, Qwen, Google Gemini, MiniMax, GLM, Ollama, OpenRouter, Volcengine + Mock
- **13 Built-in Tools** — read, write, patch, git, bash, search, web_fetch, code_analysis, delete, glob, list_dir
- **Harness Engineering** — middleware for retry, timeout, loop detection, output validation, sandbox gate, and token usage tracking
- **Thinking Mode** — Real-time reasoning/chain-of-thought display in TUI (DeepSeek reasoning_content)
- **Code Review** — Pattern-based (8 detectors) + LLM-deep review with structured findings
- **Security Check** — Secret detection (11 regex patterns), code injection scan, dependency CVE check, config audit
- **SWE-Bench Harness** — Full evaluation framework: parallel instance execution, git lifecycle, test runner, detailed reporting
- **Context Engineering** — 3-layer context (Permanent/Session/Working), smart compression, token budget management, prompt cache optimization
- **Sandbox / Safety Controls** — NativeSandbox only: path checks, timeout/output truncation, operation safety classification (Safe/Sensitive/Dangerous), approval gates
- **Plan/Act/Auto/Goal Modes** — Plan mode for analysis, Act for execution, Auto for planned execution, Goal for autonomous task pursuit
- **Extensible Core** — Skills/plugins/MCP modules exist, with MCP and WASM support still early-stage
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
 (no IO)  10provs  safety    3-layer  13tools  Native   Skill/
          +Mock    stack     +budget          only     MCP/WASM
```

## Crates

| Crate | Description | Lines | Tests |
|-------|-------------|-------|-------|
| `catcode-core` | Core types, traits, errors (zero IO) | 1,436 | 133 |
| `catcode-provider` | 10 real providers + Mock | 6,162 | 136 |
| `catcode-middleware` | 8-layer safety middleware chain | 2,918 | 86 |
| `catcode-context` | Context engineering, compression, budget | 2,694 | 105 |
| `catcode-daemon` | Agent loop, sessions, persistence, harness planning, code_review, security_check, swe_bench | ~13,700 | 257 |
| `catcode-tools` | 13 built-in tools | 2,555 | 120 |
| `catcode-sandbox` | Native sandbox, classification, approvals | 963 | 31 |
| `catcode-plugin` | Skills (TOML), Plugins, MCP client, WASM sandbox | 2,123 | 59 |
| `catcode-api` | axum REST + SSE + WebSocket API + Auth + harness/changes/review/handoff endpoints | 3,008 | 58 |
| `catcode-tui` | ratatui TUI with cat mascot, thinking mode, recovery insights | 4,457 | 114 |
| `catcode-cli` | Non-interactive CLI binary + harness/changes/review/handoff inspection | 1,210 | 1 |
| **Total** | **11 crates** | **33,562** | Run `cargo test --workspace -- --list` for current count |

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

## Coding Harness Contract

The shared runtime prompt is designed for coding-agent work, not generic chat:

- inspect relevant files and tests before editing
- make scoped, reviewable changes
- preserve unrelated user changes
- run relevant verification after edits
- diagnose failed verification and report blockers clearly
- keep progress, changed files, and verification visible to the user

The daemon also builds a deterministic harness plan for every run:

- repo profile: language stack, package manager, important files
- phase plan: intake, repo scan, context pack, edit, diff review, verification, recovery, final report
- suggested verification commands such as `cargo check --workspace` or `pytest`
- lightweight context packs selected from repo guidance, manifests, current changes, task keyword matches, entrypoints, and test surfaces
- structured harness step events that TUI/API clients can render without parsing plain text
- post-run git snapshots that emit `DiffReview`, `Verification`, or `Recovery` steps based on whether the working tree changed and whether the run succeeded
- diff review summaries that show the changed file list without loading large patch bodies
- safe auto-verification execution for allowlisted, non-shell commands such as `cargo check --workspace`
- actionable verification failure diagnostics that extract error summaries, file locations, and next-step repair suggestions
- verification repair plans that name the files to inspect, repair steps, and a narrow rerun command when one can be derived
- one scoped automatic repair pass after failed auto-verification, followed by a verification rerun
- workspace change summaries via `catcode changes`, TUI `/changes`, and `GET /api/v1/changes`
- local pattern-based changed-file review via `catcode review`, TUI `/review`, and `GET /api/v1/review`
- CLI/API access via `catcode harness [task]` and `GET /api/v1/harness`
- final handoff gates via `catcode handoff [task]`, TUI `/handoff`, and `GET /api/v1/handoff`, combining changed-file summaries, local review findings, and safe verification into a ready/blocker report

### Final Handoff Gate

For coding tasks, CatCode can run a final handoff check before the user accepts the result. The handoff report summarizes the working tree changes, runs the local changed-file review, runs safe auto-verification when an allowlisted command is available, and marks the result as ready only when there are no blocking review errors or failed verification.

Available through `catcode handoff [task]`, TUI `/handoff`, `GET /api/v1/handoff`, and JSON output with `catcode handoff --json`.

## TUI Recovery UX

The TUI includes an adaptive insights panel on wider terminals:

- active session state and failure reason
- startup provider setup warnings before the first model call fails
- send-time provider preflight that stops guaranteed missing-key failures before calling the model
- token usage summary for input, output, and cache tokens
- deterministic recovery hints after failed or tool-heavy turns
- `/recovery` command for an in-chat recovery plan

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
| `/provider <name>` | Set/view provider |
| `/set-provider <name>` | Alias for `/provider` |
| `/model <name>` | Set/view model |
| `/usage` | Show token usage |
| `/recovery` | Show recovery plan |
| `/harness` | Show coding harness plan |
| `/changes` | Show current changed files |
| `/review` | Review current changed files |
| `/handoff` | Run final handoff gate for current changes |
| `/plan` | Enter plan mode (no tool execution) |
| `/act` | Enter act mode (default, tools available) |
| `/auto` | Plan first, then execute after approval |
| `/goal <objective>` | Create autonomous goal |
| `/goal status\|pause\|resume\|clear` | Manage goals |
| `/benchmark list\|results\|clear` | Benchmark evaluation |
| `/cat on\|off` | Toggle cat mascot |
| `/help` | Show commands |
| `/quit` | Exit CatCode |

## CLI Subcommands

```
catcode version              # Show version
catcode help                 # Show help
catcode daemon start         # Start daemon process
catcode daemon stop          # Stop daemon started by CLI
catcode daemon status        # Check daemon status
catcode daemon restart       # Restart daemon process
catcode session list         # List sessions
catcode session create <n>   # Create session
catcode session audit <id>   # Show session audit log
catcode session messages <id> # Show persisted message history
catcode session usage <id>   # Show aggregated token usage
catcode session recovery <id> # Show recovery plan
catcode run <message>        # Non-interactive agent run
catcode harness [task]       # Show coding harness plan
catcode changes              # Show current working tree changes
catcode review               # Review current changed files
catcode handoff [task]       # Run changes, review, and verification gate
```

## API Endpoints

```
GET    /api/v1/health            # Health check
GET    /api/v1/version           # Version info
GET    /api/v1/providers         # List providers
GET    /api/v1/harness           # Get coding harness plan
GET    /api/v1/changes           # Get changed file summary
GET    /api/v1/review            # Review changed files
GET    /api/v1/handoff           # Run final handoff gate

GET    /api/v1/sessions          # List sessions
POST   /api/v1/sessions          # Create session
GET    /api/v1/sessions/:id      # Get session
DELETE /api/v1/sessions/:id      # Delete session
GET    /api/v1/sessions/:id/audit    # List audit log entries
GET    /api/v1/sessions/:id/messages # List persisted messages
GET    /api/v1/sessions/:id/recovery # Get recovery plan
GET    /api/v1/sessions/:id/usage    # Get token usage summary
POST   /api/v1/sessions/:id/message  # Send message
POST   /api/v1/sessions/:id/pause    # Pause session
POST   /api/v1/sessions/:id/resume   # Resume session

GET    /api/v1/events            # SSE event stream
GET    /api/v1/ws                # WebSocket
```

## Mobile / IM 集成

CatCode 提供 **cc-connect** 适配脚本和配置示例；真正的 IM 平台接入由外部 cc-connect 负责。无需为 CatCode 开发独立 App，也通常不需要公网服务器。

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
