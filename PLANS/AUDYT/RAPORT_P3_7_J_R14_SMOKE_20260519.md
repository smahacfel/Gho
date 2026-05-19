# P3.7-J R14 Smoke Report

Date: 2026-05-19

Scope: forward-only V3/MFS + shadow-burnin lifecycle smoke for `shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke`.

## Verdict

R14 smoke is partially accepted.

Accepted:

- Runtime started under the R14 smoke profile and stopped under the planned `timeout 30m` guard.
- V3/MFS replay payload emission worked.
- Strict V3 full replay passed.
- Shadow transport, shadow entry, and shadow lifecycle artifacts were emitted for one shadow dispatch.
- No P2/live/promotion evidence was found in the active config/log path.

Not accepted for full R14 collection yet:

- Formal preflight wrapper remains blocked by `.ghost/baseline_accepted_revision`.
- Shadow-onchain lifecycle truth is inconclusive for this smoke because the one shadow position did not close inside the smoke scope.
- Join-key coverage is degraded: decision artifacts carry `ab_record_id` and V3 hashes, while shadow artifacts carry `candidate_id` and do not carry `ab_record_id`.

Operational decision:

- Full R14 collection: HOLD.
- V3 selector prototype: still blocked.
- P2/live/runtime threshold work: blocked.
- Next required decision: resolve join-key parity or explicitly accept degraded mint/time-window joins before running full R14.

## Commands

Preflight reconciliation and smoke checks were run against:

```bash
configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

Smoke command:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

Post-smoke reports:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --json

python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --strict \
  --json

python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/p3_7_mfs_lifecycle_join_key_audit.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_J_R14_SMOKE_JOIN_KEY_AUDIT_20260519.md

python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml \
  --all-sessions \
  --output /root/Gho/logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/shadow_onchain_lifecycle_report.jsonl
```

## Runtime Evidence

Runtime start:

- actual smoke runtime start: `2026-05-19T13:29:28Z`
- last runtime log timestamp before timeout stop: `2026-05-19T13:54:33Z`
- process state after timeout: no `ghost-launcher` process remained
- command exit: `124`, expected from the outer `timeout 30m`

Config/log invariants observed:

- `execution_mode=Shadow`
- `entry_mode=shadow_only`
- `funding_lane_mode=disabled`
- `Gatekeeper V3 sidecar config: enabled=false shadow_emit=true`
- `LiveSellHandle: skipped (no live transport required at startup) execution_mode=Shadow`
- `TriggerComponent initialized (execution_mode: Shadow, entry_mode: ShadowOnly)`

Note: the generic launcher banner is not used as truth for live inclusion. The active config and startup logs above show the smoke ran in shadow-only execution.

## Generated Artifacts

Decision/log artifacts:

| artifact | rows |
| --- | ---: |
| `seer_runtime_coverage_audit.jsonl` | 416 |
| `v2.2/legacy_live/.../gatekeeper_v2_decisions.jsonl` | 416 |
| `v2.2/legacy_live/.../gatekeeper_v2_buys.jsonl` | 1 |
| `v2.5/v25_shadow/.../gatekeeper_v2_decisions.jsonl` | 150 |

Shadow runtime artifacts:

| artifact | rows |
| --- | ---: |
| `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/buys.jsonl` | 1 |
| `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/shadow_entries.jsonl` | 1 |
| `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/shadow_lifecycle.jsonl` | 1 |
| `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/shadow_onchain_lifecycle_report.jsonl` | 0 |
| `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/p3_7_shadow_lifecycle_labels.jsonl` | 0 |

Runtime logs:

- `logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/system.log.2026-05-19`
- `logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/oracle.log.2026-05-19`

Event datasets were emitted under:

```text
datasets/events/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke/
```

## V3 Payload Report

`v3_shadow_report.py` output:

- `status=ok`
- `replay_status=full`
- `raw_rows=150`
- `deduped_rows=150`
- `v3_rows=150`
- `no_v3_rows=0`
- `bad_rows=0`
- `duplicate_rows_removed=0`
- `policy_hash_unique_count=1`
- `rows_missing_policy_hash=0`
- `rows_missing_snapshot_hash=0`
- `stale_against_config=false`
- execution outcomes: `missing=149`, `shadow_data_problem=1`
- execution success count: `0`

`v3_full_replay_report.py --strict` output:

- `status=ok`
- `replay_status=full_replay_ok`
- `total_rows=150`
- `v3_rows=150`
- `bad_rows=0`
- `status_counts.full_replay_ok=150`

Interpretation: the V3/MFS payload smoke is accepted. This is not an execution-success claim.

## Shadow Lifecycle Report

