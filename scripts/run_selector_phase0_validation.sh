#!/usr/bin/env bash
set -u

ROOT="/root/Gho"
SCOPE=""
EVENTS=()
DECISIONS=()
CONFIGS=()
LIFECYCLE_REPORT=""
PRICE_PATHS=""
PNL_TARGET_NET_PCT="40"
TARGET_NET_PCT="40"
STOP_NET_PCT="40"
HORIZON_MS="60000"
REPLAY_ARTIFACT_VERSION=""
ALLOW_DEGRADED_EVENTS=0
ALLOW_DECISION_UNIVERSE=0
ALLOW_INCOMPLETE_UNIVERSE=0
INCLUDE_DECISION_CONTEXT_FOR_FEATURES=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      ROOT="$2"
      shift 2
      ;;
    --scope)
      SCOPE="$2"
      shift 2
      ;;
    --events)
      EVENTS+=("$2")
      shift 2
      ;;
    --decisions)
      DECISIONS+=("$2")
      shift 2
      ;;
    --config-snapshot)
      CONFIGS+=("$2")
      shift 2
      ;;
    --lifecycle-report)
      LIFECYCLE_REPORT="$2"
      shift 2
      ;;
    --price-paths)
      PRICE_PATHS="$2"
      shift 2
      ;;
    --pnl-target-net-pct)
      PNL_TARGET_NET_PCT="$2"
      shift 2
      ;;
    --target-net-pct)
      TARGET_NET_PCT="$2"
      shift 2
      ;;
    --stop-net-pct)
      STOP_NET_PCT="$2"
      shift 2
      ;;
    --horizon-ms)
      HORIZON_MS="$2"
      shift 2
      ;;
    --replay-artifact-version)
      REPLAY_ARTIFACT_VERSION="$2"
      shift 2
      ;;
    --allow-degraded-events)
      ALLOW_DEGRADED_EVENTS=1
      shift
      ;;
    --allow-decision-universe)
      ALLOW_DECISION_UNIVERSE=1
      shift
      ;;
    --allow-incomplete-universe)
      ALLOW_INCOMPLETE_UNIVERSE=1
      shift
      ;;
    --include-decision-context-for-features)
      INCLUDE_DECISION_CONTEXT_FOR_FEATURES=1
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

if [[ -z "$SCOPE" ]]; then
  echo "--scope is required" >&2
  exit 64
fi
if [[ -z "$LIFECYCLE_REPORT" ]]; then
  echo "--lifecycle-report is required" >&2
  exit 64
fi

cd "$ROOT" || exit 66

REPORT_DIR="$ROOT/reports/selector/$SCOPE"
DATASET_DIR="$ROOT/datasets/selector/$SCOPE"
mkdir -p "$REPORT_DIR" "$DATASET_DIR"
LOG="$REPORT_DIR/phase0_validation_run.log"
exec > >(tee -a "$LOG") 2>&1

echo "selector_phase0_validation_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "scope=$SCOPE"
echo "root=$ROOT"
echo "dataset_dir=$DATASET_DIR"
echo "report_dir=$REPORT_DIR"
echo "events=${EVENTS[*]}"
echo "decisions=${DECISIONS[*]}"
echo "lifecycle_report=$LIFECYCLE_REPORT"
echo "price_paths=${PRICE_PATHS:-none}"
echo "configs=${CONFIGS[*]}"

echo "phase=preflight_py_compile"
python3 -m py_compile \
  scripts/selector_pipeline_common.py \
  scripts/build_selector_candidate_universe.py \
  scripts/build_selector_accepted_lifecycle.py \
  scripts/build_selector_feature_snapshots.py \
  scripts/build_selector_training_view.py \
  scripts/compare_selector_gatekeepers.py \
  scripts/train_selector_baseline.py \
  scripts/build_selector_dataset.py \
  scripts/test_selector_pipeline.py \
  scripts/shadow_onchain_lifecycle_report.py
