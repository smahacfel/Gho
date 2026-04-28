# ADR-0071: Live BUY/SELL Jito gRPC-only transport

**Date:** 2026-04-01
**Status:** Accepted
**Author:** Ghost Father

## Context

A strict transport requirement was requested for production execution:

- live BUY transactions must be executed only through Jito Bundle over gRPC,
- live SELL transactions must be executed only through Jito Bundle over gRPC.

Code review before this change showed that the architecture only partially met that requirement:

1. `ghost-launcher/src/components/trigger/component.rs` already had a Jito submission path for live BUY, but the live dispatch flow still retained an RPC fallback.
2. `ghost-launcher/src/components/post_buy_runtime.rs`, which is the launcher SSOT for live SELL execution, still submitted SELL bullets through async RPC.
3. the launcher could silently degrade live SELL handling by omitting `LiveSellHandle` at startup or by routing a live lane into the paper lifecycle when the live handle was missing.
4. config/startup validation did not fully enforce that live-capable execution profiles had a valid Jito transport contract.

This created an undesirable gap between the intended production architecture and the actual failover behavior. In particular, a supposedly live path could still submit over RPC or degrade into paper semantics instead of failing closed.

## Decision

Live production execution is now fail-closed on a Jito Bundle gRPC contract.

Implemented decisions:

1. `ghost-launcher/src/config.rs` now rejects live-capable execution profiles unless:
   - `trigger.use_jito == true`, and
   - `trigger.jito_endpoint` is present and non-blank.
2. `ghost-launcher/src/components/trigger/component.rs` no longer permits live BUY RPC fallback:
   - `dispatch_prepared_buy_with_shadow(...)` guards live modes with `ensure_live_jito_transport(...)`,
   - live BUY submission always uses the Jito path,
   - missing/invalid Jito transport causes a fail-closed live dispatch outcome.
3. `ghost-launcher/src/main.rs` now builds `LiveSellHandle` through a fail-closed helper for `ExecutionMode::Live` and `ExecutionMode::Dual`:
   - requires a readable trigger keypair,
   - requires a non-empty Jito endpoint,
   - constructs `JitoClient` explicitly for the live SELL path.
4. `ghost-launcher/src/components/post_buy_runtime.rs` now submits live SELL bullets through `JitoClient::submit_single_transaction_and_confirm(...)` instead of async RPC.
5. when a live lane has no live SELL handle, the launcher no longer degrades into the paper lifecycle; it releases the slot and fails closed.

## Scope Clarification

This ADR applies to **execution transport** for live BUY/SELL transaction submission.

It does **not** state that every preparation-time read must already be migrated away from RPC. Read-only operations such as point-in-time state queries or balance checks may still use RPC where required. The enforced contract is that actual live transaction execution/landing is Jito Bundle over gRPC only.

## Architectural Impact

This decision aligns the runtime SSOTs with a single live transport contract:

- live BUY authority remains in `ghost-launcher/src/components/trigger/component.rs`, now without RPC submit fallback,
- live SELL authority remains in `ghost-launcher/src/components/post_buy_runtime.rs`, now using Jito gRPC bundle submission,
- startup authority remains in `ghost-launcher/src/main.rs`, now fail-closing if the live SELL Jito prerequisites are missing,
- configuration authority remains in `ghost-launcher/src/config.rs`, now fail-closing invalid live/dual profiles before runtime.

Operationally, this removes ambiguous runtime behavior where production traffic could silently drift onto RPC submit or paper fallback semantics.

## Risk Assessment

**Rate:** Medium

Primary risks:

- stricter startup/config validation can now prevent launch in environments that previously booted with incomplete live transport configuration,
- live execution becomes more sensitive to Jito endpoint/keypair misconfiguration because fallback behavior is intentionally removed,
- SELL-path transport migration changes confirmation semantics and may surface integration assumptions that were previously masked by RPC submit behavior.

These risks are acceptable because they enforce the requested production architecture and prevent silent transport drift.

## Consequences

### Positive

- live BUY and live SELL now share the same explicit transport policy: Jito Bundle over gRPC only,
- runtime behavior is easier to reason about because misconfiguration fails closed instead of degrading silently,
- live SELL no longer piggybacks on paper lifecycle behavior when the live path is unavailable,
- config fixtures and tests now encode the Jito-only contract directly.

### Trade-offs

- operators must provide a valid Jito endpoint and keypair for any live-capable profile,
- launcher startup is less permissive by design,
- some read-only RPC dependencies still remain outside the scope of this ADR.

## Alternatives Considered

### 1. Keep BUY on Jito but allow SELL to continue on RPC

Rejected because the requested contract explicitly required both sides of live execution to use Jito Bundle gRPC.

### 2. Keep RPC fallback as an emergency transport

Rejected because fallback behavior blurs the architecture and allows production execution to violate the declared transport policy.

### 3. Degrade missing live SELL transport into paper lifecycle

Rejected because paper semantics are not an acceptable substitute for a misconfigured live lane.

## Validation Steps

Validated in this session with targeted checks:

1. Editor diagnostics:
   - no editor errors in:
     - `ghost-launcher/src/config.rs`
     - `ghost-launcher/src/components/trigger/component.rs`
     - `ghost-launcher/src/components/post_buy_runtime.rs`
     - `ghost-launcher/src/main.rs`
     - `ghost-launcher/tests/post_buy_runtime_integration.rs`

2. Targeted config and trigger coverage:
   - `config::tests::test_validate_execution_profile_accepts_live_transport_with_jito`
   - `config::tests::test_validate_execution_profile_rejects_live_transport_without_jito`
   - `config::tests::test_validate_execution_profile_rejects_live_transport_without_jito_endpoint`
   - `components::trigger::component::tests::live_transport_guard_rejects_blank_jito_endpoint`
   - `components::trigger::component::tests::live_dispatch_fails_closed_without_jito_transport`
   - result: passed

3. Targeted config fixture/startup coverage:
   - `config::tests::test_from_file_rebases_relative_runtime_paths`
   - `config::tests::test_from_file_preserves_explicit_trigger_entry_mode_live`
   - `config::tests::test_startup_smoke_loaded_trigger_mode_reaches_component`
   - result: passed

4. Launcher integration coverage:
   - `cargo test -p ghost-launcher --test post_buy_runtime_integration -- --nocapture`
   - result: `4 passed; 0 failed`

5. Integration regression specifically validated that:
   - live lane no longer falls back to paper when `LiveSellHandle` is absent,
   - live lane still routes into the revolver-based live SELL lifecycle when the live handle is present.
