#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-ghost-brain/tests/fixtures/replay}"
mkdir -p "$OUT_DIR"

generate_fixture() {
  local n="$1"
  local out_file="$2"
  : > "$out_file"
  for ((i=0; i<n; i++)); do
    local submit=$((1700000000000 + i * 73))
    local amount=$((1000000 + (i % 17) * 25000))
    local min_out=$((900 + (i % 31) * 11))
    printf '{"candidate_id":"cand-%04d","submit_time_ms":%d,"amount_lamports":%d,"min_tokens_out":%d}\n' \
      "$i" "$submit" "$amount" "$min_out" >> "$out_file"
  done
}

generate_fixture 500 "$OUT_DIR/candidates_500.jsonl"
generate_fixture 2000 "$OUT_DIR/candidates_2000.jsonl"

wc -l "$OUT_DIR/candidates_500.jsonl" "$OUT_DIR/candidates_2000.jsonl"
