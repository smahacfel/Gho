#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

# Baseline proof: N=50/500/2000
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 50,500,2000 \
  --profiles baseline \
  --scenarios none \
  --seed 42 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_baseline

# Stress proof: N=500
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 500 \
  --profiles stress \
  --scenarios none \
  --seed 42 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_stress

# Pathological proof: N=500
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 500 \
  --profiles pathological \
  --scenarios none \
  --seed 42 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_pathological

# F1 scenarios: channel closed/full
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 500 \
  --profiles baseline \
  --scenarios f1_channel_closed,f1_channel_full \
  --seed 42 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_f1

# F2 scenario: recovery sweep terminalization
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 500 \
  --profiles baseline \
  --scenarios f2_recovery_sweep \
  --seed 42 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_f2

# Failcheck (expected FAIL + exit code 1)
set +e
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 500 \
  --profiles baseline \
  --scenarios none \
  --seed 42 \
  --timing-threshold-pct -100 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_failcheck
rc=$?
set -e

if [[ "$rc" -eq 0 ]]; then
  echo "failcheck expected non-zero exit code, got 0"
  exit 1
fi

echo "failcheck exit code: $rc (expected non-zero)"
