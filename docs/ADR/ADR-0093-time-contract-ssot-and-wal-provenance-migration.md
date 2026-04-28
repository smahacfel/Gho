# ADR-0093: Time-contract SSOT and WAL provenance migration across Seer -> Launcher -> Brain

**Date:** 2026-04-12
**Status:** Accepted
**Author:** Ghost Father

## Context

This patch series started from a cluster of operator-visible failures that looked unrelated on the surface:

1. apparent `tx_count` / `snapshot_count` resets after BUY,
2. inconsistent timing between on-chain transaction time and runtime logs,
3. coverage / truth windows pulling the wrong transactions,
4. replay / WAL restoration using timestamps with mixed meaning,
5. Gatekeeper / tx-intelligence / snapshot logic reading one field (`timestamp_ms` / `ts_ms`) as if it always meant the same clock.

The real root cause was not a single counter bug. The pipeline was mixing three different clocks without a strict source-of-truth contract:

1. **chain event time** - on-chain epoch-like time from `block_time` or equivalent chain-native source,
2. **ingress wall time** - local epoch time when the process observed the event,
3. **ingress monotonic time** - process-local monotonic / arrival timing.

Before this patch, several components collapsed these into one legacy field and then inferred meaning from the mere presence of a number. That made:

- a local ingress fallback look like chain time,
- legacy compatibility timestamps look like canonical event time,
- replay/write clocks look like event clocks,
- timing diagnostics and decision windows drift apart.

This ADR consolidates the full patch from the initial timestamp-SSOT design through the final WAL / replay / coverage alignment and the late test-surface hardening.

## Goal

The goal of the patch was to make time semantics explicit and stable across the full observation pipeline while preserving backward compatibility:

1. introduce one additive SSOT for event-time provenance,
2. stop fabricating chain time from local fallbacks,
3. separate event-time, wall-time, and monotonic-time usage by purpose,
4. version WAL semantics where write-clock and event-clock diverged,
5. keep legacy `timestamp_ms` / `ts_ms` fields as compatibility shims rather than silently changing their meaning.

## State Before Implementation

| Area | Before |
| --- | --- |
| Seer adapters | `event_ts_ms` could be populated from local ingress epoch even when chain `block_time` was absent. |
| Timestamp quality | `event_ts_ms.is_some()` or `timestamp_ms > 0` could be treated as evidence of chain-quality time. |
| Transport types | `DetectedPool.timestamp_ms`, `PoolTransaction.timestamp_ms`, `TradeEvent.timestamp_ms`, and related fields carried overloaded semantics. |
| Gatekeeper / runtime | Event-axis helpers could fall back to legacy timestamps or `arrival_ts_ms`, causing windows and deadlines to mix clocks. |
| Tx intelligence | Interval, burst, and fingerprint timing could read compatibility timestamps as if they were canonical event time. |
| Snapshot engine | Legacy timestamps could masquerade as `event` source, and default source state was optimistic rather than provenance-safe. |
| WAL / replay | `WalRecord.ts_ms` was read in some places as replay/write time even though writers were storing event/compat timestamps there. |
| Coverage / truth | Window membership could include ingress-wall fallback as if it were chain truth. |
| Test surface | Many fixtures did not carry explicit `event_time`, so tests accidentally encoded the old ambiguous contract. |

## Decision

The accepted implementation strategy was:

1. introduce a shared additive provenance model first,
2. propagate it through Seer, Launcher, and the Brain surfaces that still matter,
3. keep legacy serialized fields intact,
4. make event-axis selection explicit at each consumer,
5. version WAL semantics rather than mutating old records in place,
6. harden test fixtures and demos so they reflect the new contract instead of reintroducing the old one.

Three rules remained non-negotiable through the rollout:

1. **Do not reinterpret legacy `timestamp_ms` / `ts_ms` in place.**
2. **Do not let monotonic arrival time pretend to be epoch-like event time.**
3. **Do not change replay semantics without an explicit WAL compatibility layer.**

