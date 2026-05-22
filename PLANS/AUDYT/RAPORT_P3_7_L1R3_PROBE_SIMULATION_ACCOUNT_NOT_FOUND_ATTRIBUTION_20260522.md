# RAPORT P3.7-L1R3 / J3M — Probe Simulation AccountNotFound Attribution Repair

## Status

```text
P3.7-L1R3 / J3M code-level repair: IMPLEMENTED
Runtime gate: R16-r4 AccountNotFound attribution smoke
Policy ablation: HOLD
Collection: HOLD
Phase B / P2 / live / threshold tuning: NO-GO
```

## Problem

R16-r3 fixed the L1R2 reporting denominator and active-shadow payer contract,
but probe simulation became blocked by blind `AccountNotFound`:

```text
probe_transport_rows = 15
probe_shadow_entries_rows = 15
simulation_error_kind = AccountNotFound
simulation_error_account_role = null
simulation_error_account_pubkey = null
probe_lifecycle_rows = 0
```

Without missing-account attribution, the run cannot distinguish route identity
mismatch, prepared-request/account-override mismatch, RPC visibility gap, a true
missing execution account, or a precheck/simulation account-set divergence.

## Code Changes

- Added a prepared-request account manifest for counterfactual probe simulation
  attempts.
- Added comparable account-set hashes and counts:
  `precheck_account_set_hash`, `prepared_request_account_set_hash`,
  `simulation_account_set_hash`, plus counts and match/mismatch reason.
- Added a diagnostic manifest account-presence helper in the Trigger component.
- Added layered `AccountNotFound` attribution:
  - exact pubkey/role/source when known;
  - candidate list when several manifest accounts are missing;
  - explicit `simulation_account_not_found_unattributed` or
    `simulation_rpc_visibility_gap` when attribution cannot be exact.
- Extended probe transport rows and probe entry rows with additive optional
  attribution fields.
- Marked simulation-error entry artifacts as
  `probe_entry_materialization_status=simulation_error` and
  `probe_lifecycle_eligibility_status=not_lifecycle_eligible`.
- Extended the join-key audit to count attributed, multi-candidate,
  unattributed and RPC-visibility-gap AccountNotFound rows.

## New Fields

```text
simulation_error_account_pubkey
simulation_error_account_role
simulation_error_account_source
simulation_error_instruction_index
simulation_error_account_index
simulation_error_account_candidates
simulation_error_category
account_manifest_available
account_manifest_summary
simulation_account_manifest
precheck_account_set_hash
prepared_request_account_set_hash
simulation_account_set_hash
precheck_account_set_count
prepared_request_account_set_count
simulation_account_set_count
account_set_match
account_set_mismatch_reason
accounts_only_in_precheck
accounts_only_in_simulation
probe_entry_materialization_status
probe_lifecycle_eligibility_status
```

All fields are additive and backward-compatible for legacy JSONL readers.

## Audit Extensions

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports:

```text
account_not_found_rows
account_not_found_attributed_rows
account_not_found_multi_candidate_rows
account_not_found_unattributed_rows
simulation_rpc_visibility_gap_rows
account_set_match_counts
precheck_simulation_account_set_mismatch_rows
successful_probe_entry_rows
simulation_error_entry_rows
lifecycle_eligible_entry_rows
```

Collection readiness is blocked if:

```text
account_not_found_unattributed_rows > 0
```

or if a precheck/simulation account-set mismatch appears without an explicit
`account_set_mismatch_reason`.

## Tests

Targeted tests added or extended:

- Rust:
  - account manifest and account-set hash construction;
  - exact missing-account attribution;
  - multi-candidate attribution;
  - unattributed AccountNotFound classification;
  - simulation-error entry rows are not lifecycle-eligible.
- Python:
  - attributed AccountNotFound;
  - multi-candidate AccountNotFound;
  - unattributed AccountNotFound;
  - account-set mismatch;
  - simulation-error entry rows;
  - successful lifecycle-eligible entry rows.

## Runtime Gate

Created fresh attribution-smoke config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution.toml
```

Expected command:

```bash
timeout 45m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution.toml
```

R16-r4 passes attribution if every `AccountNotFound` row has exact attribution
or a candidate set and `account_not_found_unattributed_rows = 0`.

## Known Limitations

- If RPC simulation returns `AccountNotFound` while manifest lookup sees all
  manifest accounts as present, the row is classified as
  `simulation_rpc_visibility_gap`. That is an explicit runtime diagnosis, not a
  success.
- If manifest lookup itself fails, the row is classified as
  `simulation_account_not_found_unattributed` and blocks collection.
- The manifest is derived from the prepared transaction account set and the
  existing role mapper; it does not introduce new route inference or policy
  behavior.

## Next Decision

```text
Run R16-r4 AccountNotFound attribution smoke.
If AccountNotFound is attributed -> repair the identified class.
If AccountNotFound remains unattributed -> stop and repair attribution again.
If no AccountNotFound appears -> continue only if L1 diagnostics remain clean.
```

No L2 ablation, collection, Phase B, P2/live or threshold tuning is allowed
until blind probe simulation `AccountNotFound` is eliminated.
