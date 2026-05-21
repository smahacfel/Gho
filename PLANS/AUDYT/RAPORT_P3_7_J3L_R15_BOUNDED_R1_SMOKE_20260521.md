# RAPORT P3.7-J3L R15 BOUNDED-R1 Counterfactual Probe Entry/Lifecycle Collection

Date: 2026-05-21

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3l-r1.toml`

Runtime namespace:

`shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3l-r1`

## Verdict

`J3L bounded-r1 = ENTRY-LEVEL PASS / LIFECYCLE NOT VALIDATED`

The bounded counterfactual probe run reached the configured dispatch cap and
validated the V3/MFS -> probe transport -> probe entry path at the 25-dispatch
scale.

The run did not produce probe lifecycle rows. Lifecycle/on-chain truth remains
the next blocker and must not be inferred from entry rows.

Full collection / Phase B / P2 / live / threshold tuning remain `NO-GO`.

## Runtime Status

The run was launched in tmux session `p37_j3l_r1` with a `2h` timeout. After
timeout, process checks showed no active `ghost-launcher` process for this
namespace and no live tmux session.

Runtime log:

`/tmp/p37_j3l_r1_runtime.log`

## V3 Replay

`v3_shadow_report.py`:

- `status = ok`
- `v3_rows = 247`
- `bad_rows = 0`
- `full_snapshot_payload_rows = 247`
- `hash_only_rows = 0`
- `stale_against_config = false`
- policy hash missing = 0
- feature snapshot hash missing = 0

`v3_full_replay_report.py --strict`:

- `replay_status = full_replay_ok`
- `total_rows = 247`
- `v3_rows = 247`
- `bad_rows = 0`

V3 verdict split:

- active verdict `REJECT` + V3 `PENDING`: 103
- active verdict `REJECT` + V3 `REJECT`: 144

## Probe Artifacts

JSONL parse:

- `probe_selection.jsonl`: 96 rows, 0 malformed
- `probe_skips.jsonl`: 2997 rows, 0 malformed
- `probe_transport.jsonl`: 25 rows, 0 malformed
- `probe_shadow_entries.jsonl`: 25 rows, 0 malformed
- `probe_shadow_lifecycle.jsonl`: missing / 0 rows
- `buys.jsonl`: missing / 0 active BUY rows
- `shadow_entries.jsonl`: missing / 0 active shadow entry rows
- `shadow_lifecycle.jsonl`: missing / 0 active shadow lifecycle rows

Probe join-key audit:

- `probe_readiness = ready_for_probe_transport_entry_join`
- `probe_join_key_acceptance = pass`
- `probe_join_quality = exact_probe_id_and_ab_record_id`
- `probe_decision_join_acceptance = pass`
- `probe_required_exact_decision_v3_join_coverage = 1.0`
- `probe_chain_ab_record_id_coverage = 1.0`
- `probe_chain_probe_id_coverage = 1.0`

Probe decision/V3 join:

- `probe_selection`: 96/96 exact decision/V3 join
- `probe_transport`: 25/25 exact decision/V3 join
- `probe_entry`: 25/25 exact decision/V3 join
- feature hash mismatches: 0
- policy hash mismatches: 0

## Entry Materialization

The dispatch cap was reached:

- `probe_transport_rows = 25`
- `probe_entry_rows = 25`
- `transport_without_entry_rows = 0`

Entry/materialization classification:

- `entry_materialized = 23`
- `simulation_error = 2`
- `simulation_error_custom_code_counts = {"custom_2006": 2}`
- `entry_materialization_reason_counts = {"entry_row_present": 23, "simulation_account_layout_mismatch:custom_2006": 2}`

All transported probes used:

- `buy_variant = routed_exact_sol_in`
- `token_param_role = min_tokens_out`

All transported rows preserved `ab_record_id`, `probe_id`, V3 feature snapshot
hash and V3 policy config hash.

## Current Error Class

The remaining simulation error class is the same narrow creator-vault authority
class identified before J3L:

- `creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 2}`
- `creator_vault_mismatch_reason_counts = {"actual_expected_mismatch": 2}`
- `creator_identity_source_counts = {"account_overrides.creator_pubkey": 2}`
- `custom_2006 = 2`

This did not corrupt join-key continuity and did not create active BUY artifacts,
but it remains a classified simulation-error class that should stay stop-loss
guarded in any future larger run.

## Skip Distribution

Skip rows were expected because the scan universe is wider than dispatch.

Key skip counts:

- `creator_vault_source_not_authoritative = 1661`
- `verdict_type_not_in_sample_scope = 994`
- `probe_execution_precheck_failed = 275`
- `max_probes_per_run_exceeded = 64`
- `execution_account_not_ready = 3`

Creator-vault fail-closed behavior remains active:

- `skip_creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 1661}`
- `skip_creator_vault_mismatch_reason_counts = {"creator_identity_source_not_authoritative": 1661}`
- `skip_creator_identity_source_counts = {"detected_pool.creator": 1661}`

## Lifecycle Status

No lifecycle layer was observed:

- `probe_shadow_lifecycle_rows = 0`
- no `probe_shadow_lifecycle.jsonl` file
- no on-chain lifecycle report generated
- no lifecycle labels generated

The post-buy runtime and guardian started, but this run did not emit
probe-specific lifecycle rows. This is now the main missing layer.

## Interpretation

J3L achieved the bounded entry-level objective:

- V3/MFS replay stayed strict and clean;
- probe selection/transport/entry exact join stayed 100%;
- 25/25 transported probes produced entry artifact rows;
- 23/25 rows were clean `entry_materialized`;
- active BUY artifacts remained absent;
- JSONL logs were well-formed.

J3L did not validate lifecycle/on-chain truth. Entry rows are not lifecycle
labels and must not be used as lifecycle truth.

## Decision

`J3L bounded-r1 = ENTRY-LEVEL PASS`

`Lifecycle/on-chain label path = NOT VALIDATED`

Recommended next gate:

`P3.7-J4 Probe Lifecycle Handoff / Post-Buy Monitor Validation`

J4 should determine whether counterfactual probe entries are intentionally
entry-only or whether they must be handed off into a probe-specific lifecycle
monitor/ledger path. Do not scale to full collection, Phase B, P2, live,
active policy mutation, IWIM changes, or threshold tuning from this run.