## Implementation Summary

### 1. Shared time provenance contract

The patch introduced a shared additive time envelope in `ghost-core`:

| Files | Change | Purpose |
| --- | --- | --- |
| `ghost-core/src/event_time.rs` | Added `EventTimeMetadata` with `chain_event_ts_ms`, `ingress_wall_ts_ms`, and `ingress_monotonic_ts_ms`. | Establish explicit SSOT for time provenance. |
| `ghost-core/src/lib.rs` | Re-exported new provenance and coverage types. | Make the contract available across crates without duplicate local definitions. |

This created a stable contract for:

- canonical chain time,
- local wall-clock observation time,
- monotonic arrival time,
- explicit helpers for effective / compatibility fallback behavior.

### 2. Seer ingress and parser migration

The ingest edge was changed so provenance becomes explicit as early as possible:

| Files | Change | Purpose |
| --- | --- | --- |
| `off-chain/components/seer/src/types.rs` | Raw and parsed event types now carry `EventTimeMetadata`. | Preserve provenance at the source instead of reconstructing it downstream. |
| `off-chain/components/seer/src/grpc_connection.rs` | Stopped treating ingress fallback as fake chain time. | Remove the most dangerous `event_ts_ms -> Chain` lie. |
| `off-chain/components/seer/src/websocket_connection.rs` | Propagates explicit wall / chain provenance. | Keep adapter semantics aligned across transports. |
| `off-chain/components/seer/src/helius_websocket_adapter.rs` | Propagates explicit time envelope. | Same reason: source-local provenance instead of implicit fallback. |
| `off-chain/components/seer/src/pumpportal_connection.rs` | Propagates explicit time envelope. | Preserve provenance in PumpPortal-origin events. |
| `off-chain/components/seer/src/lib.rs` | Tightened timestamp-quality inference and conversion into downstream types. | Stop treating a non-zero timestamp as proof of chain time. |
| `off-chain/components/seer/src/binary_parser.rs` | Preserved richer provenance while fixing parser fallout around trade arbitration. | Prevent parser-local regressions from collapsing the new contract. |

Result:

- Seer no longer upgrades local fallback timestamps into chain authority.
- Parsed trades and candidate pools preserve explicit time-source metadata.
- Compatibility reconstruction remains possible, but it is no longer the authoritative path.

### 3. Launcher transport, runtime, session, and decision-path hardening

The largest part of the rollout was in `ghost-launcher`, where many call-sites had been mixing event and wall clocks:

| Files | Change | Purpose |
| --- | --- | --- |
| `ghost-launcher/src/events.rs` | `DetectedPool` and `PoolTransaction` now carry explicit `event_time`; `DetectedPool` also carries `detected_wall_ts_ms`. | Stop flattening detection and transaction time into a single ambiguous field. |
| `ghost-launcher/src/components/seer.rs` | Detection-latency logs now separate `chain_event_ts_ms`, `effective_event_ts_ms`, and compatibility time. | Make operator logs truthful about what clock they are reporting. |
| `ghost-launcher/src/components/gatekeeper.rs` | `tx_event_ts_ms()` now prefers explicit effective event time and no longer uses `arrival_ts_ms` as epoch-like fallback; curve-t0 source tracking was added. | Keep decision windows and curve latch logic on the correct axis. |
| `ghost-launcher/src/tx_intelligence/engine.rs` | Split epoch-like event timing from local ordering timing. | Ensure interval, burst, and fingerprint logic uses the right clock while ordering can still use local tie-breakers. |
| `ghost-launcher/src/oracle_runtime.rs` | Tightened event-time helpers, wall-time observation identity, orphan freshness logic, buy-log provenance, and export helpers. | Remove remaining runtime-side clock ambiguity and expose provenance in diagnostics. |
| `ghost-launcher/src/session/observation.rs` | Preserved session-owned wall-clock semantics and added explicit curve-t0 source propagation. | Keep session deadlines and observation ownership on wall time while curve logic stays provenance-aware. |
| `ghost-launcher/src/components/oracle_pipeline.rs` | Candidate conversion now prefers explicit event or detected-wall time. | Prevent legacy-only `timestamp_ms` from acting as authoritative event time. |
| `ghost-launcher/src/components/snapshot_listener.rs` | Pool activation on `NewPoolDetected` now happens before replay of staged transactions. | Fix replay/bootstrap behavior without reactivating pools on every transaction. |
| `ghost-launcher/src/wal_recovery.rs` | Replay delta filtering now uses `WalReplayEntry.write_wall_ts_ms`. | Align replay watermark semantics with snapshot write-clock semantics. |

