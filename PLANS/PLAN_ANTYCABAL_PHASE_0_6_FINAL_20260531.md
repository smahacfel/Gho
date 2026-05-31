# Final plan: Phase 0.6 Coordination Risk Repair Export-Only

**Date:** 2026-05-31
**Status:** Final execution plan; core substrate implemented partially, runtime closure pending
**Related ADR:** `docs/ADR/ADR-0140-coordination-risk-phase06-core-substrate-status.md`
**Source plan:** `PLANS/PLAN_ANTYCABAL_REPAIR.md`
**Phase position:** after FSC Phase 0.5 capture/evidence, before Phase 1 selector dataset work

## Objective

Phase 0.6 repairs coordination-risk feature definitions before they enter feature snapshots,
datasets or selector analysis.

The objective is not to tune a new Gatekeeper. The objective is to produce cleaner, auditable,
decision-time-safe feature evidence.

```text
Primary goal:
  build export-only coordination-risk evidence substrate

Primary non-goal:
  do not change active BUY/REJECT/TIMEOUT/size behavior
```

## Background

The original Antycabal Repair plan included proxy metrics CTC, CPCR and ETC as a
"funding-blind replacement stack" for FSC. That assumption changed after FSC v2 NLN capture became
available.

The corrected strategy is:

```text
FSC v2 capture/evidence exists or is warming.
Do not replace FSC v2 with a proxy stack.
Keep FSC v2 separate from legacy FSC semantics.
Keep coordination-risk features additive and export-only.
Return to Phase 1 denominator/dataset work after this bounded repair.
```

## Current Foundation

The repo already had a useful inert foundation before Phase 0.6:

- `ObservedBuyTx` includes `slot_index`, time-source metadata, economic spend, price, compute unit
  and fingerprint shells;
- buyer sample selection excludes failed, non-buy, sell, unknown direction and dev create/init
  transactions;
- first-buy-per-signer selection is deterministic;
- sequence metrics require `slot_index` and fail on missing causal order;
- metric evidence status and metric values support unavailable/degraded/insufficient states;
- default config is inert: `enabled=false`, `export_only=true`;
- HHI, diversity, Tau-b, CV and robust CV utilities are present.

This plan builds on that foundation without changing active runtime decisions.

## In Scope

Phase 0.6 includes:

- FTDI v2;
- DBIA v2;
- SFD v2;
- FSC v2 compatibility;
- CPV v2;
- DES fixed export-only;
- BSE export-only;
- CUCD export-only;
- additive `CoordinationRiskFeatures`;
- metric breakdowns;
- JSONL-ready evidence unit;
- synthetic/unit tests;
- no-policy-drift guards at the core boundary.

## Out Of Scope

Phase 0.6 explicitly excludes:

- CTC;
- CPCR;
- ETC;
- funding-blind FSC replacement stack;
- active coordination penalty;
- active interaction penalty;
- reject threshold tuning;
- size-down threshold tuning;
- active Gatekeeper scoring;
- active V3 scoring;
- BUY/REJECT/TIMEOUT behavior changes;
- `MaterializedFeatureSet` active payload changes;
- selector usefulness claim;
- R2 readiness claim;
- promotion readiness claim.

## Global Invariants

Every PR in this plan must preserve these invariants:

1. active BUY/REJECT verdict drift is zero;
2. active size multiplier drift is zero;
3. Gatekeeper V2/V2.5/V3 do not read coordination-risk evidence for decisions;
4. `coordination_risk.enabled` defaults to `false`;
5. `coordination_risk.export_only` defaults to `true`;
6. new config/schema fields use serde defaults where backward compatibility matters;
7. missing metric evidence is `None`, unavailable, degraded or insufficient sample, never clean
   `0.0`;
8. DES and BSE are export-only;
9. CTC, CPCR and ETC remain skipped/not-configured;
10. FSC v2 is not replaced by a proxy stack;
11. sidecar evidence has enough join metadata for replay/dataset work;
12. no selector claim is made before denominator and R2 labels exist.

