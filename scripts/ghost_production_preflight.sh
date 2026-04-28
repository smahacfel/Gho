#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/ghost_production_preflight.sh [--config PATH] [--baseline-file PATH]

Checks that the current git revision has an accepted baseline stamp and then
runs the Phase 0 structural acceptance gate and runtime preflight against the selected rollout config.
EOF
}

print_baseline_remediation() {
  cat >&2 <<EOF
[hint] baseline.accepted_revision: create or refresh the local baseline stamp after green baseline checks
[hint] run:
[hint]   mkdir -p "$(dirname "$BASELINE_FILE")"
[hint]   cargo test --workspace --no-run
[hint]   printf '%s\n' "$(git -C "$REPO_ROOT" rev-parse HEAD)" > "$BASELINE_FILE"
EOF
}

run_structural_acceptance_gate() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "[fail] structural.acceptance: python3 is required" >&2
    exit 1
  fi
  python3 "$REPO_ROOT/scripts/refactor_phase0_guardrails.py" structural-check --repo-root "$REPO_ROOT"
}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_PATH="$REPO_ROOT/config.toml"
BASELINE_FILE="$REPO_ROOT/.ghost/baseline_accepted_revision"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config)
      CONFIG_PATH="$2"
      shift 2
      ;;
    --baseline-file)
      BASELINE_FILE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

mkdir -p "$(dirname "$BASELINE_FILE")"

if ! command -v git >/dev/null 2>&1; then
  echo "[fail] baseline.accepted_revision: git is required" >&2
  exit 1
fi

if [[ ! -f "$BASELINE_FILE" ]]; then
  echo "[fail] baseline.accepted_revision: missing baseline stamp $BASELINE_FILE" >&2
  print_baseline_remediation
  exit 1
fi

CURRENT_REVISION="$(git -C "$REPO_ROOT" rev-parse HEAD)"
ACCEPTED_REVISION="$(tr -d '[:space:]' < "$BASELINE_FILE")"

if [[ -z "$ACCEPTED_REVISION" ]]; then
  echo "[fail] baseline.accepted_revision: baseline stamp is empty" >&2
  print_baseline_remediation
  exit 1
fi

if [[ "$ACCEPTED_REVISION" != "$CURRENT_REVISION" ]]; then
  echo "[fail] baseline.accepted_revision: expected $CURRENT_REVISION but stamp contains $ACCEPTED_REVISION" >&2
  print_baseline_remediation
  exit 1
fi

echo "[ok] baseline.accepted_revision: $ACCEPTED_REVISION"

cd "$REPO_ROOT"
run_structural_acceptance_gate
cargo run --quiet -p ghost-launcher --bin ghost-launcher -- --config "$CONFIG_PATH" --preflight