Two runtime-specific provenance additions became important for observability:

1. `first_seen_clock_source`,
2. `curve_t0_clock_source`.

These now flow into buy logs and make it visible whether a pool's timing anchor came from chain time, ingress wall time, detected wall time, or a compatibility path.

### 4. Snapshot engine and Brain-side provenance tightening

The user explicitly limited broad `ghost-brain` refactors because large parts are legacy. The patch therefore touched only the surfaces required to make time semantics correct:

| Files | Change | Purpose |
| --- | --- | --- |
| `ghost-brain/src/oracle/snapshot_engine.rs` | Added a common event-time resolver, `EventTsSource`, `LegacyCompat` labeling, and `MarketSnapshot.event_ts_source`. Snapshot storage still retains `LegacyCompat` / `Arrival` / `Wallclock` for buffering and observability, but those sources are now explicit instead of pretending to be canonical event time. | Prevent legacy timestamps from masquerading as canonical event source while keeping buffering/replay compatibility. |
| `ghost-brain/src/oracle/engine.rs` | Prediction / TCR timing now accepts only explicit epoch-like clocks (`Event` or `IngressWall`) and preserves prior source on fallback. Decision windows stay unready when only storage-only clocks are available. | Stop scoring/timing logic from silently downgrading provenance. |
| `ghost-brain/src/oracle/decision_logger.rs` | Decision/buy logs now carry explicit timing provenance surfaces. | Make downstream audit files explain which clock a decision used. |
| `ghost-brain/src/oracle/ultrafast/panic.rs` | Added additive `event_time` while keeping observation-time semantics intact. | Preserve compatibility without leaving PANIC outside the contract. |

Result:

- new pools no longer default to optimistic `event` timing source,
- soft-truth snapshots retain source provenance,
- hard/soft truth comparison no longer compares chain truth against non-chain-compatible snapshot timing,
- Brain storage/order clocks stay backward compatible, but decision/event-axis logic is explicitly narrower than the raw snapshot buffer contract.

### 5. WAL V2 and replay/write-clock migration

WAL required a dedicated compatibility-safe migration because the old payload timestamp had mixed semantics:

| Files | Change | Purpose |
| --- | --- | --- |
| `ghost-core/src/wal.rs` | Added WAL V2 types, `WalRecordClock`, `WalReplayEntry`, and explicit `write_wall_ts_ms`; fixed the bincode-unsafe optional-field serialization bug. | Separate record write time from payload event/compat time without breaking legacy replay. |
| `ghost-launcher/src/wal_recovery.rs` | Replay now consumes `WalReplayEntry` instead of assuming `WalRecord.ts_ms()` is the correct replay watermark. | Make snapshot restore and delta replay consistent with write-clock semantics. |

This was necessary because:

- Seer writers were storing event/compat timestamps in the legacy WAL payload,
- launcher snapshot watermark logic already behaved like a write-wall-clock boundary,
- replay therefore had a hidden clock mismatch.

After the migration:

- legacy records still replay,
- V2 records carry explicit write wall time,
- delta filtering and snapshot restoration no longer compare incompatible clocks.

Important follow-up clarification:

