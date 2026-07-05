# Raport Shadow V2 L2-D3: density contract fixture scope 20260705

## Status

```text
final_verdict=L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED
pr55_contract_fixture_verdict=PR55_AS_L2_D3_CONTRACT_FIXTURE_ACCEPTED
pr55_runtime_density_verdict=PR55_AS_RUNTIME_DENSITY_EMISSION_PROOF_NOT_ACCEPTED
runtime_density_emission_proof=false
stage=L2-D3_DENSITY_CONTRACT_FIXTURE_SCOPE
base_after_pr54=7c522f39fc405cf8a3ae12ffb9b8df828f412bf6
run_id=shadow-v2-l2-d3-density-ready-validation-20260705-r1
configured_run_seconds=121
scope_root=reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1
density_contract_fixture_pass=true
density_fixture_l2_f_allowed_next=false
l2_f_allowed_next=false
L2_F_ALLOWED_NEXT=false
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
approval_flags=false
```

L2-D3 generuje waski, deterministyczny density contract fixture. To nie jest
runtime/harness density emission proof, L2-F research validation run ani
strategy/research-grade claim. Celem jest tylko potwierdzenie, ze D2/D3 audit
contract potrafi przejsc declared baseline horizons przy syntetycznie
przygotowanych density rows o wystarczajacej retencji i gestosci path samples.

Ten PR nie dowodzi, ze realny runtime albo harness emituje path samples przez
`121000ms`, ani ze realny pipeline wyprowadza `shadow_path_density_v2` z
kanonicznych `shadow_position_event_v2` rows. To musi dowiezc osobny etap
`L2-D3B`.

## Zakres

W zakresie:

- wygenerowanie swiezego density-only contract fixture scope;
- uzycie retention contract `121000ms`;
- uzycie path samples co `1000ms` do `121000ms`;
- uruchomienie `shadow_v2_path_density_horizon_audit.py` na fixture rows;
- wygenerowanie summary CSV;
- utrzymanie 300s/500s jako undeclared long horizons.

Poza zakresem:

- brak L2-F research validation run;
- brak Gatekeeper policy change;
- brak threshold tuning;
- brak BUY/REJECT change;
- brak selector runtime change;
- brak TX/Jito/live path change;
- brak nowych provider streams;
- brak `runtime_approval`, `research_grade`, `live_equivalence`,
  `strategy_research_unblocked`, `shadow_close_only`, `active_close`.
- brak zgody na L2-F bez `L2-D3B`;
- brak runtime/harness density emission proof.

## Run Configuration

```text
run_id=shadow-v2-l2-d3-density-ready-validation-20260705-r1
scope_root=reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1
configured_run_seconds=121
duration_ms=121000
sample_interval_ms=1000
position_count=25
path_sample_count=3050
density_row_count=175
```

Generator:

```bash
python3 scripts/shadow_v2_l2_d3_density_ready_scope.py \
  --scope-root reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1 \
  --run-id shadow-v2-l2-d3-density-ready-validation-20260705-r1 \
  --positions 25 \
  --duration-ms 121000 \
  --sample-interval-ms 1000
```

Generated raw JSONL scope is a local validation artifact. The PR carries the
generator, tests, report, ADR, and CSV summary, not the raw JSONL event dump.

## Declared Baseline Horizons

Declared supported horizons:

```text
2000
3000
10000
30000
120000
```

Retention contract:

```text
retention_contract_ms=121000
required_replay_coverage_ms=121000
```

Long horizons remain undeclared:

```text
300000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
500000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

## Audit Result

Audit command:

```bash
python3 scripts/shadow_v2_path_density_horizon_audit.py \
  --scope-root reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1 \
  --pass-verdict L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED \
  --output-csv reports/selector/shadow_v2_l2_d3_density_ready_validation_summary.csv \
  --pretty
