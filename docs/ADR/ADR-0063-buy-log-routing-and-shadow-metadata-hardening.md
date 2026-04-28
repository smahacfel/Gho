# ADR-0063: BUY Log Routing and Shadow Metadata Hardening

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Post-refactor production forensics showed that the canonical BUY-only log file `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` had nearly dried up even though BUY-path activity was still observable in `gatekeeper_v2_decisions.jsonl`.

The investigation established three distinct facts:

1. The live runtime is already on the session/materialized-feature architecture described in `PLANS/REFACTOR.md`; the older forensic picture in `ADR-0054` is no longer a complete description of the current code.
2. The user compared two different runtime modes: current `paper + shadow_only` output versus older `live_and_shadow` output. This explains part of the observed field-shape degradation, but not the starvation of the canonical BUY-only file.
3. Fresh BUY-path records were present in `gatekeeper_v2_decisions.jsonl` with BUY-style telemetry, but some of them were missing canonical BUY verdict metadata (`decision_verdict_buy`, `verdict_type`, and deterministic routing context). Because `DecisionLogger` only mirrors `decision_verdict_buy == true` records into `gatekeeper_v2_buys.jsonl`, those records were silently stranded in the all-decisions file.

A second degradation also remained on the BUY path: when local task state contained incomplete pool metadata, the runtime would return early with `LocalTaskState` instead of attempting to upgrade from richer runtime metadata or observation identity, causing `shadow_ready=false` and lower-quality BUY records in `shadow_only` mode.

## Decision

The BUY path in `ghost-launcher/src/oracle_runtime.rs` is hardened as follows:

1. `enrich_buy_log_with_observation_identity(...)` now explicitly populates:
   - `observation_start_ts_ms`
   - `observation_end_ts_ms`
   - `observation_window_ms`
2. Shadow metadata hydration no longer blindly accepts incomplete local task-state metadata. The runtime now:
   - scores candidate metadata completeness,
   - chooses the richer runtime source between runtime registry and runtime state snapshot,
   - merges incomplete local metadata with runtime fallback and observation identity.
3. BUY-path JSONL emission now force-establishes canonical BUY routing semantics before write:
   - `decision_verdict_buy = true`
   - `verdict_type = "BUY"`
   - `decision_reason` fallback when absent
   - deterministic `ab_record_id` fallback when absent
4. Regression tests were added/expanded to cover:
   - explicit observation window field enrichment,
   - richer-source selection for shadow metadata,
   - incomplete-local metadata upgrade,
   - BUY routing fallback hardening.

## Architectural Impact

This preserves the current post-refactor SSOT:

- Session/materialized-feature runtime remains authoritative for verdict generation.
- `DecisionLogger` remains authoritative for canonical file routing semantics.
- `OracleRuntime` is now responsible for ensuring BUY-path records are fully materialized before they reach the logger.

The change does **not** reintroduce legacy Gatekeeper compat paths, does **not** change execution mode semantics, and does **not** alter the `DecisionLogger` contract. Instead, it makes the BUY path satisfy that contract deterministically.

## Risk Assessment

**Rate:** Medium

Primary risks:

- BUY-path logs may change shape for fields previously emitted as `null`.
- Metadata source selection may prefer a different runtime source than before when one source is demonstrably richer.
- Downstream tooling that implicitly relied on missing BUY fields may observe more complete payloads.

Regression blast radius is limited to BUY-path log hydration and canonical BUY-file routing. Reject/timeout routing semantics are unchanged.

## Consequences

### Positive

- Canonical BUY-only file routing is hardened against missing upstream decision metadata.
- BUY-path records contain explicit observation-window fields needed by downstream coverage/forensics tooling.
- Incomplete local task-state metadata can be upgraded instead of immediately degrading to `shadow_skipped_not_ready` semantics.
- The RCA is now anchored in live runtime behavior rather than historical ADR assumptions alone.

### Negative / Trade-offs

- BUY-path hydration logic is more explicit and slightly more complex.
- Runtime metadata source selection now has scoring logic that must stay aligned with shadow-readiness requirements.

## Alternatives Considered

### 1. Fix only `DecisionLogger`
Rejected because it would treat the symptom, not the root cause. The BUY path must emit semantically correct BUY records before they reach logging.

### 2. Revert to older legacy Gatekeeper log construction
Rejected because it would violate the refactor completion boundary and reintroduce deprecated semantics.

### 3. Leave local task-state metadata authoritative even when incomplete
Rejected because this was directly responsible for degraded `shadow_only` BUY records and avoidable `shadow_ready=false` outcomes.

## Validation Steps

1. Re-run targeted unit tests in `ghost-launcher`:
   - `oracle_runtime::tests::test_enrich_buy_log_with_observation_identity_populates_all_fields`
   - `oracle_runtime::tests::test_choose_shadow_metadata_pool_prefers_richer_snapshot`
   - `oracle_runtime::tests::test_merge_local_buy_path_pool_data_upgrades_incomplete_local_metadata`
   - `oracle_runtime::tests::test_enforce_buy_log_buy_routing_backfills_missing_buy_fields`
2. Inspect fresh output in:
   - `logs/decisions.jsonl/gatekeeper_v2_decisions.jsonl`
   - `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`
3. Confirm new BUY-path rows always carry:
   - `decision_verdict_buy:true`
   - `verdict_type:"BUY"`
   - populated `observation_*` fields
   - canonical `ab_record_id`
4. Confirm `shadow_metadata_source` and `shadow_missing_fields` reflect merged/hydrated metadata rather than incomplete local task state where runtime fallbacks exist.
5. Note: a broad `cargo test -p ghost-launcher oracle_runtime::tests::` run still contains pre-existing unrelated failures outside this change surface; targeted regression tests for this ADR pass.
