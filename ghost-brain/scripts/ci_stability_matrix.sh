#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

# OOM mitigation for weak runners.
# Keep linker concurrency low and optionally use lld.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
if command -v ld.lld >/dev/null 2>&1; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-fuse-ld=lld"
fi

# Job A
cargo test -p ghost-brain --lib execution::live::tests -- --nocapture

# Job B
cargo test -p ghost-brain --lib aem::tests -- --nocapture

# Job C
cargo test -p ghost-brain --lib execution::paper::tests -- --nocapture

# Job D
cargo test -p ghost-brain --lib execution::dual::tests -- --nocapture

# Job E
cargo run -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 50,500,2000 \
  --profiles baseline \
  --scenarios none \
  --seed 42 \
  --output-dir ghost-brain/artifacts/replay_equivalence/ci_baseline
