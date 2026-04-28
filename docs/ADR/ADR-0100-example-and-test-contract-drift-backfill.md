# ADR-0100: Example and Test Contract Drift Backfill

**Date:** 2026-04-15
**Status:** Accepted
**Author:** Ghost Father

## Context
After the earlier `ghost-brain` fixture provenance repair, the workspace baseline still failed on stale non-production call sites. Two `ghost-launcher` examples still invoked `start_oracle_runtime_task(...)` with the pre-expansion 14-argument signature even though the runtime now requires a 15th boolean, `authoritative_funding_stream_available`. Once those examples were repaired, the next baseline failure surfaced in `trigger` tests that still asserted a removed `JitoClient.uuid` field after the auth surface had been split into `grpc_auth` and `status_uuid`.

## Decision
Backfill the stale contracts instead of reintroducing legacy fields or compatibility shims:

1. Update `ghost-launcher/examples/oracle_pipeline_diagnostic.rs` and `ghost-launcher/examples/oracle_validation_comprehensive.rs` to pass `false` for `authoritative_funding_stream_available`, matching existing degraded/test call sites that do not provide an authoritative funding stream.
2. Update `off-chain/components/trigger/src/jito_client.rs` unit tests to assert the current `grpc_auth` / `status_uuid` contract instead of the removed `uuid` field.

## Architectural Impact
This keeps examples and tests aligned with the current SSOT instead of silently widening production APIs for legacy callers. The runtime task contract remains explicit about funding-stream availability, and the Jito client auth model remains split between gRPC auth metadata and REST status UUID semantics.

## Risk Assessment
Low. The change is constrained to examples and tests. No production control flow, wire protocol, account layout, or runtime config semantics were altered.

## Consequences
Workspace compilation no longer fails on stale example/test surfaces, and future regressions should now point at real implementation drift rather than obsolete harness code. The trade-off is that direct call sites and internal-field assertions remain brittle when contracts evolve, so helper constructors or test builders may still be worth introducing later.

## Alternatives Considered
1. Reintroduce a legacy 14-argument overload or compatibility wrapper for `start_oracle_runtime_task(...)`. Rejected because it would preserve stale example code and weaken the explicit runtime contract.
2. Restore a public `uuid` field on `JitoClient`. Rejected because the auth model was intentionally split and the old field name would conflate gRPC auth metadata with REST status polling identity.

## Validation Steps
1. Compile `trigger` library tests with `cargo test -p trigger --lib --no-run --quiet`.
2. Compile the full workspace with `cargo test --workspace --no-run --quiet`.
3. Confirm the prior failures in `ghost-launcher` examples and `trigger` tests no longer appear.
