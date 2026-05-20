# RAPORT P3.7-J3R R15-r2 Counterfactual Probe Smoke

Date: 2026-05-20

Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2.toml`

Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2`

HEAD: `cd520fc` (`Implement P3.7-J3R probe runtime repair`)

## Verdict

`R15-r2 smoke: FAIL / NOT_READY`

The run confirms the bounded counterfactual probe plane emits selection and transport rows without mutating active BUY state, and V3/MFS replay remains clean. It does not pass the runtime gate because probe entries are absent, all probe simulations ended with `AccountNotFound`, and exact decision/V3 hash continuity is only `1/5`.

Decision:

- `P3.7-J3R code-level repair`: remains accepted.
- `R15-r2 runtime smoke`: not accepted.
- `Full/bounded collection`: HOLD.
- `Phase B V3/MFS lifecycle feature prototype`: HOLD.
- `P2/live/tuning`: NO-GO.

## Runtime

Command:

```bash
timeout 45m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2.toml
```

Result:

- Process exit code: `124`
- Interpretation: expected timeout termination.
- Runtime start observed: `2026-05-20T06:19:56Z`
- Runtime end observed: `2026-05-20T07:00:12Z`

The timeout includes the release build phase, so the live runtime window was shorter than 45 minutes.

## Pre-Run State

Before runtime, the R15-r2 namespace was absent. The pre-run join-key audit was generated and stored at:

- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2/p3_7_j3r_r15_r2_pre_join_key_audit.json`
- `PLANS/AUDYT/RAPORT_P3_7_J3R_R15_R2_PRE_JOIN_KEY_AUDIT_20260520.md`

The pre-run audit was expected to be `not_ready` because no runtime probe artifacts existed yet.

## V3 Replay

`v3_shadow_report.py` output:

- `raw_rows`: 79
- `deduped_rows`: 79
- `v3_rows`: 79
- `bad_rows`: 0
- `no_v3_rows`: 0
- `stale_against_config`: false
- `rows_missing_policy_hash`: 0
- `rows_missing_snapshot_hash`: 0
- `policy_hash_unique_count`: 1
- active verdict split:
  - active `REJECT` + V3 `REJECT`: 54
  - active `REJECT` + V3 `PENDING`: 25

`v3_full_replay_report.py --strict` output:

- `status`: `ok`
- `replay_status`: `full_replay_ok`
- `total_rows`: 79
- `v3_rows`: 79
- `bad_rows`: 0
- `status_counts.full_replay_ok`: 79

Verdict: `V3/MFS replay path PASS`.

## Probe Artifacts

Post-run artifact counts:

- `probe_selection.jsonl`: 5 valid JSONL rows
- `probe_skips.jsonl`: 458 valid JSONL rows
- `probe_transport.jsonl`: 5 valid JSONL rows
- `probe_shadow_entries.jsonl`: 0 rows / absent
- `probe_shadow_lifecycle.jsonl`: 0 rows / absent
- active `buys.jsonl` / active BUY files: not found in the R15-r2 rollout/shadow namespace
- normal shadow transport `buys.jsonl`: 0 rows / absent
- normal `shadow_entries.jsonl`: 0 rows / absent
- normal `shadow_lifecycle.jsonl`: 0 rows / absent

Probe skip reasons:

- `probe_rate_limit_exceeded`: 257
- `max_probes_per_run_exceeded`: 174
- `verdict_type_not_in_sample_scope`: 27

Probe buckets:

- selection/transport:
  - `v3_reject_manipulation_contradiction`: 4
  - `active_reject_v3_pending`: 1
- skips:
  - `v3_reject_manipulation_contradiction`: 407
  - `active_reject_v3_pending`: 24
  - `random_eligible_control`: 27

All selection, skip, and transport rows carry:

- `dispatch_source = counterfactual_shadow_probe`
- `probe_amount_source = fixed_lamports`

Bounded runtime verdict: `PASS` for max-probe bounding and nonblocking skip emission.

## Probe Simulation

All 5 probe transport rows ended as simulation errors:

- `execution_outcome`: `counterfactual_shadow_probe_simulation_error` = 5
- `err`: `AccountNotFound` = 5
- `error_class`: `data_problem` = 5
- `simulation_error_kind`: `AccountNotFound` = 5
- `simulation_error_message`: `AccountNotFound` = 5

The new diagnostic fields are present, but the missing account is still not identified precisely:

- `prepared_buy_account_set_present`: true on all 5 rows
- `account_overrides_present`: true on all 5 rows
- `curve_data_known`: true on all 5 rows
- `curve_readiness_status`: `curve_data_known_and_account_present` on all 5 rows
- `simulation_error_account_pubkey`: null on all 5 rows
- `simulation_error_account_role`: null on all 5 rows
- `quote_age_ms`: null on all 5 rows
- `curve_age_ms`: null on all 5 rows

Verdict: `FAIL` for probe simulation readiness. `AccountNotFound` remains a blocker, and the current diagnostics are not precise enough to classify this as `NOT_READY_DIAGNOSED`.

## Join-Key Audit

Post-run audit artifacts:

- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2/p3_7_j3r_r15_r2_join_key_audit.json`
- `PLANS/AUDYT/RAPORT_P3_7_J3R_R15_R2_JOIN_KEY_AUDIT_20260520.md`

