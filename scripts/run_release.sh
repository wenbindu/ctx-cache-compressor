#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
TARGET_TRIPLE="${TARGET:-${HOST_TARGET}}"
BINARY="${BINARY:-}"
SERVICE="${SERVICE:-api}"
CONFIG_FILE="${CONFIG_FILE:-}"

if [[ "${TARGET_TRIPLE}" == *"windows"* ]]; then
  EXE_SUFFIX=".exe"
else
  EXE_SUFFIX=""
fi

case "${SERVICE}" in
  api)
    BINARY_NAME="ctx-cache-compressor-api"
    ;;
  demo)
    BINARY_NAME="ctx-cache-compressor-demo"
    ;;
  compat|combined)
    BINARY_NAME="ctx-cache-compressor"
    ;;
  *)
    BINARY_NAME="${SERVICE}"
    ;;
esac

if [[ -z "${BINARY}" ]]; then
  candidates=(
    "${ROOT_DIR}/bin/${BINARY_NAME}${EXE_SUFFIX}"
    "${ROOT_DIR}/target/${TARGET_TRIPLE}/release/${BINARY_NAME}${EXE_SUFFIX}"
    "${ROOT_DIR}/target/release/${BINARY_NAME}${EXE_SUFFIX}"
    "${ROOT_DIR}/bin/ctx-cache-compressor-api${EXE_SUFFIX}"
    "${ROOT_DIR}/target/${TARGET_TRIPLE}/release/ctx-cache-compressor-api${EXE_SUFFIX}"
    "${ROOT_DIR}/target/release/ctx-cache-compressor-api${EXE_SUFFIX}"
  )
  for candidate in "${candidates[@]}"; do
    if [[ -x "${candidate}" ]]; then
      BINARY="${candidate}"
      break
    fi
  done
fi

if [[ ! -x "${BINARY}" ]]; then
  echo "release binary not found: ${BINARY}" >&2
  echo "build it first with: cargo build --release --target ${TARGET_TRIPLE} --bin ${BINARY_NAME}" >&2
  exit 1
fi

if [[ -n "${CONFIG_FILE}" ]]; then
  export CTX_CACHE_COMPRESSOR_CONFIG_FILE="${CONFIG_FILE}"
  export CTX_COMPRESSOR_CONFIG_FILE="${CONFIG_FILE}"
elif [[ -z "${CTX_CACHE_COMPRESSOR_CONFIG_FILE:-}" && -z "${CTX_COMPRESSOR_CONFIG_FILE:-}" ]] && [[ -f "${ROOT_DIR}/config/prod.toml" ]]; then
  export CTX_CACHE_COMPRESSOR_CONFIG_FILE="${ROOT_DIR}/config/prod.toml"
  export CTX_COMPRESSOR_CONFIG_FILE="${ROOT_DIR}/config/prod.toml"
elif [[ -z "${CTX_CACHE_COMPRESSOR_CONFIG_FILE:-}" && -z "${CTX_COMPRESSOR_CONFIG_FILE:-}" ]] && [[ -f "${ROOT_DIR}/config.toml" ]]; then
  export CTX_CACHE_COMPRESSOR_CONFIG_FILE="${ROOT_DIR}/config.toml"
  export CTX_COMPRESSOR_CONFIG_FILE="${ROOT_DIR}/config.toml"
fi

echo "service=${SERVICE}"
echo "binary=${BINARY}"
echo "config=${CTX_CACHE_COMPRESSOR_CONFIG_FILE:-${CTX_COMPRESSOR_CONFIG_FILE:-<defaults>}}"

exec "${BINARY}"
