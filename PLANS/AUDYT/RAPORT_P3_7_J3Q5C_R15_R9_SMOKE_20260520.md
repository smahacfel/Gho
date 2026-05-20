# RAPORT P3.7-J3Q5C R15-r9 Smoke

Date: 2026-05-20
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c`
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c.toml`

## Verdict

`R15-r9-q5c: MINIMAL PASS / TOKEN-PARAM DIAGNOSTICS POPULATED`

This run used a freshly rebuilt `target/debug/ghost-launcher`. The previous Q5b smoke used a stale launcher binary and therefore did not prove runtime population of token-param fields.

## Runtime Handling

The run was stopped manually after the bounded probe gate produced probe transport rows with populated token-param fields. Waiting for the full timeout was not useful for this gate.

## V3 Replay

`scripts/v3_shadow_report.py`:

```text
status = ok
v3_rows = 1
raw_rows = 1
deduped_rows = 1
bad_rows = 0
full_snapshot_payload_rows = 1
hash_only_rows = 0
stale_against_config = false
policy_hash_unique_count = 1
snapshot_hash_unique_count = 1
```

`scripts/v3_full_replay_report.py --strict`:

```text
status = ok
replay_status = full_replay_ok
total_rows = 1
v3_rows = 1
bad_rows = 0
full_replay_ok = 1
```

The replay sample is small because the smoke was intentionally stopped early after the probe diagnostics gate was satisfied.

## Probe Artifacts

```text
probe_selection_rows = 16
probe_skips_rows = 10
probe_transport_rows = 5
probe_shadow_entry_rows = 4
probe_lifecycle_rows = 0
```

Probe skip reasons:

```text
probe_rate_limit_exceeded = 9
execution_account_not_ready = 1
```

Probe transport outcomes:

```text
counterfactual_shadow_probe_simulated = 5
simulation_error_rows = 0
```

## Token-Parameter Fields

`probe_transport.jsonl` now carries the Q5 token-param fields:

```text
buy_variant populated = 5 / 5
token_param_role populated = 5 / 5
entry_token_amount_raw populated = 4 / 5
min_tokens_out populated = 5 / 5
```

Variant distribution:

```text
legacy_buy / token_amount = 4
routed_exact_sol_in / min_tokens_out = 1
```

The single `routed_exact_sol_in` probe had:

```text
entry_token_amount_raw = null
min_tokens_out = 1
```

That row produced a simulated transport record but no probe entry row, because entry-price derivation requires token quantity. The 4 `legacy_buy` rows carried token quantities and produced probe entry rows.

## Join-Key Audit

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py`:

```text
probe_readiness.status = ready_for_probe_transport_entry_join
probe_readiness.join_key_acceptance = pass
probe_readiness.decision_join_acceptance = pass
probe_readiness.join_quality = exact_probe_id_and_ab_record_id

probe_selection exact_decision_v3_join = 16 / 16
probe_transport exact_decision_v3_join = 5 / 5
probe_entry exact_decision_v3_join = 4 / 4
feature_hash_mismatch = 0
policy_hash_mismatch = 0
unmatched_probe_transport_rows = 0
unmatched_probe_entry_rows = 0
```

The generic active-shadow join readiness remains `not_ready`, as expected for a counterfactual probe smoke with no active BUY/shadow artifacts.

## Simulation Error Analysis

`scripts/v3_p37_probe_simulation_error_report.py`:

```text
transport_rows = 5
simulation_error_rows = 0
category_counts = {}
custom_code_counts = {}
program_counts = {}
```

The previous `TooMuchSolRequired / Custom(6002)` class did not reproduce in this smoke.

## Decision

```text
Q5 token-param population: PASS
V3 strict replay: PASS
probe transport: PASS
probe entry: PARTIAL PASS, 4/5
exact decision/V3 join: PASS
active BUY mutation: PASS / no active BUY path required
simulation error status: PASS / no errors observed
lifecycle/on-chain labels: NOT VALIDATED
collection: HOLD pending next operator decision
Phase B / P2 / live / tuning: NO-GO
```

## Recommended Next Step

The immediate token-param blocker is closed. The remaining concrete issue is the `routed_exact_sol_in` transport row with `entry_token_amount_raw = null`. Recommended next step before scaling beyond smoke:

```text
P3.7-J3Q6:
  classify routed_exact_sol_in rows with missing entry_token_amount_raw as
  probe_entry_not_materialized / no_entry_price_token_quantity_missing,
  or tighten probe eligibility to dispatch only rows with entry_token_amount_raw when entry rows are required.
```

If the next step is a very small bounded collection, reports must explicitly separate:

```text
transport-only probes
entry-materialized probes
simulation-error probes
lifecycle-closed probes
```
