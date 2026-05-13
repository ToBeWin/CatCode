#!/usr/bin/env bash
# catcode — one-command entry point for CatCode
#
# Usage:
#   catcode              → starts TUI (with daemon auto-started in background)
#   catcode daemon       → starts daemon only
#   catcode tui          → starts TUI only (daemon must be running)
#   catcode cli <msg>    → runs CLI mode with message
#   catcode build        → builds all binaries
#   catcode help         → shows this help

set -euo pipefail

CATCODE_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$CATCODE_DIR/target/release"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/catcode"
CONFIG_FILE="$CONFIG_DIR/config.toml"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RED='\033[0;31m'
NC='\033[0m'
BOLD='\033[1m'

ensure_binaries() {
  local missing=()
  for bin in catcode catcode-tui catcode-daemon; do
    if [ ! -x "$BIN_DIR/$bin" ]; then
      missing+=("$bin")
    fi
  done

  if [ ${#missing[@]} -gt 0 ]; then
    echo -e "${YELLOW}Missing binaries: ${missing[*]}${NC}"
    echo -e "${CYAN}Building with cargo --release...${NC}"
    cargo build --release --manifest-path "$CATCODE_DIR/Cargo.toml" 2>&1 | tail -5
    echo -e "${GREEN}Build complete.${NC}"
  fi
}

ensure_config() {
  if [ ! -f "$CONFIG_FILE" ]; then
    echo -e "${CYAN}Generating default config at $CONFIG_FILE...${NC}"
    mkdir -p "$CONFIG_DIR"
    cat > "$CONFIG_FILE" <<-EOF
[daemon]
host = "127.0.0.1"
port = 7070
auto_start = true
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[defaults]
provider = "deepseek"
model = "deepseek-chat"
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
    echo -e "${GREEN}Default config created.${NC}"
    echo -e "  Edit $CONFIG_FILE to set your provider and model."
    echo -e "  ${YELLOW}IMPORTANT: Set DEEPSEEK_API_KEY (or your provider's API key) environment variable.${NC}"
  fi
}

start_daemon() {
  if pgrep -f "catcode-daemon" > /dev/null 2>&1; then
    echo -e "${GREEN}CatCode daemon already running${NC}"
    return 0
  fi
  echo -e "${CYAN}Starting CatCode daemon...${NC}"
  ensure_binaries
  "$BIN_DIR/catcode-daemon" &
  DAEMON_PID=$!
  sleep 1
  if kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo -e "${GREEN}Daemon started (PID: $DAEMON_PID)${NC}"
  else
    echo -e "${RED}Failed to start daemon. Run with cargo: cargo run -p catcode-daemon${NC}"
    return 1
  fi
}

run_tui_with_daemon() {
  ensure_config
  start_daemon
  echo -e "${CYAN}Starting TUI...${NC}"
  ensure_binaries
  "$BIN_DIR/catcode-tui"
}

run_tui_only() {
  ensure_config
  if ! pgrep -f "catcode-daemon" > /dev/null 2>&1; then
    echo -e "${YELLOW}Daemon is not running. Starting daemon automatically...${NC}"
    start_daemon
  fi
  ensure_binaries
  "$BIN_DIR/catcode-tui"
}

run_cli() {
  ensure_config
  if ! pgrep -f "catcode-daemon" > /dev/null 2>&1; then
    echo -e "${YELLOW}Daemon is not running. Starting daemon automatically...${NC}"
    start_daemon
  fi
  ensure_binaries
  shift
  "$BIN_DIR/catcode" run "$*"
}

run_build() {
  echo -e "${CYAN}Building all CatCode binaries (release)...${NC}"
  cargo build --release --manifest-path "$CATCODE_DIR/Cargo.toml"
  echo ""
  echo -e "${GREEN}Build complete! Binaries in:${NC}"
  echo -e "  $BIN_DIR/catcode"
  echo -e "  $BIN_DIR/catcode-tui"
  echo -e "  $BIN_DIR/catcode-daemon"
}

show_help() {
  echo -e "${BOLD}catcode — CatCode AI coding agent${NC}"
  echo ""
  echo -e "${BOLD}Usage:${NC}"
  echo -e "  $(basename "$0")                   ${CYAN}Start TUI (auto-starts daemon)${NC}"
  echo -e "  $(basename "$0") daemon            ${CYAN}Start daemon only${NC}"
  echo -e "  $(basename "$0") tui               ${CYAN}Start TUI only (auto-starts daemon if needed)${NC}"
  echo -e "  $(basename "$0") cli <message>     ${CYAN}Run CLI mode with message${NC}"
  echo -e "  $(basename "$0") build             ${CYAN}Build all release binaries${NC}"
  echo -e "  $(basename "$0") help              ${CYAN}Show this help${NC}"
  echo ""
  echo -e "${BOLD}Environment:${NC}"
  echo -e "  DEEPSEEK_API_KEY    ${YELLOW}DeepSeek provider key${NC}"
  echo -e "  ANTHROPIC_API_KEY   ${YELLOW}Anthropic provider key${NC}"
  echo -e "  OPENAI_API_KEY      ${YELLOW}OpenAI provider key${NC}"
  echo -e "  GOOGLE_API_KEY      ${YELLOW}Google Gemini key${NC}"
  echo -e "  (and others — see README.md for full list)"
  echo ""
  echo -e "${BOLD}Config:${NC}"
  echo -e "  Global:  $CONFIG_FILE"
  echo -e "  Project: ./catcode.toml or ./.catcode/config.toml"
}

case "${1:-}" in
  daemon)
    ensure_config
    start_daemon
    wait
    ;;
  tui)
    run_tui_only
    ;;
  cli)
    if [ $# -lt 2 ]; then
      echo -e "${RED}Usage: $(basename "$0") cli <message>${NC}"
      exit 1
    fi
    run_cli "$@"
    ;;
  build)
    run_build
    ;;
  help|--help|-h)
    show_help
    ;;
  "")
    run_tui_with_daemon
    ;;
  *)
    echo -e "${RED}Unknown command: $1${NC}"
    echo -e "Usage: $(basename "$0") [daemon|tui|cli|build|help]"
    exit 1
    ;;
esac
