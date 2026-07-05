# Raport Shadow V2 L2-E2: candidate universe denominator repair 20260705

## Status

```text
final_verdict=CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E
runtime_decision_behavior_changes=NONE
runtime_evidence_schema_changes=NO
audit_output_schema_changes=YES
new_provider_streams=NONE
approval_flags=false
```

L2-E2 dostarcza offline-only repair/wiring dla event-level denominatora
wymaganego przez L2-E. Zmiana nie przyznaje L2, nie startuje L2-F i nie
zmienia Gatekeeper policy, BUY/REJECT, selector runtime, TX/Jito/live path,
`shadow_close_only` ani active close.

## Zakres

W zakresie:

- produkcja `candidate_universe_v1.jsonl` z event-level candidate observations;
- produkcja `candidate_universe_manifest_v1.json`;
- uzycie `gatekeeper_v2_decisions.jsonl` tylko jako context;
- deterministyczny join po `candidate_id`, `join_key` albo `base_mint+pool_id`;
- ponowne uruchomienie L2-E Gatekeeper coverage denominator audit;
- mapowanie L2-E result do dozwolonych verdictow L2-E2;
- summary CSV w formacie `metric,value,notes`;
- wąska korekta metrologii: `SHADOW_INSUFFICIENT_DATA` jest typed
  insufficient-data timeout bucket, nie `UNKNOWN`.

Poza zakresem:

- brak Gatekeeper threshold tuning;
- brak nowych provider/NLN subscriptions;
- brak zmian Rust runtime;
- brak zmian decyzji BUY/REJECT;
- brak zmian selector/live/TX/Jito path;
- brak research validation run;
- brak `runtime_approval`, `research_grade`, `live_equivalence` albo strategy
  unlock.

## Implementowany Kontrakt

L2-E2 akceptuje tylko te finalne verdicts:

```text
CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E
BLOCKED_CANDIDATE_UNIVERSE_SOURCE_MISSING
BLOCKED_GATEKEEPER_DECISION_JOIN_MISSING
BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS
BLOCKED_GATEKEEPER_THRESHOLD_STARVATION
```

Denominator moze byc uznany za gotowy tylko wtedy, gdy manifest potwierdza:

```text
denominator_invariant_status=PASS
decision_logs_created_denominator_rows=0
candidate_ids_from_decision_only=0
```

Gatekeeper decision logs pozostaja context-only. Skrypt L2-E2 nie udostepnia
trybu `allow_decision_universe`, wiec decyzje nie moga utworzyc wierszy
denominatora.

## Zmiany

### `scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py`

Dodany offline wrapper:

- zbiera event JSONL z `--events` albo `--events-root`;
- zbiera Gatekeeper decisions z `--decision-jsonl` albo `--decision-root`;
- buduje `candidate_universe_v1.jsonl` przez istniejacy
  `build_selector_candidate_universe.py`;
- buduje `candidate_universe_manifest_v1.json`;
- uruchamia `shadow_v2_gatekeeper_coverage_denominator_audit.py`;
- zapisuje L2-E2 summary CSV;
- opcjonalnie zapisuje JSON report.

### `scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py`

Dodano waska klasyfikacje:

```text
SHADOW_INSUFFICIENT_DATA -> TIMEOUT_SHADOW_INSUFFICIENT_DATA
```

To nie zmienia runtime. To usuwa false `UNKNOWN` dla wierszy, ktore maja
typed `reason_code=SHADOW_INSUFFICIENT_DATA` oraz szczegolowy
`decision_reason=TIMER_FIRED_INSUFFICIENT_DATA...`.

### Testy

Dodano:

```text
tests/test_shadow_v2_l2_e2_candidate_universe_denominator_repair.py
```

oraz rozszerzono:

```text
tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
```

Nowe testy pokrywaja:

