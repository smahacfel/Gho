# P3.7-L1R3 / R16-r4 AccountNotFound Attribution Smoke

Status: DIAGNOSTIC ATTRIBUTION PASS / PROBE EXECUTION STILL BLOCKED

Commit under test: `bbdad17 Implement P3.7-L1R3 probe AccountNotFound attribution`

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution.toml`

Namespace:

`shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution`

## Runtime Handling

The tmux run reached the probe dispatch cap and was stopped manually after enough evidence was produced.

The run is not a collection run and is not an L2 policy ablation run.

## Core Results

- V3 strict replay: PASS
- `total_rows`: 423
- `v3_rows`: 423
- `bad_rows`: 0
- L1 diagnostic quality: PASS
- R16 artifact identity: PASS
- single active brain/policy hash: PASS
- probe selection rows: 22
- probe transport rows: 15
- probe shadow entry rows: 15
- probe lifecycle rows: 0
- active shadow BUY rows: 4
- active shadow lifecycle rows: 4

## AccountNotFound Attribution

All 15 probe transport rows ended with:

`execution_outcome = counterfactual_shadow_probe_simulation_error`

`simulation_error_kind = AccountNotFound`

The L1R3 attribution repair worked as intended:

- `account_not_found_rows`: 15
- `account_not_found_attributed_rows`: 0
- `account_not_found_multi_candidate_rows`: 15
- `account_not_found_unattributed_rows`: 0
- `simulation_rpc_visibility_gap_rows`: 0
- `precheck_simulation_account_set_mismatch_rows`: 0
- `account_set_match = true`: 15/15

Every `AccountNotFound` row now carries a candidate set instead of remaining blind.

Typical candidate set:

- `payer_pubkey`, source `payer`
- `user_ata`, source `user_ata`
- `user_volume_accumulator`, source `route_builder`
- `bonding_curve_v2`, source `route_builder`

This is diagnostic success, not execution success. The candidate set still needs to be narrowed before collection.

## Probe Entry / Lifecycle Eligibility

The run correctly distinguishes entry artifacts from lifecycle-eligible entries:

- `probe_entry_rows`: 15
- `successful_probe_entry_rows`: 0
- `simulation_error_entry_rows`: 15
- `lifecycle_eligible_entry_rows`: 0
- `probe_entry_materialization_status = simulation_error`: 15
- `probe_lifecycle_eligibility_status = not_lifecycle_eligible`: 15

No report should treat these probe entry artifacts as successful lifecycle candidates.

## Join Quality

Probe decision/V3 join remained clean:

- probe selection exact decision/V3 join: 22/22
- probe transport exact decision/V3 join: 15/15
- probe entry exact decision/V3 join: 15/15
- feature hash mismatch: 0
- policy hash mismatch: 0
- probe chain `ab_record_id` coverage: 100%
- probe chain `probe_id` coverage: 100%

## Active Shadow Lifecycle

The run produced active shadow artifacts:

- `buys.jsonl`: 4 rows
- `shadow_entries.jsonl`: 4 rows
- `shadow_lifecycle.jsonl`: 4 rows

The active shadow lifecycle rows were dispatch failures:

`dispatch_status = failed`

The on-chain lifecycle report wrote zero rows because there were no closed positions in scope.

Therefore no lifecycle labels were produced for R16-r4.

## Generated Artifacts

Machine reports:

- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_v3_shadow_report.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_v3_full_replay_report.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_join_key_audit.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_l1_reject_diagnostics_summary.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_l1_reject_diagnostics.jsonl`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_shadow_onchain_lifecycle_report.jsonl`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_shadow_lifecycle_labels.jsonl`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_shadow_lifecycle_label_summary.json`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution/r16_r4_shadow_lifecycle_feature_availability.json`

Markdown reports:

- `PLANS/AUDYT/RAPORT_P3_7_L1R3_R16_R4_ACCOUNT_ATTRIBUTION_JOIN_KEY_AUDIT_20260522.md`
- `PLANS/AUDYT/RAPORT_P3_7_L1R3_R16_R4_REJECT_DIAGNOSTICS_20260522.md`
- `PLANS/AUDYT/RAPORT_P3_7_L1R3_R16_R4_SHADOW_LIFECYCLE_LABEL_SUMMARY_20260522.md`
- `PLANS/AUDYT/RAPORT_P3_7_L1R3_R16_R4_SHADOW_LIFECYCLE_FEATURE_AVAILABILITY_20260522.md`

## Decision

R16-r4 passes the L1R3 attribution gate:

- no blind `AccountNotFound`
- no unattributed `AccountNotFound`
- no unexplained precheck/simulation account-set mismatch
- simulation-error entries are not lifecycle eligible

R16-r4 does not unblock collection:

- all probe dispatches still fail simulation
- every `AccountNotFound` is a four-candidate set
- no successful probe entry rows
- no probe lifecycle rows

## Next Step

Open a narrow follow-up to reduce `simulation_account_not_found_multi_candidate` to exact attribution.

Recommended next repair:

P3.7-L1R4 / J3N - AccountNotFound Candidate Narrowing

Focus:

- distinguish ephemeral/creatable accounts from true missing execution accounts in the attribution candidate set,
- verify whether `payer_pubkey`, `user_ata`, and `user_volume_accumulator` should remain candidates for probe-mode simulation,
- narrow the likely blocker to route-specific execution accounts, especially `bonding_curve_v2`, if evidence supports it,
- keep `AccountNotFound` fail-closed until exact role/pubkey attribution or an accepted narrow candidate class exists.

No L2 ablation, no collection, no Phase B, no P2/live until this is narrowed.
