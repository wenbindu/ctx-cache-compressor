#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"

require_bin() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

require_bin curl
require_bin jq

echo "== health =="
curl -sS "$BASE_URL/health" | jq .

echo "== create session =="
SESSION_ID=$(curl -sS -X POST "$BASE_URL/sessions" \
  -H 'content-type: application/json' \
  -d '{"system_prompt":"你是一个编程助手"}' | jq -r '.session_id')

echo "session_id=$SESSION_ID"

echo "== append basic turn =="
curl -sS -X POST "$BASE_URL/sessions/$SESSION_ID/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"user","content":"帮我写一个快速排序"}' | jq .

curl -sS -X POST "$BASE_URL/sessions/$SESSION_ID/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"assistant","content":"好的，下面是 Rust 版本"}' | jq .

echo "== fetch context =="
curl -sS "$BASE_URL/sessions/$SESSION_ID/context" | jq '{turn_count,is_compressing,compressed_turns,token_estimate,message_count:(.messages|length)}'

echo "== tool-call chain =="
TOOL_SESSION=$(curl -sS -X POST "$BASE_URL/sessions" -H 'content-type: application/json' -d '{}' | jq -r '.session_id')
echo "tool_session=$TOOL_SESSION"

curl -sS -X POST "$BASE_URL/sessions/$TOOL_SESSION/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"user","content":"查一下 rust axum"}' | jq '{turn_count,compression_triggered}'

curl -sS -X POST "$BASE_URL/sessions/$TOOL_SESSION/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"assistant","content":null,"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"search","arguments":"{\"query\":\"rust axum\"}"}}]}' | jq '{turn_count,compression_triggered}'

curl -sS -X POST "$BASE_URL/sessions/$TOOL_SESSION/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"tool","content":"搜索结果：...","tool_call_id":"call_abc","name":"search"}' | jq '{turn_count,compression_triggered}'

curl -sS -X POST "$BASE_URL/sessions/$TOOL_SESSION/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"assistant","content":"结论：建议用 axum 0.8"}' | jq '{turn_count,compression_triggered,message_count}'

echo "== invalid sequence should return 400 =="
HTTP_CODE=$(curl -sS -o /tmp/ctx_invalid_seq.json -w '%{http_code}' -X POST "$BASE_URL/sessions/$TOOL_SESSION/messages" \
  -H 'content-type: application/json' \
  -d '{"role":"assistant","content":"不合法"}')

echo "status=$HTTP_CODE"
cat /tmp/ctx_invalid_seq.json | jq .

echo "== done =="
