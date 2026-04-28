# ADR-0069: Shadow readiness without observed BUY transaction

**Date:** 2026-04-01
**Status:** Accepted
**Author:** Ghost Father

## Context

After the metadata and account-override hardening recorded in `ADR-0068`, residual shadow BUY failures still appeared in production logs with the following shape:

- `core_pass=true`
- `shadow_ready=false`
- `shadow_missing_fields=["observed_buy_tx"]`
- `shadow_execution_outcome="shadow_skipped_not_ready"`

The failing records commonly showed `shadow_metadata_source="runtime_registry"`, which meant canonical runtime metadata was already present. Additional log evidence showed that many of these cases advanced far enough to surface downstream outcomes such as `shadow_insufficient_balance`, confirming that the blocker was not missing account semantics but a stale readiness gate.

Code investigation confirmed a contract mismatch:

1. `ghost-launcher/src/oracle_runtime.rs` still treated a successful buffered observed BUY transaction as mandatory for shadow readiness.
2. `ghost-launcher/src/components/trigger/component.rs::prepare_buy_request(...)` does not require observed BUY telemetry when canonical metadata and point-in-time chain state are available.
3. `off-chain/components/trigger/src/direct_buy_builder.rs` already canonicalizes BUY accounts and safely ignores invalid `associated_bonding_curve` overrides.

This left `observed_buy_tx` acting as a historical proxy for readiness even after the real safety requirements had shifted to validated metadata plus current chain state.

## Decision

`observed_buy_tx` is no longer part of the shadow BUY readiness contract.

Implemented decisions:

1. `compute_shadow_run_readiness(...)` no longer appends `"observed_buy_tx"` to missing fields when buffered observed BUY telemetry is absent.
2. Observed BUY transactions remain optional enrichment sources for metadata and forensic context, but they do not gate shadow execution when canonical metadata is complete.
3. Regression tests now assert that complete metadata may proceed without any buffered observed BUY transaction.
4. Shadow-only readiness tests exercise the full preparation path using a deterministic local mock RPC server and a temporary test keypair, matching the current transport dependency of `prepare_buy_request(...)`.

## Architectural Impact

This decision tightens the system around the current SSOT for shadow BUY execution:

- `OracleRuntime` decides readiness from canonical metadata completeness.
- `TriggerComponent` prepares BUY requests using current chain state fetched through point queries.
- `DirectBuyBuilder` remains the canonical enforcer for BUY account layout.
- Observed BUY telemetry is demoted from readiness requirement to optional signal enrichment.

Practically, this reduces false-negative shadow skips while preserving the stricter creator/account hardening introduced in `ADR-0068`.

## Risk Assessment

**Rate:** Medium

Primary risks:

- removing `observed_buy_tx` as a gate may allow shadow preparation to proceed earlier in pool lifecycles,
- deeper execution paths are now reached in cases that were previously skipped before preparation,
- transport assumptions remain coupled to RPC for point-in-time preparation reads.

These risks are acceptable because the runtime now gates on the real prerequisites for request preparation rather than on stale telemetry presence.

## Consequences

### Positive

- residual `shadow_skipped_not_ready` cases caused solely by missing observed BUY telemetry are eliminated,
- readiness semantics now match the actual requirements of the BUY preparation path,
- production logs become more meaningful because failures move from stale readiness gates to real preparation/simulation outcomes,
- shadow-only tests now cover the full preparation path more realistically.

### Trade-offs

- tests require a local mock RPC service because the preparation path still depends on point-in-time RPC-style queries,
- `observed_buy_tx` can no longer be used as a shorthand proxy for “the system has seen enough to proceed”,
- future transport abstraction work is still needed if preparation is to rely on gRPC-backed cached state instead of RPC calls.

## Alternatives Considered

### 1. Keep `observed_buy_tx` as a mandatory readiness field

Rejected because the production evidence showed it had become a stale proxy and was skipping cases where canonical metadata was already sufficient.

### 2. Reintroduce observed-BUY dependence indirectly through account-override hydration

Rejected because `ADR-0068` explicitly moved the system away from trusting literal observed overrides ahead of canonical derivation.

### 3. Fully migrate preparation-time point queries to gRPC before changing readiness

Rejected for this fix because it expands the blast radius unnecessarily. The immediate defect was a stale readiness contract, and removing that gate was the minimal safe correction.

## Validation Steps

Validated in this session with targeted checks:

1. Source diagnostics on touched file:
   - no editor errors in `ghost-launcher/src/oracle_runtime.rs`

2. Targeted readiness and shadow-only test batch:
   - `cargo test -p ghost-launcher --lib reports_missing_required_fields -- --nocapture`
   - `cargo test -p ghost-launcher --lib shadow_run_readiness -- --nocapture`
   - `cargo test -p ghost-launcher --lib test_shadow_readiness -- --nocapture`
   - `cargo test -p ghost-launcher --lib shadow_only_skips_when_shadow_readiness_is_incomplete -- --exact --nocapture`
   - `cargo test -p ghost-launcher --lib shadow_only_derives_initial_liquidity_from_curve_context -- --exact --nocapture`
   - result: terminal exit code `0`

3. Production-log evidence review before patch:
   - repeated records in `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` showed `shadow_missing_fields=["observed_buy_tx"]` with `shadow_metadata_source="runtime_registry"`
   - this was used as the acceptance target for the readiness-contract correction
