# P3.7 J4C R15 Lifecycle Label R1 Final

Date: 2026-05-21

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1
```

Status:

```text
J4C lifecycle-label run: COMPLETE
probe transport/entry/lifecycle: PASS
probe label generation: PASS
feature availability: INSUFFICIENT_FOR_SELECTOR
Phase B / P2 / live: NO-GO
```

## Process State

The tmux run was stopped after the probe dispatch cap had already been reached.

```text
probe_transport_rows = 50
probe_shadow_entry_rows = 50
```

No `ghost-launcher` process for this namespace remained after shutdown.

## V3 Replay

Strict replay completed successfully.

```text
v3_rows = 267
full_replay_ok = 267
bad_rows = 0
full_snapshot_payload_rows = 267
hash_only_rows = 0
stale_against_config = false
```

## Probe Artifacts

```text
probe_selection_rows = 151
probe_skip_rows = 2025
probe_transport_rows = 50
probe_shadow_entry_rows = 50
probe_shadow_lifecycle_rows = 84
probe_onchain_lifecycle_rows = 42
probe_label_rows = 42
```

Lifecycle rows are paired:

```text
exit_filled = 42
position_closed = 42
closed_probe_positions = 42
```

## Probe Join-Key Audit

Probe join-key status passed.

```text
probe_readiness.status = ready_for_probe_transport_entry_join
probe_readiness.join_quality = exact_probe_id_and_ab_record_id
probe_readiness.join_key_acceptance = pass
probe_readiness.decision_join_acceptance = pass
```

Exact decision/V3 join coverage:

```text
probe_selection = 151/151
probe_transport = 50/50
probe_entry = 50/50
probe_lifecycle = 84/84
```

Probe chain coverage:

```text
probe_chain_ab_record_id_coverage = 1.0
probe_chain_probe_id_coverage = 1.0
```

## Probe Execution Outcomes

```text
counterfactual_shadow_probe_simulated = 42
counterfactual_shadow_probe_simulation_error = 8
```

Entry materialization:

```text
entry_materialized = 42
simulation_error = 8
transport_without_entry = 0
transport_only_missing_token_quantity = 0
```

Simulation errors:

```text
custom_2006 = 8
category = simulation_account_layout_mismatch
account_role = creator_vault
```

This confirms the routed token-quantity materialization path is fixed, but
creator-vault authority/route identity remains a real error class.

## Probe Skips

Dominant skip reasons:

```text
creator_vault_source_not_authoritative = 1198
verdict_type_not_in_sample_scope = 484
probe_execution_precheck_failed = 241
max_probes_per_run_exceeded = 69
probe_rate_limit_exceeded = 18
execution_account_not_ready = 7
probe_concurrency_limit_exceeded = 6
active_buy_excluded = 2
```

The run remains heavily constrained by fail-closed route/account authority
checks. This is expected under the current probe safety contract, but it keeps
yield low.

## Natural Shadow Artifacts

The run also produced natural shadow-only active BUY artifacts:

```text
buys.jsonl = 2
shadow_entries.jsonl = 2
shadow_lifecycle.jsonl = 6
```

Those rows are `entry_mode = shadow_only`; no live/P2 path was enabled.

## On-Chain Lifecycle Report

The probe on-chain lifecycle report wrote 42 rows.

```text
close_truth_coverage = 42/42
entry_drift_pct.mean = 0.895253
entry_drift_pct.p95_abs = 3.318891
exit_drift_pct.mean ~= 0
entry_truth_gap_ms.mean = 7991.928571
exit_truth_gap_ms.mean = 38148.333333
```

## Labels

The labeler generated 42 labels.

```text
rows_total = 42
analysis_status = ok: 42
buy_quality_bad = 42
market_bad_clean = 42
label_quality = degraded: 42
phase_f_label_status = not_accepted
```

Degradation drivers:

```text
missing_gatekeeper_buy_context = 42
speculative_curve_finality = 38
nonstandard_curve_finality = 4
entry_truth_gap_too_large = 10
exit_truth_gap_too_large = 4
```

## Feature Availability

Feature availability matched the 42 labels, but the dataset is not selector
ready.

```text
rows_total = 42
buy_quality_bad = 42
market_bad_clean = 42
feature_availability_status = insufficient_for_selector
```

The main limitation is class balance: all labels are bad. There are no good or
dirty-good examples in this run.

## Decision

This run validates the counterfactual probe lifecycle plumbing:

```text
V3/MFS decision row
-> probe selection
-> probe transport
-> probe shadow entry
-> probe lifecycle close
-> on-chain lifecycle report
-> lifecycle labels
-> feature availability join
```

It does not validate selector readiness.

Operational verdict:

```text
J4C lifecycle-label plumbing: PASS
current labels: all bad / degraded
selector prototype: NO-GO
P2/live: NO-GO
next policy work: investigate active Gatekeeper config / PDD long-mode suppression before more probe scaling
```
