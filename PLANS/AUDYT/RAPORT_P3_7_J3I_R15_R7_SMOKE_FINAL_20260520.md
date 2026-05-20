# RAPORT P3.7-J3I R15-r7 Smoke Final

Date: 2026-05-20

Status:

```text
R15-r7 runtime smoke: NOT_READY_DIAGNOSED
R15-r7 termination: operator_stopped_before_timeout
J3I scan/dispatch split: OBSERVED
V3/MFS replay path: PASS
Probe selection -> decision/V3 join: PASS
Probe transport/entry/lifecycle: ABSENT
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
Next repair: P3.7-J3I2 scan-plane throughput repair
```

## Scope

This report supersedes the in-flight R15-r7 partial smoke snapshot.

The `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7` runtime was
manually stopped before its natural timeout after the operator requested that
the run be closed and the next repair task be started. The final artifacts were
then read from disk and summarized without starting a new run.

## Inputs

- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7.toml`
- runtime stdout: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/r15_r7_runtime_stdout.log`
- V3 shadow report: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/v3_shadow_report_final.json`
- strict replay report: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/v3_full_replay_report_strict_final.json`
- join-key audit: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/p3_7_j3i_r15_r7_join_key_audit_final.json`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_selection.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_skips.jsonl`

## Final Counts

```text
v3_shadow_report.raw_rows = 199
v3_shadow_report.deduped_rows = 199
v3_shadow_report.v3_rows = 199
v3_shadow_report.bad_rows = 0
v3_shadow_report.replay_status = full

strict_replay.total_rows = 199
strict_replay.v3_rows = 199
strict_replay.bad_rows = 0
strict_replay.status = full_replay_ok

probe_selection_rows = 548
probe_skipped_rows = 1079
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
```

All final JSONL files that existed for probe selection and skips parsed cleanly:

```text
probe_selection.jsonl bad_json = 0
probe_skips.jsonl bad_json = 0
```

## Probe Skip Breakdown

Final `probe_skips.jsonl` breakdown:

```text
execution_account_not_ready = 543
probe_scan_concurrency_limit_exceeded = 283
verdict_type_not_in_sample_scope = 249
probe_execution_precheck_failed = 4
```

Execution-account readiness roles:

```text
bonding_curve_v2 = 519
creator_vault = 22
associated_bonding_curve = 1
mint = 1
none / not applicable = 536
```

Readiness status:

```text
not_ready = 543
pending_runtime_precheck = 283
precheck_failed = 4
none = 249
```

Probe buckets:

```text
probe_selection.v3_reject_manipulation_contradiction = 493
probe_selection.active_reject_v3_pending = 55

probe_skips.v3_reject_manipulation_contradiction = 747
probe_skips.random_eligible_control = 249
probe_skips.active_reject_v3_pending = 83
```

## Join-Key Result

The final join-key audit reports:

```text
probe_selection_rows = 548
probe_selection exact_decision_v3_join = 548
probe_selection exact_decision_v3_join_coverage = 1.0
probe_selection feature_hash_mismatch = 0
probe_selection policy_hash_mismatch = 0

probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
probe_readiness.status = not_ready
probe_join_key_acceptance = fail
```

The join-key failure is caused by missing downstream probe artifacts, not by
selection hash drift. R15-r7 still does not satisfy the minimal runtime smoke
gate because it produced no probe transport or entry rows.

## Interpretation

J3I successfully prevents execution-account-not-ready rows from consuming the
dispatch quota. The run did not produce counterfactual transport or entry rows
because no candidate reached execution-account-ready dispatch.

However, R15-r7 also shows material scan-plane pressure:

```text
probe_scan_concurrency_limit_exceeded = 283
```

That means the run cannot be used as proof that there were no
execution-account-ready rows in the candidate universe. A large number of
candidate rows were not evaluated by strict readiness precheck because the scan
plane itself was saturated.

## Decision

```text
R15-r7 final smoke: NOT_READY_DIAGNOSED
V3/MFS replay: PASS
selection -> decision exact V3 join: PASS
probe transport / entry: ABSENT
collection: HOLD
```

Next repair:

```text
P3.7-J3I2 Probe Scan-Plane Throughput Repair
```

The next repair must increase the ability to evaluate candidate readiness
without increasing the dispatch probe quota, without weakening strict execution
account precheck, and without treating missing execution accounts as success.

