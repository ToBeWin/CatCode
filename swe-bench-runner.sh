#!/usr/bin/env bash
# SWE-Bench evaluation runner for CatCode
#
# Usage:
#   ./swe-bench-runner.sh                          # sample instances (quick test)
#   ./swe-bench-runner.sh --dataset <path>          # real SWE-Bench dataset
#   ./swe-bench-runner.sh --help
#
# Examples:
#   # Quick test with mock provider (5 instances)
#   ./swe-bench-runner.sh
#
#   # Real run with DeepSeek on SWE-Bench Lite
#   curl -o swe-bench-lite.json https://huggingface.co/datasets/princeton-nlp/SWE-bench_Lite/resolve/main/data/train.json
#   DEEPSEEK_API_KEY="sk-xxx" ./swe-bench-runner.sh --dataset swe-bench-lite.json --provider deepseek --parallel 4
#
#   # Real run with Anthropic
#   ANTHROPIC_API_KEY="sk-ant-xxx" ./swe-bench-runner.sh --dataset swe-bench-lite.json --provider anthropic --parallel 2

set -euo pipefail

CATCODE_DIR="$(cd "$(dirname "$0")" && pwd)"

# Parse args
dataset=""
provider="mock"
model=""
parallel=2
output="$CATCODE_DIR/swe-bench-results"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dataset) dataset="$2"; shift 2 ;;
    --provider) provider="$2"; shift 2 ;;
    --model) model="$2"; shift 2 ;;
    --parallel) parallel="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --help) echo "Usage: $0 [--dataset path] [--provider mock|deepseek|anthropic] [--model id] [--parallel N] [--output dir]"; exit 0 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

# Check Rust toolchain
if ! command -v cargo &>/dev/null; then
  echo "Error: Rust toolchain not found. Install from https://rustup.rs"
  exit 1
fi

# Build if needed
if [ ! -f "$CATCODE_DIR/target/release/catcode-swe-bench" ]; then
  echo "Building SWE-Bench runner..."
  cargo build --release --bin catcode-swe-bench 2>&1
fi

# Build args
ARGS=()
if [ -n "$dataset" ]; then
  ARGS+=(--dataset "$dataset")
fi
if [ -n "$model" ]; then
  ARGS+=(--model "$model")
fi
ARGS+=(--provider "$provider")
ARGS+=(--parallel "$parallel")
ARGS+=(--output "$output")

echo "=== SWE-Bench Evaluation ==="
echo "Provider: $provider"
echo "Parallel: $parallel"
[ -n "$dataset" ] && echo "Dataset: $dataset"
[ -n "$model" ] && echo "Model: $model"
echo "Output: $output"
echo ""

# Run
"$CATCODE_DIR/target/release/catcode-swe-bench" "${ARGS[@]}"
