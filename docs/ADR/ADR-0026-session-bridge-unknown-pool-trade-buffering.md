# ADR-0026: Session Bridge Unknown-Pool Trade Buffering

**Date:** 2026-03-22  
**Status:** Superseded  
**Author:** Ghost Father  

## Context

Gatekeeper Phase-5 dev metrics in `gatekeeper_v2_buys.jsonl` showed a consistent contradiction in production-like gRPC runs: `dev_has_sold=true` and non-zero `dev_tx_ratio` / `dev_volume_ratio`, while `dev_buy_total_sol` remained `0.0` across buy-eligible logs.

Code analysis established that `dev_buy_total_sol` in `ghost-launcher/src/components/gatekeeper.rs` is populated only from the creator's canonical primary buy transaction, while `dev_has_sold` and the dev ratios are accumulated from all buffered transactions attributed to the creator wallet.

The launcher-side `SessionPoolTradeBridge` in `ghost-launcher/src/components/seer.rs` already contained a bounded pending-trade buffer with TTL and eviction logic, but its active ingress path did not use that buffer. Unknown-pool trades were immediately classified as `SilentDrop` and never replayed when `PoolDetected` arrived moments later.

This made the create/trade race window load-bearing: if the creator's first buy reached the launcher before `NewPoolDetected`, the trade was lost to Gatekeeper Phase-5 tracking, while later creator sells still updated `dev_has_sold=true`.

## Decision

Use the existing short-lived session pending-trade buffer for unknown-pool trades instead of immediately dropping them.

Specifically:

- `SessionPoolTradeBridge::ingest_trade(...)` now buffers unknown-pool trades with valid pool + mint identity and returns `BufferedPending`.
- The per-pool and global caps already present in the bridge are now enforced on actual inserts.
- `register_detected_pool(...)` remains the replay point; if `PoolDetected` arrives before TTL expiry, buffered trades are replayed in order.
- Trades with invalid identity (`pool_amm_id == default` or `mint == default`) remain unbuffered and are still dropped.
- A regression test now covers `Trade -> PoolDetected` ordering in the launcher-side bridge, in addition to the existing `PoolDetected -> Trade` path.

## Architectural Impact

This changes the launcher-side contract between Seer IPC ingress and Gatekeeper buffering:

- Before: unknown-pool trades were treated as irrelevant session noise and discarded immediately.
- After: unknown-pool trades are treated as potentially race-window-relevant for a bounded TTL window.

The change is intentionally local to `ghost-launcher` and does not alter:

- ShadowLedger replay math,
- Seer canonical trade production,
- Gatekeeper phase scoring formulas,
- protocol genesis or curve-state SSOT.

The main effect is that the creator's first buy is now much less likely to be lost before Gatekeeper can attribute it to the pool creator.

## Risk Assessment

**Rate:** Medium

Regression risks:

1. **Noise amplification risk** — global-stream trades for pre-session pools can now enter the short pending buffer instead of being dropped immediately.
2. **Memory/queue pressure risk** — mitigated by the existing TTL, per-pool cap, and global cap enforcement.
3. **Duplicate replay risk** — mitigated by existing dedupe keys based on pool/signature/event ordinal.
4. **Semantic shift risk** — monitoring and tests must assume unknown-pool does not always mean discarded anymore.

No SSOT account layout, program address, or ShadowLedger timeline contract was changed.

## Consequences

### Positive

- Preserves the creator's first buy across short create/trade race windows.
- Aligns Phase-5 dev metrics with observed creator sell behavior.
- Activates existing buffer infrastructure instead of introducing a new subsystem.
- Keeps the fix bounded to ingress/session bridging rather than distorting dev-metric formulas.

### Negative

- Some pre-session global-stream noise is now buffered briefly before expiring.
- Observability semantics change: buffered unknown-pool trades are no longer indistinguishable from silent discard.
- The launcher now depends more heavily on TTL/cap sizing for stable behavior under bursty global-stream load.

## Alternatives Considered

1. **Keep silent drop and patch only Gatekeeper metrics**  
   Rejected because it would mask the missing creator buy instead of preserving the real transaction in the observation window.

2. **Infer `dev_buy_total_sol` from sells or aggregate creator volume**  
   Rejected because it would corrupt the semantics of `dev_buy_total_sol`, which is intended to represent the creator's canonical primary buy, not a heuristic reconstruction.

3. **Modify upstream Seer forwarding contract first**  
   Rejected for this fix scope because the launcher already had a purpose-built session buffer that was simply not active in the unknown-pool ingress path.

4. **Buffer every unknown trade without identity checks**  
   Rejected because trades with unresolved/default pool or mint identity are too noisy and would increase false buffering pressure.

## Validation Steps

1. Run launcher-side regressions:
   - `session_bridge_buffers_unknown_pool_trade_until_pool_detected`
   - `seer_trade_before_pool_detected_replays_after_registration`
   - `seer_pool_detected_then_trade_emits_new_pool_detected_then_pool_transaction`
2. Inspect fresh `gatekeeper_v2_buys.jsonl` output and confirm `dev_buy_total_sol` is no longer systematically `0.0` when `dev_wallet_known=true` and creator activity is present.
3. Monitor session-bridge metrics for:
   - buffered unknown-pool trades,
   - TTL expirations,
   - per-pool/global evictions.
4. Validate that event bus emission order remains:
   - `NewPoolDetected` first,
   - replayed `PoolTransaction` second,
   for buffered race-window trades.