`shadow_onchain_lifecycle_report.py` wrote zero rows:

- `scope_candidates=1`
- `rows_written=0`
- skipped reason from CLI output: `no_closed_positions_in_scope=1`

Interpretation: the smoke produced shadow dispatch artifacts, but did not produce a closed-position lifecycle truth row inside the smoke duration. This is inconclusive for lifecycle truth, not a lifecycle failure.

The downstream labeler and feature availability reports therefore correctly stayed empty:

- `p3_7_shadow_lifecycle_label_summary.json`: `rows_total=0`, `phase_f_label_status=not_accepted`
- `p3_7_shadow_lifecycle_feature_availability.json`: `feature_availability_status=lifecycle_only`, `phase_b_possible=false`, `v3_selector_prototype_possible=false`

## Join-Key Audit

`v3_p37_mfs_lifecycle_join_key_audit.py` result:

- readiness: `degraded`
- reason: `no_common_candidate_id_across_nonempty_artifacts`
- decision rows: `567`
- V3 payload rows: `567`
- shadow transport rows: `1`
- shadow entry rows: `1`
- shadow lifecycle rows: `1`

Field coverage:

| artifact | rows | candidate_id | ab_record_id | pool_id | mint | v3_payload | feature_hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `decision` buy | 1 | 0 | 1 | 1 | 1 | 1 | 1 |
| `decision` legacy live | 416 | 0 | 416 | 416 | 416 | 416 | 416 |
| `decision` v25 shadow | 150 | 0 | 150 | 150 | 150 | 150 | 150 |
| `shadow_entry` | 1 | 1 | 0 | 1 | 1 | 0 | 0 |
| `shadow_lifecycle` | 1 | 1 | 0 | 1 | 1 | 0 | 0 |
| `shadow_transport` | 1 | 1 | 0 | 0 | 1 | 0 | 0 |

Cross-artifact intersections:

- `mint`: `common_values=1`
- `ab_record_id`: `common_values=0`
- `candidate_id`: `common_values=0`
- `pool_id`: `common_values=0`

Interpretation: R14 smoke confirms that V3 decision payload and shadow lifecycle artifacts exist in the same namespace, but the join is not yet first-class. The full collection should not rely primarily on pool/mint/time-window matching if `ab_record_id` can be propagated additively into shadow transport/entry/lifecycle artifacts.

## Preflight Baseline

Formal wrapper status is documented separately:

```text
PLANS/AUDYT/RAPORT_P3_7_J_PREFLIGHT_BASELINE_RECONCILIATION_20260519.md
```

Current conclusion:

- direct runtime preflight passed,
- config-load tests passed,
- workspace no-run build passed,
- wrapper remains blocked only by `.ghost/baseline_accepted_revision`,
- `.ghost/baseline_accepted_revision` was not changed.

## Acceptance Matrix

| criterion | status | evidence |
| --- | --- | --- |
| Runtime starts | pass | startup logs at `2026-05-19T13:29:28Z` |
| Runtime stops under guard | pass | command exited via `timeout`, no process remained |
| Shadow-only execution | pass | `execution_mode=Shadow`, `entry_mode=shadow_only` |
| No live sell handle | pass | `LiveSellHandle: skipped` |
| V3 payload rows > 0 | pass | `v3_rows=150` |
| Full snapshot payload rows equal V3 rows | pass | strict replay `full_replay_ok=150` |
| Hash coverage | pass | no missing policy/snapshot hashes |
| Stale against config | pass | `stale_against_config=false` |
| Shadow dispatch artifacts | pass | transport/entry/lifecycle rows = `1/1/1` |
| On-chain lifecycle truth rows | inconclusive | no closed position in smoke scope |
| Lifecycle labels | inconclusive | raw lifecycle truth rows = `0` |
| Join-key coverage | degraded | no common `ab_record_id` or `candidate_id` across decision and shadow artifacts |
| Formal wrapper preflight | blocked | baseline stamp mismatch |

## Decision

Do not run full R14 collection yet.

Before full R14, choose one of these paths:

1. Additive join-key repair: propagate `ab_record_id` or a stable candidate key into shadow transport, shadow entry, and lifecycle artifacts, then rerun R14 smoke.
2. Governance exception: explicitly accept degraded joins for the next collection, with analysis limited to mint/pool/time-window quality and with no selector claim.

Recommended path: additive join-key repair before full R14. It directly addresses the main smoke finding and prevents the new V3/MFS+lifecycle dataset from inheriting the historical weak join problem.

## Non-Goals Preserved

- No P2.
- No live.
- No runtime threshold tuning.
- No V3 selector claim.
- No promotion.
- No MFS extension as active policy.
- No lifecycle outcome used as a decision-time feature.
