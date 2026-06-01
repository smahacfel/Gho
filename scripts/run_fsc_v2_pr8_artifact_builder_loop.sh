#!/usr/bin/env bash
set -u

ROOT="${ROOT:-/root/Gho}"
SCOPE="${1:-shadow-burnin-v3-fsc-capture-nln-r1}"
INTERVAL_SECONDS="${FSC_PR8_BUILDER_INTERVAL_SECONDS:-300}"
CANARY_MINUTES="${FSC_PR8_CANARY_MINUTES:-45}"
REQUIRE_EXTERNAL_AUDIT="${FSC_PR8_REQUIRE_EXTERNAL_AUDIT:-0}"
MIN_BENCHMARK_HOURS="${FSC_PR8_MIN_BENCHMARK_HOURS:-0}"
MIN_AUDIT_SLOTS="${FSC_PR8_MIN_AUDIT_SLOTS:-1000}"
MIN_AUDIT_TRANSFER_EVENTS="${FSC_PR8_MIN_AUDIT_TRANSFER_EVENTS:-10000}"
BUILDER_MODE="${FSC_PR8_BUILDER_MODE:-bounded-tail}"
MAX_TRANSFER_ROWS="${FSC_PR8_MAX_TRANSFER_ROWS:-200000}"

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
    --min-audit-slots "${MIN_AUDIT_SLOTS}"
    --min-audit-transfer-events "${MIN_AUDIT_TRANSFER_EVENTS}"
    --canary-minutes "${CANARY_MINUTES}"
    --mode "${BUILDER_MODE}"
    --max-transfer-rows "${MAX_TRANSFER_ROWS}"
    --nln-create "${CAPTURE_DIR}/pumpfun_create_raw_v1.jsonl"
    --nln-trade "${CAPTURE_DIR}/pumpfun_trade_raw_v1.jsonl"
    --nln-transfer "${CAPTURE_DIR}/system_transfers_raw_v1.jsonl"
    --nln-normalization-error "${CAPTURE_DIR}/nln_normalization_errors_v1.jsonl"
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
  elif [[ "${REQUIRE_EXTERNAL_AUDIT}" == "1" ]]; then
    printf '%s status=2 scope=%s interval_seconds=%s canary_minutes=%s reason=missing_required_external_audit\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      "${SCOPE}" \
      "${INTERVAL_SECONDS}" \
      "${CANARY_MINUTES}" >"${REPORT_DIR}/fsc_provider_qualification_builder_status.txt"
    sleep "${INTERVAL_SECONDS}"
    continue
  fi

  if [[ -n "${FSC_PR8_EVENTUAL_FSC_SNAPSHOT_PATH:-}" ]]; then
    IFS=':' read -r -a EVENTUAL_PATHS <<< "${FSC_PR8_EVENTUAL_FSC_SNAPSHOT_PATH}"
    for eventual_path in "${EVENTUAL_PATHS[@]}"; do
      if [[ -n "${eventual_path}" ]]; then
        CMD+=(--eventual-fsc-snapshot "${eventual_path}")
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
  printf '%s status=%s scope=%s interval_seconds=%s canary_minutes=%s require_external_audit=%s min_benchmark_hours=%s min_audit_slots=%s min_audit_transfer_events=%s builder_mode=%s max_transfer_rows=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "${STATUS}" \
    "${SCOPE}" \
    "${INTERVAL_SECONDS}" \
    "${CANARY_MINUTES}" \
    "${REQUIRE_EXTERNAL_AUDIT}" \
    "${MIN_BENCHMARK_HOURS}" \
    "${MIN_AUDIT_SLOTS}" \
    "${MIN_AUDIT_TRANSFER_EVENTS}" \
    "${BUILDER_MODE}" \
    "${MAX_TRANSFER_ROWS}" >"${LAST_STATUS}"

  sleep "${INTERVAL_SECONDS}"
done
