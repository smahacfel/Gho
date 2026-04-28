# ADR-0024: gRPC dev metrics and IWIM source split

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Production `gatekeeper_v2_buys.jsonl` records show a persistent inconsistency:

- `dev_pubkey` is present in the final decision log
- Phase-5 dev metrics remain empty/zero-equivalent:
  - `dev_wallet_known=false`
  - `dev_buy_total_sol=0.0`
  - `dev_tx_ratio=0.0`
  - `dev_volume_ratio=0.0`
  - `dev_has_sold=false`
  - `dev_unknown=true`
- At the same time, creator-derived signals remain populated:
  - `iwim_fetch_status="OK"`
  - `iwim_quality="HIGH"`
  - `iwim_confidence=1.0`
  - `iwim_n_tx=30`
  - `iwim_rpc_used="primary"`
  - `dev_paperhand_latency_ms` / `dev_sold_within_3s` / `dev_sold_within_5s` are often meaningful

The active runtime configuration is:

- `config.toml`: `source_mode = "grpc"`
- `config.toml`: `stream_mode = "single_global"`

Therefore the canonical hot path is gRPC/Yellowstone, not PumpPortal WS.

## Decision

The root cause was established as a **source-of-truth split** between Phase-5 dev tracking and creator-aware post-processing:

1. `ghost-launcher/src/components/gatekeeper.rs`
   - Gatekeeper Phase-5 learns the dev wallet only inside `GatekeeperBuffer::update_tracking()`.
   - It sets `self.dev_wallet` **only when** an observed transaction has `tx.is_dev_buy == true`.
   - If that never happens, `compute_dev_behavior()` returns `dev_wallet_known=false` and all Phase-5 dev metrics stay zero/false.

2. `off-chain/components/seer/src/binary_parser.rs`
   - In the gRPC/Yellowstone parsing path, parsed `TradeEvent`s are constructed with `is_dev_buy: false`.
   - This means the active `source_mode = "grpc"` path does not attribute the creator’s initial buy to the dev wallet, even when the creator is otherwise known.

3. `ghost-launcher/src/oracle_runtime.rs`
   - Final buy logs are enriched with `dev_pubkey` from observation identity / `DetectedPool.creator`, not from Gatekeeper Phase-5 state.
   - The per-pool `FingerprintAggregator` is also initialized with `pool_data.creator`, so `dev_paperhand_latency_ms`, `dev_sold_within_3s`, and `dev_sold_within_5s` can be valid even while Phase-5 says `dev_unknown=true`.
   - IWIM veto is invoked with `dev_wallet_for_iwim = pool_data.creator`, not with Phase-5 `dev_wallet`.

4. `ghost-launcher/src/components/iwim_veto.rs`
   - IWIM independently fetches creator history from RPC (`primary` → `fallback` → `runtime`) using the creator pubkey passed from `pool_data.creator`.
   - Therefore IWIM is not inferring from empty Phase-5 metrics; it is using a separate creator identity path and separate RPC fetch.

Conclusion:

- **Phase-5 dev metrics are blind on the current gRPC path because `is_dev_buy` is not populated there.**
- **`dev_pubkey`, early-fingerprint creator metrics, and IWIM remain valid because they use `DetectedPool.creator` / observation identity, not Phase-5 discovery.**
- **IWIM is not “ściemnia”; it is drawing from a different and currently healthier source path than Gatekeeper Phase-5.**

## Architectural Impact

This establishes two concurrent creator/dev identity planes in production:

- **Phase-5 Gatekeeper plane** — inferred from observed transactions via `is_dev_buy`
- **Metadata / identity plane** — sourced from `DetectedPool.creator` and observation identity

Because BUY gating uses Phase-5 while IWIM and early fingerprint use metadata identity, the system can produce internally inconsistent creator assessments.

Affected components:

- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/iwim_veto.rs`

## Risk Assessment

**Rate: High**

Why high:

- Gatekeeper Phase-5 currently degrades to `dev_unknown=true` for gRPC-fed pools even when creator identity is known.
- This weakens creator-aware gating and shifts protection burden onto stricter `dev_unknown` rules plus IWIM.
- Offline analysis can be misled if analysts assume `dev_pubkey` and `dev_wallet_known` describe the same source of truth.
- Threshold tuning on historical JSONL can be biased because Phase-5 metrics under-report creator participation under gRPC.

## Consequences

### Positive

- The inconsistency is now explained without ambiguity.
- IWIM credibility is preserved: it is using explicit creator metadata + RPC history, not hallucinating from empty Phase-5 metrics.
- Early-fingerprint dev sell metrics are also explained as metadata-driven, not Phase-5-driven.

### Negative

- Production logs currently expose a semantic mismatch: `dev_pubkey` present does **not** imply Phase-5 dev tracking was active.
- Creator-risk decisions are split across two non-equivalent pipelines.
- Historical “healthy” records with populated Phase-5 metrics are likely coming from PumpPortal/synthetic create coverage or older paths, not the current pure gRPC path.

## Alternatives Considered

1. **IWIM is fabricating strength from empty dev metrics**  
   Rejected: code shows IWIM receives `pool_data.creator` directly and performs independent RPC fetches.

2. **Decision logger is dropping Phase-5 fields during serialization**  
   Rejected: logger fields are wired correctly; Phase-5 values are genuinely absent upstream.

3. **`dev_pubkey` in JSONL is the same field as Gatekeeper `dev_wallet`**  
   Rejected: `dev_pubkey` is injected from observation identity / pool metadata, while `dev_wallet` is inferred only from `is_dev_buy` transactions.

4. **The issue is caused by missing pool metadata**  
   Rejected for the reported cases: the logs clearly contain valid `dev_pubkey`, `base_mint`, and `shadow_metadata_source="local_task_state"`.

## Validation Steps

1. Confirm active runtime path:
   - `config.toml` has `source_mode = "grpc"` and `stream_mode = "single_global"`.

2. Confirm Phase-5 discovery contract:
   - `ghost-launcher/src/components/gatekeeper.rs` sets `self.dev_wallet` only on `tx.is_dev_buy && self.dev_wallet.is_none()`.

3. Confirm gRPC parser behavior:
   - `off-chain/components/seer/src/binary_parser.rs` builds parsed `TradeEvent`s with `is_dev_buy: false`.

4. Confirm metadata identity path:
   - `ghost-launcher/src/oracle_runtime.rs` enriches `dev_pubkey` from observation identity / `pool_data.creator`.
   - `FingerprintAggregator::new(..., pool_data.creator)` confirms creator-aware fingerprinting is metadata-based.

5. Confirm IWIM independence:
   - `ghost-launcher/src/oracle_runtime.rs` passes `pool_data.creator` into `run_iwim_veto_gate()`.
   - `ghost-launcher/src/components/iwim_veto.rs` fetches creator signatures independently over RPC.

6. Confirm production symptom:
   - Recent `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` entries show `dev_pubkey` populated with Phase-5 dev metrics empty and IWIM `OK/HIGH`.
