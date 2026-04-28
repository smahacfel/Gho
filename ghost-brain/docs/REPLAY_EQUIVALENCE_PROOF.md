# Replay Equivalence Proof v2 (Live-only vs Dual[Live lane])

## Scope i kontrakt porównania
- Porównanie dotyczy wyłącznie `lane=Live`.
- Run A: `mode=live_only`.
- Run B: `mode=dual`, ale proof używa tylko `dual(lane=live)`.
- `lane=paper` w Dual jest generowane równolegle i ignorowane w werdykcie ekwiwalencji.

## Deterministyczne wejście (bez sieci)
- Fixtures: `ghost-brain/tests/fixtures/replay/`
1. `candidates_test_set.jsonl` (N=50)
2. `candidates_500.jsonl` (N=500)
3. `candidates_2000.jsonl` (N=2000)
- Wszystkie runy są seedowane (`--seed`), bez streamów live.

Deterministyczna regeneracja fixture'ów:
```bash
ghost-brain/scripts/generate_replay_fixtures.sh
```

## CLI v2
```bash
cargo run -j 4 -p ghost-brain --bin replay_equivalence_proof -- \
  --fixture-dir ghost-brain/tests/fixtures/replay \
  --sizes 50,500,2000 \
  --profiles baseline \
  --scenarios none \
  --seed 42 \
  --timing-threshold-pct 10 \
  --terminal-delta-pct 5 \
  --terminal-delta-abs-min 2 \
  --output-dir ghost-brain/artifacts/replay_equivalence/v2_baseline
```

Parametry:
- `--sizes`: CSV N (np. `50,500,2000`)
- `--profiles`: `baseline|stress|pathological`
- `--scenarios`: `none|f1_channel_closed|f1_channel_full|f2_recovery_sweep`
- `--timing-threshold-pct`: domyślnie `10`
- `--terminal-delta-pct`: domyślnie `5` (reguła "close enough")
- `--terminal-delta-abs-min`: domyślnie `2`
- `--ttl-ms`, `--pathological-timeout-pct`, `--scenario-impact-pct`: kontrola timeoutów i iniekcji

## Profile latency
1. `baseline`: `U(200..400)ms`
2. `stress`: `U(200..600)ms + jitter U(0..50)ms`
3. `pathological`: bazowo baseline + deterministyczna frakcja opóźnień > TTL (timeout terminal)

## Scenariusze failure injection
1. `f1_channel_closed`: deterministyczna frakcja zleceń kończy się terminalnym `Failed`
2. `f1_channel_full`: deterministyczna frakcja zleceń kończy się terminalnym `Failed`
3. `f2_recovery_sweep`: deterministyczna frakcja zleceń kończy się terminalnym `Unknown` oznaczonym jako recovery-driven

## Twarde kryteria PASS/FAIL
Dla każdego case (`N x profile x scenario`) i dla obu runów (`live_only`, `dual_live`):
1. `submitted == filled + failed + timeout + unknown`
2. `in_flight == 0`
3. `missing_terminal_orders == []`
4. `multiple_terminal_orders == []`
5. `duplicates_fill == 0`, `duplicates_opened == 0`, `duplicates_closed == 0`
6. `candidate_without_opened_or_failed == []`

Dodatkowo ekwiwalencja Live-only vs Dual(Live):
1. `submitted_live_only == submitted_dual_live`
2. rozkład terminali "close enough":
   `abs(delta_class) <= max(abs_min, terminal_delta_pct * N_terminal)`
   (domyślnie `max(2, 5%)`)
3. timing:
   `p50_dual_live <= p50_live_only * (1 + timing_threshold_pct)`
   `p90_dual_live <= p90_live_only * (1 + timing_threshold_pct)`

Polityka `unknown`:
1. poza `f2_recovery_sweep`: `unknown == 0`
2. w `f2_recovery_sweep`: `unknown > 0` dozwolone tylko jako recovery-driven

## Artefakty
Dla każdego case:
- `.../<case_id>/live/events.jsonl`
- `.../<case_id>/dual/events.jsonl`
- `.../<case_id>/live/live_only_report.json`
- `.../<case_id>/dual/dual_live_lane_report.json`
- `.../<case_id>/comparison_report.json`
- `.../<case_id>/replay_equivalence_verdict.txt`

Raport zbiorczy:
- `.../replay_equivalence_v2_report.json`
- `.../replay_equivalence_v2_verdict.txt`
- aliasy kompatybilności:
1. `.../replay_equivalence_report.json`
2. `.../replay_equivalence_verdict.txt`

## Minimalny zestaw uruchomień v2
```bash
ghost-brain/scripts/run_replay_equivalence_v2.sh
```

Skrypt uruchamia:
1. baseline: N=50/500/2000
2. stress: N=500
3. pathological: N=500
4. F1: channel_closed + channel_full (N=500)
5. F2: recovery_sweep (N=500)
6. failcheck: oczekiwany FAIL + exit code != 0

## CI stability / OOM runbook
Canonical matrix + mitigacja OOM:
```bash
ghost-brain/scripts/ci_stability_matrix.sh
```

Skrypt:
1. ustawia `CARGO_BUILD_JOBS=1` (low link concurrency)
2. używa `lld` jeśli dostępny
3. rozdziela testy na podzbiory:
   - Job A: `execution::live::tests`
   - Job B: `aem::tests`
   - Job C: `execution::paper::tests`
   - Job D: `execution::dual::tests`
   - Job E: proof run (`replay_equivalence_proof`)

## Interpretacja raportów
- Każdy raport JSON ma `verdict` i `failed_checks`.
- Globalny PASS wymaga, by wszystkie case'y miały PASS.
- FAIL kończy proces binarki kodem `!= 0` (CI-friendly).
