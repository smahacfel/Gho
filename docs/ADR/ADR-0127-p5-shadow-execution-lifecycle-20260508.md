# ADR-0127: P5 Shadow execution lifecycle — implementation 2026-05-08

**Date:** 2026-05-08
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P5 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
close the shadow execution evidence chain — ephemeral payer for shadow_only,
idempotency keys for dedup, eventbus backpressure alerts, and lifecycle metrics
distinguishing `no_dispatch` from `failed_reconciliation`.

## Summary of work

1. **Ephemeral payer** — `ShadowPayerStrategy` enum with `Configured` (default) and
   `Ephemeral` variants. Enables `shadow_only` to run without `GHOST_TRIGGER_KEYPAIR_PATH`
   by generating a local keypair (B6: shadow_only does not depend on live payer).

2. **Idempotency key** — `make_shadow_idempotency_key(pool_id, join_key, rollout_profile)`
   using `blake3` hash. Added `idempotency_key: Option<String>` field to
   `ShadowBuySimulationRecord`. Deterministic per-pool dedup key.

3. **Eventbus backpressure** — `record_event_bus_lag()` now emits `tracing::warn!`
   when `skipped > EVENTBUS_DROP_WARN_THRESHOLD` (100). Per-consumer alert for
   on-call visibility.

4. **Lifecycle metrics** — `GATEKEEPER_SHADOW_LIFECYCLE_STATUS_TOTAL{status}` counter
   with labels `no_dispatch_eligible`, `no_dispatch_rejected`, `dispatched`,
   `failed_reconciliation`. `record_shadow_lifecycle_status()` helper.

## Decisions made

1. **`ShadowPayerStrategy::Ephemeral` is default-`Configured`** — backward-compatible.
   No existing payer loading code is changed. The ephemeral path is a new opt-in
   for shadow-burnin rollout.

2. **Idempotency key uses `blake3`** — same hasher already imported in `shadow_run.rs`.
   Key format: `hex(blake3(pool_id : join_key : rollout_profile))`. Non-empty,
   deterministic, hex-encoded.

3. **Backpressure threshold = 100** — alert when `skipped > 100`. Per the plan's
   per-minute monitoring with `tracing::warn!` for on-call.

4. **Lifecycle metrics separate `no_dispatch` from `failed_reconciliation`** —
   critical operational distinction per the plan (section 8.2). No dispatch is not
   a bug; failed reconciliation is.

## Files changed

| File | Change |
|------|--------|
| `ghost-launcher/src/components/trigger/shadow_run.rs` | Added `ShadowPayerStrategy` enum, `make_shadow_idempotency_key()`, `idempotency_key` field on `ShadowBuySimulationRecord` |
| `ghost-launcher/src/events.rs` | Added `EVENTBUS_DROP_WARN_THRESHOLD = 100`, `tracing::warn!` in `record_event_bus_lag` |
| `ghost-launcher/src/oracle_metrics.rs` | Added `GATEKEEPER_SHADOW_LIFECYCLE_STATUS_TOTAL` + `record_shadow_lifecycle_status()` |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | 3 P5 contract tests |

## Test results

- **23/23** `gatekeeper_v25_regression` tests pass (20 prior + 3 P5)
- **186/186** gatekeeper lib tests pass
- `ghost-launcher` compiles without errors

## DoD P5 checklist

- [x] `ShadowPayerStrategy::Ephemeral` defined + default = `Configured`
- [x] `make_shadow_idempotency_key()` deterministic hash function
- [x] `ShadowBuySimulationRecord` carries `idempotency_key`
- [x] Eventbus `tracing::warn!` when skipped > 100
- [x] Lifecycle metrics: `no_dispatch` vs `dispatched` vs `failed_reconciliation`
- [x] Test: `shadow_payer_strategy_ephemeral_exists`
- [x] Test: `shadow_idempotency_key_deterministic`
- [x] Test: `shadow_record_has_idempotency_key_field`
