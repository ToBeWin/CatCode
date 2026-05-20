#!/usr/bin/env bash
# catcode-agent.sh — cc-connect adapter for CatCode
#
# Bridges cc-connect to CatCode's daemon API.
# cc-connect calls this script for each user message.
#
# Usage:
#   catcode-agent.sh <message>
#
# Environment:
#   CATCODE_API_URL    (default: http://127.0.0.1:7070)
#   CATCODE_PROJECT_DIR (default: current directory)
#   CATCODE_PROVIDER_ID (default: deepseek)
#   CATCODE_MODEL_ID    (default: deepseek-chat)
#
# Requires: curl, jq (optional, for pretty output)

set -euo pipefail

API_URL="${CATCODE_API_URL:-http://127.0.0.1:7070}"
PROJECT_DIR="${CATCODE_PROJECT_DIR:-$(pwd)}"
PROVIDER_ID="${CATCODE_PROVIDER_ID:-deepseek}"
MODEL_ID="${CATCODE_MODEL_ID:-deepseek-chat}"
SESSION_ID=""

# Create a session if none exists
create_session() {
  local name
  name="cc-connect-$(date +%s)"
  SESSION_ID=$(curl -s -X POST "$API_URL/api/v1/sessions" \
    -H "Content-Type: application/json" \
    -d "$(printf '{"name":"%s","project_dir":"%s","provider_id":"%s","model_id":"%s"}' \
      "$name" \
      "$(printf '%s' "$PROJECT_DIR" | sed 's/\\/\\\\/g; s/"/\\"/g')" \
      "$PROVIDER_ID" \
      "$MODEL_ID")" \
    | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

  if [ -z "$SESSION_ID" ]; then
    echo "ERROR: Failed to create session" >&2
    exit 1
  fi
  echo "$SESSION_ID"
}

# Send message and get response
send_message() {
  local msg="$1"

  if [ -z "$SESSION_ID" ]; then
    SESSION_ID=$(create_session)
  fi

  local resp
  resp=$(curl -s -X POST "$API_URL/api/v1/sessions/$SESSION_ID/message" \
    -H "Content-Type: application/json" \
    -d "$(printf '{"content":"%s"}' "$(echo "$msg" | sed 's/"/\\"/g')")")

  local text
  text=$(echo "$resp" | grep -o '"response":"[^"]*"' | head -1 | cut -d'"' -f4)

  if [ -z "$text" ]; then
    text=$(echo "$resp" | grep -o '"error":"[^"]*"' | head -1 | cut -d'"' -f4)
    if [ -n "$text" ]; then
      echo "ERROR: $text"
      return 1
    fi
    echo "$resp"
    return 0
  fi

  echo "$text"
}

# Main
if [ $# -eq 0 ]; then
  echo "Usage: $0 <message>"
  exit 1
fi

send_message "$*"
