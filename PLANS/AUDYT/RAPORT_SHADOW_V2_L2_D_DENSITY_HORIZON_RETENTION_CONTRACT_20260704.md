# Raport Shadow V2 L2-D: density horizon retention contract 20260704

## Status

```text
final_verdict=BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
contract_implementation=L2_D_DENSITY_HORIZON_CONTRACT_IMPLEMENTED
runtime_decision_behavior_changes=NONE
runtime_evidence_schema_changes=NO
audit_output_schema_changes=YES
new_provider_streams=NONE
approval_flags=false
```

L2-D implementuje metrology-only kontrakt density / horizon / retention dla
Shadow V2 L2 offline research baseline. Zmiana nie przyznaje L2. Zmiana nie
dotyka Gatekeeper, BUY/REJECT, selector runtime, TX/Jito/live path,
`shadow_close_only` ani active close.

## Zakres

W zakresie:

- jawna lista declared supported horizons dla pierwszego L2 baseline;
- jawna lista undeclared long horizons;
- fail-closed audit dla declared horizons;
- osobne `NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE` dla 300s/500s;
- retention/replay coverage contract dla max declared horizon plus margin;
- per-horizon metryki coverage, sample density, gap, input defect i verdict;
- testy kontraktowe dla pass, missing declared, sparse declared i retention gap.

Poza zakresem:

- brak nowych NLN/provider subscriptions;
- brak zmian runtime path sampler;
- brak zmian runtime decision behavior;
- brak zmian Gatekeeper policy;
- brak zmian selector/live/TX/Jito path;
- brak Gatekeeper denominator audit;
- brak dedicated research validation run;
- brak `runtime_approval`, `research_grade`, `live_equivalence` albo strategy
  unlock.

## Implementowany Kontrakt

Pierwszy L2 baseline deklaruje tylko:

```text
2000
3000
10000
30000
120000
```

Domyslne long horizons:

```text
300000
500000
```

sa traktowane jako:

```text
NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

To oznacza:

- 300s/500s nie blokuja pierwszego L2 baseline, jezeli nie sa jawnie
  zadeklarowane;
- 300s/500s nie moga byc uzyte do pozytywnego research claim;
- declared horizons musza miec `EVALUABLE_EXACT` albo `EVALUABLE_APPROX`;
- `SPARSE_APPROX_ONLY` nie przechodzi jako L2 baseline PASS;
- kazdy declared horizon musi miec replay/retention coverage co najmniej:

```text
max_declared_horizon_ms + retention_margin_ms
```

Domyslny margin w audycie:

```text
retention_margin_ms=1000
required_replay_horizon_ms=121000
```

## Zmiany

### `scripts/shadow_v2_path_density_horizon_audit.py`

Skrypt zostal rozszerzony z prostego "any evaluable row" do kontraktu L2-D:

- `--declared-horizons-ms`
- `--undeclared-horizons-ms`
- `--retention-margin-ms`
- `declared_supported_horizons_ms`
- `undeclared_horizons_ms`
- `required_replay_horizon_ms`
- `declared_horizon_missing_count`
- `declared_horizon_density_blocker_count`
- `declared_horizon_retention_blocker_count`
- `undeclared_horizons_block_l2_baseline=false`
- per-horizon `positive_research_claim_allowed`
- per-horizon `l2_baseline_blocker`

Dozwolone finalne verdicts audytu:

```text
L2_D_DENSITY_HORIZON_CONTRACT_PASS_FOR_DECLARED_HORIZONS
BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE
BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
```

### `tests/test_shadow_v2_path_density_horizon_audit.py`

Dodane testy:

```text
test_declared_horizons_pass_and_long_horizons_are_non_blocking
test_missing_declared_horizon_blocks_l2_density
test_sparse_declared_horizon_is_not_promoted_to_pass
test_retention_margin_shortfall_blocks_even_when_declared_rows_are_evaluable
```

## Aktualny Wynik Na Istniejacym Scope

Audit uruchomiony na:

```text
reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1
```

zwraca:

```text
verdict=BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
density_rows=7154
horizon_count=7
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_retention_blocker_count=5
undeclared_horizon_present_count=2
undeclared_horizons_block_l2_baseline=false
```

Interpretacja:

- declared horizons sa obecne, ale replay/retention nie pokrywa wymaganego
  `121000ms`;
- 300s/500s sa obecne w danych jako long horizons, ale nie blokuja pierwszego
  L2 baseline, bo nie sa declared;
- 300s/500s nadal nie moga byc uzywane do pozytywnego research claim;
- L2 pozostaje zablokowane do czasu realnego validation run z density PASS.

## Co Nadal Blokuje L2

Po L2-D nadal wymagane sa:

- L2-E Gatekeeper coverage / denominator / starvation audit;
- dedicated L2-F research validation run;
- temporal audit PASS na realnym runie;
- density audit PASS dla declared horizons na realnym runie;
- manifest/replay/lifecycle PASS;
- sample-size gate;
- `research_candidate_roundtrip_count > 0`.

## Weryfikacja

Wykonane checki:

```bash
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py tests/test_shadow_v2_path_density_horizon_audit.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty
```

## Final Verdict

```text
final_verdict=BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=L2-E_GATEKEEPER_COVERAGE_DENOMINATOR_STARVATION_AUDIT
```
