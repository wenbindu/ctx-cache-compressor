#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
TARGET_TRIPLE="${TARGET:-${HOST_TARGET}}"
DEFAULT_BINARY="${ROOT_DIR}/target/${TARGET_TRIPLE}/release/ctx-cache-compressor"
LEGACY_BINARY="${ROOT_DIR}/target/release/ctx-cache-compressor"
BINARY="${BINARY:-}"
CONFIG_FILE="${CONFIG_FILE:-}"

if [[ -z "${BINARY}" ]]; then
  if [[ -x "${DEFAULT_BINARY}" ]]; then
    BINARY="${DEFAULT_BINARY}"
  else
    BINARY="${LEGACY_BINARY}"
  fi
fi

if [[ ! -x "${BINARY}" ]]; then
  echo "release binary not found: ${BINARY}" >&2
  echo "build it first with: cargo build --release --target ${TARGET_TRIPLE}" >&2
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
