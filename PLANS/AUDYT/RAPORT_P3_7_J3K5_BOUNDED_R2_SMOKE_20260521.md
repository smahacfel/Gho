# RAPORT P3.7-J3K5 R15 Bounded R2 Smoke

## Status

```text
R15 J3K5-r2 bounded smoke: MINIMAL PASS / DIAGNOSED
Probe transport/entry path: PASS
Exact decision/V3 join: PASS
Creator-vault authority diagnostics: RUNTIME OBSERVED
Amount guard diagnostics: CODE/TEST PASS, NOT OBSERVED IN R2
Lifecycle/on-chain labels: NOT VALIDATED
Collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Config

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2.toml
```

Runtime was stopped after the bounded probe transport target was reached.

```text
probe_selection_rows = 19
probe_skips_rows = 7
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_lifecycle_rows = 0
active_buys_rows = 0
```

## V3 Replay

```text
v3_rows = 4
full_snapshot_payload_rows = 4
hash_only_rows = 0
bad_rows = 0
strict replay = full_replay_ok
stale_against_config = false
policy_hash_coverage = 1.0
feature_snapshot_hash_coverage = 1.0
```

## Probe Join-Key Audit

Detailed audit:

```text
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R2_JOIN_KEY_AUDIT_20260521.md
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R2_JOIN_KEY_AUDIT_20260521.json
```

Probe-side gate:

```text
probe_readiness = ready_for_probe_transport_entry_join
probe_join_key_acceptance = pass
probe_decision_join_acceptance = pass
probe_required_exact_decision_v3_join_coverage = 1.0
probe_chain_ab_record_id_coverage = 1.0
probe_chain_probe_id_coverage = 1.0
```

The generic shadow lifecycle gate remains `not_ready` because this run did not
produce active shadow transport, shadow entries, shadow lifecycle, or on-chain
lifecycle rows. That is expected for this counterfactual probe smoke and does
not override the probe-side PASS.

## Entry Materialization

```text
probe_transport_rows = 10
probe_shadow_entry_rows = 9
entry_materialized = 7
simulation_error = 2
transport_only_missing_token_quantity = 1
unknown_rows = 0
```

Materialization classes:

```text
entry_row_present = 7
simulation_account_layout_mismatch:custom_2006 = 2
routed_exact_sol_in_entry_token_amount_raw_null = 1
```

Buy/parameter distribution:

```text
buy_variant_counts = {"legacy_buy": 9, "routed_exact_sol_in": 1}
token_param_role_counts = {"token_amount": 9, "min_tokens_out": 1}
```

## Creator-Vault Diagnostics

J3K5-r2 runtime-observed the creator-vault class:

```text
simulation_error_custom_code_counts = {"custom_2006": 2}
creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 2}
creator_vault_mismatch_reason_counts = {"actual_expected_mismatch": 2}
creator_identity_source_counts = {"account_overrides.creator_pubkey": 2}
```

Both `custom_2006` rows had:

```text
simulation_error_category = simulation_account_layout_mismatch
simulation_error_account_role = creator_vault
creator_identity_authoritative = false
```

Interpretation:

```text
The probe request used creator identity from account_overrides.creator_pubkey,
but the vault derived from that source did not match the vault expected by the
program in Anchor diagnostics. The expected value remains diagnostic-only and
must not be fed back into request construction as a post-hoc repair source.
```

## Amount Guard

R15 J3K5-r2 did not observe `custom_6002` / `TooMuchSolRequired`:

```text
amount_guard_status_counts = {}
simulation_error_custom_code_counts excludes custom_6002
```

The inline Anchor `Left:` / `Right:` parser was fixed after J3K5-r1 and covered
by targeted Rust tests, but r2 did not provide a new runtime `custom_6002` row
to re-observe that class.

## Decision

```text
R15 J3K5-r2: MINIMAL PASS / DIAGNOSED
Probe transport/entry plumbing: PASS
Join-key continuity: PASS
Creator-vault mismatch classification: PASS / RUNTIME OBSERVED
Amount shortfall classification: CODE/TEST PASS / RUNTIME NOT OBSERVED IN R2
Collection: HOLD
```

The next repair should target creator-vault source authority / route identity.
Scaling collection before that would produce known `custom_2006` simulation
mismatch rows for a route class whose authority source is not yet reliable.
