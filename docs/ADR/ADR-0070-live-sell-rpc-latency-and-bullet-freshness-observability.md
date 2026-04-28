# ADR-0070: Live sell RPC latency and bullet freshness observability

**Date:** 2026-04-01
**Status:** Accepted
**Author:** Ghost Father

## Context

A direct review of the SELL path was requested to answer two operational questions:

1. whether SELL transactions are exposed to the same recent-blockhash freshness constraints as BUY transactions,
2. whether the current system measures how long RPC operations take in the live SELL path.

The code analysis established an important split in the current architecture:

- `off-chain/components/trigger/src/revolver_sell_builder.rs` signs SELL bullets with a recent blockhash, so SELL transactions are subject to the same blockhash-validity rules as any other Solana transaction.
- `off-chain/components/trigger/src/revolver_shoot.rs` sends already-signed SELL bytes directly through `TpuClient`, so that helper does not fetch RPC data at trigger time.
- the launcher SSOT for live exits is **not** `revolver_shoot.rs`; it is `ghost-launcher/src/components/post_buy_runtime.rs`, where `run_live_sell_lifecycle(...)` loads pre-signed bullets and later submits them via async RPC.

The review also found two concrete operational gaps:

1. live SELL RPC operations were not being timed in the launcher path (`get_token_account_balance`, `load_magazine_from_direct_buy`, `send_transaction`, `confirm_transaction`), so operators had no direct evidence for the actual RPC latency budget.
2. when a live SELL submit failed, the launcher recreated a new `Bullet` from raw bytes instead of reinserting the original one. That reset `last_update`, `created_at`-relative freshness tracking, and retry metadata, masking stale-blockhash risk on subsequent retries.

## Decision

The live SELL path keeps its current transport semantics, but gains explicit observability and preserves bullet freshness metadata across retries.

Implemented decisions:

1. `ghost-launcher/src/components/post_buy_runtime.rs` now records latency histograms/counters for live SELL RPC stages:
   - `get_token_account_balance`
   - `query_actual_ata_balance`
   - `load_magazine_from_direct_buy`
   - `send_transaction`
   - `confirm_transaction`
2. the same file now records bullet-age telemetry before submit and emits an explicit warning when a SELL bullet is already stale at fire time.
3. failed live SELL submits now reinsert the **original** `Bullet` object instead of reconstructing a new one from raw bytes, preserving:
   - blockhash age metadata (`last_update`),
   - time-stop state,
   - retry metadata.
4. `off-chain/components/trigger/src/revolver_worker.rs` now logs blockhash-fetch latency for both:
   - background bullet refresh cycles,
   - initial magazine loading.

## Architectural Impact

This decision does **not** change the authoritative launcher live-exit transport yet:

- launcher live SELL remains RPC-submitted from `post_buy_runtime.rs`,
- off-chain `revolver_shoot.rs` remains a separate TPU-oriented helper,
- `RevolverWorker` remains the explicit blockhash-refresh mechanism where integrated.

What changes is operational clarity:

- launcher operators now get direct visibility into RPC time spent in the live SELL path,
- stale-bullet conditions are surfaced explicitly instead of being hidden by object reconstruction,
- future transport decisions (RPC vs TPU vs background refresh integration) can be made from measured evidence instead of guesswork.

## Risk Assessment

**Rate:** Low

Primary risks:

- additional metrics/logging slightly increase telemetry volume,
- preserved bullet metadata may expose stale-bullet conditions that were previously hidden by reset-on-retry behavior.

These risks are acceptable because they improve correctness and make existing failure modes observable rather than latent.

## Consequences

### Positive

- SELL-path RPC latency is now measurable in production,
- stale blockhash exposure in live SELL retries becomes visible,
- retry semantics are more truthful because bullet freshness metadata survives failed submits,
- launcher/live SELL and off-chain/TPU SELL path differences are now easier to reason about.

### Trade-offs

- this does not yet solve the larger architectural question of whether launcher live SELL should migrate to TPU submission or integrate an always-on refresh worker,
- metrics alone do not reduce latency; they only reveal where it is spent,
- operators may now see warnings that were previously silent, which can raise the short-term alert surface.

## Alternatives Considered

### 1. Change launcher live SELL transport immediately from RPC to TPU

Rejected for this change because the user’s immediate concern was observability and correctness. Transport migration is a wider architectural move with broader blast radius.

### 2. Integrate `RevolverWorker` into launcher live SELL in the same patch

Rejected for this iteration because it mixes telemetry work with lifecycle orchestration changes. The safer first step is to expose latency and stale-bullet behavior directly.

### 3. Keep recreating bullets on failed submit

Rejected because it hides the real age/freshness of the serialized transaction and can make stale blockhash issues harder to diagnose.

## Validation Steps

Validated in this session with targeted checks:

1. Editor diagnostics:
   - no errors in:
     - `ghost-launcher/src/components/post_buy_runtime.rs`
     - `off-chain/components/trigger/src/revolver_worker.rs`

2. Targeted launcher test:
   - `cargo test -p ghost-launcher --lib post_buy_runtime -- --nocapture`
   - result: `1 passed; 0 failed`

3. Targeted trigger/revolver worker tests:
   - `cargo test -p trigger --lib revolver_worker -- --nocapture`
   - result: `9 passed; 0 failed`
