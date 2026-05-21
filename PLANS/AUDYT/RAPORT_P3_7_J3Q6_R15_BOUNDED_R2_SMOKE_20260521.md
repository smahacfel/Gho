# RAPORT P3.7-J3Q6 R15 Bounded Q6-R2 Smoke

## Verdict

```text
R15 bounded q6-r2: MINIMAL PASS / ENTRY PATH VALIDATED
V3/MFS replay: PASS
probe selection -> transport -> entry exact join: PASS
probe simulation errors: 0
probe lifecycle/on-chain labels: NOT VALIDATED
active BUY mutation: PASS / no active BUY artifacts
bounded/full collection scale-up: HOLD
Phase B / P2 / live / tuning: NO-GO
```

This run supersedes the earlier `r15-bounded-q6` result for runtime interpretation,
because the earlier run used a stale `target/release/ghost-launcher` binary. For
`q6-r2`, `target/release/ghost-launcher` was rebuilt at
`2026-05-21 03:48:58 UTC`, the config was created at
`2026-05-21 03:49:21 UTC`, preflight passed, and the run ended under the
fresh namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2
```

## Runtime Summary

```text
config:
  configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2.toml

runtime log:
  logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2/tmux_launcher.log

release binary:
  target/release/ghost-launcher

probe_selection_rows = 514
probe_skip_rows = 559
probe_transport_rows = 4
probe_shadow_entry_rows = 4
probe_lifecycle_rows = 0
active_buys_rows = 0
active_shadow_entry_rows = 0
active_shadow_lifecycle_rows = 0
```

## Replay Status

```text
v3_rows = 90
strict_replay_status = full_replay_ok
strict_replay_total = 90
strict_replay_bad_rows = 0
full_snapshot_payload_rows = 90
hash_only_rows = 0
```

## Probe Join-Key Status

```text
probe_readiness = ready_for_probe_transport_entry_join
probe_join_key_acceptance = pass
probe_join_quality = exact_probe_id_and_ab_record_id
probe_decision_join_acceptance = pass
required_exact_decision_v3_join_coverage = 1.0

probe_selection exact_decision_v3_join = 514 / 514
probe_transport exact_decision_v3_join = 4 / 4
probe_entry exact_decision_v3_join = 4 / 4
feature_hash_mismatch = 0
policy_hash_mismatch = 0
```

The main lifecycle join gate remains `not_ready` only because this namespace
does not contain active shadow transport/entry/lifecycle rows. That is expected
for this probe-only smoke and does not invalidate the counterfactual probe join.

## Entry Materialization

```text
transport_rows = 4
entry_rows = 4
status_counts = {"entry_materialized": 4}
reason_counts = {"entry_row_present": 4}
buy_variant_counts = {"legacy_buy": 4}
token_param_role_counts = {"token_amount": 4}
simulation_error_rows = 0
transport_only_missing_token_quantity_rows = 0
```

All four dispatched probes were `legacy_buy` / `token_amount` rows and produced
entry rows with `entry_token_amount_raw` and `min_tokens_out`.

## Simulation Error Status

```text
transport_rows = 4
simulation_error_rows = 0
category_counts = {}
custom_code_counts = {}
program_counts = {}
```

This is the key improvement over the stale-binary `q6` run: the previous
`InstructionError(3, Custom(3005))` class did not reproduce with the fresh
release binary.

## Skip Distribution

```text
probe_execution_precheck_failed = 396
verdict_type_not_in_sample_scope = 163

precheck_failure_reason:
  missing_bonding_curve = 385
  missing_execution_route_identity = 10
  missing_required_account:payer_pubkey:9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw = 1
```

Interpretation:

```text
probe transport/entry path is now valid for execution-ready legacy_buy rows;
the limiting factor is execution-account readiness coverage, especially
missing_bonding_curve for the sampled universe.
```

## Governance

```text
counterfactual probe rows are not BUY rows
shadow simulation is not live inclusion
lifecycle outcome is still absent for probe rows
no P2/live/threshold tuning was performed
no active Gatekeeper/IWIM policy was changed
```

## Decision

```text
Q6 entry materialization guard: PASS
R15 bounded q6-r2 probe transport/entry: MINIMAL PASS
probe lifecycle collection: NOT VALIDATED
next blocker: execution-account readiness coverage / missing_bonding_curve
bounded/full collection scale-up: HOLD until readiness strategy is decided
```

Recommended next step:

```text
P3.7-J3J — Probe Execution-Account Readiness Coverage / Wait Strategy
```

The next decision should not be another blind timeout. Either tighten sampling
to rows with execution-ready account identity, or add a bounded
decision-time-safe wait for required execution accounts and log the wait result.
