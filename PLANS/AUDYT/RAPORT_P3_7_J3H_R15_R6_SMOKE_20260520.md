# RAPORT P3.7-J3H R15-r6 Probe Execution-Account Eligibility Smoke

Date: 2026-05-20

Status:

```text
P3.7-J3H code/runtime eligibility semantics: PASS
R15-r6 runtime smoke: NOT_READY_DIAGNOSED
V3/MFS replay path: PASS
probe selection exact decision/V3 join: PASS
probe transport/entry/lifecycle: ABSENT
collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Inputs

- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6.toml`
- namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6`
- runtime stdout: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/r15_r6_runtime_stdout.log`
- V3 shadow report: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/v3_shadow_report.json`
- strict replay report: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/v3_full_replay_report_strict.json`
- join-key audit: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/p3_7_j3h_r15_r6_join_key_audit.json`
- readiness report: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/p3_7_j3h_probe_execution_account_readiness.json`

## Runtime Result

```text
runtime_exit_code = 124
runtime_exit_reason = timeout_45m
```

The process completed by the configured bounded smoke timeout. No panic/crash
was observed in the gate artifacts.

## V3 / Replay

```text
v3_shadow_report.status = ok
v3_shadow_report.replay_status = full
raw_rows = 250
deduped_rows = 250
v3_rows = 250
bad_rows = 0
no_v3_rows = 0
full_replay_report.status = ok
full_replay_report.replay_status = full_replay_ok
full_replay_report.total_rows = 250
full_replay_report.bad_rows = 0
full_snapshot_payload_rows = 250
hash_only_rows = 0
policy_hash_coverage = 1.0
feature_snapshot_hash_coverage = 1.0
```

## Probe Artifacts

```text
probe_selection_rows = 5
probe_skipped_rows = 1062
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
```

`probe_selection.jsonl` and `probe_skips.jsonl` parsed cleanly:

```text
probe_selection.bad_json = 0
probe_skips.bad_json = 0
```

Skip reason distribution:

```text
probe_rate_limit_exceeded = 760
max_probes_per_run_exceeded = 159
verdict_type_not_in_sample_scope = 110
probe_concurrency_limit_exceeded = 28
execution_account_not_ready = 5
```

## Join-Key Result

Selection-level exact join is clean:

```text
probe_selection.exact_decision_v3_join = 5 / 5
probe_selection.feature_hash_match = 5 / 5
probe_selection.policy_hash_match = 5 / 5
probe_selection.unmatched_rows = 0
```

Overall probe join-key audit remains `not_ready` because no transport or entry
rows were generated:

```text
probe_readiness.status = not_ready
probe_readiness.reasons = [
  "missing_probe_transport_rows",
  "missing_probe_entry_rows",
  "probe_rows_missing_exact_decision_v3_join"
]
```

The last reason is an aggregate audit consequence of missing required probe
artifacts, not a selection hash mismatch. Selection exact decision/V3 continuity
is 100%.

## Execution-Account Readiness

All selected probes were diagnosed and converted into structured execution
account readiness skips:

```text
selected_probe_rows = 5
diagnosed_selected_probe_rows = 5
exact_decision_v3_join_rows = 5
classifications = {"execution_account_not_ready": 5}
missing_account_roles = {"bonding_curve_v2": 5}
```

Per selected probe, `precheck_failure_reason` uses the explicit schema:

```text
execution_account_not_ready:bonding_curve_v2:<pubkey>
```

This satisfies the J3H requirement that missing `bonding_curve_v2` is treated as
a strict core execution-account readiness failure, not as generic
`AccountNotFound` and not as a successful probe dispatch.

## Decision

```text
R15-r6 smoke = NOT_READY_DIAGNOSED
J3H semantics = PASS
collection = HOLD
Phase B = HOLD
P2/live/tuning = NO-GO
```

The counterfactual probe plane now fails closed with auditable
`execution_account_not_ready` skips for strict execution accounts. It still does
not produce probe transport/entry rows, so collection remains blocked.

## Recommended Next Step

Open a narrow follow-up:

```text
P3.7-J3I Probe Execution-Account Materialization Or Eligibility Narrowing
```

The next stage should decide whether to:

- materialize `bonding_curve_v2` readiness explicitly in decision-time-safe
  artifacts; or
- narrow probe eligibility to rows where strict execution accounts are already
  known and ready; or
- introduce a bounded decision-time-safe readiness wait, with explicit
  `probe_selected_ts_ms`, `probe_execution_ready_ts_ms`, and `probe_wait_ms`.

Do not bypass strict precheck, do not increase probe limits, and do not start
collection before a smoke run produces either probe entries or a deliberate
eligibility contract that excludes these rows before selection.
