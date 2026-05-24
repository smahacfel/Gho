# ADR P3.7-L2D Gatekeeper V2 Axis Replay Contract

## Status

Accepted for L2D implementation planning.

## Context

P3.7-L2A validated a manifest-locked executable subset:

- allowed runs: `J4C`, `R16-r1`
- buy-quality denominator: `85`
- dirty-good rows: `4`
- blocked namespaces: `R16-r3..R16-r13`

P3.7-L2B correctly blocked causal axis ablation because no deterministic
Gatekeeper V2 counterfactual backend was available.

P3.7-L2C then verified that the input problem is not missing V3 replay payload:

- `85/85` denominator rows have full V3 replay payload
- `42/85` rows, all from J4C, lack `gatekeeper_gate_trace`
- baseline parity is only `41/85`
- `standard_mode_shorter_window` requires temporal replay snapshots
- diagnostic flags are not accepted as causal ablation

Therefore the gap is a Gatekeeper V2 axis replay contract gap. A V3
materialized payload proves what the sidecar saw, but it does not by itself
prove what Gatekeeper V2/V2.5 would have done under a single-axis config change.

## Decision

Do not attempt full five-axis replay at once.

Implement a minimal manifest-locked Gatekeeper V2 replay contract for
non-temporal axes first, and keep `standard_mode_shorter_window` unsupported
until temporal decision snapshots exist.

L2D must remain offline-only and must use the L1R21 manifest as the input SSOT.
It must not read current rollout configs, live runtime state, raw transaction
streams, or mutable account state.

## Required Baseline Parity

Every row used for causal axis replay must first pass baseline parity:

```text
baseline_replay_verdict == observed_verdict
baseline_replay_reason is compatible with observed reason
```

If the backend cannot reproduce the observed Gatekeeper V2/V2.5 decision for a
row under its observed config, that row is excluded from causal axis replay with:

```text
reason = baseline_parity_gap
```

No axis result may be reported as causal for a row that fails baseline parity.

## Minimal Replayable Axes

The first L2D backend may attempt only non-temporal axes:

- `soft_pdd_instead_of_hard_pdd`
- `prosperity_filter_disabled`
- `hhi_hard_fail_relaxed`
- `elapsed_aware_entry_drift`

`standard_mode_shorter_window` remains:

```text
unsupported_temporal_replay_required
```

until future runs emit canonical temporal snapshots such as:

- `features_at_2s`
- `features_at_3s`
- `features_at_5s`
- `features_at_7s`
- `features_at_10s`

or an equivalent ordered checkpoint sequence sufficient to replay the shorter
window decision.

## Required Row Fields

All non-temporal axes require:

- observed verdict and reason code
- `gatekeeper_gate_trace`
- `gatekeeper_first_kill_gate`
- `gatekeeper_terminal_gate`
- hard gate pass/fail status
- soft budget context where the axis can move hard PDD into soft penalty
- enough phase/core/diversity evidence to prove that non-axis gates still pass

Axis-specific requirements:

### soft_pdd_instead_of_hard_pdd

Required:

- `pdd_hard_fail`
- `soft_points`
- `max_soft_points`
- `pdd_soft_penalty_points` or an equivalent authoritative soft penalty field
- gate trace proving whether other hard gates pass

The variant may only convert PDD hard reject into BUY if the PDD gate is the
only remaining hard blocker and the soft budget remains within limit.

### prosperity_filter_disabled

Required:

- `prosperity_filter_enabled`
- prosperity gate trace status or authoritative prosperity reject trigger
- gate trace proving whether other hard gates pass

The variant may only remove the prosperity gate. Other hard failures still
block BUY.

### hhi_hard_fail_relaxed

Required:

- `hhi`
- hard HHI threshold evidence from row fields or gate trace
- `max_hhi`
- `top3_volume_pct`
- `same_ms_tx_ratio`
- phase/core diversity gate status

Relaxing the HHI hard wall does not remove top3, same-ms, or core diversity
failures.

### elapsed_aware_entry_drift

Required:

- `pdd_entry_drift_pct`
- `pdd_entry_drift_effective_max_pct`
- `pdd_entry_drift_threshold_source`
- `pdd_hard_fail`
- gate trace proving whether other hard gates pass

The variant may only remove entry-drift hard rejection when the logged drift is
within the logged elapsed-aware effective threshold.

## Source-Run Scope

Forward causal axis replay should be evaluated on rows whose source run did not
already contain the axis being tested.

With the current L1R21 manifest:

- J4C is the natural baseline source for forward R16 axes, but lacks
  `gatekeeper_gate_trace`.
- R16-r1 contains the full bundle, so its rows cannot prove that adding one
  axis caused the dirty-good emergence.

If only R16-r1 has sufficient trace data, L2D may report internal sensitivity,
but it must not label that as J4C-to-R16 causal ablation.

## Failure Modes

L2D must fail closed with explicit reasons:

- `BLOCK_L2D_INPUT_MANIFEST_CONTRACT`
- `BLOCK_L2D_DENOMINATOR_MISMATCH`
- `BLOCK_L2D_DIRTY_GOOD_MISMATCH`
- `BLOCK_L2D_BASELINE_PARITY_GAP`
- `BLOCK_L2D_MISSING_GATE_TRACE`
- `BLOCK_L2D_MISSING_AXIS_FIELDS`
- `BLOCK_L2D_TEMPORAL_REPLAY_REQUIRED`
- `BLOCK_L2D_NO_CAUSAL_AXIS_REPLAY_ROWS`

Unsupported axes must never be reported as causal.

## Non-Goals

- no runtime
- no new runs
- no threshold tuning
- no Phase B
- no P2/live
- no adding `R16-r3..R16-r13`
- no using diagnostic flag matrices as ablation results
- no standard/shorter-window replay without temporal snapshots

## Consequences

L2D can become a deterministic backend, but the current manifest may still
produce a blocked result if no row satisfies:

```text
manifest allowed run
AND executable lifecycle label
AND complete Gatekeeper V2 trace
AND baseline parity
AND source run did not already include the tested axis
```

That blocked result is acceptable. It means the project needs either richer
future Gatekeeper V2 replay payloads or temporal snapshots, not manual
interpretation of diagnostic flags.

