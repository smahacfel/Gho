# ADR-0081: Jito gRPC SendBundle auth gap and Frankfurt runtime evidence

**Date:** 2026-04-07
**Status:** Accepted
**Author:** Ghost Father

## Context
A dual live rerun on 2026-04-07 still reported in-flight bundle status `Invalid` after fixing `packet.meta.size` in `build_grpc_send_bundle_request(...)`. The fresh live bundle `58367a7ad55ea8b68ca4c937343dbde496ff43dd88cae847e6303332cff77652` received a gRPC submit ACK and then transitioned from `Invalid` to `Rejected` without any structured reason. We needed to verify whether the remaining fault domain was payload construction, gRPC transport, or gRPC authentication.

## Decision
We treat the current Jito gRPC SendBundle path as unauthenticated at the application metadata layer until proven otherwise.

Confirmed facts from `off-chain/components/trigger/src/jito_client.rs`:
- The gRPC send path builds a raw tonic channel with `Endpoint::from_shared(...).connect()`.
- `SearcherServiceClient` is created directly from that channel.
- `send_bundle` uses `tonic::Request::new(request)` with no interceptor and no metadata injection.
- `self.uuid` is not attached to the gRPC request; the code explicitly logs that UUID-configured gRPC submit currently runs without UUID metadata.
- There is no keypair-signature auth interceptor on the gRPC SendBundle path.

Confirmed runtime evidence from `logs/rollout/dual-micro-live/system.log.2026-04-07`:
- The live ACK for bundle `58367a7ad55ea8b68ca4c937343dbde496ff43dd88cae847e6303332cff77652` was sent through endpoint `https://frankfurt.mainnet.block-engine.jito.wtf/`.
- The fixed public gRPC failover order remains Frankfurt -> Amsterdam -> London -> Dublin.

## Architectural Impact
This decision narrows the active fault domain for `Invalid` bundle statuses:
- `build_grpc_send_bundle_request(...)` packet sizing is no longer the sole or primary suspect in this live path.
- The gRPC transport/auth layer is now a first-class investigation target.
- Any future remediation must preserve the live failover contract while adding explicit gRPC auth semantics, if Jito requires them.

## Risk Assessment
**Rate:** High

If Jito expects request metadata or another auth handshake on gRPC SendBundle, the current implementation can keep producing accepted transport ACKs followed by `Invalid`/`Rejected` status outcomes, wasting live opportunities and obscuring the real root cause.

## Consequences
### Positive
- Investigation focus is tightened around gRPC auth/metadata rather than packet body shape alone.
- Runtime evidence now confirms the actual regional host used in live execution.

### Negative
- The system currently maintains a split-brain auth model: SDK/JSON status path can hold UUID context while direct gRPC submit does not.
- Operators may assume UUID auth is active on gRPC when it is not.

## Alternatives Considered
1. **Assume `packet.meta.size` was the root cause**
   - Rejected because live evidence after the fix still produced `Invalid` immediately after ACK.
2. **Assume region mismatch or random endpoint rotation caused the issue**
   - Rejected as primary cause for this run because the live log shows a deterministic Frankfurt endpoint on the first-round ACK.
3. **Assume payload bytes remain malformed despite ACK**
   - Not fully eliminated, but deprioritized relative to the explicit lack of gRPC auth metadata.

## Validation Steps
1. Capture the exact gRPC request metadata sent on `send_bundle` and confirm it is empty.
2. Compare current send path against Jito’s expected authenticated gRPC flow, including UUID or signer-based metadata if required.
3. Re-run dual live using the same rollout lane after adding explicit gRPC auth instrumentation.
4. Verify whether `Invalid` disappears before the later `Rejected` terminal state.
