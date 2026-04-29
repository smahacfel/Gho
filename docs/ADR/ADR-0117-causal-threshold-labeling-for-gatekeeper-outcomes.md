# ADR-0117: Causal Threshold Labeling for Gatekeeper Outcomes

**Date:** 2026-04-29
**Status:** Accepted
**Author:** Ghost Father

## Context

`fetch_pool_price_at_30s.py` is used to label Gatekeeper BUY outcomes against a post-entry threshold target, especially the `+40%` / `-40%` decision analysis. The previous labeler could overstate or distort outcomes because it allowed the entry anchor to be selected from the nearest available price sample, including samples after the intended entry target or samples far away from it.

That is unsafe for Gatekeeper calibration:

- a future sample can leak post-entry movement into the entry price;
- a stale sample can move the entry baseline away from the actual decision point;
- starting the monitoring window from the matched sample instead of the intended entry time can shift the token lifecycle being measured.

## Decision

The threshold labeler now uses a causal entry-price contract:

1. Entry price from vectors is selected as the last sample with `sample_ts <= entry_target_ts`.
2. Future samples are not allowed to anchor entry.
3. Entry samples must be no more than `entry_max_match_delta_ms` stale; the default is `5000 ms`.
4. DIAG account-update lookup no longer falls back to an update after the requested target timestamp.
5. RPC signature fallback selects only block times less than or equal to the target timestamp.
6. Threshold monitoring starts at the intended `entry_target_ts_ms`, not at the matched entry sample timestamp.

The labeler records the match strategy through `*_match_selection` fields and invalidates old checkpoints by bumping `ANALYSIS_VERSION`.

## Architectural Impact

Touched surfaces:

- `logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py`
- `scripts/test_fetch_pool_price_at_30s.py`

This does not introduce any new production hot-path dependency. The script may still use RPC as an offline labeler fallback, but the Gatekeeper decision path remains Yellowstone-first and unchanged.

## Consequences

- Outcome coverage may drop because stale or future-only entry prices are rejected.
- Remaining labels are stricter and safer for replay calibration.
- Historical `pool_threshold_hits*.jsonl` generated with the old analysis version should not be mixed with new labels.
- `NONTARGET` and unresolved rates may change because the monitoring window now begins at the intended entry target, not at the selected price sample.

## Rollback

To restore the old behavior:

1. Revert the causal entry-selection changes in `fetch_pool_price_at_30s.py`.
2. Restore the prior `ANALYSIS_VERSION`.
3. Regenerate labels from scratch; do not reuse mixed-version checkpoints.

Rollback is not recommended for Gatekeeper calibration because the previous contract can leak future prices into labels.

## Validation

1. `python3 scripts/test_fetch_pool_price_at_30s.py`
2. `python3 -m py_compile logs/decisions.json/rollout/shadow-burnin/decisions/fetch_pool_price_at_30s.py scripts/test_fetch_pool_price_at_30s.py`
