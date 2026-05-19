# P3.7-J1 Additive Join-Key Repair

Date: 2026-05-19

## Goal

Propagate the stable decision correlation key from V3/Gatekeeper decision artifacts into the shadow-burnin artifact chain before running the full R14 collection.

Target chain:

```text
Gatekeeper/V3 decision row
-> trigger shadow transport
-> shadow_entries.jsonl
-> shadow_lifecycle.jsonl
-> shadow_onchain_lifecycle_report.py
-> labeler / feature availability audit
```

Preferred key: `ab_record_id`.

Additional optional context:

- `v3_feature_snapshot_hash`
- `v3_policy_config_hash`
- `decision_plane`
- `rollout_namespace`

## Problem Statement

R14 smoke confirmed that V3/MFS replay payload emission works, but the join-key audit reported degraded join quality:

- decision artifacts contain `ab_record_id`, V3 hashes, `decision_plane`, and rollout identity,
- shadow transport, shadow entry, and lifecycle artifacts contain `candidate_id`,
- the only common key across decision and shadow artifacts was `mint`.

This is not acceptable for full R14. The purpose of R14 is to collect V3/MFS decision snapshots and shadow-burnin lifecycle truth in one row-joinable dataset. A mint/time-window join is a fallback only, not the primary contract.

## Source Findings

`ab_record_id` is produced in runtime by `enrich_buy_log_with_window()` in `ghost-launcher/src/oracle_runtime.rs`. It uses:

```text
{pool_id}:{t0}:{t_end}:{verdict_tag}
```

For BUY rows, the decision JSONL gets `ab_record_id` after execution returns. Therefore the BUY path must compute the same deterministic value before dispatch and pass it through the shadow handoff.

## Implementation Scope

In scope:

- Add optional join metadata to `PreparedBuyRequest`.
- Copy join metadata into `ShadowBuySimulationReport`.
- Copy join metadata into `ShadowBuySimulationEvent`.
- Add optional fields to `ShadowBuySimulationRecord`.
- Add optional fields to `ShadowDispatchLifecycleRecord`.
- Add optional fields to canonical `shadow_entries.jsonl` rows.
- Pass join metadata into shadow `PostBuySubmitted` handoff and into `PositionEventContext`.
- Persist join metadata in guardian `shadow_lifecycle.jsonl` rows.
- Propagate these fields in `shadow_onchain_lifecycle_report.py` output when present.
- Extend `v3_p37_mfs_lifecycle_join_key_audit.py` to report exact `ab_record_id` coverage.

Out of scope:

- No P2.
- No live enablement.
- No threshold tuning.
- No IWIM changes.
- No live sender changes.
- No Gatekeeper decision behavior changes.
- No MFS extension as active policy.
- No lifecycle label used as a decision-time feature.

## Required Artifact Fields

New rows should carry, when available:

```text
ab_record_id
candidate_id
pool_id
base_mint / mint_id
decision_ts_ms
v3_feature_snapshot_hash
v3_policy_config_hash
decision_plane
rollout_namespace
```

Legacy rows without these fields must continue to parse.

## Acceptance Criteria

Runtime artifact acceptance:

- `trigger.shadow_run.output_path` rows include `ab_record_id` when the BUY decision row has it.
- `shadow_entries.jsonl` rows include `ab_record_id` when the BUY decision row has it.
- `shadow_lifecycle.jsonl` dispatch rows include `ab_record_id`.
- `shadow_lifecycle.jsonl` post-buy rows inherit `ab_record_id` from position state.
- Existing historical rows without `ab_record_id` remain valid.

Audit acceptance:

- join-key audit reports:
  - `decision_rows_with_ab_record_id`
  - `shadow_transport_rows_with_ab_record_id`
  - `shadow_entry_rows_with_ab_record_id`
  - `shadow_lifecycle_rows_with_ab_record_id`
  - `full_chain_ab_record_id_coverage`
  - `join_quality`
- If shadow artifacts exist after repair smoke, exact `ab_record_id` coverage should be the primary join path.
- `mint_only` must not be reported as the primary join quality for full R14 readiness.

Full R14 gate:

- Full R14 remains HOLD until a second smoke shows V3/MFS replay PASS and join-key audit PASS or an explicit governance exception accepts degraded joins.

## Test Plan

```bash
python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo test -p ghost-launcher shadow_join_metadata -- --nocapture
cargo test -p ghost-brain shadow_lifecycle_join_metadata -- --nocapture
git diff --check
```

Optional post-fix smoke:

```bash
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/p3_7_mfs_lifecycle_join_key_audit_after_j1.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_J1_R14_SMOKE_JOIN_KEY_AUDIT_AFTER_REPAIR_20260519.md
```
