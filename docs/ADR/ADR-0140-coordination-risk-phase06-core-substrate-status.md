# ADR-0140: Coordination Risk Phase 0.6 core substrate status

**Date:** 2026-05-31
**Status:** Accepted as corrected core substrate; Phase 0.6 not fully closed
**Author:** Codex, based on Antycabal Repair implementation and review
**Follows:** `ADR-0138-fsc-v2-nln-program-streams-capture-evidence.md`, `ADR-0139-fsc-v2-pr8-runtime-capture-state.md`
**Plan:** `PLANS/PLAN_ANTYCABAL_PHASE_0_6_FINAL_20260531.md`

## Context

`PLAN_ANTYCABAL_REPAIR.md` defined a repair path for early coordination-risk metrics around
anti-cabal evidence. The first foundation tasks had already been implemented before this ADR:

- `ObservedBuyTx` carried decision-time ordering, spend, price, compute and fingerprint shells.
- `is_buyer_sample_candidate()` excluded failed, non-buy, sell, unknown direction and dev
  create/init transactions from buyer samples.
- `unique_first_buys_by_signer()` selected a deterministic first buy per signer.
- `sequence_buys()` required `slot_index` for sequence metrics and failed on missing or duplicate
  causal positions.
- `MetricValue`, `MetricEvidenceStatus`, `FundingVisibility`, `CoordinationRiskFeatures` and
  `CoordinationRiskConfig` existed as inert evidence/config shells.
- HHI, diversity, Tau-b, CV and robust CV helpers existed in the coordination statistics module.

After FSC v2 NLN capture became available, the original proxy replacement stack in the old plan
became obsolete. In particular, CTC, CPCR and ETC were no longer valid priorities as a
"funding-blind replacement stack" for FSC. They may return later as orthogonal features, but not as
Phase 0.6 replacements for FSC v2.

The resulting Phase 0.6 objective is therefore narrow:

```text
Repair coordination-risk feature definitions.
Keep them export-only and additive.
Do not change active Gatekeeper behavior.
Do not claim selector or promotion readiness.
```

## Decision

Accept the current `ghost-core::features::coordination` work as a corrected core substrate for
Phase 0.6, with explicit residual gaps. This ADR does not close the entire Phase 0.6 program.

The accepted state is:

```text
CORE_SUBSTRATE_STATUS = corrected_partial_pass
ACTIVE_GATEKEEPER_IMPACT = none
MATERIALIZED_FEATURE_SET_EXTENSION = none
DECISION_LOGGER_RUNTIME_SIDECAR = not_implemented
RUNTIME_FROZEN_BUFFER_HOOK = not_implemented
BEHAVIORAL_NO_POLICY_DRIFT_GATE = not_implemented
SELECTOR_PROMOTION_CLAIM = forbidden
```

## Implemented Core Substrate

The following PR-AC areas are implemented or substantially implemented in `ghost-core` as pure,
export-only evidence APIs.

### PR-AC4: FTDI v2

Implemented as HHI-based diversity over fee topology fingerprints:

```text
count_by(fee_topology_fp)
hhi_norm = normalized_hhi_from_counts(counts)
ftdi_v2 = diversity_from_hhi_norm(hhi_norm)
```

Properties:

- sample is the clean buyer sample from `unique_first_buys_by_signer()`;
- clean value requires `min_unique_buyers_for_diagnostics`;
- fingerprint coverage is explicit;
- low coverage returns degraded/unavailable evidence, not a clean value over a partial subset;
- policy mode is `ExportOnly`.

### PR-AC5: DBIA v2

Implemented with a separate dev reference object instead of pretending that dev create/init
transactions are ordinary buyer samples.

Properties:

- `compute_dbia_v2()` takes `buyer_samples` and `Option<DevFingerprintEvidence>`;
- `DevFingerprintMode::NotComparable` returns `None + DevTxNotComparable`;
- `DevFingerprintMode::CreateTxSwapSliceOnly` requires `explicit_swap_slice=true`;
- pure comparable dev buy confidence is `1.0`;
- explicit swap-slice confidence is `0.6`;
- buyer fingerprint coverage is explicit;
- policy mode is `ExportOnly`.

### PR-AC6: SFD v2

Implemented with an economic-spend hierarchy:

```text
economic_spent_lamports
-> decoded_buy_sol_lamports
-> curve_sol_delta_lamports
-> signer_delta_minus_known_overheads
```

Properties:

- signer delta is a degraded fallback;
- fallback subtracts known fee lamports when available;
- missing fee/cost metadata is reported as degraded evidence;
- missing or zero pre-balance fails the sample item;
- outlier spend fraction is degraded and skipped instead of silently clamped;
- coverage and source confidence are included in evidence;
- policy mode is `ExportOnly`.

### PR-AC7: FSC v2 compatibility

Partially implemented in `ghost-core`.

Implemented:

