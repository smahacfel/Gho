# ADR-0096: Phase 6 FSC starts with data-plane boundaries, not metric math

**Date:** 2026-04-14
**Status:** Accepted
**Author:** Ghost Father

## Context

Phase 6 of `PLAN_IMPLEMENTACJI_NOWYCH_METRYK.md` introduces `FSC` (`funding_source_concentration`) as the last and most operationally risky anti-sybil metric.

Repository analysis confirms four critical facts:

1. the canonical feature contract already exists under `MaterializedFeatureSet.sybil_resistance`, including `funding_source_concentration`, degraded reasons, and neutral config defaults,
2. `CPV` is already implemented as a bounded global rolling index and provides the correct integration pattern for a cross-session metric,
3. the repository now contains a canonical funding-transfer transport payload across Seer IPC / launcher events, but the active producer still emits only filtered `grpc_global_stream` observations with `full_chain_coverage=false`,
4. the repository contains an RPC-based historical tracer in `ghost-brain/src/oracle/cluster_hunter.rs` (`trace_funding_source`), but that path is not suitable for authoritative hot-path FSC because it is RPC-dependent, latency-variable, non-replayable, and external to the canonical event pipeline.

Additional code inspection shows that:

- `TradeEvent` / `PoolTransaction` already carry raw fields needed by earlier phases (`signer_post_balance_lamports`, `toolchain_fingerprint`),
- `GhostEvent::GeyserTransaction` exists as a schema variant, but no active producer path was found during this audit,
- `PoolObservationSession::materialize_features(...)` is already the correct place to *consume* a global index result, not to discover funding sources itself,
- the active buy-log and config surfaces already mirror `funding_source_concentration`, so the remaining gap is runtime data-plane correctness, not serializer shape.

This means Phase 6 is primarily a **data-plane and source-of-truth problem**. If implementation begins from metric math alone, the repo risks introducing a non-canonical or partially observed funding heuristic that looks plausible in telemetry but is unfit for authoritative policy.

## Decision

Phase 6 must begin with a strict preflight boundary:

**FSC is implemented only after introducing an explicit, authoritative funding-transfer ingest path from the full feed.**

### Accepted implementation boundary

1. **Freeze a canonical transport payload for funding transfers.**
	- `FundingTransferObserved` remains the Seer→Launcher transport payload, not an ad-hoc session-side heuristic.
	- The payload must stay additive and backward compatible.
	- `full_chain_coverage: bool` remains the stable downstream readiness bit.
	- Additive provenance must carry enough contract detail to support replay, ordering and lane separation:
	  - `from`
	  - `to`
	  - `lamports`
	  - `signature`
	  - `slot` (if known)
	  - `event_time` / effective timestamp metadata
	  - `arrival_ts_ms` or equivalent ingress timing
	  - explicit funding-lane / coverage / replay provenance

2. **Keep current filtered funding observations distinct from the future authoritative full-feed ingest layer.**
	- The active `grpc_global_stream` producer remains a filtered observation lane only.
	- It must stay explicitly classified as filtered and must not start emitting `full_chain_coverage=true`.
	- A future dedicated authoritative lane may emit `true`, but only under a separate contract.
	- Parser / ingest is responsible for identifying the raw transfer and emitting the transport event.
	- Policy and session code must not rediscover funding transfers by re-parsing incomplete local state.

3. **Produce authoritative funding-transfer events from a dedicated full-feed ingest layer.**
	- The source must see the full gRPC transaction stream, not only pool-linked buys.
	- Only `SystemProgram::Transfer` events above `funding_dust_threshold_lamports` are eligible in v1.

4. **Do not derive FSC from pool-trade path only.**
	- `PoolTransaction` / session-local buffers are insufficient because they only see transactions already associated with an analyzed pool.
	- Funding provenance must remain independent from pool-attachment logic.

5. **Do not use RPC backfill or `cluster_hunter::trace_funding_source` in the hot path.**
	- RPC tracing is acceptable for offline diagnostics only.
	- It must not become an authoritative dependency for `funding_source_concentration`.

6. **Implement a dedicated bounded `FundingSourceIndex` modeled after `CrossPoolVelocityIndex`.**
	- Hold it globally in runtime/session infrastructure, not per session.
	- Key: recipient wallet.
	- Value: bounded deque of recent eligible funding transfers.
	- Required controls:
	  - TTL / lookback window
	  - per-recipient cap
	  - global recipient cap
	  - eviction metrics
	  - hit/miss metrics
	  - readiness / warmup state

7. **Freeze lookup semantics before writing scoring logic.**
	- For each buyer, FSC lookup should use the **latest eligible pre-buy funding transfer** within the lookback window.
	- Post-buy transfers must not redefine the source that funded the buy.
	- v1 is strictly one-hop; no recursive funding graph search.

8. **Model neutral funders as classification, not as a shared synthetic source bucket.**
	- `neutral_funding_sources` must be versioned config input.
	- Neutral funders must not collapse many buyers into one artificial concentration cluster.
	- The implementation should use an explicit resolution type, e.g. concrete / neutral / unknown / unavailable, rather than a plain string sentinel that can be accidentally grouped.

9. **Materialization remains read-only and canonical.**
	- `PoolObservationSession::materialize_features(...)` queries the global `FundingSourceIndex` for current pool buyers and writes the result into `MaterializedFeatureSet.sybil_resistance.funding_source_concentration`.
	- No network calls, no historical scans, and no parser-side verdict logic are allowed in materialization.

