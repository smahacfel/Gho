# ADR-0072: Dual Micro Live Fail-Closed Jito Reservation Diagnosis

**Date:** 2026-04-05
**Status:** Accepted
**Author:** Ghost Father

## Context
The dual live rollout started with `configs/rollout/dual-micro-live.toml` produced Gatekeeper PASS events in `datasets/events/dual-micro-live/exec_launcher-1775416812596_20260405_192012_0000.jsonl`, but operators observed no on-chain BUY and no visible execution events in the JSONL stream. Metrics also showed `seer_grpc_connection_status=0`, creating uncertainty about whether the system ever reached the live submit path.

## Decision
The root cause was diagnosed as a fail-closed live-entry reservation chain:

1. OracleRuntime reached Gatekeeper BUY and called Trigger live execution.
2. Trigger prepared a Jito bundle and submitted it via gRPC.
3. Jito returned an ACK, but the bundle later resolved to `Rejected`, and fail-closed on-chain reconciliation still could not prove non-landing within the configured deadline.
4. Trigger treated the landing as uncertain and intentionally kept the position slot reserved.
5. Because `trigger.max_concurrent_positions = 1`, every later Gatekeeper PASS was rejected by bulkhead safety before any second live submission.

The empty execution JSONL was classified as an observability gap, not proof of no submit. The OracleRuntime event emitter is explicitly initialized for Gatekeeper decision events only, so Trigger live submit activity is visible in runtime logs but not in the Gatekeeper JSONL stream.

The `seer_grpc_connection_status=0` signal was also classified as a metrics blind spot. The run had an established stream and continuous event flow; the metric exists in Seer metrics definitions but is not updated anywhere in the Seer source tree.

## Architectural Impact
This diagnosis confirms the authoritative live BUY path is:

`Seer -> OracleRuntime Gatekeeper PASS -> Trigger prepare_buy_request -> Trigger dispatch_prepared_buy_with_shadow -> Jito gRPC submit -> fail-closed reconciliation`

It also confirms two SSOT observability limitations:

- `datasets/events/.../exec_*.jsonl` is not a complete execution ledger for the OracleRuntime-triggered live path.
- `seer_grpc_connection_status` cannot be trusted for runtime health decisions in its current implementation.

## Risk Assessment
**Rate:** High

Regression and operational risks:

- A single uncertain live bundle can stall the lane indefinitely when `max_concurrent_positions=1`.
- Operators can misdiagnose the system as “never attempted execution” because the JSONL stream omits Trigger live submit events.
- Operators can misdiagnose Seer transport health because the gRPC status metric is effectively inert.

## Consequences
- Easier: The absence of on-chain BUY is now concretely explained by one rejected/uncertain first bundle plus fail-closed slot retention.
- Harder: Live operations remain brittle under bundle uncertainty when the lane width is one position.
- Harder: Incident response depends on log inspection instead of the JSONL event stream and current gRPC metric.

## Alternatives Considered
- **Config endpoint failure** — Rejected because runtime logs show real RPC/Jito endpoints, not placeholder `.invalid` values.
- **OracleRuntime never reached live submit path** — Rejected because Trigger prepared buy requests and Jito submit ACKs are present in logs.
- **Seer gRPC outage caused no BUY** — Rejected because the stream established successfully and kept forwarding pool/trade events throughout the run.
- **Pure Gatekeeper rejection issue** — Rejected because at least one PASS advanced into Trigger/Jito submission before the lane locked.

## Validation Steps
1. Verify first PASS path for pool `6Z147wp2BrbUY8LivxVdnAJ6Dh8m2uGRYk6PEq7hXa81`:
   - Gatekeeper BUY log
   - Trigger prepared buy request
   - Jito submit ACK
   - Jito rejection/uncertain landing
   - reserved position slot log
2. Verify later PASS path for pool `mp875F2NfK1nCenQiwymVWzD87r8tDxbsqWhwqQdoT6`:
   - Gatekeeper BUY log
   - Trigger bulkhead rejection with `active_positions=1 max_concurrent_positions=1`
3. Confirm JSONL writer scope in `ghost-launcher/src/oracle_runtime.rs` comments and usage.
4. Confirm `seer_grpc_connection_status` is defined but never updated in `off-chain/components/seer/src`.
