# ADR-0048: Jito gRPC Bundle Submit Transport Migration

**Date:** 2026-03-28
**Status:** Proposed
**Author:** Ghost Father

## Context
`off-chain/components/trigger/src/jito_client.rs` currently submits Jito bundles through JSON-RPC over `reqwest` inside `submit_bundle_request_with_failover`, while the rest of the client already depends on `jito-sdk-rust` for bundle status APIs. The requested migration is intentionally narrow: preserve higher-level call sites, keep bundle status polling behavior stable, and replace only the submission transport with Jito gRPC.

The existing implementation normalizes operator-provided endpoints to `/api/v1/bundles`, retries across public regions, preserves optional UUID-based authentication for JSON-RPC headers, and passes bundle payloads around as the current JSON structure:

`[[<base64 signed tx>...], {"encoding":"base64"}]`

The migration must preserve observable behavior for callers while changing the submission path from JSON-RPC to `SearcherService/SendBundle`.

## Decision
Adopt a split-transport Jito client:

1. Keep JSON-RPC endpoint normalization for probe logic and `jito-sdk-rust` status APIs.
2. Add a second, gRPC-normalized endpoint representation used only for `SearcherService` bundle submission.
3. Preserve `submit_bundle_request_with_failover(&Value) -> Result<String>` as the stable internal seam for higher-level callers.
4. Convert the existing JSON `params` payload into a gRPC `SendBundleRequest { bundle: Some(Bundle { header: None, packets }) }`.
5. Preserve optional UUID state on `JitoClient`, but do not rely on it for gRPC auth unless the deployment has confirmed support for UUID->metadata translation. Default gRPC submission should work without auth metadata on public endpoints; authenticated metadata should be added only behind an explicit implementation path.

## Architectural Impact
The client becomes explicitly dual-stack:

- **JSON-RPC path remains authoritative for**
  - `probe_jito_endpoint`
  - `JitoJsonRpcSDK`
  - `get_bundle_statuses`
  - `get_in_flight_bundle_statuses`

- **gRPC path becomes authoritative for**
  - `sendBundle`
  - endpoint failover submit attempts
  - packet construction from signed transactions

This isolates transport concerns without changing bundle construction, dry-run logic, diagnostics flow, status polling, or higher-level bundle submission APIs.

## Risk Assessment
**Rate:** Medium

Primary risks:

1. **Endpoint normalization drift** — current code stores `/api/v1/bundles` URLs, but gRPC requires a bare authority URL rather than a JSON-RPC path.
2. **Auth mismatch** — current UUID semantics are JSON-RPC-specific (`x-jito-auth` header / `?uuid=` query). gRPC auth in Jito examples is typically bearer-token metadata, not UUID metadata.
3. **Request translation correctness** — the current code accepts generic JSON params; malformed shapes must fail deterministically before transport.
4. **Retry classification parity** — existing rate-limit and retry behavior is HTTP/JSON specific and must be remapped for tonic transport and gRPC status codes.
5. **Connection reuse behavior** — recreating gRPC channels per attempt may introduce latency if not managed carefully.

## Consequences
### Positive
- Removes bundle submission dependence on JSON-RPC.
- Preserves existing higher-level bundle submission APIs.
- Keeps bundle status APIs unchanged, minimizing blast radius.
- Allows later auth hardening without reopening submission call sites.

### Negative
- `JitoClient` now needs separate normalized endpoint fields for JSON-RPC and gRPC.
- Some UUID semantics become transport-specific and potentially asymmetric.
- Unit tests must move from raw TCP HTTP fixtures to mock gRPC services for submit-path coverage.

## Alternatives Considered
### 1. Full Jito client rewrite to gRPC for all APIs
Rejected because the requested change is intentionally minimal and the current status APIs already work through `jito-sdk-rust`.

### 2. Continue using JSON-RPC submit path and wrap it behind an abstraction
Rejected because it does not satisfy the transport migration objective.

### 3. Infer gRPC auth from existing UUID and always send it as metadata
Rejected because UUID header/query auth is documented for JSON-RPC, while public Jito gRPC examples rely on either no-auth clients or bearer-token interceptors. Blind UUID metadata forwarding would be speculative and unsafe without endpoint-specific confirmation.

## Validation Steps
1. Add unit coverage for JSON-RPC endpoint normalization and new gRPC authority normalization.
2. Add unit coverage for translating the existing `params` JSON into `SendBundleRequest`.
3. Add unit coverage for tonic/gRPC retry classification (unavailable, deadline exceeded, resource exhausted).
4. Replace HTTP mock submit tests with mock gRPC SearcherService tests.
5. Run targeted trigger tests plus a full `cargo test -p trigger jito_client`.
6. In staging, verify:
   - public endpoint submission succeeds without auth metadata,
   - optional authenticated path is exercised only where explicitly configured,
   - bundle UUID returned by gRPC still feeds existing status polling unchanged.
