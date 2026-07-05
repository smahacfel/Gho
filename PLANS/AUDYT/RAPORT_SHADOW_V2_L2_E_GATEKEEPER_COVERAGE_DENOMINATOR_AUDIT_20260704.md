# Raport Shadow V2 L2-E: Gatekeeper coverage denominator starvation audit 20260704

## Status

```text
final_verdict=BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
contract_implementation=L2_E_GATEKEEPER_COVERAGE_DENOMINATOR_AUDIT_IMPLEMENTED
runtime_decision_behavior_changes=NONE
runtime_evidence_schema_changes=NO
audit_output_schema_changes=YES
new_provider_streams=NONE
approval_flags=false
```

L2-E implementuje offline-only metrology gate dla Gatekeeper coverage,
denominator i threshold starvation. Zmiana nie przyznaje L2. Zmiana nie dotyka
Gatekeeper policy, BUY/REJECT, selector runtime, TX/Jito/live path,
`shadow_close_only` ani active close.

## Zakres

W zakresie:

- jawny event-level `candidate_universe_v1.jsonl` jako denominator;
- `eligible_denominator_count` z `candidate_universe_status=ok`;
- optional `candidate_universe_manifest_v1.json` jako invariant proof;
- Gatekeeper decision JSONL jako context, nie denominator creator;
- reject reason distribution;
- `unknown_reason_count` dla brakujacych/generic non-BUY reason rows;
- `checkpoint_reach_count` jako liczba unikalnych eligible candidates z joined
  Gatekeeper decision row;
- entry/exit/research roundtrip counters z explicit summary CSV, bez inferencji;
- fail-closed threshold starvation classification.

Poza zakresem:

- brak nowych NLN/provider subscriptions;
- brak zmian runtime path;
- brak zmian Gatekeeper thresholds;
- brak policy tuning przy wykryciu starvation;
- brak dedicated research validation run;
- brak `runtime_approval`, `research_grade`, `live_equivalence` albo strategy
  unlock.

## Implementowany Kontrakt

L2-E przyjmuje tylko cztery finalne verdicts:

```text
GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN
BLOCKED_GATEKEEPER_THRESHOLD_STARVATION
BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS
```

Denominator jest znany tylko wtedy, gdy:

- `candidate_universe_v1.jsonl` istnieje;
- zawiera rows;
- `eligible_denominator_count > 0`;
- manifest, jezeli dostarczony, nie wskazuje `NO-GO`;
- `decision_logs_created_denominator_rows == 0`;
- `candidate_ids_from_decision_only == 0`.

Gatekeeper decision logs sa uzywane tylko jako context. Nie wolno im tworzyc
denominator rows.

`checkpoint_reach_count` w tym audycie oznacza:

```text
unique eligible candidate_id with a joined gatekeeper_v2_decisions row
```

To nie jest runtime DOW checkpoint count. To metryka coverage: czy eligible
universe realnie dochodzi do Gatekeeper decision evidence.

Threshold starvation jest blokujace, gdy denominator jest znany, ale:

- nie ma Gatekeeper decisions;
- albo `checkpoint_reach_count == 0`;
- albo wszystkie joined Gatekeeper decisions sa typed reject/timeout i
  `gatekeeper_buy_count == 0`.

Wykrycie starvation nie luzuje progow. Wtedy wymagany jest osobny review/policy
stage.

## Zmiany

### `scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py`

Dodany offline audit script:

- `--candidate-universe`;
- `--candidate-manifest`;
- `--decision-jsonl`;
- `--decision-root`;
- `--summary-csv`;
- `--output-csv`;
- fail-closed JSON output;
- `metric,value,notes` summary CSV output.

Skrypt klasyfikuje:

- denominator failures;
- Gatekeeper decision join coverage;
- reject/timeout/BUY counts;
- reject reason top-N;
- unknown/generic non-BUY reason rows;
- checkpoint reach;
- research candidate counters tylko z explicit summary metrics;
- threshold starvation verdict.

### `tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py`

Dodane testy:

```text
test_missing_candidate_universe_blocks_denominator
test_known_denominator_with_typed_reasons_passes_l2e_metrology
test_generic_reject_reason_blocks_l2e
test_typed_rejects_without_buy_are_classified_as_threshold_starvation
test_manifest_decision_created_denominator_rows_blocks_denominator
```

## Aktualny Wynik Na Obecnym Checkoutcie

Audit uruchomiony z explicit summary:

```bash
python3 scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py \
  --summary-csv reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv \
  --output-csv reports/selector/shadow_v2_l2_e_gatekeeper_coverage_denominator_summary.csv \
  --pretty
```

Wynik:

```text
final_verdict=BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
candidate_universe_count=0
eligible_denominator_count=0
denominator_contract_failures=[
  candidate_universe_file_missing,
  candidate_universe_empty,
  eligible_denominator_zero
]
gatekeeper_decision_count=0
checkpoint_reach_count=0
threshold_starvation_verdict=NOT_EVALUATED_DENOMINATOR_UNKNOWN
exit_research_candidate_count=0
complete_executable_roundtrip_positions=28
```

Interpretacja:

- obecny tracked checkout nie zawiera L2-E event-level `candidate_universe_v1`;
- obecny tracked checkout nie zawiera Gatekeeper decision JSONL scope dla L2-E;
- PR41 summary pokazuje 28 complete executable diagnostic roundtrips, ale to
  nadal nie jest Gatekeeper denominator proof;
- threshold starvation nie jest ocenialne bez znanego denominatora;
- L2 pozostaje zablokowane.

## Co Nadal Blokuje L2

Po L2-E nadal wymagane sa:

- dostarczenie albo wygenerowanie L2 candidate universe z event-level denominator;
- Gatekeeper decision evidence joined do eligible denominator;
- typed reject reason distribution bez generic/unknown buckets;
- brak threshold starvation albo osobny policy/starvation review;
- L2-D density PASS na realnym validation scope;
- L2-F dedicated research validation run;
- temporal audit PASS;
- manifest/replay/lifecycle PASS;
- sample-size gate;
- `research_candidate_roundtrip_count > 0`.

## Weryfikacja

Wykonane checki:

```bash
python3 -m py_compile scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
python3 tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
python3 scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py --summary-csv reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv --output-csv reports/selector/shadow_v2_l2_e_gatekeeper_coverage_denominator_summary.csv --pretty
```

## Final Verdict

```text
final_verdict=BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=repair_candidate_universe_denominator_before_l2_f
```