- `FundingVisibility::from_fsc_v2_lane_health()` maps lane health only;
- FSC metric quality does not drive `FundingVisibility`;
- low coverage, neutral-only or insufficient non-neutral support can leave the funding lane
  `Available` if capture/index health is otherwise good;
- `funding_source_concentration_from_fsc_v2()` exports FSC only when the evidence is:
  - decision-time;
  - clean;
  - no gap suspected;
  - capture ready;
  - index warm;
  - `hhi_norm_count = Some(...)`;
  - no excluded reason.

Not implemented:

- runtime integration into a coordination-risk sidecar row;
- any active policy use;
- any replacement of legacy FSC semantics.

### PR-AC8: CPV v2

Implemented as signer-level intensity, not a binary "other pools > 0" flag.

Properties:

- one active signer cannot dominate the whole metric by itself;
- `SignerCrossPoolActivity` carries current-pool exclusion and cutoff proof fields;
- missing current-pool exclusion proof degrades evidence;
- activity observed after cutoff degrades evidence;
- rolling state unavailable returns unavailable evidence;
- policy mode is `ExportOnly`.

Residual contract:

```text
missing SignerCrossPoolActivity currently means verified zero only if the runtime rolling index
declares sparse_verified_zero mode.
```

The runtime integration PR must explicitly choose either:

```text
sparse_verified_zero:
  missing signer entry means verified zero at cutoff

full_proof:
  missing signer entry is degraded / low coverage
```

No runtime integration may use implicit missing-as-zero without declaring the mode.

### PR-AC9: DES fixed export-only

Implemented as a causal sequence metric using `sequence_buys()` and Tau-b.

Properties:

- no arrival-time ordering;
- missing `slot_index` returns unavailable evidence;
- duplicate `(slot, slot_index)` returns unavailable evidence;
- requires enough pairs for Tau-b;
- same-slot dominated sequences are degraded;
- invalid price evidence fails/degrades;
- policy mode is `ExportOnly`.

### PR-AC10: BSE export-only

Implemented with the intended offset:

```text
x[j] = price_impact[j]
y[j] = economic_spent[j + 1]
tau = kendall_tau_b(x, y)
```

Properties:

- uses `sequence.windows(2)`;
- requires at least four sequence transactions, yielding at least three pairs;
- exports both `tau_b_raw` and `tau_b_abs`;
- missing spend or price evidence degrades evidence;
- policy mode is `ExportOnly`.

### PR-AC11: CUCD export-only

Implemented as compute-unit dispersion diagnostics.

Properties:

- primary value is robust CV of `compute_units_consumed`;
- low value means homogeneous compute footprint and can be suspicious;
- high value is not positive organic evidence;
- bucket HHI and dominant bucket share are exported;
- missing/low compute-unit coverage degrades evidence;
- policy mode is `ExportOnly`.

### PR-AC12: Additive evidence schema

Substantially implemented as a `ghost-core` schema and builder contract.

Implemented:

- `MetricPolicyMode` separates policy mode from evidence status;
- `MetricEvidenceRecord<T>` carries evidence status, policy mode, score eligibility, degraded
  reasons and metric-specific breakdown;
- `CoordinationRiskEvidenceUnit` carries join fields:
  - `schema_version`;
  - `scope_id`;
  - `run_id`;
  - `candidate_id`;
  - `pool_id`;
  - `mint`;
  - `decision_id`;
  - `decision_ts_ms`;
  - `decision_slot`;
  - `feature_cutoff_ts_ms`;
  - `feature_cutoff_slot`;
  - `source_buffer_watermark_slot`;
  - `computed_at_recv_ts_ns`;
  - `gatekeeper_version`;
  - `source_snapshot_hash`;
- field name `metric_breakdowns` is canonical, with serde alias for legacy `breakdowns`;
- `skipped_metrics` exists at top level and inside metric breakdowns;
- source snapshot hash is required for clean payload;
- missing snapshot or missing frozen proof fail-closes;
- builder sanitizes `total_coordination_penalty` and `interaction_penalty`;
- CTC, CPCR and ETC are represented as skipped/not-configured.

Not implemented:

- runtime frozen-buffer capture in `ghost-launcher`;
- runtime JSONL sidecar writer;
- durable sidecar file naming and write health;
- replay join validation against real decision logs.

### PR-AC13: Tests and guards

Partially implemented.

Implemented:

- synthetic/unit tests for FTDI, DBIA, SFD, CPV, DES, BSE, CUCD;
- tests for FSC lane-health vs metric-quality separation;
- tests for sidecar frozen proof fail-closed behavior;
- tests that sidecar sanitizes penalties and skips proxy stack;
- core guard that `MaterializedFeatureSet::default()` does not contain coordination-risk fields or
  penalty payloads.

Not implemented:

- behavioral Gatekeeper V2/V2.5 no-policy-drift test comparing:
  - `coordination_risk.enabled=false/export_only=true`;
  - `coordination_risk.enabled=true/export_only=true`;
