# ADR-0084: Jito status polling pinned to ACK host

**Date:** 2026-04-07
**Status:** Accepted
**Author:** Ghost Father

## Context

Live Jito bundle submission was recently migrated to explicit gRPC transport with regional failover. Runtime evidence showed that `sendBundle` ACKs were coming back from one regional block-engine host (for example `frankfurt.mainnet.block-engine.jito.wtf`), while subsequent status polling was still using the client's default JSON-RPC base URL (for example `amsterdam.mainnet.block-engine.jito.wtf`).

This created a cross-region observation race: the bundle was acknowledged by one region, but `getInflightBundleStatuses` / `getBundleStatuses` could be polled against another region before status replication converged. The result was transient `Invalid` / `400 Bad Request` observations that were not attributable to the gRPC submit transport itself.

## Decision

Status polling is now pinned to the exact Jito host that acknowledged `sendBundle`.

Implementation details:
- `send_bundle_request_to_endpoint(...)` now returns both the Jito `bundle_uuid` and the exact `submit_endpoint` that produced the ACK.
- `JitoBundleSubmission` now carries `submit_endpoint` alongside `bundle_uuid` and transaction signatures.
- `get_bundle_status_by_uuid(...)`, `get_final_bundle_status(...)`, `wait_for_bundle_status(...)`, and `wait_for_bundle_status_with_timeout(...)` now accept the submit endpoint and build a region-matched status SDK base URL from it.
- `submit_bundle_and_confirm(...)`, `submit_single_transaction(...)`, `confirm_bundle_submission(...)`, and `bundle_builder.rs` now forward the ACK host into the status poll path.

## Architectural Impact

This change tightens the coupling between Jito submit receipts and Jito status polling:
- Bundle receipts are now region-aware, not just UUID-aware.
- Confirmation logic must preserve the ACK host through every layer that can block on status.
- Future transport abstractions must treat `bundle_uuid` and `submit_endpoint` as a pair for correctness.

## Risk Assessment

**Risk:** Medium

- The change touches the core live BUY/SELL confirmation path.
- Public APIs inside the `trigger` crate changed signatures and receipt shape.
- However, the risk is bounded by targeted unit coverage proving that status polling honors the submit host and that fail-closed rejection reconciliation still works.

## Consequences

### Positive
- Eliminates regional mismatch between `sendBundle` ACK and status polling.
- Reduces false-negative bundle observations caused by querying the wrong block-engine region.
- Makes Jito runtime evidence more trustworthy by logging and preserving the exact ACK host.

### Negative
- Bundle receipts are slightly larger and more stateful.
- Any future code path that reconstructs bundle status polling must preserve `submit_endpoint` or fall back explicitly.

## Alternatives Considered

### 1. Keep polling on the client's default JSON endpoint
Rejected because runtime evidence already showed Frankfurt ACKs with Amsterdam status polling, which is operationally inconsistent under regional replication lag.

### 2. Randomize or sweep status polling across regions
Rejected because it increases ambiguity and can mix stale / divergent regional observations instead of anchoring to the authoritative ACK source.

### 3. Poll only the public alias `mainnet.block-engine.jito.wtf`
Rejected because the live submit transport is explicitly region-aware and the alias does not guarantee immediate regional convergence semantics for just-submitted bundles.

## Validation Steps

- Run `cargo test -p trigger test_get_bundle_status_by_uuid_uses_submit_endpoint_host_for_polling -- --nocapture`
- Run `cargo test -p trigger test_confirm_bundle_submission_rejected_bundle_keeps_tip_signature_offchain -- --nocapture`
- Confirm runtime logs show:
  - gRPC ACK endpoint host
  - status polling using the same normalized host
  - no Frankfurt→Amsterdam mismatch for a single bundle lifecycle