## Evidence Status vs Policy Mode

Metric data quality and policy usage are separate concepts.

Required model:

```text
MetricEvidenceStatus:
  Clean
  Degraded
  Unavailable
  InsufficientSample
  NotConfigured

MetricPolicyMode:
  ExportOnly
  ScoreEligible
  Disabled
```

`MetricEvidenceStatus::ExportOnly` may remain only as legacy compatibility. New code must prefer
`MetricPolicyMode::ExportOnly`.

For Phase 0.6:

```text
policy_mode = ExportOnly
score_eligible = false
```

## Funding Visibility Semantics

`FundingVisibility` describes FSC lane health only.

It must not describe whether the FSC metric has enough denominator support.

Correct mapping:

```text
gap_suspected=true
  -> GapSuspected

lane disconnected / no transfer lane
  -> Unavailable

index cold / TTL not warmed
  -> Warmup

lane connected + index warm + no gap
  -> Available
```

Metric quality cases such as:

- low coverage;
- neutral-only;
- unknown-only;
- insufficient non-neutral support;
- `hhi_norm_count = None`;

must affect `MetricValue.status`, breakdowns and degraded reasons, not lane health.

## PR-AC4: FTDI v2

Goal:

```text
Replace naive unique topology count with HHI-based fee topology diversity.
```

Required behavior:

- use clean first-buy-per-signer sample;
- count buyers by `fee_topology_fp`;
- compute `normalized_hhi_from_counts(counts)`;
- compute `diversity_from_hhi_norm(hhi_norm)`;
- report `fingerprint_coverage`;
- clean evidence only if coverage reaches configured threshold;
- partial fingerprint sample is degraded/unavailable, not clean subset evidence.

Implemented core status:

```text
implemented in ghost-core
policy impact: none
```

## PR-AC5: DBIA v2

Goal:

```text
Compare buyer infrastructure to a comparable dev reference, not to a full dev create/init tx.
```

Required behavior:

- compute API takes buyer samples plus `Option<DevFingerprintEvidence>`;
- dev create/init is not treated as ordinary buyer sample;
- `NotComparable` returns `None + DevTxNotComparable`;
- `CreateTxSwapSliceOnly` requires explicit adapter/slice proof;
- comparable pure buy confidence is higher than swap-slice confidence;
- missing dev reference is unavailable, not `0.0`.

Implemented core status:

```text
implemented in ghost-core
policy impact: none
```

## PR-AC6: SFD v2

Goal:

```text
Use economic spend, not raw signer balance delta, as the primary spend fraction source.
```

Required source priority:

```text
economic_spent_lamports
-> decoded_buy_sol_lamports
-> curve_sol_delta_lamports
-> signer_delta_minus_known_overheads
```

Required behavior:

- signer delta is degraded fallback;
- fallback subtracts known overheads when available;
- missing overhead metadata is degraded;
- `first_pre_balance` must exist and be greater than zero;
- spend fraction must be finite;
- outliers are degraded/skipped, not silently clamped.

Implemented core status:

```text
implemented in ghost-core
policy impact: none
```

## PR-AC7: FSC v2 Compatibility

Goal:

```text
Remove the funding-blind replacement assumption and keep FSC v2 evidence separate.
```

Required behavior:

- no `use_funding_blind_proxy_stack`;
- `FundingVisibility` maps lane health only;
- FSC metric availability maps to metric status/reasons;
- clean FSC v2 can export `funding_source_concentration`;
- degraded/warmup/gap/neutral/unknown/insufficient support returns `None`;
- legacy FSC field semantics are not silently redefined.

Implemented core status:

```text
core API implemented in ghost-core
runtime sidecar integration pending
policy impact: none
```

## PR-AC8: CPV v2

Goal:

```text
Use intensity per signer rather than binary other_pools > 0.
```

Required behavior:

