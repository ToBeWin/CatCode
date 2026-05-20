#!/usr/bin/env bash
# Local smoke test for CatCode.
#
# This uses the mock provider, so it does not require network access or API keys.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/catcode-smoke.XXXXXX")"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cat > "$TMP_DIR/catcode.toml" <<'EOF'
[daemon]
host = "127.0.0.1"
port = 7070
auto_start = true
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[defaults]
provider = "mock"
model = "mock-model"
sandbox = false

[budget]
session_limit_tokens = 500000
per_request_limit_tokens = 50000
warning_threshold = 0.80

[context]
compression_enabled = true
dedup_tool_outputs = true
max_file_content_tokens = 8000

[observability]
log_level = "info"
log_format = "text"
EOF

echo "[smoke] cargo check"
cargo check --manifest-path "$ROOT_DIR/Cargo.toml"

echo "[smoke] catcode run with mock provider"
output="$(
  cargo run \
    --manifest-path "$ROOT_DIR/Cargo.toml" \
    -p catcode-cli \
    -- run "hello from local smoke" \
    --provider mock \
    --model mock-model \
    --project-dir "$TMP_DIR"
)"

echo "$output"

if ! grep -q "Mock provider response" <<<"$output"; then
  echo "[smoke] expected mock provider response not found" >&2
  exit 1
fi

echo "[smoke] ok"
