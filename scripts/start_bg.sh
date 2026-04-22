#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_FILE="${LOG_FILE:-/tmp/ctx-cache-compressor.log}"
PID_FILE="${PID_FILE:-/tmp/ctx-cache-compressor.pid}"
ENV_FILE="${ENV_FILE:-}"

#export CARGO_HOME="${CARGO_HOME:-/tmp/.cargo}"
#export RUSTUP_HOME="${RUSTUP_HOME:-/tmp/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"

if [[ ! -f "${CARGO_HOME}/env" ]]; then
  echo "missing Rust env file: ${CARGO_HOME}/env" >&2
  echo "install toolchain first, then re-run." >&2
  exit 1
fi

# shellcheck disable=SC1090
source "${CARGO_HOME}/env"

if [[ -f "${PID_FILE}" ]]; then
  old_pid="$(cat "${PID_FILE}" || true)"
  if [[ -n "${old_pid}" ]] && kill -0 "${old_pid}" 2>/dev/null; then
    echo "ctx-cache-compressor already running (pid=${old_pid})"
    exit 0
  fi
fi

if [[ -z "${ENV_FILE}" ]]; then
  if [[ -f "${ROOT_DIR}/.env.local" ]]; then
    ENV_FILE="${ROOT_DIR}/.env.local"
  elif [[ -f "${ROOT_DIR}/.env" ]]; then
    ENV_FILE="${ROOT_DIR}/.env"
  fi
fi

if [[ -n "${ENV_FILE}" ]]; then
  nohup bash -lc "cd '${ROOT_DIR}' && source scripts/source_env.sh '${ENV_FILE}' && cargo +${RUSTUP_TOOLCHAIN} run" >"${LOG_FILE}" 2>&1 &
else
  nohup bash -lc "cd '${ROOT_DIR}' && cargo +${RUSTUP_TOOLCHAIN} run" >"${LOG_FILE}" 2>&1 &
fi
pid="$!"
echo "${pid}" >"${PID_FILE}"

for _ in $(seq 1 20); do
  if curl -fsS "http://127.0.0.1:8080/health" >/dev/null 2>&1; then
    echo "ctx-cache-compressor started (pid=${pid})"
    echo "log file: ${LOG_FILE}"
    exit 0
  fi
  sleep 0.5
done

echo "ctx-cache-compressor did not become healthy in time. recent log:" >&2
tail -n 40 "${LOG_FILE}" >&2 || true
exit 1
