# P3.7-J2b Shadow Dispatch Join-Key Harness

Date: 2026-05-19

Status: **PASS_CODE_HARNESS**

Decision:

- Shadow dispatch join-key propagation is validated at code/test harness level.
- Full R14 collection remains **HOLD**.
- Runtime validation remains pending for **P3.7-J2c** with real or controlled shadow rows.
- No P2, live, runtime threshold tuning, IWIM change, live sender change, or active policy mutation was performed.

## Context

P3.7-J2 sentinel produced V3/MFS decision-side evidence, but no Gatekeeper BUY and no shadow dispatch artifacts.

Observed J2 result:

- V3/V2.5 rows: `505`
- strict full replay: `PASS`
- decision-side `ab_record_id` / V3 hash coverage: `100%`
- shadow transport rows: `0`
- shadow entry rows: `0`
- shadow lifecycle rows: `0`

Therefore the runtime chain remained unobserved:

`decision -> shadow transport -> shadow entry -> shadow lifecycle`

J2b adds a deterministic code-level harness for that chain without changing active policy.

## Harness Scope

Synthetic join metadata used by the harness:

- `ab_record_id`
- `candidate_id`
- `pool_id`
- `base_mint`
- `decision_ts_ms`
- `v3_feature_snapshot_hash`
- `v3_policy_config_hash`
- `decision_plane`
- `rollout_namespace`

Validated surfaces:

- shadow transport row serialization
- shadow dispatch lifecycle row serialization
- canonical `shadow_entries.jsonl` row serialization from shadow-simulated receipt
- shadow dispatch lifecycle row emitted by `apply_trigger_dispatch_receipt`
- post-buy guardian shadow lifecycle row inherited from `PositionEventContext`
- audit fixture classification for `exact_ab_record_id`
- legacy shadow rows without `ab_record_id`

## Code Changes

- `ghost-launcher/src/components/trigger/shadow_run.rs`
  - expanded `shadow_join_metadata_flows_from_request_to_transport_and_dispatch_records`
  - added serialized JSON assertions for transport and dispatch lifecycle rows
  - added legacy parse test for shadow transport rows without join metadata

- `ghost-launcher/src/oracle_runtime.rs`
  - expanded `p5_shadow_dispatch_lifecycle_writes_closed_with_idempotency_join_key_rollout_profile`
  - validates canonical shadow entry JSON and dispatch lifecycle JSON carry the same join metadata

- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
  - added additive `readiness.join_key_acceptance`
  - `exact_ab_record_id` + ready lifecycle feature join => `pass`

- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
  - expanded fixture with full V3 hash / decision plane / namespace metadata
  - asserts `join_key_acceptance=pass`
  - keeps candidate-id-only legacy path degraded, not failed parsing

## Validation

Commands run:

```bash
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo test -p ghost-launcher shadow_join_metadata_flows_from_request_to_transport_and_dispatch_records -- --nocapture
cargo test -p ghost-launcher p5_shadow_dispatch_lifecycle_writes_closed_with_idempotency_join_key_rollout_profile -- --nocapture
cargo test -p ghost-brain shadow_lifecycle_join_metadata_is_inherited_from_position_context -- --nocapture
cargo test -p ghost-launcher legacy_shadow_transport_without_join_metadata_still_parses -- --nocapture
rustfmt --edition 2021 --check ghost-launcher/src/components/trigger/shadow_run.rs ghost-launcher/src/oracle_runtime.rs
git diff --check
```

Results:

- Python audit tests: `PASS`
- shadow transport harness: `PASS`
- canonical entry / dispatch lifecycle harness: `PASS`
- post-buy guardian lifecycle metadata inheritance: `PASS`
- legacy transport parse: `PASS`
- touched Rust file format check: `PASS`
- whitespace check: `PASS`

Warnings observed are pre-existing broad crate warnings from targeted Rust test compilation.

## Compatibility

Legacy compatibility is preserved:

- shadow transport rows without `ab_record_id` still deserialize
- audit fixture without `ab_record_id` remains classified as degraded `exact_candidate_id`
- no legacy artifact is promoted to `exact_ab_record_id`

## Remaining Gap

J2b does not replace runtime validation.

Still unproven in runtime:

- real Gatekeeper BUY -> shadow transport with `ab_record_id`
- shadow transport -> shadow entry with `ab_record_id`
- shadow entry -> guardian lifecycle with `ab_record_id`
- shadow-onchain lifecycle report over forward V3/MFS rows

## Decision

P3.7-J2b result: **PASS_CODE_HARNESS**

Full R14 collection: **HOLD**

Next gate:

**P3.7-J2c runtime validation** using real or controlled shadow rows.

Full R14 may start only after J2c shows shadow artifacts with stable join-key coverage, or after an explicitly accepted operator decision that code-harness proof is sufficient for a bounded collection attempt.
