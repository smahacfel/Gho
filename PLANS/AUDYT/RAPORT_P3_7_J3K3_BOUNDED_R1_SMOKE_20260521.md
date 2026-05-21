# RAPORT P3.7-J3K3 Bounded R1 Smoke

Date: 2026-05-21

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1
```

Status:

```text
bounded_r1_status = STOPPED_EARLY_DIAGNOSED
probe_transport_entry_path = PASS
exact_decision_v3_join = PASS
simulation_error_status = DIAGNOSED_NOT_CLEAN
lifecycle_path = NOT_VALIDATED
collection / Phase B / P2 / live / tuning = HOLD / NO-GO
```

## Why The Run Was Stopped

The bounded run was intentionally stopped before timeout after the first live
artifact snapshot showed simulation errors. This was not a process crash. It
was a controlled early stop to avoid scaling a newly visible error class.

## Replay / V3

```text
v3_rows = 5
strict_full_replay = full_replay_ok
bad_rows = 0
full_snapshot_payload_rows = 5
hash_only_rows = 0
stale_against_config = false
```

## Probe Artifacts

```text
probe_selection_rows = 26
probe_skip_rows = 28
probe_transport_rows = 17
probe_shadow_entry_rows = 16
probe_lifecycle_rows = 0
active_buys_rows = 0
active_shadow_entries_rows = 0
```

Probe join-key audit:

```text
probe_join_key_acceptance = pass
probe_join_quality = exact_probe_id_and_ab_record_id
probe_decision_join_acceptance = pass
probe_required_exact_decision_v3_join_coverage = 1.0
probe_chain_ab_record_id_coverage = 1.0
probe_chain_probe_id_coverage = 1.0
probe_transport_rows_with_ab_record_id = 17
probe_entry_rows_with_ab_record_id = 16
```

## Entry Materialization

```text
entry_materialized = 13
simulation_error = 3
transport_only_missing_token_quantity = 1
```

Reasons:

```text
entry_row_present = 13
simulation_account_layout_mismatch:custom_2006 = 2
simulation_slippage_or_price_mismatch:custom_6002 = 1
routed_exact_sol_in_entry_token_amount_raw_null = 1
```

## Simulation Error Classes

### Custom 6002

```text
program = pumpfun
error_name = too_much_sol_required
category = simulation_slippage_or_price_mismatch
instruction_index = 3
amount_lamports = 7000000
program_left = 7000000
program_right = 7739140
```

Interpretation: probe request reached the Pump.fun program, but the fixed
probe amount was below the SOL required to buy the encoded token amount at
simulation time. This is an amount/quote/slippage class, not an account
readiness class.

### Custom 2006

```text
program = pumpfun
error_name = anchor_constraint_seeds
category = simulation_account_layout_mismatch
instruction_index = 3
affected_account = creator_vault
rows = 2
```

The runtime log tail showed:

```text
AnchorError caused by account: creator_vault
Left = provided creator_vault PDA
Right = expected creator_vault PDA
```

This means the probe used a creator-vault PDA derived from the local
`creator_pubkey`, while the Pump.fun program expected a different
creator-vault PDA. The issue is now classified as creator-vault route/account
identity mismatch, not as `AccountNotFound`.

## Execution-Account Readiness

```text
diagnosed_selected_probe_rows = 6
missing_account_roles = {"creator_vault": 6}
readiness_latency_class = never_observed_in_run
recommended_next_stage = account_coverage_or_route_identity_investigation
```

The prior dominant `missing_bonding_curve` class did not recur in this bounded
snapshot. The remaining readiness skips are route-specific `creator_vault`
materialization gaps.

## Decision

Bounded R1 proves the counterfactual probe path can produce exact-joined
transport and entry rows at small scale. It does not justify broader collection
yet because:

- simulation errors are present and must remain explicitly classified;
- routed exact-SOL rows still may be transport-only without token quantity;
- lifecycle/on-chain labels have not been validated.

Next step:

```text
P3.7-J3K4 Simulation Error Diagnostics / Creator-Vault Mismatch Classification
```

J3K4 should preserve the actual and expected creator-vault pubkeys directly in
future probe transport rows. Broader collection remains on HOLD until the next
bounded run either keeps these errors low and fully diagnosed or produces a
specific route/materialization repair target.