PYCOMPILE_STATUS=$?
echo "py_compile_status=$PYCOMPILE_STATUS"

echo "phase=selector_unit_tests"
python3 -m unittest scripts/test_selector_pipeline.py -v
SELECTOR_TEST_STATUS=$?
echo "selector_unit_tests_status=$SELECTOR_TEST_STATUS"

echo "phase=lifecycle_contract_tests"
python3 -m unittest scripts/test_shadow_onchain_lifecycle_report_contract.py -v
LIFECYCLE_TEST_STATUS=$?
echo "lifecycle_contract_tests_status=$LIFECYCLE_TEST_STATUS"

BUILD_ARGS=(
  scripts/build_selector_dataset.py
  --scope "$SCOPE"
  --root "$ROOT"
  --lifecycle-report "$LIFECYCLE_REPORT"
  --pnl-target-net-pct "$PNL_TARGET_NET_PCT"
  --target-net-pct "$TARGET_NET_PCT"
  --stop-net-pct "$STOP_NET_PCT"
  --horizon-ms "$HORIZON_MS"
  --json
)

for path in "${EVENTS[@]}"; do
  BUILD_ARGS+=(--events "$path")
done
for path in "${DECISIONS[@]}"; do
  BUILD_ARGS+=(--decisions "$path")
done
for path in "${CONFIGS[@]}"; do
  BUILD_ARGS+=(--config-snapshot "$path")
done
if [[ -n "$PRICE_PATHS" ]]; then
  BUILD_ARGS+=(--price-paths "$PRICE_PATHS")
fi
if [[ -n "$REPLAY_ARTIFACT_VERSION" ]]; then
  BUILD_ARGS+=(--replay-artifact-version "$REPLAY_ARTIFACT_VERSION")
fi
if [[ "$ALLOW_DEGRADED_EVENTS" -eq 1 ]]; then
  BUILD_ARGS+=(--allow-degraded-events)
fi
if [[ "$ALLOW_DECISION_UNIVERSE" -eq 1 ]]; then
  BUILD_ARGS+=(--allow-decision-universe)
fi
if [[ "$ALLOW_INCOMPLETE_UNIVERSE" -eq 1 ]]; then
  BUILD_ARGS+=(--allow-incomplete-universe)
fi
if [[ "$INCLUDE_DECISION_CONTEXT_FOR_FEATURES" -eq 1 ]]; then
  BUILD_ARGS+=(--include-decision-context-for-features)
fi

echo "phase=build_selector_dataset"
python3 "${BUILD_ARGS[@]}"
DATASET_BUILD_STATUS=$?
echo "dataset_build_status=$DATASET_BUILD_STATUS"

echo "phase=manifest_summary"
python3 - "$REPORT_DIR/dataset_manifest_v1.json" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
if not path.exists():
    print(f"manifest_missing={path}")
    raise SystemExit(1)
manifest = json.loads(path.read_text(encoding="utf-8"))
print(f"manifest_status={manifest.get('status')}")
print(f"manifest_fail_reasons={manifest.get('fail_reasons')}")
for name, report in sorted((manifest.get("stage_reports") or {}).items()):
    print(f"stage_status[{name}]={report.get('status')}")
    reasons = report.get("fail_reasons")
    if reasons:
        print(f"stage_fail_reasons[{name}]={reasons}")
PY
SUMMARY_STATUS=$?
echo "manifest_summary_status=$SUMMARY_STATUS"

echo "selector_phase0_validation_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "preflight_statuses py_compile=$PYCOMPILE_STATUS selector_tests=$SELECTOR_TEST_STATUS lifecycle_tests=$LIFECYCLE_TEST_STATUS dataset_build=$DATASET_BUILD_STATUS summary=$SUMMARY_STATUS"
echo "log=$LOG"

if [[ "${KEEP_OPEN:-1}" == "1" ]]; then
  echo "tmux shell pozostaje otwarty do inspekcji."
  exec bash -i
fi

exit "$DATASET_BUILD_STATUS"
