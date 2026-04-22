#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-quick}"
REPORT_DIR="${REPORT_DIR:-${ROOT_DIR}/data/reports}"
REPORT_FILE="${REPORT_FILE:-${REPORT_DIR}/test_summary_${MODE}.md}"

mkdir -p "${REPORT_DIR}"

declare -a STEP_ROWS=()
overall_status=0

usage() {
  cat <<'EOF'
usage: scripts/test_suite.sh [quick|load-1000|all]

quick      Run the default cargo test suite with concise output.
load-1000  Run the ignored 1000-session concurrency test only.
all        Run quick + load-1000.
EOF
}

timestamp_utc() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

run_step() {
  local label="$1"
  shift

  local log_file
  local start_ts
  local end_ts
  local duration
  local status
  local summary

  log_file="$(mktemp)"
  start_ts="$(date +%s)"

  if "$@" >"${log_file}" 2>&1; then
    status="passed"
  else
    status="failed"
    overall_status=1
  fi

  end_ts="$(date +%s)"
  duration="$((end_ts - start_ts))s"
  summary="$(grep -E 'running [0-9]+ tests|test result:|Finished `' "${log_file}" | tail -n 6 | tr '\n' '; ' | sed 's/; $//')"

  if [[ -z "${summary}" ]]; then
    summary="$(tail -n 3 "${log_file}" | tr '\n' '; ' | sed 's/; $//')"
  fi

  STEP_ROWS+=("| ${label} | ${status} | ${duration} | ${summary} |")

  echo "${label}: ${status} (${duration})"
  echo "  ${summary}"

  if [[ "${status}" != "passed" ]]; then
    echo "---- ${label} failure log ----" >&2
    cat "${log_file}" >&2
  fi

  rm -f "${log_file}"
}

case "${MODE}" in
  quick)
    run_step "cargo test --quiet" cargo test --quiet
    ;;
  load-1000)
    run_step \
      "ignored 1000-session concurrency test" \
      cargo test --test integration scenario_6_concurrent_1000_sessions_append_without_deadlock -- --ignored --exact
    ;;
  all)
    run_step "cargo test --quiet" cargo test --quiet
    run_step \
      "ignored 1000-session concurrency test" \
      cargo test --test integration scenario_6_concurrent_1000_sessions_append_without_deadlock -- --ignored --exact
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac

{
  echo "# Test Summary"
  echo
  echo "- Generated at: $(timestamp_utc)"
  echo "- Mode: ${MODE}"
  echo "- Repo: ${ROOT_DIR}"
  echo
  echo "| Step | Status | Duration | Summary |"
  echo "|---|---|---:|---|"
  printf '%s\n' "${STEP_ROWS[@]}"
} > "${REPORT_FILE}"

echo "report=${REPORT_FILE}"
exit "${overall_status}"
