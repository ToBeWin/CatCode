# CatCode AGENTS.md

## Agent Rules

### Code Style
- Error handling: Use anyhow::Result for public APIs, thiserror for internal errors
- Async: Use tokio throughout, no blocking calls in async contexts
- Logging: Use tracing macros (info!/warn!/error!/debug!), never println!
- Format: cargo fmt before commits, zero clippy warnings
- Tests: Each crate should have #[cfg(test)] module in src/lib.rs

### Architecture Rules
- catcode-core must not depend on any IO crate
- Tool execution must pass through OperationLevel checks
- All write operations must be recorded in audit_log
- Token usage must be recorded after every API call

### Architecture Decisions
- **Sandbox**: NativeSandbox only (path checks + timeout + truncation). No Docker.
- **IM/Mobile**: No native mobile app or IM SDK. Use cc-connect as local bridge (covers Feishu, DingTalk, WeChat Work, Weixin, Telegram, Discord, Slack, QQ, LINE).
- **No server required**: CatCode + cc-connect both run locally; IM platforms connect via outbound WebSocket/long-polling.

### Development Workflow
1. Run `cargo check` after any code change
2. Run `cargo test` before committing
3. Run `cargo clippy` to check for warnings
4. Keep CLAUDE.md updated with architecture changes
