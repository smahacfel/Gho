# RAPORT P3.7-J3Q5B R15-r9 Smoke

Date: 2026-05-20
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b`
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b.toml`

## Verdict

`R15-r9-q5b: MINIMAL PASS / TOKEN-PARAM CONTEXT STILL MISSING`

This run confirms the counterfactual probe transport/entry path remains runtime-valid after Q5, with exact V3 decision join and no active BUY mutation. It does not fully close Q5 token-parameter diagnostics because the new token-param fields were not populated in the emitted transport rows.

## Runtime Handling

The run was stopped manually after the bounded probe gate had already produced the required 5 probe transport rows and 5 probe entry rows. Waiting for the original timeout was not useful for this gate.

## V3 Replay

`scripts/v3_shadow_report.py`:

```text
status = ok
v3_rows = 14
raw_rows = 14
deduped_rows = 14
bad_rows = 0
full_snapshot_payload_rows = 14
hash_only_rows = 0
stale_against_config = false
policy_hash_unique_count = 1
snapshot_hash_unique_count = 14
```

`scripts/v3_full_replay_report.py --strict`:

```text
status = ok
replay_status = full_replay_ok
total_rows = 14
v3_rows = 14
bad_rows = 0
full_replay_ok = 14
```

## Probe Artifacts

```text
probe_selection_rows = 50
probe_skips_rows = 49
probe_transport_rows = 5
probe_shadow_entry_rows = 5
probe_lifecycle_rows = 0
active_buys_jsonl = missing
active_shadow_entries_jsonl = missing
active_shadow_lifecycle_jsonl = missing
```

Probe skip reasons:

```text
probe_rate_limit_exceeded = 37
verdict_type_not_in_sample_scope = 4
max_probes_per_run_exceeded = 4
execution_account_not_ready = 3
probe_concurrency_limit_exceeded = 1
```

Probe transport outcomes:

```text
counterfactual_shadow_probe_simulated = 5
simulation_error_rows = 0
```

## Join-Key Audit

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py`:

```text
probe_readiness.status = ready_for_probe_transport_entry_join
probe_readiness.join_key_acceptance = pass
probe_readiness.decision_join_acceptance = pass
probe_readiness.join_quality = exact_probe_id_and_ab_record_id

probe_selection exact_decision_v3_join = 50 / 50
probe_transport exact_decision_v3_join = 5 / 5
probe_entry exact_decision_v3_join = 5 / 5
feature_hash_mismatch = 0
policy_hash_mismatch = 0
unmatched_probe_transport_rows = 0
unmatched_probe_entry_rows = 0
```

The generic shadow join readiness remains `not_ready` because this smoke intentionally did not produce active shadow transport/entry/lifecycle artifacts. The probe-specific readiness gate is PASS.

## Simulation Error Analysis

`scripts/v3_p37_probe_simulation_error_report.py`:

```text
transport_rows = 5
simulation_error_rows = 0
category_counts = {}
custom_code_counts = {}
program_counts = {}
```

The previous `TooMuchSolRequired / Custom(6002)` class did not reproduce in this 5-probe smoke.

## Account Readiness

`scripts/v3_p37_probe_execution_account_readiness_report.py`:

```text
status = PASS
selected_probe_rows = 50
exact_decision_v3_join_rows = 50
classifications:
  execution_account_not_ready = 3
  unknown = 47
missing_account_roles:
  creator_vault = 3
  none = 47
collection_gate = HOLD
recommended_next_stage = read paired smoke and simulation-error report
```

The 3 `creator_vault` readiness skips did not consume the dispatch quota. Dispatch quota was consumed by ready rows that produced probe transport and entry records.

## Token-Parameter Context

The run did not populate the following Q5 diagnostic fields in `probe_transport.jsonl`:

```text
buy_variant = 0 / 5 populated
token_param_role = 0 / 5 populated
entry_token_amount_raw = 0 / 5 populated
min_tokens_out = 0 / 5 populated
```

This means R15-r9-q5b confirms probe transport/entry stability and no simulation-error reproduction, but it does not yet prove token-param-aware diagnostics are populated at runtime.

## Decision

```text
R15-r9-q5b smoke: MINIMAL PASS
V3 strict replay: PASS
probe transport/entry: PASS
exact decision/V3 join: PASS
active BUY mutation: PASS / no active BUY artifacts
simulation error status: PASS / no errors observed
token-param diagnostic completeness: INCOMPLETE
lifecycle/on-chain labels: NOT VALIDATED
small bounded collection: HOLD pending operator decision
Phase B / P2 / live / tuning: NO-GO
```

## Next Gate

Recommended next step:

```text
Either:
  A) proceed to a very small bounded collection with simulation-error classification enabled,
     accepting that token-param fields are not yet populated;
or
  B) repair/populate token-param fields in probe transport, then run one more tiny smoke.
```

Given the user goal of avoiding more timeout-based probing, option B should be a narrow code/logging fix only if token-param fields are considered required before bounded collection.
