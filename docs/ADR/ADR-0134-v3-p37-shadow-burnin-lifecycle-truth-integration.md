# ADR-0134: V3 P3.7 Shadow-Burnin Lifecycle Truth Integration

Date: 2026-05-19

Status: Accepted

## Context

P3.7 R10/R11/R13 audits correctly found no local executable-good proof inside
their primary-only replay namespaces. That conclusion was scoped to those
datasets, not to Ghost as a whole.

The repo still contained a first-class shadow-burnin runtime lane and the
historical `shadow-burnin-buy-heavy-rerun` artifacts. Phase E recovered that
lane into a shadow/on-chain lifecycle report:

- `2386` rows total.
- `2386` rows with `analysis_status=ok`.
- `2386` rows with `truth_status=resolved`.
- `2386` rows with `position_closed` and `exit_filled`.
- `579` positive lifecycle PnL rows.
- `1807` negative lifecycle PnL rows.
- `571` rows with Gatekeeper BUY context.

All recovered entry/exit curve finality values are speculative snapshots. This
is usable shadow/on-chain execution evidence, but it is not finalized proof and
not live inclusion.

Phase F labeled the recovered rows with separated axes:

- `market_outcome_class`
- `execution_verification_class`
- `truth_gap_class`
- `buy_quality_class`

The recovered label counts are:

- `market_good_clean = 579`
- `market_bad_clean = 1807`
- `shadow_onchain_speculative_snapshot_verified = 2386`
- `buy_quality_dirty_good = 579`
- `buy_quality_bad = 1807`
- `buy_quality_good = 0`

Phase H found decision-time features for a subset of the recovered lifecycle
labels:

- `feature_availability_status = v2_features_available`
- `phase_b_scope = diagnostic_v2_v25_feature_prototype_only`
- `v3_selector_prototype_possible = false`
- `buy_quality_dirty_good_with_features = 179`
- `buy_quality_bad_with_features = 559`
- `gatekeeper_context_dirty_good_with_features = 154`
- `gatekeeper_context_bad_with_features = 417`
- V3/MFS coverage is `0`.

P3.7-I then confirmed that the V2/V2.5 feature subset has diagnostic signal,
but only as historical design input. It does not authorize runtime thresholds
or V3 selector claims.

## Decision

Accept `shadow_burnin_lifecycle_onchain` as a separate P3.7 truth dataset kind.

This dataset kind may be used to label and audit shadow lifecycle rows, but it
must remain segmented from:

- primary-only market-path replay truth,
- live execution proof,
- V3/MFS selector evidence.

P3.7 reports and downstream tooling must treat the recovered shadow-burnin
lifecycle lane as:

```text
truth_dataset_kind = shadow_burnin_lifecycle_onchain
feature_generation = v2_v25_or_legacy unless V3/MFS payload is present
live_inclusion = false
```

## Evidence Classes

P3.7 must not collapse lifecycle truth into one mixed field such as
`good_executable`.

Minimum axes:

```text
market_outcome_class
execution_verification_class
truth_gap_class
buy_quality_class
```

`market_outcome_class` describes market/lifecycle result only.

`execution_verification_class` describes proof quality of shadow execution
against on-chain account-state truth.

`truth_gap_class` describes entry and exit truth timing quality.

`buy_quality_class` combines market outcome, execution verification, truth-gap
quality, lifecycle constraints, and policy context into a final diagnostic
label.

## Curve Finality Semantics

Speculative curve finality is not finalized proof.

Rules:

- `curve_finality=speculative` maps to
  `shadow_onchain_speculative_snapshot_verified`.
- `curve_finality=confirmed` maps to
  `shadow_onchain_confirmed_verified`.
- `curve_finality=finalized` maps to
  `shadow_onchain_finalized_verified`.
- missing or nonstandard finality maps to degraded or unknown execution proof.

Rows verified only by speculative snapshots can be useful for research labels,
but they must not be promoted to clean finalized BUY quality.

