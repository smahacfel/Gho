# P3.7-L1R6 Active Shadow AccountNotFound Attribution

## Status

Code-level repair in progress for active shadow dispatch diagnostics.

## Problem

R16-r6 closed the probe-side `bonding_curve_v2` simulation contract, but active
shadow BUY dispatch rows still failed with `AccountNotFound` without the same
account-manifest attribution used by the probe plane.

That blocked R16 policy diagnostics because BUY verdict rows existed but active
shadow dispatch failures could not be classified into payer, route account,
RPC visibility, account-set mismatch, or unknown.

## Implemented Repair

The active shadow failure path now carries an additive, flattened account
diagnostic block through:

- `ShadowBuySimulationEvent`
- `ShadowBuySimulationRecord`
- `ShadowDispatchLifecycleRecord`
- canonical active shadow entry rows

For failure rows with a `PreparedBuyRequest`, L1R6 builds the prepared request
account manifest, account-set hashes, missing-account candidates and narrowed
candidate classification before writing active shadow artifacts.

Rows without a prepared request are explicitly marked:

```text
active_shadow_precheck_status = not_run_no_prepared_request
active_shadow_lifecycle_eligibility_status = not_lifecycle_eligible
```

Failure rows from prepared requests are marked:

```text
active_shadow_precheck_status = not_run_post_simulation_attribution
active_shadow_lifecycle_eligibility_status = not_lifecycle_eligible
```

## New Fields

Additive fields emitted on active shadow failure artifacts:

```text
active_shadow_precheck_status
active_shadow_lifecycle_eligibility_status
simulation_error_kind
simulation_error_message
simulation_error_account_pubkey
simulation_error_account_role
simulation_error_account_source
simulation_error_account_candidates
simulation_error_account_candidates_raw
simulation_error_account_candidates_narrowed
simulation_error_account_candidates_excluded
simulation_error_account_narrowing_status
simulation_error_account_narrowing_reason
simulation_error_category
precheck_account_set_hash
prepared_request_account_set_hash
simulation_account_set_hash
precheck_account_set_count
prepared_request_account_set_count
simulation_account_set_count
account_set_match
account_set_mismatch_reason
account_manifest_available
account_manifest_summary
simulation_account_manifest
```

## Audit Updates

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports active shadow
dispatch attribution separately from probe materialization:

```text
active_shadow_dispatch_failure_rows
active_shadow_account_not_found_rows
active_shadow_account_not_found_attributed_rows
active_shadow_account_not_found_multi_candidate_rows
active_shadow_account_not_found_unattributed_rows
active_shadow_rpc_visibility_gap_rows
active_shadow_lifecycle_eligible_failure_rows
active_shadow_account_not_found_role_counts
```

`scripts/v3_p37_l1_reject_diagnostics.py` now surfaces the same active shadow
failure counts in the R16 diagnostic summary.

## Runtime Gate

Next smoke config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r7-active-shadow-attribution.toml
```

Acceptance:

```text
strict replay = full_replay_ok
diagnostic_quality = PASS
identity/hash = PASS
active_shadow_account_not_found_unattributed_rows = 0
active_shadow_lifecycle_eligible_failure_rows = 0
active BUY / live / P2 untouched
```

## Non-Goals Preserved

No Gatekeeper threshold changes, PDD changes, IWIM changes, Phase B, P2/live,
collection scaling, probe amount changes, or baseline config edits.
