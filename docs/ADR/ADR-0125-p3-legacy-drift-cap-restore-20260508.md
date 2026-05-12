# ADR-0125: P3 Legacy drift cap restore — tymczasowy safety cap 1.50

**Date:** 2026-05-08
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P3 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
close the blind spot where `max_price_change_ratio = 9999.0` effectively disables
the legacy hard-fail check HF-4 (`price_change_ratio > max_price_change_ratio`).

## Problem

- `max_price_change_ratio = 9999.0` — effectively disabled
- Path A (legacy) does not block drift. PDD entry-drift (5%) operates only in shadow
- Live legacy could permit BUY with +500% drift
- This violates the `ghost-execution` skill mandate: "Entry drift is a hard configured limit, not 9999.0"

## Decision

**Set `max_price_change_ratio = 1.50`** (+50% price change) as a temporary legacy safety cap.

This is NOT full PDD alignment (which would be 1.05 = +5% drift). The plan calls for
a gradual approach:

1. **Step 1 (this ADR):** `1.50` (+50%) — cuts only extreme pumps, preserves A-pool capture
2. **Step 2 (separate ADR, after validation):** tighten to `1.20` or `1.10`
3. **Step 3 (separate ADR, data-driven):** consider full alignment to `1.05` (PDD parity)

## Alternatives considered

| Value | Drift cap | Effect |
|-------|-----------|--------|
| `9999.0` | disabled | Current — blind spot, rejects nothing |
| `1.50` | +50% | **Chosen** — cuts extreme pumps, safe first step |
| `1.20` | +20% | Moderate — catches more but risks A-pool false negatives |
| `1.10` | +10% | Aggressive — near PDD alignment |
| `1.05` | +5% | Full PDD parity — requires outcome data to justify |

The `1.50` choice minimizes A-pool false negatives while definitively closing the
`9999.0` blind spot. A backfill audit on the latest scope should verify that pools
with `price_change_ratio > 1.50` are genuinely pathological.

## Gated bool alternative (not implemented)

The plan mentions an optional `legacy_drift_cap_enabled = true` gate with
`#[serde(default = "default_true")]`. This was not implemented in P3 because:

1. `1.50` is permissive enough that a gated toggle adds complexity without benefit
2. If future refinement requires a toggle, it can be added in Step 2/3 ADR

## Consequences

- Pools with `price_change_ratio > 1.50` (current/initial > 1.5x) will now be
  hard-failed by HF-4 in legacy live plane
- These are extreme pump pools — likely scams/traps, not A-pool capture candidates
- Data for further tightening comes from shadow-burnin V2.5 repair scope

## Files changed

| File | Change |
|------|--------|
| `ghost-brain/ghost_brain_config.toml` | `max_price_change_ratio = 9999.0` → `1.50` |

## Post-review: prosperity overlay alignment

Lines 143, 147 (`prosperity_overlay_max_price_change_ratio`,
`prosperity_overlay_branch2_max_price_change_ratio`) were also at `9999.0`.
Aligned to `1.50` for defense-in-depth. Note: `enable_prosperity_overlay = false`
means these are currently not active — alignment is preventive.

## Backfill audit — deferred to first clean rerun

**Scope:** `shadow-burnin-v25-repair-r2` (next clean rollout)
**Status:** Deferred — `shadow-burnin-v25-repair` scope is empty (no decision logs
available for backfill at this time).

The audit will be executed after the first clean rerun produces decision logs.
Methodology for the deferred audit:
```
jq 'select(.verdict_type == "REJECT_HARD_FAIL" and .decision_reason | contains("price_change_ratio"))' decisions.jsonl | wc -l
jq 'select(.phase6_price_change_ratio > 1.50)' decisions.jsonl | jq '{pool_id, price_change_ratio: .phase6_price_change_ratio, verdict: .verdict_type}'
```

Expected outcome: pools with `price_change_ratio > 1.50` should be pathological
(extreme pumps >50%), not legitimate A-pool candidates. If legitimate pools are
caught, the cap can be raised to `2.00` in a separate ADR (Step 2).

**Mitigation while deferred:** `1.50` is +50% price change — extremely permissive.
A-pool false negatives are unlikely at this threshold. Risk is minimal.
If a pool organically appreciates 50%+ in 8 seconds, it's almost certainly a pump.

## Alternative config alignment

`configs/rollout/ghost_brain_buy_heavy.local.toml` had the same `9999.0` blind spots.
All three fields aligned to `1.50`:
- `max_price_change_ratio = 1.50`
- `prosperity_overlay_max_price_change_ratio = 1.50`
- `prosperity_overlay_branch2_max_price_change_ratio = 1.50`

## DoD P3 checklist

- [x] `max_price_change_ratio = 1.50` w toml (legacy HF-4)
- [x] `prosperity_overlay_max_price_change_ratio = 1.50` (preventive)
- [x] `prosperity_overlay_branch2_max_price_change_ratio = 1.50` (preventive)
- [x] ADR w `docs/ADR/ADR-0125`
- [x] ADR explicite nazywa `1.50` jako tymczasowy legacy safety cap
- [x] Config parse test przechodzi
- [x] Test behawioralny: `p3_legacy_drift_cap_blocks_extreme_pump`
- [x] Test konfiguracyjny: `p3_config_has_legacy_drift_cap_1_50`
- [x] Backfill audit: deferred do pierwszego clean rerun `shadow-burnin-v25-repair-r2`. Metodologia + mitigacja w ADR. `1.50` jest wystarczająco permissive by nie ryzykować A-pool false negatives w międzyczasie.