## Truth Gap Semantics

Entry and exit truth gaps are separate quality dimensions.

The labeler must keep at least:

```text
entry_truth_gap_class
exit_truth_gap_class
truth_gap_class
```

Exit gaps may be close-reason-aware. In the recovered dataset, exit gaps around
30 seconds are acceptable only as degraded lifecycle truth for TimeStop-like
closures, not as clean truth. Faster exits such as Target or StopLoss require
stricter scrutiny.

## Market vs Execution vs Buy Quality

`market_good_clean` means the lifecycle outcome was positive under resolved
truth. It does not imply execution proof finality or strategy edge.

`shadow_onchain_speculative_snapshot_verified` means shadow execution/lifecycle
matched on-chain executable state snapshots. It does not imply live inclusion.

`buy_quality_dirty_good` means the row is market-positive and execution-usable
for degraded research, but not clean finalized BUY quality.

`buy_quality_good` requires clean or accepted execution verification semantics.
In the recovered dataset it remains `0` because all rows are speculative.

## Dataset Segmentation

R10/R11/R13 primary-only replay datasets remain market-path truth datasets.
They must not inherit shadow-burnin lifecycle labels unless there is an
explicit run/config/session-level join proving the same rows and same policy
context.

Combined reports may show secondary aggregate counts, but primary conclusions
must remain segmented by:

- truth dataset kind,
- rollout namespace,
- config/policy generation,
- feature generation,
- Gatekeeper context availability,
- curve finality class,
- truth-gap class.

## Current Consequences

The recovered shadow-burnin dataset answers the old execution-proof blocker:
Ghost has a working shadow lifecycle/on-chain truth lane.

It does not unblock V3 selector prototype work because the recovered rows have
no V3/MFS payload coverage.

Allowed:

- diagnostic V2/V2.5 feature analysis on recovered lifecycle labels,
- using the findings to design a forward-only V3/MFS+lifecycle collection run,
- reporting `buy_quality_dirty_good` vs `buy_quality_bad` as degraded research
  labels.

Blocked:

- V3 selector prototype on this recovered dataset,
- P2/live promotion,
- threshold tuning,
- treating speculative snapshot rows as finalized clean BUY quality.

## Non-Goals

This ADR does not authorize:

- P2 or live behavior,
- active V2/V2.5 policy changes,
- IWIM changes,
- live sender changes,
- runtime threshold changes,
- MFS schema extension,
- FSC active gate under the single-stream constraint,
- treating shadow simulation as live inclusion,
- treating lifecycle labels as decision-time features.

## Rejected Alternatives

### Treat shadow simulation as live execution proof

Rejected. Shadow simulation can be compared with on-chain executable state, but
it is not a transaction signature, not confirmation, and not live inclusion.

### Promote speculative snapshots to finalized proof

Rejected. Speculative snapshot verification is useful but must remain degraded
or dirty unless separate confirmed/finalized evidence exists.

### Merge R10/R11/R13 market truth with shadow-burnin lifecycle truth

Rejected. These are different dataset kinds and namespaces. Merging them
without segmentation would recreate the earlier `good_executable` ambiguity.

### Start V3 selector prototype from V2/V2.5 feature coverage

Rejected. The recovered feature coverage is V2/V2.5/legacy, with `0` V3/MFS
coverage. It can guide the design of a new collection run, not stand in for V3
selector evidence.

## Acceptance Criteria

This ADR is satisfied when:

- P3.7 reports expose `shadow_burnin_lifecycle_onchain` separately from
  primary replay market-path truth.
- speculative finality is never reported as finalized proof.
- entry and exit truth-gap classes remain explicit.
- market outcome, execution verification, truth-gap quality, and buy-quality
  labels remain separate.
- V2/V2.5 recovered features are treated as diagnostic-only.
- V3 selector work remains blocked until V3/MFS lifecycle rows exist.
