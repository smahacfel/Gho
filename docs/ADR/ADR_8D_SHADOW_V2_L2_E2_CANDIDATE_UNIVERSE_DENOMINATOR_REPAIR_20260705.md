# ADR-8D: Shadow V2 L2-E2 Candidate Universe Denominator Repair 20260705

## Status

Accepted implementation stage.

## Decision

Dodajemy offline-only L2-E2 repair/wiring stage dla event-level candidate
universe denominator:

```text
final_verdict=CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E
contract_implementation=L2_E2_CANDIDATE_UNIVERSE_DENOMINATOR_REPAIR_IMPLEMENTED
```

Decyzja architektoniczna: `candidate_universe_v1.jsonl` pozostaje event-level
denominator SSOT. Gatekeeper decision logs sa contextem i nie moga tworzyc
denominator rows. L2-E2 moze zwrocic ready dla L2-E denominator coverage, ale
nie przyznaje L2 research-grade i nie uruchamia L2-F.

## Context

PR52 / L2-E wprowadzil fail-closed audit dla Gatekeeper coverage,
denominatora i starvation. Jego poprawny wynik na tracked checkout byl:

```text
BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
```

To bylo poprawne, poniewaz tracked checkout nie mial gotowego
`candidate_universe_v1.jsonl` ani explicit Gatekeeper decision scope. L2-E2
jest nastepnym etapem: ma wyprodukowac albo podlaczyc event-level denominator
oraz decyzje jako context-only evidence.

## Implemented Contract

L2-E2 dodaje:

```text
scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py
```

Skrypt:

1. zbiera event-level candidate observations z `--events` albo `--events-root`;
2. zbiera Gatekeeper decisions z `--decision-jsonl` albo `--decision-root`;
3. buduje `candidate_universe_v1.jsonl` przez istniejacy
   `build_selector_candidate_universe.py`;
4. buduje `candidate_universe_manifest_v1.json`;
5. uruchamia `shadow_v2_gatekeeper_coverage_denominator_audit.py`;
6. zapisuje `metric,value,notes` summary CSV;
7. mapuje L2-E verdict na L2-E2 verdict.

Dozwolone finalne verdicts:

```text
CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E
BLOCKED_CANDIDATE_UNIVERSE_SOURCE_MISSING
BLOCKED_GATEKEEPER_DECISION_JOIN_MISSING
BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS
BLOCKED_GATEKEEPER_THRESHOLD_STARVATION
```

Denominator invariant:

```text
denominator_invariant_status=PASS
decision_logs_created_denominator_rows=0
candidate_ids_from_decision_only=0
```

Jezeli event source jest pusty albo nie daje eligible denominator rows, L2-E2
fail-closed zwraca:

```text
BLOCKED_CANDIDATE_UNIVERSE_SOURCE_MISSING
```

Jezeli Gatekeeper decisions istnieja, ale nie dolaczaja sie do denominatora,
L2-E2 fail-closed zwraca:

```text
BLOCKED_GATEKEEPER_DECISION_JOIN_MISSING
```

Jezeli reject reasons sa generic/unknown, L2-E2 fail-closed zwraca:

```text
BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS
```

Jezeli znany denominator nie daje BUY albo nie dochodzi do checkpoint reach,
L2-E2 fail-closed zwraca:

```text
BLOCKED_GATEKEEPER_THRESHOLD_STARVATION
```

## Narrow Metrology Classification Change

W `scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py` klasyfikujemy:

```text
SHADOW_INSUFFICIENT_DATA -> TIMEOUT_SHADOW_INSUFFICIENT_DATA
```

Uzasadnienie:

- lokalne decision rows mialy `verdict_type=SHADOW_INSUFFICIENT_DATA`;
- mialy typed `reason_code=SHADOW_INSUFFICIENT_DATA`;
- mialy szczegolowe `decision_reason=TIMER_FIRED_INSUFFICIENT_DATA...`;
- poprzedni audit klasyfikowal je jako `UNKNOWN` tylko dlatego, ze brakowalo
  mapowania w metrologii.

