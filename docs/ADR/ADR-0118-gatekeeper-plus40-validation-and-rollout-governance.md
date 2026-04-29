# ADR-0118: Gatekeeper +40% Validation and Rollout Governance

**Date:** 2026-04-29
**Status:** Accepted
**Author:** Ghost Father

## Context

Gatekeeper V2 already emits rich decision telemetry, but BUY was still primarily interpreted as passing a layered filter stack. For stricter autonomous execution, the target must be made explicit: accept only candidates with evidence of reaching at least `+40%` after the executable entry point while rejecting non-prospering pools, rugs, and sybil/cabal structures.

The previous process had three weaknesses:

1. `+40%` was not a first-class label contract.
2. Policy changes could be discussed without walk-forward / permutation / bootstrap validation.
3. Shadow promotion lacked a single fail-closed gate linking replay results to rollout.

## Decision

Gatekeeper calibration now follows a staged +40% governance pipeline:

1. Build causal labels with `scripts/gatekeeper_outcome_labeler.py`.
2. Validate labels with `scripts/gatekeeper_40pct_validation.py`.
3. Replay candidate policies with `scripts/gatekeeper_policy_replay_grid.py`.
4. Promote only through `scripts/gatekeeper_shadow_bake_gate.py`.
5. Keep runtime data-plane boundaries unchanged: no new RPC reads are allowed in the Gatekeeper hot path.

Accepted label contract:

- `hit_40_before_stop` is the primary positive class.
- `rug_or_early_death` is the main toxic negative class.
- invalid or non-causal entry labels are excluded from statistical validation.
- output labels must keep decision fields so policy replay can explain drift.

## Architectural Impact

Touched surfaces:

- `scripts/gatekeeper_outcome_labeler.py`
- `scripts/gatekeeper_40pct_validation.py`
- `scripts/gatekeeper_policy_replay_grid.py`
- `scripts/gatekeeper_shadow_bake_gate.py`
- `scripts/gatekeeper_config_ssot_check.py`
- `configs/shadow-burnin.toml`
- `configs/rollout/shadow-burnin.toml`
- `docs/ADR/ADR-0117-causal-threshold-labeling-for-gatekeeper-outcomes.md`

No production Gatekeeper policy threshold is changed by this ADR. Runtime activation remains a separate step after evidence is generated.

## Consequences

- Policy changes must be justified by labeled outcomes, not by raw correlation or narrative fit.
- A candidate policy can be rejected even if it looks stricter, unless precision, rug-rate, and permutation checks pass.
- Shadow bake is treated as the first activation lane. Paper/live promotion remains blocked until shadow evidence is stable.
- The process intentionally favors lower coverage over weak precision.

## Rollout Contract

Minimum promotion gates:

1. `precision >= min_precision`
2. `rug_rate <= max_rug_rate`
3. `selected >= min_selected`
4. permutation sanity `p_value <= max_permutation_p`
5. no config SSOT drift before replay/bake

Recommended command sequence:

```bash
python3 scripts/gatekeeper_config_ssot_check.py
python3 scripts/gatekeeper_outcome_labeler.py --decisions gatekeeper_v2_decisions.jsonl --threshold-hits pool_threshold_hits.jsonl --output gatekeeper_plus40_labels.jsonl
python3 scripts/gatekeeper_40pct_validation.py --labels gatekeeper_plus40_labels.jsonl --output gatekeeper_plus40_validation.json
python3 scripts/gatekeeper_policy_replay_grid.py --labels gatekeeper_plus40_labels.jsonl --output gatekeeper_plus40_replay.json
python3 scripts/gatekeeper_shadow_bake_gate.py --replay-report gatekeeper_plus40_replay.json --validation-report gatekeeper_plus40_validation.json
```

## Rollback

If the governance pipeline produces unstable or contradictory results:

1. Keep current runtime thresholds unchanged.
2. Treat new labels as research-only.
3. Regenerate labels after correcting data quality issues.
4. Do not promote sybil combo-veto, prosperity overlay, or soft-budget changes.

## Validation

1. `python3 scripts/test_fetch_pool_price_at_30s.py`
2. `python3 -m py_compile scripts/gatekeeper_outcome_labeler.py scripts/gatekeeper_40pct_validation.py scripts/gatekeeper_policy_replay_grid.py scripts/gatekeeper_shadow_bake_gate.py scripts/gatekeeper_config_ssot_check.py`
3. `python3 scripts/gatekeeper_config_ssot_check.py`