- startup recovery now uses `WalReplayEntry.write_wall_ts_ms` for watermark correctness,
- low-level `replay_order_key()` behavior is regression-tested,
- but production startup replay still follows streamed WAL order plus per-flow tx-key sorting; wiring `WalReplayEntry::replay_order_key()` into the global recovery ordering remains separate work.

### 6. Coverage / truth / audit alignment

Coverage and truth validation were tightened to accept only explicit chain-compatible evidence:

| Files | Change | Purpose |
| --- | --- | --- |
| `ghost-core/src/coverage_audit.rs` | Introduced source-aware truth signature state and time-source reporting. | Make coverage diagnostics explain what kind of time evidence was used. |
| `ghost-launcher/src/oracle_runtime.rs` | Coverage audit records now include runtime-side provenance sources. | Align runtime truth emission with the new provenance model. |

Result:

- ingress-wall fallback can no longer inflate chain-truth window membership,
- coverage diagnostics now distinguish truth source and runtime source instead of compressing them into one opaque counter.

### 7. Test-surface migration and fixture hardening

The rollout exposed many stale fixtures that still modeled the old world. Those were updated intentionally rather than leaving green tests that validated the wrong contract:

| Files | Change | Purpose |
| --- | --- | --- |
| `off-chain/components/seer/tests/time_contract.rs` | Added ingress-vs-chain provenance regressions. | Lock the Seer-side contract. |
| `ghost-launcher/tests/time_contract_bridge.rs` | Bridge fixtures updated for explicit `event_time`. | Verify transport preservation. |
| `ghost-launcher/tests/snapshot_engine_integration.rs` | Added explicit event-time fixtures. | Keep snapshot timing tests aligned with production semantics. |
| `ghost-launcher/tests/oracle_event_bus_integration.rs` | Updated transport fixtures. | Same reason. |
| `ghost-launcher/tests/oracle_transaction_gathering.rs` | Updated tx fixtures with explicit provenance. | Same reason. |
| `ghost-launcher/tests/gatekeeper_policy_tests.rs` | Rebased session deadlines onto real wall-clock semantics. | Prevent ancient synthetic timestamps from failing due to real deadline logic. |
| `ghost-launcher/tests/oracle_logging_demo.rs` | Demo now verifies BUY decision log and lifecycle closure instead of deprecated post-BUY `score_pool()` behavior. | Match the real authoritative runtime contract. |
| `ghost-launcher/tests/session_lifecycle_tests.rs` | Helper transactions now carry explicit ingress wall time. | Prevent synthetic tests from hitting immediate deadline due to fallback to current wall clock. |
| `ghost-launcher/tests/tx_intelligence_tests.rs` | Helper transactions now carry explicit ingress wall time. | Keep interval/burst timing deterministic under the new contract. |
| `ghost-launcher/tests/wal_startup_recovery.rs` | WAL recovery semantics verified against the V2 write-clock contract. | Guard replay correctness. |

## State After Implementation

| Area | After |
| --- | --- |
| Time provenance | Every relevant event type now has explicit additive provenance instead of relying on one overloaded timestamp field. |
| Chain-time semantics | Only explicit chain-native timestamps can claim chain authority. |
| Event-axis selection | Consumers use explicit `effective_event_ts_ms()` / provenance-aware helpers rather than ad hoc fallbacks. |
| Wall-clock semantics | Detection deadlines, observation identity, WAL write time, and operator latency use dedicated wall-time fields. |
| Monotonic semantics | `arrival_ts_ms` remains available for local ordering / telemetry but no longer pretends to be epoch-like event time. |
| Snapshot source labels | Snapshot and prediction flows carry explicit source labels such as `event`, `ingress_wall`, and `legacy_compat`. |
| Replay / WAL | Delta replay now uses explicit write wall time and remains compatible with legacy records. |
| Coverage / truth | Chain-truth windows no longer accept ingress-wall fallback as if it were canonical chain evidence. |
| Test surface | Fixtures encode explicit provenance and therefore validate the actual runtime contract. |

## Architectural Impact

