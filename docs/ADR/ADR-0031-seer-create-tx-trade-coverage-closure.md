# ADR-0031: Seer create-TX trade coverage closure

**Date:** 2026-03-23  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Production investigation of `dev_buy_total_sol = 0.0` established that pump.fun genesis transactions can contain both:

- a `Create` / `CpiCreate` event that produces `PoolDetected`, and
- a co-located `Trade` / `CpiTrade` event representing the creator's initial buy.

`off-chain/components/seer/src/lib.rs` previously parsed trades only in the `process_event()` branch where `parse_initialize_pool()` returned `None`. When a single transaction contained both Create and Trade, the Create branch executed and the trade branch never ran. The result was:

- Gatekeeper never received the creator's genesis buy from that transaction,
- `find_primary_creator_buy_index()` had no matching observed buy in the buffer,
- `dev_buy_total_sol` remained `0.0` despite later creator sells.

An initial hotfix added `parse_trades()` to the Create branch, but that left a production-quality gap:

- trade coverage counters were updated only in the `None` branch,
- create-TX trades were functionally forwarded but undercounted in observability,
- there was no direct regression test for `process_event()` on a same-TX `CpiCreate + CpiTrade` case.

## Decision

`off-chain/components/seer/src/lib.rs` was hardened as follows:

1. Added a shared helper `parse_and_forward_binary_trades()` that centralizes:
   - trade parser invocation metric accounting,
   - `trade_candidate_total`, `trade_parsed_total`, and parse-miss accounting,
   - live-forwarded signature accounting,
   - replay/fallback-adjacent trade forwarding behavior via `handle_trade_event()`.

2. Replaced the duplicated `parse_trades()` logic in the `process_event()` `None` branch with the shared helper.

3. Reused the same helper in the `Some(pool_event)` Create branch after:
   - sending `PoolDetected`, and
   - calling `register_curve_mapping()`.

4. Lowered the create-path co-located trade log from `info!` to `debug!` so the fix does not add noisy high-cardinality logs on the hot path.

5. Added a regression test proving that a single gRPC transaction carrying both `CpiCreate` and `CpiTrade` results in:
   - IPC ordering `PoolDetected -> Trade`,
   - correct resolved `pool_amm_id`, `mint`, `signer`, and `signature`,
   - correct trade coverage increments.

## Architectural Impact

This preserves existing SSOT boundaries:

- Seer remains the canonical event producer.
- ShadowLedger remains the canonical curve-state authority.
- The session gate in `ghost-launcher/src/components/seer.rs` still accepts only pools whose `PoolDetected` was observed in the current session.
- No historical/backfilled pool is made eligible by this change.

The change removes an observability split inside Seer itself:

- create-TX trades now follow the same coverage accounting path as standalone trade transactions,
- production telemetry for parser coverage and forwarded trade signatures remains internally consistent.

## Risk Assessment

**Rate:** Medium

Why not low:

- The Create path now performs the same trade parsing/forwarding work that the standalone trade path already performed.
- This adds bounded extra CPU work on Create transactions.

Why not high:

- The additional work is limited to the Create path, not all transactions.
- No new buffering, retry loop, or state authority was introduced.
- IPC ordering remains deterministic because `PoolDetected` is still emitted before co-located trade forwarding.

## Consequences

### Positive

- `dev_buy_total_sol` can now be anchored to an actually observed genesis buy from the create transaction.
- Seer trade coverage counters no longer undercount co-located create-TX trades.
- The regression is protected by a direct end-to-end unit test on `process_event()`.
- Log pressure stays controlled by using `debug!` for the create-path co-located trade message.

### Negative

- Create transactions now incur one additional trade-parse pass, increasing Create-path CPU cost modestly.
- This ADR does **not** solve the broader gRPC semantic gap where parsed trades still carry `is_dev_buy = false`.

## Alternatives Considered

1. **Keep ad-hoc `parse_trades()` only in the Create branch**  
   Rejected because it would leave coverage accounting split and observability inconsistent.

2. **Infer `dev_buy_total_sol` from `initial_liquidity_sol`**  
   Rejected because pump.fun genesis virtual reserves are not the creator's observed buy size.

3. **Synthesize `is_dev_buy = true` on the gRPC parser path in the same change**  
   Rejected as scope creep. That is a broader semantic contract change with larger blast radius than needed to close this production fix safely.

4. **Reintroduce launcher-side unknown-pool buffering**  
   Rejected because it violates the session-gate contract that only pools detected in the current session may be processed.

## Validation Steps

1. `cargo check --package seer`
2. `cargo test --package seer --lib test_process_event_emits_co_located_create_and_trade_with_coverage`
3. `cargo test --package seer --lib -- --skip test_account_update --skip account_update_before --skip parse_initialize_pool_works_without`
4. Confirm that `process_event()` now routes both Create-branch trades and standalone trades through the same helper.
5. Confirm that the Create branch still preserves IPC ordering `PoolDetected -> Trade`.
6. Confirm that the session-gate contract remains unchanged: no launcher buffering for unknown pools, no historical pool admission.