10. **Policy remains soft-only in v1.**
	- Missing stream / cold index / insufficient known sources must yield `None` plus stable degraded reason codes.
	- `FSC` contributes zero penalty until the stream quality and neutral-funder hygiene are validated.

## Architectural Impact

This decision introduces a new canonical cross-cutting path:

1. Seer ingest parses eligible funding transfers and preserves explicit lane provenance,
2. Seer IPC forwards `FundingTransferObserved`,
3. Launcher event bus accepts a funding-transfer event variant without changing `full_chain_coverage` semantics,
4. runtime/session infrastructure feeds a shared `FundingSourceIndex`,
5. session materialization computes `funding_source_concentration` from the shared index,
6. Gatekeeper policy consumes the canonical field later through `MaterializedFeatureSet` only.

Concrete implementation consequences:

- freeze `FundingTransferObserved` / `FundingTransferEvent` with additive funding provenance contract,
- keep launcher event schema propagation 1:1 for funding provenance,
- add `funding_source_index.rs` next to `cross_pool_velocity.rs`,
- store a shared `Arc<FundingSourceIndex>` in `SessionManager`, mirroring `CrossPoolVelocityIndex`,
- extend `PoolObservationSession` to receive the shared index and compute FSC during materialization,
- add runtime metrics for entry count, evictions, lookups, readiness, and prune duration,
- keep the existing buy-log / JSONL mirror fields unchanged except for now receiving live values.

## Risk Assessment

**Rate:** High

- **Medium** risk for the schema additions themselves when kept additive.
- **High** risk for classification errors that mis-tag neutral funders or choose the wrong transfer as the funding source.
- **High** risk for silent false confidence if the funding stream is partial but still produces non-null FSC values.
- **Critical** risk if hot-path FSC falls back to RPC or pool-local heuristics and becomes non-deterministic.

## Consequences

What becomes easier:

- FSC remains canonical, replayable, and auditable,
- runtime integration follows the already proven CPV pattern,
- missing data stays explicit through degraded reasons,
- future policy activation can trust that the metric came from a bounded, observable, authoritative pipeline.

What becomes harder:

- Seer IPC and launcher event schemas both need to evolve,
- full-feed filtering/classification must be correct before any policy value exists,
- neutral funder maintenance becomes an operational artifact,
- implementation requires more tests at the transport and bounded-state layers than earlier local metrics.

## Alternatives Considered

### 1. Infer FSC only from pool-linked trades

Rejected because the pool-local trade path does not observe the global funding graph and would bias the metric toward whatever happened to be attached to the analyzed pool.

### 2. Reuse `cluster_hunter::trace_funding_source` in production hot path

Rejected because it depends on RPC history lookups, can block, is not replay-stable, and sits outside the canonical ingest contract.

### 3. Build FSC from `GhostEvent::GeyserTransaction` inside launcher without a dedicated canonical transfer payload

Rejected for v1 because the current repository does not show an active producer path for that variant, and piggybacking on an underspecified raw event would blur transport ownership exactly where Phase 6 needs the clearest boundary.

### 4. Add multi-hop or graph-based source tracing immediately

Rejected because it multiplies ambiguity, memory pressure, and false-positive risk before the one-hop event stream is even validated.

## Validation Steps

1. Add additive serde-safe funding provenance fields for funding-transfer events and verify old fixtures still deserialize.
2. Add parser / ingest tests proving only eligible `SystemProgram::Transfer` events above dust threshold are emitted.
3. Add bounded-state unit tests for `FundingSourceIndex` covering:
	- TTL expiry,
	- per-recipient cap,
	- global cap eviction,
	- readiness / cold-index behavior,
	- hit/miss metrics.
4. Add lookup tests proving:
	- the latest eligible **pre-buy** transfer is chosen,
	- post-buy transfers do not rewrite causality,
	- multiple buyers funded by one concrete wallet yield high FSC,
	- neutral funders do not create artificial concentration.
5. Verify `materialize_features(...)` returns `None` + degraded reason when the stream is unavailable or the index is cold.
6. Run replay with neutral config and confirm zero decision drift.
7. Only after telemetry bake, enable any non-zero `soft_penalty_high_fsc`.

## PR-4 operationalization follow-up

Po domknięciu PR-1…PR-3 bake contract dla operatora jest następujący:

1. **Żaden committed profil nie włącza authoritative funding lane.**
   - `config.toml` oraz rollout profiles w `configs/rollout/` pozostają na `funding_lane_mode = "disabled"`.
   - Authoritative bake używa wyłącznie lokalnej kopii profilu operatorskiego.

2. **Powierzchnie diagnostyczne pozostają jawnie rozdzielone.**
   - `gatekeeper_v2_buys.jsonl -> funding_source_concentration` pozostaje kanoniczną wartością `FSC`.
   - `gatekeeper_v2_buys.jsonl -> sybil_metric_degraded_reasons[]` pozostaje miejscem, gdzie widać `FSC_*` fail-closed reasons.
   - Lane health/readiness żyją w metrykach runtime/transport (`ghost.pump.*{source_label=...}`, `seer_funding_transfer_observations_total`, `fsc_authoritative_funding_stream_available`, `fsc_warmup_ready`).

3. **Bake nadal nie aktywuje policy.**
   - `soft_penalty_high_fsc = 0`,
   - `soft_penalty_high_fsc_high_cpv_combo = 0`,
   - `enable_sybil_combo_veto = false`,
   dopóki osobny follow-up nie zaakceptuje bake package i replay diff.
