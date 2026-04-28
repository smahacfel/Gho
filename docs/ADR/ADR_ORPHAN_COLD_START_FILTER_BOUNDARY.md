# ADR: Boundary for cold-start orphan filtering

- Status: accepted
- Date: 2026-03-17
- Scope: `ghost-launcher` / `off-chain/components/seer`

## Context

During cold start, the process can receive `PoolTransaction` events for pump.fun pools that were created **before the current bot session**. Because the runtime did not observe a live `CreatePool` / `NewPoolDetected` event for those pools, they are non-canonical from the current session perspective.

Historically, some of those tx-first events were passively staged into `OracleRuntime` orphan buffers and later expired, producing massive log spam such as:

- `Dropping stale ORPHAN tx for pool=... (age=31386ms, TTL=30000ms)`

In practice, this spam dominated `oracle_decision.log` during cold start and obscured useful signals.

The architectural question was whether to eliminate this traffic earlier, inside `Seer`, by fully ignoring all trade events for pools whose live `CreatePool` was not seen during the current session.

## Decision

We keep the cold-start filter boundary in `ghost-launcher::oracle_runtime`, **not** in `Seer`.

Specifically:

1. Unknown tx-first pools that do not have canonical runtime state are ignored at the `OracleRuntime` event-loop boundary.
2. Such events must **not** enter runtime orphan buffering.
3. `Seer` remains responsible for parsing, race recovery, mapping repair, and replay semantics for live stream ordering issues.
4. We do **not** introduce a hard upstream `PerPool` drop in `Seer` based on current-session watch visibility.

## Why this boundary was chosen

### 1. The runtime orphan path was non-canonical

The staged tx-first unknown-pool orphan path in `OracleRuntime` did not drive the canonical BUY / SnapshotEngine / Gatekeeper path. It accumulated passive data for pools that were still not registered as canonical and later expired by TTL.

That made it the safest place to cut the noise without changing core ingestion semantics.

### 2. `Seer` owns correctness-sensitive recovery contracts

`Seer` is not just a pre-filter. It owns several correctness mechanisms that are intentionally more permissive than a simple watched-pool gate:

- `handle_trade_event(...)`
- `buffer_pending_trade(...)`
- `register_curve_mapping(...)`
- `replay_pending_trades_from_state(...)`

These paths protect live ordering races such as:

$$TRADE \rightarrow CREATE$$

where a trade may arrive before the corresponding create/mapping event is fully known.

### 3. Existing `Seer` tests explicitly defend permissive forwarding/replay

The current codebase includes tests that require `Seer` to forward or buffer some trades even when the pool is not yet in a watched/canonical state, including:

- `test_should_forward_grpc_trade_even_when_pool_not_watched`
- `test_should_forward_grpc_trade_on_mapping_conflict_with_known_mint`
- concurrent create/trade ordering tests and pending-trade replay tests

This means a hard upstream drop in `Seer` would not be a small optimization; it would be a behavioral contract change with real regression risk.

### 4. `tx_filter_strategy` is not currently the runtime gate users might assume

Although configuration names suggest `PerPool` filtering, the actual trade-forward decision path in `Seer` is intentionally more liberal and does not act as a strict watched-pool firewall for `grpc_global_stream` traffic.

Therefore, moving the cold-start suppression into `Seer` would effectively redefine `Seer` semantics rather than simply tightening an existing check.

## Consequences

### Positive

- Cold-start stale orphan spam is removed at the place where it is cheapest and safest to suppress.
- Canonical runtime behavior is preserved.
- `Seer` replay/mapping/race recovery semantics remain intact.
- The fix is narrowly scoped and easier to reason about during incident analysis.

### Negative / trade-offs

- Some non-canonical old-pool traffic may still be parsed upstream before being ignored by runtime.
- The filtering boundary is later than theoretically possible, but intentionally so.

## Operational rule

For the current architecture:

- **A pool becomes canonical for runtime purposes only after live session detection** (`CreatePool` / `NewPoolDetected`).
- Trade-only observations for unknown pools may exist upstream, but must not create runtime orphan state or canonical lifecycle on their own.

## Validation

The runtime contract is covered by:

- `ghost-launcher/tests/gatekeeper_v2_pipeline_integration.rs::test_runtime_router_does_not_spawn_unknown_pool_from_tx_only`

Expected invariant:

- unknown tx-only pool stays fully invisible to runtime orphan buffering (`total_orphans == 0`)

Additionally, `Seer` tests defending trade buffering/replay behavior were used as architectural evidence for **not** moving this filter upstream.

## Non-goals

This decision does **not**:

- redefine `Seer` stream semantics,
- turn watchlists into a hard forward/drop firewall,
- remove all upstream processing cost for old pools,
- change create/trade race recovery behavior.

## Follow-up

If a future upstream optimization is considered again, it must first prove all of the following:

1. no regression in `TRADE -> CREATE` recovery,
2. no regression in mapping-conflict recovery,
3. no regression in pending-trade replay invariants,
4. explicit updated tests documenting the new `Seer` contract.

Until then, cold-start stale-pool suppression remains a `ghost-launcher::oracle_runtime` responsibility.
