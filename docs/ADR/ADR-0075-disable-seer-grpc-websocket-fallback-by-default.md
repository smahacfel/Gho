# ADR-0075: Disable Seer gRPC→WebSocket Fallback by Default

**Date:** 2026-04-02
**Status:** Accepted
**Author:** Ghost Father

## Context

A follow-up hardening step was requested after the RPC-usage forensic audit confirmed that Seer is genuinely running on Yellowstone gRPC in production, while separate launcher audit/reconstruction paths account for the dominant observed RPC load.

Even though the sampled runtime did not show Seer actively using the WebSocket fallback path, the fallback flag still existed as a default-enabled setting inside standalone Seer configuration. That left an avoidable legacy transport escape hatch in code and made configuration intent less explicit than required for production operation.

The goal of this change was to make the runtime posture explicit and fail-closed by default:
- gRPC remains the canonical ingest transport
- legacy WebSocket fallback remains available only as an explicit opt-in legacy path
- launcher/runtime config and standalone defaults must agree on the same disabled-by-default posture

## Decision

The gRPC commitment fallback to WebSocket was disabled by default across the relevant Seer and launcher configuration surfaces.

Implemented decisions:
1. `off-chain/components/seer/src/config.rs` now defaults `grpc_commitment_fallback_to_websocket` to `false`.
2. `ghost-launcher/src/config.rs` now exposes `seer.grpc_commitment_fallback_to_websocket` as an explicit config field with a default of `false`.
3. `ghost-launcher/src/components/seer.rs` now passes the launcher config value through instead of hardcoding the field inline.
4. `/root/Gho/config.toml` now explicitly sets `grpc_commitment_fallback_to_websocket = false` under `[seer]`.
5. Logging/tests/comments were updated so the disabled-by-default posture is visible and verified.

The legacy WebSocket path was not deleted. It remains as an explicit, legacy, opt-in compatibility mechanism rather than an ambient default.

## Architectural Impact

This change tightens the ingest SSOT boundary:
- **Canonical live ingest:** Yellowstone gRPC
- **Legacy compatibility path:** WebSocket fallback, disabled unless explicitly enabled

It also removes ambiguity between launcher runtime config and standalone Seer defaults. Before this change, launcher runtime effectively forced `false`, while standalone Seer defaulted to `true`. After this change, both surfaces align on the same fail-closed baseline.

Affected components:
- `off-chain/components/seer`
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/components/seer.rs`
- root runtime config `config.toml`

## Risk Assessment

**Rating:** Low

Primary regression risks:
- environments that silently relied on the legacy fallback without explicitly setting it may now remain on pure gRPC and lose the implicit fallback behavior
- operators must explicitly opt in if they intentionally want legacy fallback during testing or incident response

These risks are acceptable because the desired production contract is gRPC-first and fail-closed, not implicit legacy downgrade.

## Consequences

What becomes easier:
- production intent is explicit in both code and config
- Seer transport behavior is easier to reason about during incident analysis
- accidental fallback activation becomes less likely

What becomes harder:
- emergency use of the legacy WebSocket path now requires deliberate configuration instead of inheriting a permissive default

## Alternatives Considered

### Alternative 1: Keep the standalone Seer default as `true`
Rejected because it preserved an unnecessary gap between standalone defaults and launcher production posture.

### Alternative 2: Delete the WebSocket code path entirely
Rejected for this change because the immediate goal was hardening defaults/configuration, not full legacy transport removal.

### Alternative 3: Leave the launcher hardcoded to `false` without surfacing the field in launcher config
Rejected because it hid runtime intent in code and made `config.toml` unable to represent the setting explicitly.

## Validation Steps

1. Verify `SeerConfig::default()` returns `grpc_commitment_fallback_to_websocket = false`.
2. Verify launcher config deserialization accepts and defaults `seer.grpc_commitment_fallback_to_websocket` to `false`.
3. Verify launcher runtime passes the configured value into `SeerConfig`.
4. Confirm active root config under `[seer]` explicitly sets `grpc_commitment_fallback_to_websocket = false`.
5. Re-run focused validation:
   - `cargo test -p seer test_default_config --lib`
   - `cargo test -p ghost-launcher --test seer_connection_mode_test`
