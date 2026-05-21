# RAPORT P3.7-J3K7 R15-R1 Routed Exact-SOL-In Entry Smoke

Date: 2026-05-21

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1.toml`

Runtime namespace:

`shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1`

## Verdict

`R15-r1 J3K7 smoke = MINIMAL PASS / ENTRY PATH VALIDATED`

J3K7 repaired the J3K6 blocker. Routed exact-SOL-in probe rows now carry token
quantity evidence and materialize probe entries.

Collection remains `HOLD` until the operator explicitly approves the next
bounded step.

Phase B / P2 / live / threshold tuning remain `NO-GO`.

## Runtime Status

The run was stopped early by operator action after reaching the bounded dispatch
target. It was not allowed to continue to the 45 minute timeout because the
smoke had already produced enough transport/entry evidence.

Post-stop process check showed no active `ghost-launcher` process for this
namespace.

## V3 Replay

`v3_shadow_report.py`:

- `status = ok`
- `v3_rows = 74`
- `bad_rows = 0`
- `full_snapshot_payload_rows = 74`
- `hash_only_rows = 0`
- `stale_against_config = false`
- policy hash missing = 0
- feature snapshot hash missing = 0

`v3_full_replay_report.py --strict`:

- `replay_status = full_replay_ok`
- `total_rows = 74`
- `v3_rows = 74`
- `bad_rows = 0`

## Probe Artifacts

JSONL parse:

- `probe_selection.jsonl`: 43 rows, 0 malformed
- `probe_skips.jsonl`: 495 rows, 0 malformed
- `probe_transport.jsonl`: 10 rows, 0 malformed
- `probe_shadow_entries.jsonl`: 10 rows, 0 malformed
- `probe_shadow_lifecycle.jsonl`: missing / 0 rows
- `buys.jsonl`: missing / 0 active BUY rows

Probe decision/V3 join:

- `probe_selection`: 43/43 exact decision/V3 join
- `probe_transport`: 10/10 exact decision/V3 join
- `probe_entry`: 10/10 exact decision/V3 join
- feature hash mismatches: 0
- policy hash mismatches: 0

Join-key audit:

- `probe_readiness = ready_for_probe_transport_entry_join`
- `probe_join_key_acceptance = pass`
- `probe_join_quality = exact_probe_id_and_ab_record_id`
- `probe_required_exact_decision_v3_join_coverage = 1.0`
- `probe_chain_ab_record_id_coverage = 1.0`
- `probe_chain_probe_id_coverage = 1.0`

## Entry Materialization

J3K7 fixed the previous `transport_only_missing_token_quantity` class for the
bounded smoke:

- `probe_transport_rows = 10`
- `probe_entry_rows = 10`
- `transport_without_entry_rows = 0`
- `entry_materialized_rows = 10`
- `probe_entry_materialization_status_counts = {"entry_materialized": 10}`
- `probe_entry_materialization_reason_counts = {"entry_row_present": 10}`

All transported probes were:

- `buy_variant = routed_exact_sol_in`
- `token_param_role = min_tokens_out`

All ten transported rows had populated token quantities:

- `entry_token_amount_raw` populated on 10/10 transport rows
- `min_tokens_out` populated on 10/10 transport rows

No simulation error custom codes were observed in transported rows.

## Skip Distribution

Skip rows were expected because the smoke keeps scan wider than dispatch.

Key skip counts:

- `creator_vault_source_not_authoritative = 287`
- `verdict_type_not_in_sample_scope = 166`
- `max_probes_per_run_exceeded = 20`
- `probe_execution_precheck_failed = 12`
- `execution_account_not_ready = 8`
- `probe_rate_limit_exceeded = 2`

Creator-vault fail-closed behavior remains active:

- `skip_creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 287}`
- `skip_creator_vault_mismatch_reason_counts = {"creator_identity_source_not_authoritative": 287}`
- `skip_creator_identity_source_counts = {"detected_pool.creator": 287}`

## Interpretation

J3K7 achieved its narrow goal:

- routed exact-SOL-in no longer emits transport-only rows without token quantity;
- transported rows materialize probe entries;
- exact V3 decision join remains 100%;
- active BUY artifacts remain absent;
- no live/P2 path was enabled by this smoke.

This is a transport/entry plumbing PASS. It is not lifecycle truth and not
selector readiness.

The remaining missing layer is lifecycle/on-chain label validation:

- `probe_shadow_lifecycle_rows = 0`
- no on-chain lifecycle report was generated for this smoke
- no lifecycle labels were generated

## Decision

`J3K7 code-level repair = PASS`

`R15-r1 J3K7 runtime smoke = MINIMAL PASS / ENTRY PATH VALIDATED`

Collection remains `HOLD` pending explicit operator approval.

Next reasonable gate after approval:

`P3.7-J3L small bounded counterfactual probe entry/lifecycle collection`

Recommended first bounded run should remain small and stop-loss guarded:

- dispatch cap 25-50
- max concurrent 1
- exact decision/V3 join must stay 100%
- active BUY rows must remain 0
- simulation errors and transport-only rows must stay classified
- lifecycle rows, if they appear, must preserve `ab_record_id` and `probe_id`

Do not proceed to Phase B, P2, live, active policy mutation, IWIM changes or
threshold tuning from this smoke.
