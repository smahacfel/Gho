# ADR-8D: Shadow V2 L2-D3B Runtime Harness Density Emission Proof 20260705

## Status

Accepted implementation stage, stacked after PR55/L2-D3 contract fixture.

## Decision

Dodajemy L2-D3B jako oddzielny proof, ze Shadow V2 validation harness potrafi
wyprowadzic density evidence z canonical path sample streamu:

```text
final_verdict=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F
runtime_harness_density_emission_proof=true
density_derivation_from_canonical_rows=true
density_rows_written_directly=false
live_runtime_density_emission_proof=false
l2_f_allowed_next=true
```

`l2_f_allowed_next=true` oznacza tylko, ze density gate po PR55 i L2-D3B nie
blokuje juz rozpoczecia osobnego L2-F research validation run. Nie oznacza L2
approval, research grade, live equivalence ani runtime approval.

## Context

PR54 naprawil retention config dla `standard_120s`, ale historyczny scope nadal
nie mial wystarczajacej liczby path samples. PR55 dostarczyl tylko
deterministyczny density contract fixture, wiec nie mogl byc uznany za runtime
density emission proof.

L2-D3B domyka brakujacy krok pomiedzy fixture a L2-F:

```text
ShadowPathSampleV2
  -> ShadowV2ValidationHarness::append_record
  -> shadow_position_event_v2.jsonl
  -> shadow_path_density_v2.jsonl
  -> shadow_v2_path_density_horizon_audit.py
```

## Implemented Contract

Wprowadzono offline example:

```text
ghost-brain/examples/shadow_v2_l2_d3b_runtime_harness_density_scope.rs
```

Example generuje D3B validation scope przez publiczny
`ShadowV2ValidationHarness`. Nie dotyka Gatekeeper, BUY/REJECT, selector
runtime, TX/Jito/live path ani provider streams.

W `ghost-brain/src/guardian/post_buy/shadow_v2.rs` dodano regresyjny test:

```text
shadow_v2_l2_d3b_harness_derives_density_from_canonical_path_samples
```

Test dowodzi, ze:

- `standard_120s.max_horizon_ms=121000`;
- `ShadowPathSampleV2` trafia do canonical `shadow_position_event_v2`;
- density rows sa wyprowadzane przez harness;
- final high watermark ma wszystkie declared horizons jako `EVALUABLE_EXACT`;
- 300s/500s pozostaja poza configured replay/horizon path.

## Audit Semantics

`shadow_path_density_v2.jsonl` jest snapshot streamem. Wczesne snapshoty sa
niemature i nie powinny blokowac finalnego D3B density gate, gdy finalny
snapshot dla tego samego `position_id+horizon` jest juz kompletny.

Dlatego `scripts/shadow_v2_path_density_horizon_audit.py` dostal jawny tryb:

```text
--latest-density-per-position-horizon
```

Default D2 row-level audit pozostaje bez zmian. Tryb latest-snapshot jest
wymagany dla runtime/harness snapshot streams.

## Result

```text
run_id=shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1
duration_ms=121000
sample_interval_ms=1000
canonical_path_sample_rows=122
density_rows_input=854
density_rows_evaluated=7
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_density_blocker_count=0
declared_horizon_path_coverage_blocker_count=0
declared_horizon_retention_blocker_count=0
density_audit_verdict=L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F
```

## Rejected Alternatives

### Treat PR55 fixture as runtime proof

Rejected. PR55 wrote deterministic density contract rows and did not prove
harness emission.

### Commit raw generated JSONL scope as source artifact

Rejected. The raw scope is a local validation artifact. The PR commits the
generator, tests, report, ADR, and summary CSV.

### Count every intermediate density snapshot as final density evidence

Rejected. This incorrectly makes early high watermarks block the final scope.
L2-D3B evaluates the latest snapshot per `position_id+horizon`.

### Start L2-F inside D3B

Rejected. L2-F remains a separate research validation run.

## Consequences

1. Density/retention is ready for L2-F from the D3B gate after PR55 and D3B are
   merged.
2. L2 is still not granted by this ADR.
3. Approval flags remain false.
4. L2-F must still independently prove research validation sample size,
   temporal, manifest, replay/lifecycle, Gatekeeper denominator, malformed-row
   and unknown-blocker gates.

## Verification

```bash
cargo test -p ghost-brain shadow_v2_l2_d3b_harness_derives_density_from_canonical_path_samples --quiet -- --nocapture
cargo run -p ghost-brain --example shadow_v2_l2_d3b_runtime_harness_density_scope --quiet -- --overwrite
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-l2-d3b-runtime-harness-density-emission-20260705-r1 --latest-density-per-position-horizon --pass-verdict L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F --output-csv reports/selector/shadow_v2_l2_d3b_runtime_harness_density_emission_summary.csv --pretty
```

## Final Decision

```text
runtime_harness_density_emission_proof=true
density_derivation_from_canonical_rows=true
density_rows_written_directly=false
l2_f_allowed_next=true
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```
