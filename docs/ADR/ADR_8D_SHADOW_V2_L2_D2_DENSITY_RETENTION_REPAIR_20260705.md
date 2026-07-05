# ADR-8D: Shadow V2 L2-D2 Density Retention Repair 20260705

## Status

Accepted implementation stage.

## Decision

Wprowadzamy L2-D2 density-retention repair jako shadow evidence / offline audit
stage:

```text
final_verdict=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
```

Decyzja ma dwie czesci:

1. Standardowy `ShadowPathSamplerConfigV2::standard_120s()` zachowuje probki do
   `121000ms`, zeby deklarowany horizon `120000ms` mial wymagany `1000ms`
   retention margin.
2. Offline audit D2 rozdziela:
   - declared horizon incompleteness,
   - path sample coverage insufficiency,
   - retention contract insufficiency,
   - undeclared long horizons.

L2-F nie jest odblokowane, bo aktualny validation scope nie ma path sample
coverage dla declared horizons.

## Context

L2-D zdefiniowal declared baseline horizons:

```text
2000
3000
10000
30000
120000
```

oraz retention margin:

```text
required_replay_coverage_ms=121000
```

Przed L2-D2 standardowy path sampler zachowywal tylko `120000ms`, wiec nawet
poprawny przyszly 120s validation run moglby failowac przez brak 1s margin.
Jednoczesnie obecny historyczny scope
`reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1` ma tylko
szczatkowe path samples i nie moze byc retroaktywnie uznany za density PASS.

## Implemented Contract

Declared horizons:

```text
2000
3000
10000
30000
120000
```

Unsupported horizons:

```text
300000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
500000 = NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

Retention:

```text
retention_contract_ms=121000
required_replay_coverage_ms=121000
```

Allowed D2 verdicts:

```text
L2_D2_DENSITY_RETENTION_READY_FOR_L2_F
BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE
BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
```

## Code-Level Changes

### Shadow evidence sampler

`ShadowPathSamplerConfigV2::standard_120s()`:

```text
max_horizon_ms=121000
```

This is shadow evidence retention only. It does not change Gatekeeper policy,
BUY/REJECT, selector runtime, TX/Jito/live path, active close, or provider
subscriptions.

### Offline audit

`scripts/shadow_v2_path_density_horizon_audit.py` now emits:

- `unsupported_horizons_ms`;
- `retention_contract_ms`;
- `required_replay_coverage_ms`;
- `declared_horizon_path_coverage_blocker_count`;
- `l2_f_allowed_next`;
- explicit false approval flags;
- D2 CSV via `--output-csv`.

### Tests

Added/updated:

```text
test_no_path_coverage_blocks_even_when_declared_horizon_rows_exist
test_output_csv_contains_l2_d2_required_metrics
shadow_v2_standard_120s_sampler_retains_l2_baseline_margin_sample
```

## Rejected Alternatives

### Treat current PR18C scope as PASS after config repair

Rejected. The current scope lacks real path sample coverage. A config repair
cannot retroactively create chain/replay evidence.

### Promote 300s/500s into baseline

Rejected. Long horizons remain undeclared unless explicitly promoted in a
separate stage.

### Synthesize path samples from density rows

Rejected. Density rows are audit outputs, not source path observations.
Synthesizing path samples would create fake evidence.

### Tune Gatekeeper thresholds

Rejected. L2-D2 is density/retention repair. Gatekeeper denominator and
starvation policy remain separate.

## Consequences

1. Future standard 120s Shadow V2 validation scopes can satisfy the required
   `120000ms + 1000ms` retention margin if they emit actual path samples through
   that window.
2. Current historical scope remains blocked by path sample coverage.
3. L2-F is not allowed next from the current artifacts.
4. Approval flags remain false.

## Compatibility

No JSON schema change. No runtime decision behavior change. No provider stream
change. The only runtime code change is shadow evidence path-sampler retention
configuration.

## Verification

```bash
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --output-csv reports/selector/shadow_v2_l2_d2_density_retention_repair_summary.csv --pretty
CSV parser check for reports/selector/shadow_v2_l2_d2_density_retention_repair_summary.csv
cargo test -p ghost-brain shadow_v2_standard_120s_sampler_retains_l2_baseline_margin_sample -- --nocapture
cargo fmt --check
```

## Final Decision

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
l2_f_allowed_next=false
```
