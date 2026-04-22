#!/usr/bin/env bash
set -euo pipefail

PID_FILE="${PID_FILE:-/tmp/ctx-cache-compressor.pid}"

stopped=0

if [[ -f "${PID_FILE}" ]]; then
  pid="$(cat "${PID_FILE}" || true)"
  if [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null; then
    kill "${pid}" || true
    stopped=1
    echo "stopped ctx-cache-compressor (pid=${pid})"
  fi
  rm -f "${PID_FILE}"
fi

# Fallback in case pid file is stale or missing.
pkill -f "target/debug/ctx-cache-compressor" >/dev/null 2>&1 || true

if [[ "${stopped}" -eq 0 ]]; then
  echo "no running ctx-cache-compressor found"
fi
