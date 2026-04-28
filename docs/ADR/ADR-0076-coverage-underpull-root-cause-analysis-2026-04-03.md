# ADR-0076: Coverage Underpull Root Cause Analysis 2026-04-03

**Date:** 2026-04-03
**Status:** Accepted
**Author:** Ghost Father

## Context
Coverage in `logs/decisions.jsonl/cov.py` remained materially below the 95% target after prior redesign work that aligned observed totals with `total_tx_evaluated`, wired exact-window rollout emissions, resolved runtime `min_sol_threshold`, added RPC-side dust filtering, and removed the fallback that fabricated coverage. A fresh 12-record cohort still produced `avg_coverage=0.8639520833333334` with 7 `overflow_observed_gt_onchain` rows.

## Decision
Treat the remaining undercoverage as primarily a signature window selection problem, not a base-mint resolution or dust-filter problem.

The investigation found:
- the dominant loss mode is strict `getSignaturesForAddress` `blockTime` windowing in `_collect_signatures`, which excludes many transactions emitted inside the Gatekeeper observation window because Solana `blockTime` is second-granularity and often lands 0.5-2.2s before the in-window runtime emission timestamp;
- mint-side fallback contributes almost no recovery in the failing cohort;
- classifier false negatives still exist for routed Pump.fun trades that do not expose a base-mint token balance delta, but this is a secondary issue;
- base mint inference, off-pool exclusion, and dust filtering are not dominant contributors in this cohort.

## Architectural Impact
The coverage scanner should stop treating `blockTime` as an exact canonical boundary when higher-fidelity runtime evidence already exists. Coverage logic, signature enumeration, and trade classification remain coupled to:
- rollout system-log exact-window emissions;
- `getSignaturesForAddress` address-selection strategy;
- routed Pump.fun / PumpSwap discriminator parsing.

This ADR shifts future remediation toward signature-source canonicalization and tolerant boundary handling rather than additional fallback heuristics.

## Risk Assessment
**Rate:** Medium

Regression risks:
- widening the selection boundary without a stronger post-filter could inflate `rpc_total_tx`;
- promoting rollout emission signatures to canonical inputs increases dependence on rollout log completeness;
- classifier broadening for routed trades can misclassify non-trade maintenance flows if pool-touch and discriminator guards are weak.

## Consequences
- Makes the next remediation step clearer: recover lost signatures first, then tighten classifier gaps.
- Avoids wasting effort on low-yield areas such as base-mint inference and dust logic.
- Requires careful validation because broader signature intake can raise false-positive risk if not paired with deterministic post-filtering.

## Alternatives Considered
- **Further tuning base-mint inference:** rejected because every sampled row already used `record.base_mint`.
- **Lowering dust thresholds or changing unresolved-volume handling:** rejected because dust filtered only 1 tx in the sample and unresolved volume did not explain the underpull.
- **Relying on mint-side `getSignaturesForAddress` fallback alone:** rejected because it added only 2 raw signatures across the 7 failing rows.

## Validation Steps
1. Re-run the 12-record cohort after changing signature selection logic and verify that `overflow_observed_gt_onchain` drops materially.
2. Specifically validate rows where exact-window emissions exceeded `rpc_total_tx`, with emphasis on first-second window entries.
3. Add regression cases for routed Pump.fun buys that emit `DISC_PUMP_BUY_ROUTED` / `DISC_SWAP_OUTER_WRAPPER` but lack base-mint balance deltas.
4. Confirm that improved recall does not raise `rpc_total_tx` above exact-window unique signatures in stable rows.
