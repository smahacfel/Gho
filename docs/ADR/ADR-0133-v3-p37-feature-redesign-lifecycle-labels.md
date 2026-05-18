# ADR-0133: V3 P3.7 Feature Redesign and Lifecycle-Aware Labels

Date: 2026-05-18

Status: Accepted

## Context

P3.6 has finished with enough evidence to close threshold-level calibration for
the current V3 feature family.

The measurement pipeline is no longer the blocker:

- V3 full replay payload works.
- Rust-first strict replay validator works.
- R10/R11/R13 all passed strict full replay.
- R13 delivered `2733` replay-stable V3 rows.
- Outcome labels were generated for R10/R11/R13.
- Combined calibration and feature separation audits were completed.

The resulting quality signal is weak:

- R10/R11/R13 combined protective ratio: `1.046012`
- R10/R11/R13 combined protective precision: `0.511244`
- R11/R13 recent protective ratio: `1.020734`
- R11/R13 recent protective precision: `0.505130`
- candidate `V3-P36-ORGANIC-RELAXED` unblocked `30` good vs `27` bad rows
  in all-set and `29` good vs `25` bad rows in recent-only.
- feature separation showed high overlap and weak AUC separation in
  good-vs-bad, manipulation, PENDING, and organic failure groups.

This is not a small-sample result anymore. It is evidence that the current
V3 selector family behaves mostly as a broad risk shield, not as a selective
sniper decision model.

## Decision

Close P3.6 threshold-level calibration for the current V3 feature family.

Open:

```text
P3.7 - Decision-Time Feature Redesign + Lifecycle-Aware Outcome Model
```

P3.7 will focus on better outcome truth and new decision-time-safe evidence,
not on further loosening current V3 thresholds.

Current V3 remains valuable as:

- risk shield baseline,
- audit layer,
- reason-code telemetry,
- replay/outcome tooling,
- ablation baseline,
- negative control.

Current V3 is not a P2 candidate and not a live policy candidate.

## P3.7 Governance Rules

These rules are mandatory for P3.7:

1. Outcome Label v2 must come before feature redesign conclusions.
2. Execution feasibility join must come before BUY-quality claims.
3. Temporal split is required; combined-only evidence is insufficient.
4. Fresh holdout is required before any candidate run.
5. Kill criterion is required: close V3 selector line if no stable separation.
6. Current V3 remains risk shield/audit layer, not selector candidate.

## Kill Criterion

P3.7 is the final evidence-driven attempt to determine whether V3 can become a
selector.

If, after all of the following:

- Outcome Label v2,
- execution feasibility join,
- new decision-time feature prototypes,
- temporal split over R11/R13,
- one fresh holdout run after an explicit pre-registered hypothesis,

there is no stable candidate with:

- clean BUY precision `>= 0.68` on holdout,
- enough selected rows for a meaningful precision estimate,
- controlled MAE,
- execution-feasible ratio above a separately defined minimum,
- stability across R11, R13, and holdout,

then the V3 selector line is closed.

In that case V3 remains:

- risk shield,
- audit layer,
- reason-code telemetry,
- negative control,
- replay/outcome tooling.

It must not continue as an indefinite selector research track.

## Temporal Split Requirement

Combined R10/R11/R13 may be used for power and diagnostics only. It is not
sufficient as sole proof of edge.

P3.7 must report separately:

- R11 standalone,
- R13 standalone,
- recent-only R11/R13,
- future holdout,
- combined all only as secondary context.

Forbidden pattern:

```text
find feature on combined R10/R11/R13
declare edge on the same combined R10/R11/R13
```

That would be overfitting dressed as validation.

## Outcome Label v2 First

The current `+40 before stop` label is useful but too coarse for selective
decision validation.

It does not distinguish:

- executable vs non-executable opportunity,
- acceptable vs unacceptable MAE,
- fast spike vs sustainable path,
- clean hit vs dirty hit,
- viable exit vs trapped outcome,
- late or stale entry vs real decision-time opportunity.

P3.7 must design Outcome Label v2 before drawing conclusions from new feature
families.

Minimum v2 fields:

- `label_v2_schema_version`
- `entry_price`
- `entry_price_source`
- `entry_price_confidence`
- `mfe_pct_10s`
- `mfe_pct_30s`
- `mfe_pct_60s`
- `mae_pct_10s`
- `mae_pct_30s`
- `mae_pct_60s`
- `time_to_mfe_ms`
- `time_to_mae_ms`
- `hit_plus_20`
- `hit_plus_40`
- `hit_plus_60`
- `hit_stop_20`
- `hit_stop_40`
- `survived_10s`
- `survived_30s`
- `survived_60s`
- `exit_feasible`
- `label_quality`
- `unknown_reason`

Minimum v2 classes:

- `good_clean`
- `good_dirty`
- `bad_clean`
- `bad_dirty`
- `neutral_clean`
- `unknown`
- `execution_infeasible`

Feature selection should use clean classes first, especially `good_clean` vs
`bad_clean`.

## Execution Feasibility Join

Market outcome is not the same thing as executable opportunity.

P3.7 must join decision quality with execution feasibility before making BUY
quality claims.

Minimum execution fields:

