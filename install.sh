#!/usr/bin/env bash
set -euo pipefail

CATCODE_DIR="$(cd "$(dirname "$0")" && pwd)"
BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}╔══════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║       CatCode Installer ${NC}v$(grep '^version' "$CATCODE_DIR/Cargo.toml" 2>/dev/null | head -1 | sed 's/.*= "//;s/"//' || echo "?")${CYAN}         ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════════╝${NC}"
echo ""

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

OS=$(detect_os)
ARCH=$(detect_arch)
echo -e "  Detected: ${BOLD}${OS}/${ARCH}${NC}"
echo ""

install_via_cargo() {
  echo -e "${GREEN}Installing via Cargo...${NC}"
  cargo install --path "$CATCODE_DIR" --bins
  echo ""
  echo -e "${GREEN}✓ CatCode installed successfully!${NC}"
  echo -e "  Run ${BOLD}catcode --help${NC} to get started."
  echo -e "  Run ${BOLD}catcode init${NC} to configure your provider."
  echo -e "  Run ${BOLD}catcode-tui${NC} to launch the terminal UI."
}

install_via_cargo_local() {
  echo -e "${YELLOW}Building from source...${NC}"
  cargo build --release --manifest-path "$CATCODE_DIR/Cargo.toml"
  echo ""
  echo -e "${GREEN}✓ Build complete!${NC}"
  echo -e "  Binaries:"
  echo -e "    ${CATCODE_DIR}/target/release/catcode"
  echo -e "    ${CATCODE_DIR}/target/release/catcode-tui"
  echo -e "    ${CATCODE_DIR}/target/release/catcode-daemon"
  echo ""
  echo -e "  Add to PATH:"
  echo -e "    export PATH=\"\$PATH:${CATCODE_DIR}/target/release\""
}

if command -v cargo &>/dev/null; then
  echo -e "  ${GREEN}✓${NC} Rust toolchain found"
  echo ""
  echo -e "  ${BOLD}Install options:${NC}"
  echo -e "    ${GREEN}1${NC}) cargo install (recommended — adds to PATH)"
  echo -e "    ${GREEN}2${NC}) cargo build (local build only)"
  echo -e "    ${GREEN}3${NC}) Show manual build instructions"
  echo ""
  echo -ne "  Choose [${BOLD}1${NC}]: "
  read -r choice < /dev/tty || true
  echo ""

  case "$choice" in
    2) install_via_cargo_local ;;
    3)
      echo "  Manual build:"
      echo "    cd $CATCODE_DIR"
      echo "    cargo build --release"
      echo "    ./target/release/catcode --help"
      ;;
    *) install_via_cargo ;;
  esac
else
  echo -e "${YELLOW}⚠  Rust toolchain not found${NC}"
  echo ""
  echo -e "  ${BOLD}Option A: Install Rust first${NC}"
  echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  echo "    Then re-run this installer."
  echo ""
  echo -e "  ${BOLD}Option B: Install via package manager${NC}"
  echo "    brew install catcode          # macOS (if available)"
  echo "    cargo install catcode          # crates.io"
  echo ""
  echo -e "  ${BOLD}Option C: Download pre-built binary${NC}"
  echo "    Visit: https://github.com/anomalyco/CatCode/releases"
  echo ""
  exit 1
fi
