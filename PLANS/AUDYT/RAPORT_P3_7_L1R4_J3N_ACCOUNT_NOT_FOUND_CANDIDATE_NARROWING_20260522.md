# P3.7-L1R4 / J3N — AccountNotFound Candidate Narrowing

Date: 2026-05-22

## Status

Code-level repair: PASS

Runtime gate: R16-r5 AccountNotFound Candidate Narrowing Smoke pending.

Collection, L2 ablation, Phase B, P2/live and threshold tuning remain HOLD /
NO-GO.

## Context

R16-r4 closed the blind AccountNotFound gap from L1R3:

- `account_not_found_unattributed_rows = 0`
- `account_not_found_multi_candidate_rows = 15`
- `account_set_match = true` for all AccountNotFound rows

But probe execution was still blocked:

- `probe_transport_rows = 15`
- `probe_shadow_entries_rows = 15`
- `successful_probe_entry_rows = 0`
- `simulation_error_entry_rows = 15`
- `lifecycle_eligible_entry_rows = 0`

The raw candidate set was too broad and usually contained:

- `payer_pubkey`
- `user_ata`
- `user_volume_accumulator`
- `bonding_curve_v2`

## Changes

J3N adds a candidate narrowing layer after manifest/RPC missing-account
attribution:

- raw candidates are preserved in `simulation_error_account_candidates_raw`;
- non-fatal probe-mode candidates are moved to
  `simulation_error_account_candidates_excluded`;
- fatal or conditional candidates are preserved in
  `simulation_error_account_candidates_narrowed`;
- row-level status is emitted in
  `simulation_error_account_narrowing_status`;
- row-level explanation is emitted in
  `simulation_error_account_narrowing_reason`.

Each candidate can now carry:

- `candidate_class`
- `candidate_fatality`
- `candidate_exclusion_reason`

## Candidate Semantics

Current classification rules:

- `payer_pubkey` with `payer_provenance = ephemeral`:
  `ephemeral_payer_nonfatal`, `non_fatal`,
  `ephemeral_payer_not_rpc_required`.
- configured `payer_pubkey`: strict/fatal.
- `user_ata` with `attach_idempotent_ata_create = true` and
  `ata_missing_pre_submit = true`: `idempotent_creatable_user_ata`,
  `non_fatal`, `idempotent_ata_create_attached`.
- `user_volume_accumulator` from route-builder context:
  `creatable_or_optional_route_pda`, `non_fatal`,
  `route_user_volume_accumulator_not_precheck_required`.
- `bonding_curve_v2`: `strict_execution_account`, `fatal`.

If the narrowed set contains exactly one candidate, the row becomes
`simulation_account_not_found_attributed`.

If multiple fatal/conditional candidates remain, the row becomes
`simulation_account_not_found_multi_candidate_narrow`.

If all raw candidates are non-fatal but simulation still fails, the row becomes
`all_candidates_nonfatal_but_sim_failed`.

Unattributed narrowing remains a blocking failure.

## Audit Updates

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports:

- `account_not_found_candidate_raw_counts`
- `account_not_found_candidate_narrowed_counts`
- `candidate_class_counts`
- `candidate_exclusion_reason_counts`
- `exact_after_narrowing_rows`
- `multi_candidate_narrowed_rows`
- `unattributed_after_narrowing_rows`
- `all_candidates_nonfatal_but_sim_failed_rows`

Probe readiness remains fail-closed when:

- `unattributed_after_narrowing_rows > 0`;
- narrowed multi-candidate rows are present without an explicit follow-up
  decision;
- all candidates were excluded as non-fatal but simulation still returned
  AccountNotFound.

## Validation

Completed locally:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe_account_not_found -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
python3 -m py_compile \
  scripts/v3_p37_l1_reject_diagnostics.py \
  scripts/v3_p37_probe_execution_account_readiness_report.py \
  scripts/v3_p37_shadow_lifecycle_labeler.py \
  scripts/shadow_onchain_lifecycle_report.py
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs
git diff --check
```

Targeted Rust AccountNotFound tests and full targeted probe suites pass.
Runtime smoke remains pending because L1R4 first needed code-level candidate
narrowing and audit contract validation.

## Next Runtime Gate

Run R16-r5 after validation:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing.toml
```

Acceptance:

- strict replay OK;
- diagnostic quality does not regress;
- identity/hash contract PASS;
- exact decision/V3 join = 100%;
- raw/narrowed/excluded candidate sets present on AccountNotFound rows;
- `unattributed_after_narrowing_rows = 0`;
- payer/user ATA exclusions are explained;
- either successful probe entries appear, or AccountNotFound narrows to true
  strict blockers such as `bonding_curve_v2`;
- active BUY/live/P2 untouched.
