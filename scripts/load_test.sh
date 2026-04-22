#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
THREADS="${THREADS:-4}"
CONNECTIONS="${CONNECTIONS:-1000}"
DURATION="${DURATION:-30s}"
SESSION_COUNT="${SESSION_COUNT:-1000}"
REPORT_DIR="${REPORT_DIR:-${ROOT_DIR}/data/reports}"
REPORT_FILE="${REPORT_FILE:-${REPORT_DIR}/load_test_summary.md}"

mkdir -p "${REPORT_DIR}"

if ! command -v wrk >/dev/null 2>&1; then
  echo "missing required command: wrk" >&2
  echo "install wrk, then re-run scripts/load_test.sh" >&2
  exit 1
fi

tmp_output="$(mktemp)"
trap 'rm -f "${tmp_output}"' EXIT

SESSION_COUNT="${SESSION_COUNT}" \
  wrk -t"${THREADS}" -c"${CONNECTIONS}" -d"${DURATION}" \
  -s "${ROOT_DIR}/scripts/append_load.lua" \
  "${BASE_URL}" | tee "${tmp_output}"

summary_lines="$(grep -E 'Latency|Req/Sec|Requests/sec|Transfer/sec|Socket errors' "${tmp_output}" || true)"

{
  echo "# Load Test Summary"
  echo
  echo "- Generated at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  echo "- Base URL: ${BASE_URL}"
  echo "- Threads: ${THREADS}"
  echo "- Connections: ${CONNECTIONS}"
  echo "- Duration: ${DURATION}"
  echo "- Session count: ${SESSION_COUNT}"
  echo
  echo "## Key Metrics"
  echo
  if [[ -n "${summary_lines}" ]]; then
    while IFS= read -r line; do
      echo "- ${line}"
    done <<< "${summary_lines}"
  else
    echo "- No key metrics extracted. Check raw output below."
  fi
  echo
  echo "## Raw Output"
  echo
  echo '```text'
  cat "${tmp_output}"
  echo '```'
} > "${REPORT_FILE}"

echo "report=${REPORT_FILE}"