Audit result:

- `readiness.status`: `not_ready`
- `readiness.join_key_acceptance`: `fail`
- `probe_readiness.status`: `not_ready`
- `probe_readiness.join_key_acceptance`: `fail`
- `probe_readiness.decision_join_acceptance`: `fail`
- `probe_readiness.reasons`:
  - `missing_probe_entry_rows`
  - `probe_rows_missing_exact_decision_v3_join`

Probe chain continuity:

- `probe_selection_rows`: 5
- `probe_transport_rows`: 5
- `probe_entry_rows`: 0
- `probe_lifecycle_rows`: 0
- `probe_chain_ab_record_id_coverage`: 1.0
- `probe_chain_probe_id_coverage`: 1.0
- `probe_transport_rows_with_ab_record_id`: 5
- `probe_transport_rows_with_probe_id`: 5
- `probe_transport_rows_with_dispatch_source`: 5
- `probe_transport_rows_with_feature_hash`: 5
- `probe_transport_rows_with_policy_hash`: 5

Exact decision/V3 continuity:

- `probe_selection.exact_decision_v3_join`: 1 / 5
- `probe_transport.exact_decision_v3_join`: 1 / 5
- `probe_selection.feature_hash_mismatch`: 4 / 5
- `probe_transport.feature_hash_mismatch`: 4 / 5
- `probe_selection.policy_hash_match`: 5 / 5
- `probe_transport.policy_hash_match`: 5 / 5
- mismatch reasons:
  - `feature_hash_mismatch`: 4
  - `multiple_decision_rows_for_ab_record_id`: 1

Verdict: `FAIL` for exact decision/V3 continuity. The J3R hash-continuity blocker is still present.

## Active BUY / Live Boundary

No active BUY artifacts were found in the R15-r2 rollout/shadow namespace. `v3_shadow_report.py` shows active decision rows as `REJECT` only. Probe rows are marked with `dispatch_source=counterfactual_shadow_probe`.

Verdict: `PASS` for active BUY non-mutation in this smoke.

No P2/live path was intentionally enabled by this smoke. The rollout remained configured for `entry_mode=shadow_only`, `execution_mode=shadow`, and probe dispatch source `counterfactual_shadow_probe`.

## Gate Assessment

Minimal R15-r2 PASS requirements:

| Requirement | Result |
| --- | --- |
| `v3_rows > 0` | PASS, 79 |
| strict full replay OK | PASS, 79/79 |
| `probe_selection_rows > 0` | PASS, 5 |
| `probe_transport_rows > 0` | PASS, 5 |
| transport exact decision/V3 continuity = 100% | FAIL, 1/5 |
| `probe_entry_rows > 0` | FAIL, 0 |
| entry exact AB/probe continuity = 100% | NOT APPLICABLE, no entries |
| probe JSONL logs parse cleanly | PASS |
| active BUY unchanged by probe rows | PASS |
| no live/P2 path touched | PASS by config and observed artifacts |

Final gate:

```text
R15-r2 smoke: FAIL / NOT_READY
P3.7-J3R runtime gate: BLOCKED
Full/bounded collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

## Required Next Work

Open a follow-up repair before any collection:

`P3.7-J3R2 — Counterfactual Probe Simulation and Hash Continuity Repair`

Required scope:

1. Fix exact feature-hash continuity so `probe_selection` and `probe_transport` exact-join to the persisted decision/V3 row at 100% for smoke.
2. Resolve why multiple decision rows exist for the same `ab_record_id`, or make the audit/source-row contract unambiguous enough to select the exact persisted row.
3. Improve `AccountNotFound` diagnostics so every simulation error includes a missing account pubkey and/or account role.
4. If missing account role/pubkey cannot be extracted from the simulator, add a pre-simulation account requirement check that converts incomplete rows into `probe_skipped` with a specific `precheck_failure_reason`.
5. Repeat bounded R15 smoke in a fresh namespace after the fixes.

Do not increase probe limits, do not run bounded collection, and do not move to Phase B until a smoke produces probe entries with exact join continuity.
