# P3.7-J1 R14 Smoke Report

Date: 2026-05-19

Namespace:

`shadow-burnin-v3-p37-mfs-lifecycle-r14-j1-smoke`

Config:

`configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j1-smoke.toml`

## Decision

P3.7-J1 smoke result:

- V3/MFS replay path: PASS
- Runtime startup/preflight: PASS
- Shadow BUY / lifecycle path: INCONCLUSIVE
- Join-key propagation on real shadow artifacts: INCONCLUSIVE
- Full R14 primary-only collection: HOLD
- P2/live/runtime threshold changes: NO-GO

This smoke must not be treated as a completed J1 runtime join-key validation,
because no shadow transport, shadow entry, or shadow lifecycle rows were emitted.

## Runtime Status

The direct runtime preflight passed for the J1 smoke config.

The 30 minute smoke command exited after the timeout window. No live
`ghost-launcher` process remained after the run.

Runtime entered the expected shadow-only profile:

- `entry_mode = "shadow_only"`
- `execution_mode = "shadow"`
- `funding_lane_mode = "disabled"`
- `trigger.shadow_run.enabled = true`
- `trigger.shadow_run.emit_event_bus = true`

No panic, queue-depth failure, or replay-payload mismatch was found in the
smoke logs. The only direct error-like log found by the narrow scan was the
expected cold-start snapshot warning:

`ShadowLedger restore failed, starting fresh error=no snapshot found`

## Artifact Counts

Decision and event artifacts were emitted in the isolated J1 namespace:

| artifact | rows |
| --- | ---: |
| `v2.2/legacy_live/gatekeeper_v2_decisions.jsonl` | 496 |
| `v2.5/v25_shadow/gatekeeper_v2_decisions.jsonl` | 96 |
| `seer_runtime_coverage_audit.jsonl` | 380 |
| execution event JSONL total | 496 |
| `buys.jsonl` | 0 |
| `shadow_entries.jsonl` | 0 |
| `shadow_lifecycle.jsonl` | 0 |

The absence of `buys.jsonl`, `shadow_entries.jsonl`, and
`shadow_lifecycle.jsonl` means no shadow BUY dispatch was observed during this
smoke window.

## V3 Replay Result

`scripts/v3_shadow_report.py` result:

- `status = ok`
- `raw_rows = 96`
- `v3_rows = 96`
- `bad_rows = 0`
- `full_snapshot_payload_rows = 96`
- `hash_only_rows = 0`
- `rows_missing_policy_hash = 0`
- `rows_missing_snapshot_hash = 0`
- `stale_against_config = false`
- `policy_hash_unique_count = 1`
- `snapshot_hash_unique_count = 96`

`scripts/v3_full_replay_report.py --strict` result:

- `status = ok`
- `replay_status = full_replay_ok`
- `total_rows = 96`
- `v3_rows = 96`
- `bad_rows = 0`
- `full_replay_ok = 96`

This satisfies the V3/MFS replay smoke criterion.

## Decision Verdicts

The V2.5 shadow decision rows did not produce BUY:

| class | count |
| --- | ---: |
| `v3_shadow_verdict = REJECT` | 66 |
| `v3_shadow_verdict = PENDING` | 30 |

Top V2.5 reasons:

| reason | count |
| --- | ---: |
| `REJECT_PDD_ENTRY_DRIFT` | 58 |
| `REJECT_PDD_WHALE` | 30 |
| `REJECT_PDD_RAMPING` | 3 |
| `REJECT_PDD_FLASH_CRASH` | 3 |
| `REJECT_PDD_SPIKE` | 1 |
| `REJECT_LOW_TRAJECTORY` | 1 |

This explains why no shadow transport or lifecycle rows were produced.

## Join-Key Audit

Generated:

- `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j1-smoke/p3_7_mfs_lifecycle_join_key_audit_after_j1.json`
- `PLANS/AUDYT/RAPORT_P3_7_J1_R14_SMOKE_JOIN_KEY_AUDIT_20260519.md`

Join-key audit result:

- `readiness.status = not_ready`
- `readiness.reasons = ["missing_shadow_transport_rows", "missing_shadow_entry_rows", "missing_shadow_lifecycle_rows"]`
- `decision_rows_with_ab_record_id = 592`
- decision artifact `ab_record_id` coverage: 100%
- decision artifact `feature_snapshot_hash` coverage: 100%
- decision artifact `v3_policy_config_hash` coverage: 100%
- `shadow_transport_rows_with_ab_record_id = 0`
- `shadow_entry_rows_with_ab_record_id = 0`
- `shadow_lifecycle_rows_with_ab_record_id = 0`

The audit reports `join_quality = exact_ab_record_id` for the artifacts that
exist and have rows, but the full J1 runtime chain cannot be validated without
shadow rows.

## Interpretation

This smoke confirms that the J1 namespace can run the forward V3/MFS profile and
produce strict replayable V3 rows with stable decision-side `ab_record_id`,
policy hash, and feature snapshot hash.

It does not prove the repaired propagation path across:

`decision -> shadow transport -> shadow entry -> shadow lifecycle`

because the run produced no Gatekeeper BUY and therefore no shadow dispatch.

This is not a failure of the additive J1 implementation. It is an inconclusive
runtime smoke for the shadow artifact portion.

## Gate

Full R14 remains blocked until one of the following is true:

1. A longer or better-timed J1 smoke produces at least one shadow transport and
   shadow entry row, and the join-key audit reports `ab_record_id` coverage for
   those rows.
2. A deliberately scoped collection run is approved with the documented risk
   that J1 shadow propagation is not yet runtime-observed.

The preferred next step is another J1 smoke or limited collection window that
keeps the same non-goals:

- no P2
- no live
- no threshold tuning
- no active policy changes
- no IWIM or live sender changes
- no treating shadow simulation as live inclusion

## Status

P3.7-J1 additive code repair remains locally validated by unit/script tests and
decision-side smoke artifacts.

P3.7-J1 runtime validation across real shadow BUY artifacts remains pending.
