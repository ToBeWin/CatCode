# Changelog

## Unreleased

- Added a shared runtime path for CLI, TUI, daemon, and API-backed message execution.
- Persisted sessions, messages, and token usage through SQLite-backed daemon storage.
- Added daemon API smoke coverage for health, session creation, message execution, SSE, WebSocket events, and database rows.
- Added local mock-provider smoke coverage via `scripts/smoke-local.sh`.
- Added GitHub Actions CI for workspace checks, strict clippy on production crates, focused tests, local smoke, and daemon API smoke.
- Added CLI daemon lifecycle commands for start, stop, restart, and status.
- Added audit middleware for API write operations.
- Added `GET /api/v1/sessions/:id/audit` for persisted audit-log retrieval.
- Added `catcode session audit <id>` for CLI audit-log inspection.
- Persisted agent execution failures as `failed:<reason>` session state and stored error messages for recovery/debugging.
- Added `GET /api/v1/sessions/:id/messages` and `catcode session messages <id>` for persisted message history retrieval.
- Added `GET /api/v1/sessions/:id/usage` and `catcode session usage <id>` for token usage summaries.
- Added `GET /api/v1/sessions/:id/recovery` and `catcode session recovery <id>` for deterministic recovery plans.
- Fixed the cc-connect helper script request schema and made provider, model, and project directory configurable.
- Documented the NativeSandbox-only architecture and current provider/runtime scope.
- Added installer `--check` preflight mode for validating prerequisites without building or writing files.