This patch establishes a stable architectural boundary:

1. **Seer** is responsible for capturing provenance at ingest.
2. **Launcher** is responsible for choosing the correct clock for each use-case.
3. **Brain** no longer assumes legacy timestamps mean canonical event time.
4. **WAL** distinguishes write-clock from payload-event clock through explicit versioned metadata.
5. **Coverage/audit** can now explain *why* a transaction was considered inside or outside a truth window.

The patch also preserves two important compatibility properties:

1. legacy `timestamp_ms` / `ts_ms` fields still exist,
2. old WAL records remain readable.

## What This Patch Did Not Do

This rollout intentionally did **not**:

- redesign shadow-only long-lived pool retention behavior,
- broadly refactor legacy `ghost-brain` modules outside the surfaces needed for correctness,
- perform a breaking cleanup that removes old timestamp fields from serialized contracts.

Those remain separate concerns.

## Risk Assessment

**Rate:** High implementation complexity, Medium residual operational risk

Main risks that were addressed:

- **Critical before patch:** local fallback time being interpreted as chain truth.
- **High before patch:** replay and snapshot watermark comparing different clock domains.
- **High before patch:** interval/burst/fingerprint logic drifting because monotonic or compatibility timestamps were used as epoch-like time.
- **Medium during migration:** serde / bincode / replay compatibility when extending WAL V2.

Residual risk after patch:

- future developers can still misuse legacy `timestamp_ms` if new code bypasses the provenance helpers,
- any future change to replay ordering or WAL payload semantics still requires explicit versioning and review.

## Alternatives Considered

### 1. Keep legacy fields and only improve logging

Rejected because logging-only fixes would leave replay, coverage, and runtime windows semantically broken.

### 2. Perform a breaking cleanup and delete legacy timestamp fields immediately

Rejected because the blast radius across serde, IPC, WAL, replay, and tests was too high for one patch.

### 3. Treat `arrival_ts_ms` as the universal fallback event clock

Rejected because monotonic/process-local timing must not masquerade as epoch-like event time.

### 4. Leave WAL semantics unchanged and patch replay heuristically

Rejected because `WalRecord.ts_ms` already had mixed meaning; replay correctness required explicit write-clock semantics.

## Validation

The patch series was closed only after the main surfaces and the broad launcher suite were green, including:

1. `cargo test -p ghost-core --lib --quiet`
2. `cargo test -p seer --lib --quiet`
3. `cargo test -p ghost-launcher --lib --quiet`
4. `cargo test -p ghost-launcher --test wal_startup_recovery --quiet`
5. `cargo test -p ghost-launcher --test gatekeeper_policy_tests --quiet`
6. `cargo test -p ghost-launcher --test oracle_logging_demo --quiet`
7. `cargo test -p ghost-launcher --test session_lifecycle_tests --quiet`
8. `cargo test -p ghost-launcher --test tx_intelligence_tests --quiet`
9. `cargo test -p ghost-launcher --tests --quiet`

## Consequences

What becomes easier:

- diagnosing whether drift is on-chain, ingress, or local-runtime,
- reasoning about BUY windows and curve latch behavior,
- auditing coverage/truth mismatches,
- replaying WAL data without confusing event and write clocks,
- extending the pipeline without silently changing timestamp semantics.

What becomes harder:

- new code must use provenance-aware helpers rather than raw legacy fields,
- future WAL changes must remain versioned and explicit,
- tests must carry explicit `event_time` instead of relying on incidental timestamp behavior.

## Final Outcome

The patch converted time from an implicit, overloaded byproduct into an explicit contract.

Before the rollout, the system often answered "what time was this?" with "whatever non-zero timestamp happened to be available."

After the rollout, the system can answer:

- whether the time came from the chain,
- whether it came from local ingress wall-clock,
- whether it is only a compatibility reconstruction,
- whether replay is using write-clock or event-clock semantics,
- and which source drove each operator-visible decision surface.

That is the central architectural outcome of the patch.