- `decision_to_sim_ms`
- `quote_age_ms`
- `curve_age_ms`
- `simulation_success`
- `simulation_error_class`
- `compute_units`
- `shadow_entry_possible`
- `shadow_exit_possible`
- `no_dispatch_reason`
- `unknown_execution_status`

Minimum execution quality classes:

- `execution_feasible_clean`
- `execution_feasible_degraded`
- `execution_infeasible`
- `execution_unknown`
- `no_dispatch_expected`

Rules:

- `good outcome without feasible execution != true good entry`
- `bad outcome with impossible execution != pure decision failure`

## New Decision-Time Feature Families

P3.7 may explore new feature families, but only if they are decision-time-safe,
replayable, and eventually materializable through `MaterializedFeatureSet`.

Candidate families:

### Trajectory Shape

- `burst_decay_ratio`
- `second_wave_ratio`
- `pullback_recovery_score`
- `volume_followthrough_score`
- `buy_pressure_persistence`
- `interval_stability_after_burst`
- `hhi_recovery_or_decay`
- `top3_decay_after_t0`

### Sell Absorption / Micro Pullback

- `first_sell_time_ms`
- `first_sell_impact_pct`
- `buy_recovery_after_first_sell`
- `sell_cluster_absorption`
- `post_sell_buy_ratio`
- `post_sell_new_signer_ratio`

### Entry Timing / Too-Late

- `entry_drift_slope`
- `drift_since_t0_per_ms`
- `time_above_drift_cap`
- `late_edge_decay`
- `curve_progress_velocity`

### Buyer Continuity

- `new_signer_quality_t2`
- `repeat_buyer_quality`
- `buyer_retention_without_concentration`
- `fresh_buyer_followthrough`
- `micro_whale_followthrough`

### Risk Shield Residual

- `risk_shield_reason_family`
- `risk_shield_confidence`
- `risk_shield_evidence_quality`
- `risk_shield_subtrigger_count`
- `risk_shield_subtrigger_diversity`

These are hypotheses, not approved runtime features.

## Replay-First Validation

P3.7 must prototype offline before any runtime extension.

The first implementation layer should be reporting/extraction only, for example:

- `scripts/v3_p37_outcome_label_v2.py`
- `scripts/v3_p37_lifecycle_join_report.py`
- `scripts/v3_p37_feature_prototype_report.py`

Only after offline evidence shows stable separability may a separate plan propose
additive `MaterializedFeatureSet` extension.

Any future `MaterializedFeatureSet` extension must be:

- additive,
- backward compatible,
- `#[serde(default)]` where applicable,
- `Option<T>` / `skip_serializing_if` where appropriate,
- materialized at the correct SSOT boundary,
- replay-safe,
- not computed from mutable live state inside policy.

## Non-Goals

This ADR does not authorize:

- P2,
- live trading,
- R12 calibrated candidate,
- threshold tuning from existing V3 feature families,
- thresholds generated by `analiza_porownawcza.py`,
- ML classifier as policy,
- HyperPrediction/Chaos revival,
- FSC active gate/ranking under the current single-stream provider constraint,
- active V2/V2.5 policy changes,
- IWIM changes,
- live sender changes.

## Rejected Alternatives

### Continue threshold tuning current V3

Rejected.

P3.6 evidence shows weak separability and high overlap. Tuning current gates
would likely overfit.

### Run another blind sample

Rejected.

R13 already removed the small-sample objection. A new run is useful only after a
pre-registered P3.7 hypothesis.

### Use analyzer thresholds directly

Rejected.

`analiza_porownawcza.py` remains an appendix/hypothesis tool. Its Youden,
logistic, and scoring-rule sections cannot become runtime thresholds without
full replay ablation, outcome validation, temporal split, and separate approval.

### Promote current V3 to P2 as risk shield

Rejected.

Risk shield value is not the same as selective sniper edge. Current V3 may
remain telemetry/audit infrastructure, not active selector policy.

### Start with ML

Rejected for P3.7 initial phase.

The current issue is weak feature/outcome truth, not lack of model complexity.
ML on weak/coarse labels and weak features would amplify self-deception risk.

## Consequences

Positive:

- prevents indefinite V3 selector research without evidence,
- protects the project from threshold overfitting,
- shifts attention to executable opportunity quality,
- preserves useful V3 infrastructure,
- creates a falsifiable path for one final selector attempt.

Negative:

- delays any P2 discussion,
- requires labeler and report work before new runtime features,
- may conclude that V3 should remain audit/risk-shield infrastructure only.

## Success Criteria For Reopening Candidate Discussion

A future P3.7 candidate discussion requires all of:

- Outcome Label v2 generated,
- execution feasibility joined,
- temporal split passed,
- fresh holdout passed,
- selected rows sample large enough for precision estimate,
- clean BUY precision `>= 0.68` on holdout,
- MAE controlled,
- execution-feasible ratio acceptable,
- stable R11/R13/holdout behavior,
- no active policy change,
- no P2/live implication without separate ADR.

Until then, P2 remains blocked.

## Final Rule

P3.7 is not an attempt to keep V3 alive at all costs. P3.7 is the final
evidence-driven attempt to determine whether V3 can become a selector. If
label-v2, execution-feasible, temporal-holdout evidence still does not show
stable separation, V3 selector line is closed and V3 remains risk shield /
audit infrastructure only.