To nie jest zmiana Gatekeeper policy ani runtime behavior. To tylko usuwa
false unknown bucket w offline audit taxonomy.

## Current Local Scope Result

L2-E2 run na lokalnym scope:

```text
event_source_path_count=22
candidate_universe_count=1368
eligible_denominator_count=1368
denominator_invariant_status=PASS
decision_logs_created_denominator_rows=0
candidate_ids_from_decision_only=0
decision_context_rows_joined=2584
gatekeeper_decision_count=2584
gatekeeper_decision_joined_to_candidate_count=2584
gatekeeper_decision_unmatched_count=0
checkpoint_reach_count=1363
gatekeeper_buy_count=152
gatekeeper_reject_count=1678
gatekeeper_timeout_count=754
unknown_reason_count=0
threshold_starvation_verdict=NO_GATEKEEPER_THRESHOLD_STARVATION_OBSERVED
```

Summary artifact:

```text
reports/selector/shadow_v2_l2_e2_candidate_universe_denominator_summary.csv
```

Generated local artifacts:

```text
datasets/selector/shadow-v2-l2-e2/candidate_universe_v1.jsonl
reports/selector/shadow-v2-l2-e2/candidate_universe_manifest_v1.json
```

These are generated dataset/report artifacts. They are produced by the L2-E2
script and are not a Gatekeeper decision source.

## Rejected Alternatives

### Use Gatekeeper decision logs as denominator source

Rejected. It would create selection-biased rows and violate the event-level
denominator contract.

### Tune Gatekeeper thresholds when starvation appears

Rejected. L2-E2 can report starvation but must not tune thresholds in the same
stage. Policy or observation starvation review must be separate.

### Treat `SHADOW_INSUFFICIENT_DATA` as generic unknown

Rejected. Rows with `reason_code=SHADOW_INSUFFICIENT_DATA` and
`TIMER_FIRED_INSUFFICIENT_DATA...` are typed insufficient-data outcomes. They
should be counted as timeout/insufficient-data evidence, not unknown.

### Start L2-F after denominator repair in the same PR

Rejected. L2-F is a dedicated research validation run and remains separate.

## Consequences

1. L2-E denominator blocker is repaired for the current local scope.
2. Decision logs remain context-only and do not create denominator rows.
3. Generic/unknown reject buckets still block.
4. Threshold starvation still blocks.
5. L2-F remains unstarted.
6. Approval flags remain false.

## Compatibility

No Rust runtime schema changes. No Gatekeeper policy changes. No BUY/REJECT
changes. No selector runtime changes. No TX/Jito/live path changes. No
`shadow_close_only` or active close changes.

Runtime behavior:

```text
runtime_decision_behavior_changes=NONE
runtime_evidence_schema_changes=NO
```

Audit behavior:

```text
audit_output_schema_changes=YES
```

## Verification

```bash
python3 -m py_compile scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py tests/test_shadow_v2_l2_e2_candidate_universe_denominator_repair.py
python3 tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
python3 tests/test_shadow_v2_l2_e2_candidate_universe_denominator_repair.py
python3 scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py --scope shadow-v2-l2-e2 --events-root datasets/events --decision-jsonl logs/decisions/unknown_rollout/v2.5/v25_shadow/e68ffb83c97d478e47528848c81acdeff7472c08a928e1b1d0201641ba1ded42/gatekeeper_v2_decisions.jsonl --decision-jsonl logs/decisions/unknown_rollout/v2.2/legacy_live/e68ffb83c97d478e47528848c81acdeff7472c08a928e1b1d0201641ba1ded42/gatekeeper_v2_decisions.jsonl --summary-csv reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv --output-json reports/selector/shadow-v2-l2-e2/candidate_universe_denominator_repair_report.json --output-csv reports/selector/shadow_v2_l2_e2_candidate_universe_denominator_summary.csv --pretty
```

## Final Decision

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=return_to_L2_E_gate_then_L2_F_only_after_temporal_density_and_research_gates
```
