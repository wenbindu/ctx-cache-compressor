#!/usr/bin/env bash
set -euo pipefail

LOG_FILE="${LOG_FILE:-/tmp/ctx-cache-compressor.log}"
MODE="${1:-tail}"

if [[ ! -f "${LOG_FILE}" ]]; then
  echo "log file not found: ${LOG_FILE}" >&2
  exit 1
fi

case "${MODE}" in
  tail)
    tail -f "${LOG_FILE}"
    ;;
  key)
    # key events without ripgrep dependency
    tail -f "${LOG_FILE}" | grep --line-buffered -E "started|compression|WARN|ERROR|failed|succeeded"
    ;;
  once)
    tail -n 80 "${LOG_FILE}"
    ;;
  *)
    echo "usage: $0 [tail|key|once]" >&2
    exit 1
    ;;
esac
