#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   source scripts/source_env.sh [env-file]
#
# Best used from the repository root. If no file is passed, the script looks for:
#   1. .env.local
#   2. .env

ROOT_DIR="$(pwd)"

resolve_env_file() {
  local requested="${1:-}"

  if [[ -n "${requested}" ]]; then
    if [[ -f "${requested}" ]]; then
      printf '%s\n' "${requested}"
      return 0
    fi

    if [[ -f "${ROOT_DIR}/${requested}" ]]; then
      printf '%s\n' "${ROOT_DIR}/${requested}"
      return 0
    fi

    echo "env file not found: ${requested}" >&2
    return 1
  fi

  local candidate
  for candidate in "${ROOT_DIR}/.env.local" "${ROOT_DIR}/.env"; do
    if [[ -f "${candidate}" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done

  echo "no env file found. create one first:" >&2
  echo "  cp .env.example .env.local" >&2
  return 1
}

ENV_FILE="$(resolve_env_file "${1:-}")"

set -a
# shellcheck disable=SC1090
source "${ENV_FILE}"
set +a

CONFIG_FILE_VALUE="${CTX_CACHE_COMPRESSOR_CONFIG_FILE:-${CTX_COMPRESSOR_CONFIG_FILE:-}}"
if [[ -n "${CONFIG_FILE_VALUE}" && "${CONFIG_FILE_VALUE}" != /* ]]; then
  CONFIG_FILE_VALUE="${ROOT_DIR}/${CONFIG_FILE_VALUE}"
fi

if [[ -n "${CONFIG_FILE_VALUE}" ]]; then
  export CTX_CACHE_COMPRESSOR_CONFIG_FILE="${CONFIG_FILE_VALUE}"
  export CTX_COMPRESSOR_CONFIG_FILE="${CONFIG_FILE_VALUE}"
fi

echo "loaded env from ${ENV_FILE}"
