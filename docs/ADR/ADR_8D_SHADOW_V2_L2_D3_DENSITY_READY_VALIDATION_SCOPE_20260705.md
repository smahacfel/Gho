# ADR-8D: Shadow V2 L2-D3 Density Contract Fixture Scope 20260705

## Status

Accepted fixture-only implementation stage.

## Decision

Dodajemy L2-D3 jako waski density contract fixture scope:

```text
final_verdict=L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED
pr55_contract_fixture_verdict=PR55_AS_L2_D3_CONTRACT_FIXTURE_ACCEPTED
pr55_runtime_density_verdict=PR55_AS_RUNTIME_DENSITY_EMISSION_PROOF_NOT_ACCEPTED
runtime_density_emission_proof=false
l2_f_allowed_next=false
L2_F_ALLOWED_NEXT=false
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
```

L2-D3 nie jest L2-F i nie jest research validation run. Ten stage potwierdza
tylko, ze audit density/retention moze przejsc declared baseline horizons na
deterministycznie przygotowanych fixture rows. Ten stage nie dowodzi, ze realny
runtime albo validation harness emituje path samples do `121000ms`, ani ze
runtime/harness wyprowadza `shadow_path_density_v2` z kanonicznych
`shadow_position_event_v2` rows.

## Context

PR54 / L2-D2 naprawil:

```text
standard_120s.max_horizon_ms = 121000
```

oraz rozdzielil:

- retention shortfall;
- path sample coverage insufficiency.

Historyczny scope nadal byl zablokowany przez coverage. L2-D3 tworzy nowy,
kontrolowany density contract fixture, aby sprawdzic mechanike audit contract
bez uruchamiania pelnego L2-F i bez claimu runtime emission proof.

## Implemented Contract

Run:

```text
run_id=shadow-v2-l2-d3-density-ready-validation-20260705-r1
configured_run_seconds=121
duration_ms=121000
sample_interval_ms=1000
position_count=25
path_sample_count=3050
density_row_count=175
```

Declared horizons:

```text
2000
3000
10000
30000
120000
```

Long horizons:

```text
300000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
500000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

Final D3 fixture verdict:

```text
L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED
```

## Code-Level Changes

### `scripts/shadow_v2_l2_d3_density_ready_scope.py`

New deterministic generator for a density-only contract fixture scope. It writes:

- `shadow_position_event_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `density_ready_validation_manifest.json`.

The generated raw JSONL scope is not a positive research sample and is not
runtime density emission proof. It is a controlled density contract fixture.

### `scripts/shadow_v2_path_density_horizon_audit.py`

The audit now accepts:

```text
--pass-verdict
```

This keeps the D2 default verdict intact while allowing D3 fixture to emit:

```text
L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED
```

For the D3 fixture verdict, the audit emits:

```text
density_contract_fixture_pass=true
density_fixture_l2_f_allowed_next=false
runtime_density_emission_proof=false
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
l2_f_allowed_next=false
```

### Tests

Added:

```text
tests/test_shadow_v2_l2_d3_density_ready_scope.py
```

Updated:

```text
tests/test_shadow_v2_path_density_horizon_audit.py
```

## Rejected Alternatives

### Start L2-F directly

Rejected. L2-F remains a separate dedicated research validation run.

### Use historical PR18C scope

Rejected. PR18C lacks path sample coverage and remains a blocker.

### Promote 300s/500s into baseline

Rejected. They remain undeclared until explicitly promoted in another stage.

### Tune Gatekeeper thresholds

Rejected. Density readiness is independent from policy tuning.

## Consequences

1. L2-D3 is accepted only as deterministic density contract fixture.
2. L2-F is not allowed next without L2-D3B runtime/harness density emission proof.
3. L2 is still not granted.
4. Research grade, live equivalence, strategy unlock, runtime approval,
   shadow close and active close remain false.
5. Full L2-F must still prove temporal, manifest, replay/lifecycle,
   Gatekeeper denominator, sample size, malformed-row, and unknown-blocker gates.

## Verification

```bash
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py scripts/shadow_v2_l2_d3_density_ready_scope.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
python3 tests/test_shadow_v2_l2_d3_density_ready_scope.py
python3 scripts/shadow_v2_l2_d3_density_ready_scope.py --scope-root reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1 --run-id shadow-v2-l2-d3-density-ready-validation-20260705-r1 --positions 25 --duration-ms 121000 --sample-interval-ms 1000
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1 --pass-verdict L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED --output-csv reports/selector/shadow_v2_l2_d3_density_ready_validation_summary.csv --pretty
CSV parser check for reports/selector/shadow_v2_l2_d3_density_ready_validation_summary.csv
```

## Final Decision

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
runtime_density_emission_proof=false
density_contract_fixture_pass=true
density_fixture_l2_f_allowed_next=false
l2_f_allowed_next=false
L2_F_ALLOWED_NEXT=false
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
```
