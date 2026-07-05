# Raport Shadow V2 L2-D3B: runtime harness density emission proof 20260705

## Status

```text
stage=L2-D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF
final_verdict=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F
base_dependency=PR55/L2-D3 density contract fixture
run_id=shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1
configured_run_seconds=121
duration_ms=121000
sample_interval_ms=1000
retention_contract_ms=121000
required_replay_coverage_ms=121000
density_rows_written_directly=false
runtime_harness_density_emission_proof=true
density_derivation_from_canonical_rows=true
live_runtime_density_emission_proof=false
l2_f_allowed_next=true
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

L2-D3B dowodzi tylko runtime/harness density emission path dla Shadow V2
validation harness. Ten etap nie jest L2-F, nie jest research validation run i
nie nadaje research-grade ani runtime approval.

## Scope

W zakresie:

- wygenerowanie trwalego D3B scope przez `ShadowV2ValidationHarness`;
- feedowanie `ShadowPathSampleV2` przez `ShadowV2ValidationHarness::append_record`;
- zapis canonical rows do `shadow_position_event_v2.jsonl`;
- wyprowadzenie `shadow_path_density_v2.jsonl` przez harness, a nie reczne
  pisanie density rows;
- potwierdzenie `standard_120s.max_horizon_ms=121000`;
- audit declared baseline horizons:
  `2000`, `3000`, `10000`, `30000`, `120000`;
- utrzymanie `300000` i `500000` jako
  `NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE`;
- brak zmian w Gatekeeper, BUY/REJECT, selector runtime, TX/Jito/live path i
  provider streams.

Poza zakresem:

- brak L2-F research validation run;
- brak Gatekeeper threshold tuning;
- brak runtime approval;
- brak research_grade;
- brak live_equivalence;
- brak strategy_research_unblocked;
- brak shadow_close_only;
- brak active_close.

## Runtime Harness Scope

Generator offline:

```bash
cargo run -p ghost-brain --example shadow_v2_l2_d3b_runtime_harness_density_scope --quiet -- --overwrite
```

Generator tworzy lokalny scope:

```text
reports/selector/shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1
```

Wygenerowane lokalne JSONL-e:

```text
shadow_position_event_v2.jsonl
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_path_density_v2.jsonl
d3b_runtime_harness_density_manifest.json
```

Surowe JSONL-e sa artefaktem walidacyjnym i nie sa wymagane jako tracked PR
artifact. Commitowany artefakt metryczny to:

```text
reports/selector/shadow_v2_l2_d3b_runtime_harness_density_emission_summary.csv
```

## Key Evidence

Runtime/harness emission proof:

```text
canonical_path_sample_rows=122
density_rows_input=854
density_rows_evaluated=7
latest_density_per_position_horizon=true
final_high_watermark=d3b-path-sample-121
density_rows_written_directly=false
density_derivation_source=ShadowV2ValidationHarness::append_record
```

Interpretacja:

- `122` canonical path sample rows trafilo do
  `shadow_position_event_v2.jsonl`;
- harness wyprowadzil `854` density snapshot rows, po `7` horizons na kazdy
  high watermark;
- audit D3B ocenia najnowszy density snapshot per `position_id+horizon`;
- finalne `7` rows zawiera `5` declared baseline horizons i `2` undeclared long
  horizons;
- to naprawia blad interpretacji z surowego snapshot streamu: wczesne,
  niemature snapshoty nie sa finalnym density gate.

## Density Audit

Audit command:

```bash
python3 scripts/shadow_v2_path_density_horizon_audit.py \
  --scope-root reports/selector/shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1 \
  --latest-density-per-position-horizon \
  --pass-verdict L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F \
  --output-csv reports/selector/shadow_v2_l2_d3b_runtime_harness_density_emission_summary.csv \
  --pretty
```

Aggregate result:

```text
density_audit_verdict=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_density_blocker_count=0
declared_horizon_path_coverage_blocker_count=0
declared_horizon_retention_blocker_count=0
malformed_density_rows=0
unknown_horizon_rows=0
runtime_harness_density_emission_proof=true
density_derivation_from_canonical_rows=true
live_runtime_density_emission_proof=false
l2_f_allowed_next=true
```

Declared horizon coverage:

```text
horizon_ms=2000   eligible_positions=1 evaluable_positions=1 coverage_ratio=1.0 samples_per_position_p50=122 max_gap_ms_max=1000 retention_gap_count=0 verdict=PASS
horizon_ms=3000   eligible_positions=1 evaluable_positions=1 coverage_ratio=1.0 samples_per_position_p50=122 max_gap_ms_max=1000 retention_gap_count=0 verdict=PASS
horizon_ms=10000  eligible_positions=1 evaluable_positions=1 coverage_ratio=1.0 samples_per_position_p50=122 max_gap_ms_max=1000 retention_gap_count=0 verdict=PASS
horizon_ms=30000  eligible_positions=1 evaluable_positions=1 coverage_ratio=1.0 samples_per_position_p50=122 max_gap_ms_max=1000 retention_gap_count=0 verdict=PASS
horizon_ms=120000 eligible_positions=1 evaluable_positions=1 coverage_ratio=1.0 samples_per_position_p50=122 max_gap_ms_max=1000 retention_gap_count=0 verdict=PASS
```

Long horizons:

```text
horizon_ms=300000 status=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE l2_baseline_blocker=false positive_research_claim_allowed=false
horizon_ms=500000 status=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE l2_baseline_blocker=false positive_research_claim_allowed=false
```

## Important Audit Semantics

`shadow_path_density_v2.jsonl` is a snapshot stream. When harness appends path
sample `N`, it also derives density rows for high watermark `N`. Early high
watermarks are expected to be immature for longer horizons.

For D3B, the audit uses:

```text
--latest-density-per-position-horizon
```

This evaluates the latest snapshot per `position_id+horizon`, selected by
`replay_horizon_ms`, `created_at_wall_ms`, and
`source_canonical_high_watermark`. The default D2 audit behavior remains
row-level and unchanged unless this flag is passed.

## Verification

```bash
cargo test -p ghost-brain shadow_v2_l2_d3b_harness_derives_density_from_canonical_path_samples --quiet -- --nocapture
cargo run -p ghost-brain --example shadow_v2_l2_d3b_runtime_harness_density_scope --quiet -- --overwrite
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1 --latest-density-per-position-horizon --pass-verdict L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F --output-csv reports/selector/shadow_v2_l2_d3b_runtime_harness_density_emission_summary.csv --pretty
```

## Final Verdict

```text
final_verdict=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F
l2_f_allowed_next=true
l2_f_allowed_next_scope=density_gate_only_after_PR55_and_D3B_merge
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```
