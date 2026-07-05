# Raport Shadow V2 L2-D2: density-retention repair 20260705

## Status

```text
final_verdict=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
stage=L2-D2_DENSITY_RETENTION_REPAIR
base_after_pr53=8e5b11b236a3b3a0b820bf41615660840ae3bc5b
runtime_decision_behavior_changes=NONE
shadow_evidence_retention_config_changes=YES
runtime_evidence_schema_changes=NO
new_provider_streams=NONE
l2_f_allowed_next=false
approval_flags=false
```

L2-D2 naprawia kontrakt audytu density/retention oraz usuwa jeden
konfiguracyjny blocker dla przyszlego validation scope: standardowy Shadow V2
path sampler zachowuje teraz probki do `121000ms`, czyli `120000ms` declared
horizon plus `1000ms` retention margin.

Ten stage nie przyznaje L2. Aktualny lokalny validation scope nadal nie ma
wystarczajacej liczby path samples, zeby zadeklarowane horyzonty byly
evaluable.

## Zakres

W zakresie:

- utrzymanie declared L2 baseline horizons:
  `2000, 3000, 10000, 30000, 120000`;
- utrzymanie long horizons jako undeclared:
  `300000, 500000`;
- explicit D2 PASS verdict:
  `L2_D2_DENSITY_RETENTION_READY_FOR_L2_F`;
- explicit blocker:
  `BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT`;
- CSV summary artifact z wymaganymi metrykami;
- shadow-only evidence retention margin dla standardowego 120s path sampler;
- testy kontraktowe dla CSV, path coverage blocker i sampler margin.

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

## Declared Baseline Horizons

Declared supported horizons:

```text
2000
3000
10000
30000
120000
```

Unsupported / undeclared long horizons:

```text
300000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
500000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

Retention contract:

```text
max_declared_horizon_ms=120000
retention_margin_ms=1000
retention_contract_ms=121000
required_replay_coverage_ms=121000
```

## Zmiany

### `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

`ShadowPathSamplerConfigV2::standard_120s()` zachowuje teraz probki do:

```text
max_horizon_ms=121000
```

To nie promuje 300s/500s. To tylko pozwala przyszlemu 120s baseline runowi
spelnic retention margin wymagany przez L2-D/L2-D2.

Dodany test:

```text
shadow_v2_standard_120s_sampler_retains_l2_baseline_margin_sample
```

Test potwierdza, ze:

- probka `age_ms=121000` zostaje zachowana;
- horizon `120000` moze byc `EvaluableExact`;
- `replay_horizon_ms=121000`;
- horizon `300000` nadal jest
  `NotEvaluableHorizonExceedsReplay` / poza standardowym mode.

### `scripts/shadow_v2_path_density_horizon_audit.py`

Audit zostal rozszerzony do D2:

- nowy PASS verdict:
  `L2_D2_DENSITY_RETENTION_READY_FOR_L2_F`;
- osobny blocker:
  `BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT`;
- aliasy metryk:
  `unsupported_horizons_ms`, `retention_contract_ms`,
  `required_replay_coverage_ms`;
- `l2_f_allowed_next`;
- explicit false approval flags;
- `--output-csv` dla artefaktu:
  `reports/selector/shadow_v2_l2_d2_density_retention_repair_summary.csv`.

### `tests/test_shadow_v2_path_density_horizon_audit.py`

Dodane lub zaktualizowane testy:

```text
test_declared_horizons_pass_and_long_horizons_are_non_blocking
test_no_path_coverage_blocks_even_when_declared_horizon_rows_exist
test_output_csv_contains_l2_d2_required_metrics
```

## Wynik Na Intended Validation Scope

Audit uruchomiony na:

```text
reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1
```

zwraca:

```text
density_audit_verdict=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
density_rows=7154
horizon_count=7
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_density_blocker_count=0
declared_horizon_path_coverage_blocker_count=5
declared_horizon_retention_blocker_count=0
l2_f_allowed_next=false
```

Per declared horizon:

```text
horizon_ms=2000   eligible_positions=130 evaluable_positions=0 coverage_ratio=0.0 samples_per_position_p50=1.0 samples_per_position_p90=1.0 retention_gap_count=1022 duplicate_sample_count=254 verdict=FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
horizon_ms=3000   eligible_positions=130 evaluable_positions=0 coverage_ratio=0.0 samples_per_position_p50=1.0 samples_per_position_p90=1.0 retention_gap_count=1022 duplicate_sample_count=254 verdict=FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
horizon_ms=10000  eligible_positions=130 evaluable_positions=0 coverage_ratio=0.0 samples_per_position_p50=1.0 samples_per_position_p90=1.0 retention_gap_count=1022 duplicate_sample_count=254 verdict=FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
horizon_ms=30000  eligible_positions=130 evaluable_positions=0 coverage_ratio=0.0 samples_per_position_p50=1.0 samples_per_position_p90=1.0 retention_gap_count=1022 duplicate_sample_count=254 verdict=FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
horizon_ms=120000 eligible_positions=130 evaluable_positions=0 coverage_ratio=0.0 samples_per_position_p50=1.0 samples_per_position_p90=1.0 retention_gap_count=1022 duplicate_sample_count=254 verdict=FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
```

`max_gap_ms_p90` i `max_gap_ms_max` sa puste, bo declared horizons nie maja
evaluable path coverage w tym scope.

## Interpretacja

L2-D2 zamyka konfiguracyjna dziure `120000ms` vs `121000ms`, ale historyczny
scope nie moze zostac naprawiony retroaktywnie. Obecne artefakty maja zbyt malo
path samples na pozycje, wiec wszystkie declared horizons pozostaja
nieewaluowalne.

To oznacza:

- nie wolno startowac L2-F na obecnym scope;
- nastepny validation scope musi emitowac path samples przez co najmniej
  `121000ms`;
- positive research claims moga dotyczyc tylko declared horizons po realnym
  `L2_D2_DENSITY_RETENTION_READY_FOR_L2_F`;
- 300s/500s pozostaja `NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE`.

## Required Artifact

CSV:

```text
reports/selector/shadow_v2_l2_d2_density_retention_repair_summary.csv
```

CSV zawiera wymagane metryki:

```text
declared_supported_horizons_ms
unsupported_horizons_ms
retention_contract_ms
required_replay_coverage_ms
horizon_ms
eligible_positions
evaluable_positions
coverage_ratio
samples_per_position_p50
samples_per_position_p90
max_gap_ms_p90
max_gap_ms_max
duplicate_sample_count
non_monotonic_sample_count
censored_count
horizon_unmatured_count
retention_gap_count
density_audit_verdict
```

## Weryfikacja

Wykonane checki:

```bash
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --output-csv reports/selector/shadow_v2_l2_d2_density_retention_repair_summary.csv --pretty
CSV parser check for reports/selector/shadow_v2_l2_d2_density_retention_repair_summary.csv
cargo test -p ghost-brain shadow_v2_standard_120s_sampler_retains_l2_baseline_margin_sample -- --nocapture
cargo fmt --check
```

## Final Verdict

```text
final_verdict=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
density_retention_verdict=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
l2_f_allowed_next=false
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```