```

Aggregate result:

```text
density_audit_verdict=L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED
density_rows=175
horizon_count=7
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_density_blocker_count=0
declared_horizon_path_coverage_blocker_count=0
declared_horizon_retention_blocker_count=0
density_contract_fixture_pass=true
density_fixture_l2_f_allowed_next=false
runtime_density_emission_proof=false
l2_f_allowed_next=false
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
```

Per declared horizon:

```text
horizon_ms=2000   eligible_positions=25 evaluable_positions=25 coverage_ratio=1.0 samples_per_position_p50=122.0 samples_per_position_p90=122.0 max_gap_ms_p90=1000.0 max_gap_ms_max=1000 retention_gap_count=0 duplicate_sample_count=0 non_monotonic_sample_count=0 verdict=PASS
horizon_ms=3000   eligible_positions=25 evaluable_positions=25 coverage_ratio=1.0 samples_per_position_p50=122.0 samples_per_position_p90=122.0 max_gap_ms_p90=1000.0 max_gap_ms_max=1000 retention_gap_count=0 duplicate_sample_count=0 non_monotonic_sample_count=0 verdict=PASS
horizon_ms=10000  eligible_positions=25 evaluable_positions=25 coverage_ratio=1.0 samples_per_position_p50=122.0 samples_per_position_p90=122.0 max_gap_ms_p90=1000.0 max_gap_ms_max=1000 retention_gap_count=0 duplicate_sample_count=0 non_monotonic_sample_count=0 verdict=PASS
horizon_ms=30000  eligible_positions=25 evaluable_positions=25 coverage_ratio=1.0 samples_per_position_p50=122.0 samples_per_position_p90=122.0 max_gap_ms_p90=1000.0 max_gap_ms_max=1000 retention_gap_count=0 duplicate_sample_count=0 non_monotonic_sample_count=0 verdict=PASS
horizon_ms=120000 eligible_positions=25 evaluable_positions=25 coverage_ratio=1.0 samples_per_position_p50=122.0 samples_per_position_p90=122.0 max_gap_ms_p90=1000.0 max_gap_ms_max=1000 retention_gap_count=0 duplicate_sample_count=0 non_monotonic_sample_count=0 verdict=PASS
```

Long horizons:

```text
horizon_ms=300000 verdict=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE l2_baseline_blocker=false positive_research_claim_allowed=false
horizon_ms=500000 verdict=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE l2_baseline_blocker=false positive_research_claim_allowed=false
```

## Required Artifact

```text
reports/selector/shadow_v2_l2_d3_density_ready_validation_summary.csv
```

## Interpretation

L2-D3 passes only the deterministic density contract fixture gate for declared
horizons. It does not prove runtime/harness path-sample emission and does not
prove density derivation from canonical path sample rows.

L2-F is not allowed as the next stage from this PR alone. The required next
stage is:

```text
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
```

L2-D3B must feed path samples through the real validation/harness path, derive
density rows from canonical `shadow_position_event_v2` rows, prove `standard_120s`
retention through `121000ms`, and keep all approval flags false.

## Verification

```bash
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py scripts/shadow_v2_l2_d3_density_ready_scope.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
python3 tests/test_shadow_v2_l2_d3_density_ready_scope.py
python3 scripts/shadow_v2_l2_d3_density_ready_scope.py --scope-root reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1 --run-id shadow-v2-l2-d3-density-ready-validation-20260705-r1 --positions 25 --duration-ms 121000 --sample-interval-ms 1000
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-l2-d3-density-ready-validation-20260705-r1 --pass-verdict L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED --output-csv reports/selector/shadow_v2_l2_d3_density_ready_validation_summary.csv --pretty
CSV parser check for reports/selector/shadow_v2_l2_d3_density_ready_validation_summary.csv
```

## Final Verdict

```text
final_verdict=PR55_AS_L2_D3_CONTRACT_FIXTURE_ACCEPTED
runtime_density_verdict=PR55_AS_RUNTIME_DENSITY_EMISSION_PROOF_NOT_ACCEPTED
density_verdict=L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED
runtime_density_emission_proof=false
density_contract_fixture_pass=true
density_fixture_l2_f_allowed_next=false
l2_f_allowed_next=false
L2_F_ALLOWED_NEXT=false
next_stage=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```
