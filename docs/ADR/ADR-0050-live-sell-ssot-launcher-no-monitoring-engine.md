# ADR-0050 — Live-Sell SSOT: Launcher owns the Revolver path; MonitoringEngine not used

**Date:** 2026-03-28
**Status:** Accepted
**Deciders:** Ghost team

---

## Context

After the dual live-run failures (3 runs with permanent token holdings, zero sell attempts),
the root cause was identified as:

- `PostBuyRuntime` unconditionally delegating every `PostBuySubmitted` event to
  `ghost_brain::PaperPositionLifecycle`, regardless of `lane`.
- `PaperPositionLifecycle` uses synthetic prices and a paper broker — it cannot submit
  real sell transactions.
- ghost-brain's `MonitoringEngine` / `Guardian` pipeline exists but was never wired to
  live BUY confirmations in the launcher.

The minimal fix introduced a `run_live_sell_lifecycle` function in
`ghost-launcher/src/components/post_buy_runtime.rs` that:

1. Calls `load_magazine_from_direct_buy` to arm the shared `Revolver` with pre-signed
   SELL bullets at TP levels derived from `entry_price`.
2. Polls bonding-curve price from `ShadowLedger` every 500 ms.
3. Fires triggered bullets via `rpc_client.send_transaction` with a background
   confirmation task.
4. Releases the bulkhead `position_slot_id` on lifecycle exit.

## Decision

**The launcher is the SSOT for live position exit. `MonitoringEngine` is not used for the live lane.**

Specifically:

- No ghost-brain `Guardian` session is created for live positions.
- No `MonitoringEngine::register_position` is called.
- The Revolver + bulkhead owned by `PostBuyRuntimeConfig::live_sell` is the
  authoritative exit path.

## Rationale

| Concern | Detail |
|---|---|
| Latency | `MonitoringEngine` is designed for paper-mode AEM analytics (multi-second ticks). Routing live sells through it would add latency to the price-check loop. |
| Coupling | wiring live on-chain submission through ghost-brain would couple the hot sell path to the analytics runtime and its lifecycle machinery. |
| Simplicity | The Revolver pattern (pre-signed bullets + price poll) is already self-contained. Adding a second coordinator would duplicate state and create race conditions. |
| Telemetry | Live sell telemetry is emitted via `tracing` (structured log events) rather than ghost-brain JSONL. If full AEM telemetry for live lanes is later needed, it can be added as a separate, non-blocking emitter — not by routing through MonitoringEngine. |

## Consequences

**Accepted:**
- Live positions do not appear in ghost-brain JSONL output.
- `MonitoringEngine` and `Guardian` are paper/analytics-only components.
- Sell confirmation is best-effort (background task, no lifecycle blocking).

**Future work (not in scope here):**
- A dedicated live-lane telemetry emitter (separate from paper JSONL) could be added
  if replay / analytics of live trades is needed.
- If MonitoringEngine is extended to support on-chain submission, this decision can be
  revisited.

## Files affected

- `ghost-launcher/src/components/post_buy_runtime.rs` — `run_live_sell_lifecycle`
- `ghost-launcher/src/main.rs` — `LiveSellHandle` construction