- no proof yet for identical:
  - verdict;
  - reason chain;
  - soft points/score;
  - size multiplier;
  - timeout handling.

## Explicitly Not Implemented

The following remain forbidden or out of scope for this stage:

- CTC as FSC proxy;
- CPCR as FSC proxy;
- ETC as FSC proxy;
- funding-blind replacement stack;
- `coordination_penalty`;
- `interaction_penalty`;
- reject threshold tuning;
- size-down threshold tuning;
- active Gatekeeper scoring;
- active V3 scoring;
- BUY/REJECT/TIMEOUT behavior changes;
- `MaterializedFeatureSet` active payload changes;
- R2 or selector readiness claims;
- metric usefulness claims;
- precision lift claims;
- promotion readiness claims.

## Current Problems

The implementation is not complete Phase 0.6 for two structural reasons.

### Problem 1: No runtime frozen-buffer hook

`ghost-core` now has `FrozenCoordinationDecisionSnapshot` and fail-closed sidecar builders, but
`ghost-launcher` does not yet create such a frozen input at the decision cutoff.

The missing runtime work is:

1. define the exact freeze point around terminal decision materialization;
2. copy the decision-time transaction buffer and relevant metadata;
3. derive source snapshot hash from the frozen input;
4. prove the sidecar is computed from frozen data, not terminal mutable state;
5. write the sidecar JSONL row durably without changing Gatekeeper behavior.

### Problem 2: No behavioral no-policy-drift gate

The core tests prove absence of direct field wiring and penalty payloads. They do not prove that
future runtime integration leaves active Gatekeeper behavior unchanged.

The missing behavioral test must use the same fixture and compare:

```text
coordination_risk.enabled=false/export_only=true
vs
coordination_risk.enabled=true/export_only=true
```

Required equality:

- verdict;
- reason chain;
- hard fail reason;
- soft signal points or score;
- size multiplier;
- timeout handling;
- logged decision route.

## Validation State

The corrected core substrate was validated with:

```text
cargo fmt --package ghost-core --check
git diff --check
cargo check -p ghost-core
cargo test -p ghost-core --test coordination_samples_pr1 --test coordination_evidence_pr2 --test coordination_stats_pr3 --test coordination_metrics_phase06
```

Observed result:

```text
52 targeted tests passed, 0 failed
```

`cargo check -p ghost-core` still reports existing warnings outside this change set, mainly in
legacy/deprecated `shadow_ledger` paths and unused variables. These warnings are not introduced by
the coordination-risk substrate work.

## Consequences

The current implementation is safe to keep as an inert core substrate because:

1. no active Gatekeeper policy file consumes the new APIs;
2. `MaterializedFeatureSet` is not extended with `coordination_risk`;
3. no decision penalty is emitted by the core builders;
4. no threshold tuning is introduced;
5. missing evidence is represented as `None`, degraded or unavailable, not clean `0.0`;
6. CTC, CPCR and ETC remain skipped/not-configured;
7. FSC v2 remains separate evidence and is not replaced by a proxy stack.

The current implementation is not enough for Phase 0.6 completion because:

1. no runtime JSONL sidecar is emitted;
2. no frozen decision-time buffer is captured in `ghost-launcher`;
3. no replay join is proven;
4. no Gatekeeper behavioral no-drift test exists;
5. no selector dataset joins these fields yet.

## Required Next PR

The next bounded PR should be:

```text
PR-AC12-runtime-sidecar
```

Scope:

1. add a frozen decision-time coordination snapshot boundary in `PoolObservationSession` or the
   terminal materialization path;
2. compute `CoordinationRiskEvidenceUnit` from that frozen snapshot;
3. write an additive JSONL sidecar;
4. include join metadata needed by candidate universe, feature snapshots and selector training
   views;
5. keep `MaterializedFeatureSet` unchanged;
6. keep Gatekeeper behavior unchanged;
7. add a behavioral no-policy-drift test.

Out of scope for that PR:

- active scoring;
- threshold search;
- size-down policy;
- FSC proxy stack;
- selector usefulness claim;
- R2 readiness claim.

## Non-Goals

This ADR does not authorize:

- activating coordination-risk policy;
- activating FSC v2 policy;
- using coordination evidence as a hard reject;
- using coordination evidence as a size multiplier;
- adding coordination-risk fields to `MaterializedFeatureSet`;
- changing V2/V2.5/V3 Gatekeeper decisions;
- reviving CTC, CPCR or ETC as FSC replacement;
- claiming that any metric improves precision;
- claiming selector readiness;
- claiming promotion readiness.

## Final Boundary

The correct current label is:

```text
Phase 0.6 core substrate: corrected partial PASS
Phase 0.6 runtime evidence: not complete
Phase 0.6 policy activation: forbidden
Phase 1 selector readiness: not claimed
```
