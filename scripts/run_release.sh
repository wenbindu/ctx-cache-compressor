#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${BINARY:-${ROOT_DIR}/target/release/ctx-cache-compressor}"
CONFIG_FILE="${CONFIG_FILE:-}"

if [[ ! -x "${BINARY}" ]]; then
  echo "release binary not found: ${BINARY}" >&2
  echo "build it first with: cargo build --release" >&2
  exit 1
fi

if [[ -n "${CONFIG_FILE}" ]]; then
  export CTX_CACHE_COMPRESSOR_CONFIG_FILE="${CONFIG_FILE}"
  export CTX_COMPRESSOR_CONFIG_FILE="${CONFIG_FILE}"
elif [[ -z "${CTX_CACHE_COMPRESSOR_CONFIG_FILE:-}" && -z "${CTX_COMPRESSOR_CONFIG_FILE:-}" ]] && [[ -f "${ROOT_DIR}/config.toml" ]]; then
  export CTX_CACHE_COMPRESSOR_CONFIG_FILE="${ROOT_DIR}/config.toml"
  export CTX_COMPRESSOR_CONFIG_FILE="${ROOT_DIR}/config.toml"
fi

exec "${BINARY}"