- ready denominator z typed reasons;
- brak event source bez tworzenia denominator rows z decyzji;
- brak joinu Gatekeeper decision evidence;
- generic reject reason blocker;
- threshold starvation bez tuningu progow;
- `SHADOW_INSUFFICIENT_DATA` jako typed timeout bucket.

## Aktualny Wynik Na Lokalnym Scope

Command:

```bash
python3 scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py \
  --scope shadow-v2-l2-e2 \
  --events-root datasets/events \
  --decision-jsonl logs/decisions/unknown_rollout/v2.5/v25_shadow/e68ffb83c97d478e47528848c81acdeff7472c08a928e1b1d0201641ba1ded42/gatekeeper_v2_decisions.jsonl \
  --decision-jsonl logs/decisions/unknown_rollout/v2.2/legacy_live/e68ffb83c97d478e47528848c81acdeff7472c08a928e1b1d0201641ba1ded42/gatekeeper_v2_decisions.jsonl \
  --summary-csv reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv \
  --output-json reports/selector/shadow-v2-l2-e2/candidate_universe_denominator_repair_report.json \
  --output-csv reports/selector/shadow_v2_l2_e2_candidate_universe_denominator_summary.csv \
  --pretty
```

Outputs:

```text
candidate_universe_v1=datasets/selector/shadow-v2-l2-e2/candidate_universe_v1.jsonl
candidate_universe_manifest_v1=reports/selector/shadow-v2-l2-e2/candidate_universe_manifest_v1.json
summary_csv=reports/selector/shadow_v2_l2_e2_candidate_universe_denominator_summary.csv
```

Metrics:

```text
final_verdict=CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E
l2_e_verdict=GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN
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

Interpretacja:

- event-level candidate denominator istnieje dla lokalnego scope;
- Gatekeeper decisions nie utworzyly zadnych denominator rows;
- wszystkie decision rows joinuja sie do candidate universe jako context;
- 5 eligible candidates nie ma checkpoint reach, ale L2-E zna coverage i nie
  klasyfikuje tego jako starvation;
- `complete_executable_roundtrip_positions=28` pozostaje tylko kontekstem z
  PR41 summary, nie finalnym research validation proof;
- L2-F nadal nie zostal uruchomiony.

## Co Nadal Blokuje Pelne L2

L2-E2 usuwa blocker denominatora dla obecnego lokalnego scope, ale L2
research-grade nadal wymaga osobno:

- temporal PASS na docelowym validation scope;
- density/horizon PASS;
- dedicated L2-F research validation run;
- `research_candidate_roundtrip_count > 0`;
- `complete_executable_roundtrip_positions >= 500`;
- manifest/replay/lifecycle PASS;
- malformed rows = 0;
- unknown/untyped blockers = 0;
- approval flags pozostaja false do finalnego gate.

## Weryfikacja

Wykonane:

```bash
python3 -m py_compile scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py tests/test_shadow_v2_l2_e2_candidate_universe_denominator_repair.py
python3 tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
python3 tests/test_shadow_v2_l2_e2_candidate_universe_denominator_repair.py
python3 scripts/shadow_v2_l2_e2_candidate_universe_denominator_repair.py --scope shadow-v2-l2-e2 --events-root datasets/events --decision-jsonl logs/decisions/unknown_rollout/v2.5/v25_shadow/e68ffb83c97d478e47528848c81acdeff7472c08a928e1b1d0201641ba1ded42/gatekeeper_v2_decisions.jsonl --decision-jsonl logs/decisions/unknown_rollout/v2.2/legacy_live/e68ffb83c97d478e47528848c81acdeff7472c08a928e1b1d0201641ba1ded42/gatekeeper_v2_decisions.jsonl --summary-csv reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv --output-json reports/selector/shadow-v2-l2-e2/candidate_universe_denominator_repair_report.json --output-csv reports/selector/shadow_v2_l2_e2_candidate_universe_denominator_summary.csv --pretty
```

## Final Verdict

```text
final_verdict=CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=return_to_L2_E_gate_then_L2_F_only_after_temporal_density_and_research_gates
```
