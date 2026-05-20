#!/usr/bin/env bash
# CatCode — AI coding agent installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/anomalyco/CatCode/main/install.sh | bash
#   ./install.sh [--help]
#   ./install.sh --check
#
# Detects OS, checks for Rust toolchain, builds from source,
# and installs symlinks in ~/.local/bin.

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RED='\033[0;31m'
NC='\033[0m'
BOLD='\033[1m'

CATCODE_DIR="$(cd "$(dirname "$0")" && pwd)"
VERSION=$(grep '^version' "$CATCODE_DIR/Cargo.toml" 2>/dev/null | head -1 | sed 's/.*= "//;s/"//' || echo "?")
BIN_SRC="$CATCODE_DIR/target/release"
BINS=(catcode catcode-tui catcode-daemon)
CHECK_ONLY=false

print_banner() {
  printf "\033[0;36m\n"
  cat <<'EOF'
   ____          _        ____          _
  / ___|__ _ ___| |__    / ___|___   __| | ___
 | |   / _` / __| '_ \  | |   / _ \ / _` |/ _ \
 | |__| (_| \__ \ | | | | |__| (_) | (_| |  __/
  \____\__,_|___/_| |_|  \____\___/ \__,_|\___|
EOF
  printf "\033[0m"
  echo -e "${CYAN}╔══════════════════════════════════════════╗${NC}"
  echo -e "${CYAN}║       CatCode Installer v${VERSION}            ║${NC}"
  echo -e "${CYAN}╚══════════════════════════════════════════╝${NC}"
  echo ""
}

detect_os() {
  case "$(uname -s)" in
    Linux*)  echo "linux";;
    Darwin*) echo "macos";;
    CYGWIN*|MINGW*|MSYS*) echo "windows";;
    *)       echo "unknown";;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "x86_64";;
    aarch64|arm64) echo "aarch64";;
    *)            echo "unknown";;
  esac
}

