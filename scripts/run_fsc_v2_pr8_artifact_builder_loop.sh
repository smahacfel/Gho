#!/usr/bin/env bash
set -u

ROOT="${ROOT:-/root/Gho}"
SCOPE="${1:-shadow-burnin-v3-fsc-capture-nln-r1}"
INTERVAL_SECONDS="${FSC_PR8_BUILDER_INTERVAL_SECONDS:-300}"
MIN_BENCHMARK_HOURS="${FSC_PR8_MIN_BENCHMARK_HOURS:-24}"

CAPTURE_DIR="${ROOT}/logs/nln_capture/${SCOPE}"
DECISION_ROOT="${ROOT}/logs/rollout/${SCOPE}/decisions/${SCOPE}"
REPORT_DIR="${ROOT}/reports/selector/${SCOPE}"

mkdir -p "${REPORT_DIR}"

while true; do
  mapfile -t DECISION_LOGS < <(
    find "${DECISION_ROOT}" -type f -name gatekeeper_v2_decisions.jsonl 2>/dev/null | sort
  )

  CMD=(
    python3 "${ROOT}/scripts/build_fsc_v2_provider_qualification.py"
    --scope "${SCOPE}"
    --root "${ROOT}"
    --min-benchmark-hours "${MIN_BENCHMARK_HOURS}"
    --nln-create "${CAPTURE_DIR}/pumpfun_create_raw_v1.jsonl"
    --nln-trade "${CAPTURE_DIR}/pumpfun_trade_raw_v1.jsonl"
    --nln-transfer "${CAPTURE_DIR}/system_transfers_raw_v1.jsonl"
  )

  for decision_log in "${DECISION_LOGS[@]}"; do
    CMD+=(--decision-log "${decision_log}")
  done

  if [[ -n "${FSC_PR8_AUDIT_EVENT_PATH:-}" ]]; then
    IFS=':' read -r -a AUDIT_PATHS <<< "${FSC_PR8_AUDIT_EVENT_PATH}"
    for audit_path in "${AUDIT_PATHS[@]}"; do
      if [[ -n "${audit_path}" ]]; then
        CMD+=(--audit-event "${audit_path}")
      fi
    done
  fi

  TMP_JSON="${REPORT_DIR}/fsc_provider_qualification_manifest_last.json.tmp"
  LAST_JSON="${REPORT_DIR}/fsc_provider_qualification_manifest_last.json"
  LAST_STDERR="${REPORT_DIR}/fsc_provider_qualification_manifest_last.stderr"
  LAST_STATUS="${REPORT_DIR}/fsc_provider_qualification_builder_status.txt"

  "${CMD[@]}" --json >"${TMP_JSON}" 2>"${LAST_STDERR}"
  STATUS=$?
  mv "${TMP_JSON}" "${LAST_JSON}"
  printf '%s status=%s scope=%s interval_seconds=%s min_benchmark_hours=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "${STATUS}" \
    "${SCOPE}" \
    "${INTERVAL_SECONDS}" \
    "${MIN_BENCHMARK_HOURS}" >"${LAST_STATUS}"

  sleep "${INTERVAL_SECONDS}"
done