- one active retail sniper cannot generate a strong pool-wide penalty;
- rolling signer activity must be queried at feature cutoff;
- current pool must be excluded;
- events after cutoff must be ignored;
- insufficient sample returns insufficient evidence;
- anti-leakage proof is required at runtime integration.

Implemented core status:

```text
core API implemented in ghost-core
runtime sparse/full-proof mode decision pending
policy impact: none
```

## PR-AC9: DES Fixed Export-Only

Goal:

```text
Repair causal ordering and use Tau-b.
```

Required behavior:

- use `sequence_buys()`;
- missing `slot_index` fails sequence evidence;
- duplicate causal position fails evidence;
- no signature fallback as causal proof;
- require at least three eligible pairs;
- invalid price evidence degrades/fails;
- same-slot dominated evidence is degraded.

Implemented core status:

```text
implemented in ghost-core
policy impact: none
```

## PR-AC10: BSE Export-Only

Goal:

```text
Measure relationship between current impact and next buy sizing.
```

Required formula:

```text
x[j] = price_impact[j]
y[j] = economic_spent[j + 1]
tau = kendall_tau_b(x, y)
```

Required behavior:

- require at least four ordered buys, producing at least three pairs;
- store `tau_b_raw`;
- store `tau_b_abs`;
- do not interpret direction before ablation;
- missing price/spend evidence is degraded/unavailable.

Implemented core status:

```text
implemented in ghost-core
policy impact: none
```

## PR-AC11: CUCD Export-Only

Goal:

```text
Export compute footprint dispersion as diagnostic evidence.
```

Required behavior:

- primary value is robust CV of compute units consumed;
- low value means homogeneous compute footprint and may be suspicious;
- high value is not positive organic evidence;
- include compute-unit bucket HHI;
- include dominant bucket share;
- low coverage degrades evidence.

Implemented core status:

```text
implemented in ghost-core
policy impact: none
```

## PR-AC12: Additive Evidence and JSONL Sidecar

Goal:

```text
Export coordination-risk evidence from a frozen decision-time snapshot.
```

Required sidecar schema:

- `schema_version`;
- `scope_id`;
- `run_id`;
- `candidate_id`;
- `pool_id`;
- `mint`;
- `decision_id`;
- `decision_ts_ms`;
- `decision_slot`;
- `snapshot_mode`;
- `feature_cutoff_ts_ms`;
- `feature_cutoff_slot`;
- `source_buffer_watermark_slot`;
- `computed_at_recv_ts_ns`;
- `gatekeeper_version`;
- `source_snapshot_hash`;
- `sample_summary`;
- `funding_visibility`;
- `features`;
- `metric_breakdowns`;
- `skipped_metrics`;
- `degraded_reasons`.

Required behavior:

- compute from frozen decision-time input;
- never compute from mutable terminal session state;
- missing frozen proof fails closed;
- source snapshot hash is required for clean payload;
- sidecar is additive and does not alter `MaterializedFeatureSet`.

Implemented core status:

```text
core schema and builder implemented in ghost-core
runtime frozen hook pending
runtime JSONL writer pending
policy impact: none
```

## PR-AC13: Guards and Tests

Goal:

```text
Prove export-only evidence does not change active decisions.
```

Implemented core tests:

- FTDI HHI diversity and coverage behavior;
- DBIA comparable modes and swap-slice confidence;
- SFD source priority, degraded fallback, zero balance and outlier behavior;
- CPV intensity and cutoff/current-pool proof;
- DES causal sequence behavior;
- BSE offset `j -> j+1`;
- CUCD homogeneous footprint diagnostics;
- FSC v2 lane health vs metric quality;
- sidecar frozen proof fail-closed;
- sidecar penalty sanitization;
- skipped CTC/CPCR/ETC;
- no new coordination fields in default `MaterializedFeatureSet` serialization.

Still required:

- behavioral Gatekeeper no-policy-drift test:

```text
same fixture
coordination_risk.enabled=false/export_only=true
vs
coordination_risk.enabled=true/export_only=true

must produce identical:
  verdict
  reason chain
  hard fail reason
  soft points/score
  size multiplier
  timeout handling
```

