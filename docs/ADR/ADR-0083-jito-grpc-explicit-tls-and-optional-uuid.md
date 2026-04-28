# ADR-0083: Jito gRPC explicit TLS and optional UUID auth

**Date:** 2026-04-07
**Status:** Accepted
**Author:** Ghost Father

## Context

Live Jito `SendBundle` transport was using a `tonic::transport::Endpoint` built from an `https://` authority, but without an explicit `tls_config(...)` step.

At the same time, launcher-side runtime guards had been hardened to require a non-empty `jito_uuid` before any live BUY/SELL dispatch could reach the gRPC submit path.

That made two problems overlap during diagnostics:

1. transport-layer TLS behavior was implicit instead of explicit,
2. missing UUID could block entry into the actual gRPC `send_bundle` path even when unauthenticated submission needed to be tested.

## Decision

Two changes were accepted together:

1. Jito gRPC `Endpoint` now applies explicit TLS configuration through `Endpoint::tls_config(ClientTlsConfig::new())` whenever the endpoint scheme is `https`.
2. `jito_uuid` is now optional for live Jito transport. If present and non-empty, it is attached as `x-jito-auth`; if absent, launcher still submits over gRPC without the auth metadata header.

Placeholder UUID values remain invalid when explicitly supplied.

## Architectural Impact

This keeps Jito gRPC transport explicit and deterministic:

- HTTPS endpoints always configure tonic TLS intentionally,
- UUID auth becomes an optional transport adornment rather than a hard gate for entering the submit path,
- runtime and preflight can now test unauthenticated `SendBundle` behavior without disabling Jito transport itself.

## Risk Assessment

**Risk:** Medium

TLS explicitness lowers transport ambiguity.

Making UUID optional increases reachability of the live submit path, but it can also cause real block-engine rejections when auth is actually required by the remote environment. This is acceptable for diagnostics because the transport path itself must remain observable.

## Consequences

What becomes easier:

- isolating transport/TLS issues from auth issues,
- running live diagnostics against the real gRPC path,
- comparing authenticated and unauthenticated `SendBundle` behavior.

What becomes harder:

- operators can no longer rely on launcher guards to fail-close purely because UUID is absent,
- distinguishing auth rejection from payload rejection now depends on runtime evidence rather than startup validation.

## Alternatives Considered

### 1. Keep UUID mandatory and only add TLS

Rejected because it still blocks direct observation of unauthenticated `SendBundle` behavior.

### 2. Add fake UUIDs during diagnostics instead of changing guards

Rejected because it muddies the forensic picture by changing request metadata while claiming to test the no-UUID path.

### 3. Leave TLS implicit and only relax UUID

Rejected because it preserves ambiguity in the tonic gRPC handshake path.

## Validation Steps

1. Build release successfully after adding `ClientTlsConfig`.
2. Confirm launcher starts and can reach live Jito submit path without `jito_uuid`.
3. Confirm authenticated path still attaches `x-jito-auth` when UUID is provided.
4. Observe `gRPC SendBundleRequest before submit` during a real submit attempt.