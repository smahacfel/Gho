# ADR-8D: Shadow V2 L2 Research-Grade Readiness Audit 20260703

## Status

Accepted as report-only audit candidate.

## Decision

Shadow V2 ma obecnie działający L1 deterministic diagnostic execution simulation, ale nie spełnia jeszcze L2 research-grade. Decyzja PR43-A0:

```text
L2_RESEARCH_GRADE_PATH_PRESENT
```

Ścieżka do L2 istnieje i nie wymaga L3 live calibration. L2 wymaga jednak domknięcia provenance, ordering, density, sample-size i audit gates.

## Context

Po PR33 + PR34-A/B/C + PR38-B + PR41/PR42 system potrafi wygenerować:

- entry diagnostic executable fill;
- exit diagnostic executable fill;
- terminal executable PnL;
- complete executable diagnostic roundtrip.

Ostatni smoke PR42 potwierdził:

```text
complete_executable_roundtrip_positions=28
terminal_truth_with_final_pnl_executable_bps_count=28
entry_execution_label_grade_DIAGNOSTIC_SIM_count=28
exit_execution_label_grade_DIAGNOSTIC_SIM_count=28
RESEARCH_CANDIDATE_count=0
LIVE_CONFIRMED_count=0
```

Temporal audit nadal zwraca:

```text
BLOCKED_TEMPORAL_AMBIGUITY_REMAINS
```

Path density audit nadal zwraca:

```text
BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS
```

## Evidence

Główne evidence:

- `reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.json`
- `scripts/shadow_v2_temporal_no_lookahead_audit.py`
- `scripts/shadow_v2_path_density_horizon_audit.py`
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
- `ghost-brain/src/guardian/post_buy/shadow_v2_execution.rs`
- `ghost-core/src/account_state_core/types.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`

Temporal evidence:

```text
event_order_key_present_rows=252
event_order_key_missing_required_rows=0
non_monotonic_event_seq_in_process=0
explicit_unknown_chain_order_components:
  block_time=252
  transaction_index_or_unknown=252
  instruction_index_or_unknown=252
  inner_instruction_index_or_unknown=252
  log_index_or_unknown=252
  signature=168
```

Provenance evidence:

```text
CanonicalPoolState lacks raw account bytes/hash.
AccountStateUpdate lacks raw account bytes/hash.
ShadowV2EntryBoundaryPayload has account_data_hash Option but runtime capture sets None.
PoolStateSampleV2::research_blockers() requires account_data_hash.
ShadowV2FillEngine can still produce DIAGNOSTIC_SIM with provenance blockers.
```

Density evidence:

```text
density_rows=1967
EVALUABLE_EXACT=0
EVALUABLE_APPROX=0
SPARSE_APPROX_ONLY=0
NOT_EVALUABLE_NO_COVERAGE=791
NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY=1176
path_points_median=1
path_points_max=1
```

## Consequences

1. `DIAGNOSTIC_SIM` remains valid for L1 deterministic simulation smoke evidence.
2. `RESEARCH_CANDIDATE` remains blocked until provenance and ordering blockers are cleared.
3. Lack of `account_data_hash` blocks research provenance but does not invalidate diagnostic deterministic fill.
4. Incomplete chain order blocks research-grade no-lookahead claims.
5. Current smoke density cannot support path/horizon research claims.
6. L3 live-equivalence remains explicitly out of scope for L2.

## Invariants

These flags remain false:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
```

No PR43-A0 runtime change is allowed. No burnin is allowed in this audit PR.

## Rejected Alternatives

### Treat L1 diagnostic fills as research-grade

Rejected. L1 fills can be formula-correct but still lack research provenance because of missing hash, incomplete chain order or insufficient density.

### Require L3 live calibration before L2

Rejected. L2 is offline research-grade shadow, not live-equivalent. Live calibration is required for L3, not for deterministic L2 if causal evidence is complete.

### Use event_seq_in_process as a substitute for chain order

Rejected. `event_seq_in_process` helps runtime ordering, but it is not a chain-order proof for same-slot ambiguity.

### Treat smoke path density as research density

Rejected. Smoke confirms wiring and shutdown. It does not provide enough path coverage for horizon research.

## Required Next Work

Recommended next PR:

```text
PR43-B: EventOrderKey research provenance wiring
```

Follow-up:

```text
PR44: account data hash provenance from ingest/reducer boundary
PR45: L2 path density/horizon contract
PR46: L2 research validation run and audit pack
PR47+: L3 live calibration dataset
```

## Acceptance Gates for L2 Candidate

L2 can only become a candidate after:

```text
entry/exit RESEARCH_CANDIDATE count > 0
account_data_hash present for boundary pool-state samples or typed non-applicable proof
temporal/no-lookahead audit PASS
path density evaluable for claimed horizons
manifest retention PASS
replay/lifecycle reconciliation PASS
terminal executable PnL exact links PASS
complete executable roundtrip sample-size gate PASS
unknown_or_untyped_blockers=0
```

## No-Runtime Boundary

PR43-A0 is report-only. It does not modify:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- R51;
- `shadow_close_only`;
- active close;
- runtime approval flags.

## Final Decision

```text
final_verdict=L2_RESEARCH_GRADE_PATH_PRESENT
research_grade=NOT_GRANTED
live_equivalence=NOT_GRANTED
runtime_approval=false
recommended_next_pr=PR43-B
```