## Implementation Status Matrix

| Area | Status | Notes |
| --- | --- | --- |
| FTDI v2 | Core implemented | HHI diversity, coverage gate |
| DBIA v2 | Core implemented | Dev reference mode, explicit swap-slice |
| SFD v2 | Core implemented | Economic spend hierarchy |
| FSC v2 compatibility | Core partial | Runtime sidecar join pending |
| CPV v2 | Core partial | Runtime sparse/full-proof decision pending |
| DES fixed | Core implemented | Causal sequence + Tau-b |
| BSE | Core implemented | `impact[j] -> spend[j+1]` |
| CUCD | Core implemented | Diagnostic/export-only |
| Evidence schema | Core implemented | JSONL-ready, not runtime-written |
| Runtime frozen buffer | Not implemented | Required for Phase 0.6 closure |
| Sidecar writer | Not implemented | Required for Phase 0.6 closure |
| Gatekeeper no-drift | Partial | Core guard only; behavioral guard pending |
| CTC/CPCR/ETC | Skipped | Must remain not-configured |
| Active scoring | Forbidden | No threshold tuning |

## Acceptance Criteria

Core substrate acceptance:

1. `cargo check -p ghost-core` passes.
2. Coordination PR1-PR3 tests still pass.
3. Phase 0.6 synthetic tests pass.
4. New evidence structs are serde-compatible.
5. All new metric computations are export-only.
6. No active Gatekeeper code reads new coordination-risk evidence.
7. `MaterializedFeatureSet` is not extended.
8. Missing/degraded evidence is explicit.
9. CTC/CPCR/ETC remain skipped/not-configured.
10. No selector or promotion claims are made.

Full Phase 0.6 acceptance additionally requires:

1. runtime frozen decision-time snapshot hook;
2. additive JSONL sidecar writer;
3. join metadata validated against decision logs;
4. behavioral Gatekeeper no-policy-drift test;
5. no replay hash or active payload drift;
6. no policy/size/timeout drift.

## Validation Commands

Core validation command set:

```text
cargo fmt --package ghost-core --check
git diff --check
cargo check -p ghost-core
cargo test -p ghost-core --test coordination_samples_pr1 --test coordination_evidence_pr2 --test coordination_stats_pr3 --test coordination_metrics_phase06
```

Expected current targeted result:

```text
52 passed, 0 failed
```

Warnings from existing unrelated paths do not block this phase unless they come from
`ghost-core::features::coordination`.

## Next PR: Runtime Sidecar

The next implementation PR should be narrow:

```text
PR-AC12-runtime-sidecar
```

Tasks:

1. choose exact decision-time freeze boundary;
2. freeze the transaction buffer and relevant metadata at cutoff;
3. derive a stable `source_snapshot_hash`;
4. define CPV sparse/full-proof mode;
5. compute `CoordinationRiskEvidenceUnit` from frozen data;
6. write additive JSONL sidecar;
7. add replay/join smoke validation;
8. add behavioral no-policy-drift test.

Non-goals for that PR:

- no active policy;
- no threshold tuning;
- no selector claim;
- no R2 readiness claim.

## Promotion Boundary

Phase 0.6 PASS may mean:

- coordination-risk evidence can be computed/exported;
- missing/degraded evidence is explicit;
- FSC v2 remains separate evidence;
- CTC/CPCR/ETC remain skipped;
- no-policy-drift guards pass.

Phase 0.6 PASS does not mean:

- selector readiness;
- metric usefulness;
- precision lift;
- Gatekeeper activation;
- R2 readiness;
- promotion readiness.

## Current Verdict

The correct status after the implemented core repair is:

```text
core substrate: corrected partial pass
runtime sidecar: pending
Gatekeeper behavioral no-drift: pending
active policy: forbidden
return to Phase 1: after bounded runtime sidecar closure or explicit stop decision
```