detect_install_dir() {
  # Prefer ~/.local/bin (XDG), then /usr/local/bin
  if [ -d "$HOME/.local/bin" ] || echo "$PATH" | tr ':' '\n' | grep -q "$HOME/.local/bin"; then
    echo "$HOME/.local/bin"
  elif [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
    echo "/usr/local/bin"
  elif [ -w "$HOME/.local/bin" ]; then
    echo "$HOME/.local/bin"
  else
    # Create ~/.local/bin
    mkdir -p "$HOME/.local/bin"
    echo "$HOME/.local/bin"
  fi
}

check_cargo() {
  if command -v cargo &>/dev/null; then
    echo -e "  ${GREEN}✓${NC} Rust toolchain found: $(cargo --version)"
    return 0
  fi
  return 1
}

check_source_tree() {
  local missing=false

  for path in Cargo.toml crates/catcode-cli crates/catcode-tui crates/catcode-daemon; do
    if [ -e "$CATCODE_DIR/$path" ]; then
      echo -e "  ${GREEN}✓${NC} $path"
    else
      echo -e "  ${RED}✗${NC} Missing $path"
      missing=true
    fi
  done

  if [ "$missing" = true ]; then
    return 1
  fi
}

check_release_binaries() {
  local missing=false

  echo ""
  echo -e "${CYAN}Checking release binaries...${NC}"
  for bin in "${BINS[@]}"; do
    if [ -x "$BIN_SRC/$bin" ]; then
      echo -e "  ${GREEN}✓${NC} $BIN_SRC/$bin"
    else
      echo -e "  ${YELLOW}•${NC} $BIN_SRC/$bin not built yet"
      missing=true
    fi
  done

  if [ "$missing" = true ]; then
    echo -e "  ${YELLOW}Run ./install.sh to build and install release binaries.${NC}"
  fi
}

build_binaries() {
  echo ""
  echo -e "${CYAN}Building CatCode binaries (release mode)...${NC}"
  echo -e "  This may take a few minutes the first time."
  echo ""

  cargo build --release --manifest-path "$CATCODE_DIR/Cargo.toml"

  echo ""
  echo -e "${GREEN}✓ Build complete!${NC}"
  for bin in "${BINS[@]}"; do
    if [ -x "$BIN_SRC/$bin" ]; then
      echo -e "  ${GREEN}✓${NC} $BIN_SRC/$bin"
    fi
  done
}

install_symlinks() {
  local install_dir="$1"
  echo ""
  echo -e "${CYAN}Installing symlinks to $install_dir...${NC}"

  mkdir -p "$install_dir"
  for bin in "${BINS[@]}"; do
    local link_path="$install_dir/$bin"
    if [ -L "$link_path" ] || [ -f "$link_path" ]; then
      rm -f "$link_path"
    fi
    ln -s "$BIN_SRC/$bin" "$link_path"
    echo -e "  ${GREEN}✓${NC} $link_path → $BIN_SRC/$bin"
  done
}

generate_config() {
  local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/catcode"
  local config_file="$config_dir/config.toml"

  if [ -f "$config_file" ]; then
    echo -e "  ${YELLOW}•${NC} Config already exists at $config_file (skipping)"
    return
  fi

  echo ""
  echo -e "${CYAN}Generating default config...${NC}"
  mkdir -p "$config_dir"

  cat > "$config_file" <<-EOF
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

  echo -e "  ${GREEN}✓${NC} Config written to $config_file"
}

print_next_steps() {
  local install_dir="$1"
  local in_path=false

  echo "$PATH" | tr ':' '\n' | grep -qx "$install_dir" && in_path=true

  echo ""
  echo -e "${GREEN}══════════════════════════════════════════${NC}"
  echo -e "${GREEN}  CatCode installed successfully!${NC}"
  echo -e "${GREEN}══════════════════════════════════════════${NC}"
  echo ""

  if [ "$in_path" = false ]; then
    echo -e "${YELLOW}⚠  $install_dir is not in your PATH.${NC}"
    echo -e "   Add it to your shell profile:"
    echo ""
    echo -e "   ${BOLD}echo 'export PATH=\"\$PATH:$install_dir\"' >> ~/.$(basename "$SHELL")rc${NC}"
    echo ""
  fi

  echo -e "${BOLD}Quick Start:${NC}"
  echo ""
  echo -e "  1. Set your API key (e.g. DeepSeek):"
  echo -e "     ${BOLD}export DEEPSEEK_API_KEY=\"sk-your-key-here\"${NC}"
  echo ""
  echo -e "  2. Run TUI (interactive):"
  echo -e "     ${BOLD}catcode-tui${NC}"
  echo ""
  echo -e "  3. Or use the unified launcher:"
  echo -e "     ${BOLD}$CATCODE_DIR/catcode.sh${NC}"
  echo ""
  echo -e "  4. Configure provider/model:"
  echo -e "     ${BOLD}catcode init${NC}"
  echo ""
  echo -e "  ${YELLOW}Need help?${NC} https://github.com/anomalyco/CatCode"
}

# ─── Main ────────────────────────────────────────────────────────────────────

print_banner

OS=$(detect_os)
ARCH=$(detect_arch)
echo -e "  Platform: ${BOLD}$OS ($ARCH)${NC}"
echo ""

if [ "$OS" = "unknown" ]; then
  echo -e "${YELLOW}Unrecognized OS. Proceeding anyway...${NC}"
fi

if [ "$OS" = "windows" ]; then
  echo -e "${YELLOW}Windows detected. WSL is recommended for the best experience.${NC}"
  echo ""
fi

while [ $# -gt 0 ]; do
  case "$1" in
    --check)
      CHECK_ONLY=true
      shift
      ;;
    --help|-h)
      echo "Usage: bash install.sh [--help] [--check]"
      echo ""
      echo "Options:"
      echo "  --check        Validate prerequisites without building or writing files"
      echo ""
      echo "Environment variables:"
      echo "  INSTALL_DIR    Target directory for symlinks (default: ~/.local/bin)"
      echo ""
      exit 0
      ;;
    *)
      echo -e "${RED}Unknown option: $1${NC}"
      echo "Usage: bash install.sh [--help] [--check]"
      exit 1
      ;;
  esac
done

# Step 1: Check for Rust toolchain
if ! check_cargo; then
  echo -e "${RED}✗ Rust toolchain not found${NC}"
  echo ""
  echo -e "  ${BOLD}To install Rust:${NC}"
  echo ""
  echo -e "    ${CYAN}curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh${NC}"
  echo ""
  echo -e "  Then restart your shell and re-run this installer."
  echo -e "  Or manually: ${CYAN}cargo install catcode${NC}"
  echo ""
  exit 1
fi

if [ "$CHECK_ONLY" = true ]; then
  echo ""
  echo -e "${CYAN}Checking source tree...${NC}"
  check_source_tree
  check_release_binaries
  echo ""
  echo -e "${GREEN}✓ Install preflight complete. No files were changed.${NC}"
  exit 0
fi

# Step 2: Build
build_binaries

# Step 3: Install symlinks
INSTALL_DIR="${INSTALL_DIR:-$(detect_install_dir)}"
install_symlinks "$INSTALL_DIR"

# Step 4: Generate default config
generate_config

# Step 5: Show next steps
print_next_steps "$INSTALL_DIR"
