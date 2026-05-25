# P3.7-E4RS Selected Fallback Route Handoff Smoke

Data: 2026-05-24
HEAD: `c5fe071`
Config lokalny: `configs/rollout/shadow-burnin-v3-p37-e4rs-selected-fallback-route-handoff-smoke.toml`
Namespace: `shadow-burnin-v3-p37-e4rs-selected-fallback-route-handoff-smoke`
Runtime: `timeout 20m`, shadow-only, promotion/P2/live off

## Werdykt

`E4RS` nie przechodzi. To jest runtime `FAIL` dla E4R handoffu.

Najkrotszy wynik:

```text
strict replay: PASS
diagnostic_quality: PASS
identity/hash: PASS
IWIM overflow panic: 0
post-simulation AccountNotFound: 0

legacy_buy route ready rows: active 3, probe 28
fallback_route_success_rows: active 3, probe 28
selected_fallback_route_ready_rows: active 3, probe 28
selected_fallback_route_handoff_applied_rows: active 0, probe 0
legacy_buy_selected_but_primary_bcv2_terminal_rows: active 3
successful entries: 0
lifecycle eligible rows: 0

overall: FAIL / selected fallback route handoff not applied
```

E4R nie pogorszyl readiness: resolver nadal widzi `legacy_buy` jako gotowy fallback. Problem jest dokladnie w kontrakcie, ktory E4R mial zamknac: wybrany fallback nie jest konsumowany przez active execution handoff. Active path nadal konczy terminalnie na primary `bonding_curve_v2` mimo `selected_route_kind = legacy_buy`.

## Run

Komenda:

```bash
timeout 20m env RUST_LOG=info ./target/release/ghost-launcher \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-e4rs-selected-fallback-route-handoff-smoke.toml
```

Proces zakonczyl sie kodem `124`, czyli przez oczekiwany limit `timeout 20m`. Po runie nie zostal aktywny proces `ghost-launcher`.

Artefakty:

```text
buys.jsonl                         2 rows
shadow_entries.jsonl               2 rows
shadow_lifecycle.jsonl             2 rows
probe_selection.jsonl              35 rows
probe_skips.jsonl                  534 rows
p3_7_l1_per_reject_diagnostics     539 rows
seer_runtime_coverage_audit.jsonl  463 rows
decision rows                      541 rows
```

## Replay / Diagnostics

`v3_full_replay_report.py --strict --json`:

```text
status = ok
replay_status = full_replay_ok
total_rows = 541
v3_rows = 541
bad_rows = 0
```

`v3_shadow_report.py --json`:

```text
status = ok
replay_status = full
raw_rows = 541
deduped_rows = 541
v3_rows = 541
bad_rows = 0
execution.success_count = 0
execution.outcomes.shadow_unknown_error = 2
execution.outcomes.missing = 539
```

`v3_p37_l1_reject_diagnostics.py --json`:

```text
diagnostic_quality.status = PASS
active_shadow_account_not_found_rows = 0
active_shadow_precheck_status_counts.precheck_failed = 4
active_shadow_lifecycle_eligibility_status_counts.not_lifecycle_eligible = 4
r16_buy_shadow_entry_count = 2
r16_buy_lifecycle_close_count = 2
```

Log grep:

```text
IWIM overflow / panic hits = 0
AccountNotFound / account not found hits = 0
```

## Required E4RS Counters

From `v3_p37_mfs_lifecycle_join_key_audit.py`.

Active path:

```text
selected_fallback_route_ready_rows = 3
selected_fallback_route_handoff_applied_rows = 0
selected_fallback_route_handoff_mismatch_rows = 0
selected_fallback_route_blocked_by_primary_reason_rows = 3

legacy_buy_selected_but_primary_bcv2_terminal_rows = 3
legacy_buy_selected_and_precheck_uses_legacy_account_set_rows = 0
legacy_buy_selected_and_simulation_uses_legacy_account_set_rows = 0

legacy_buy_route_ready_rows = 3
fallback_route_success_rows = 3
successful_entry_rows = 0
lifecycle_eligible_rows = 0
post_simulation_account_not_found_rows = 0
```

Probe path:

```text
selected_fallback_route_ready_rows = 28
selected_fallback_route_handoff_applied_rows = 0
selected_fallback_route_handoff_mismatch_rows = 0

legacy_buy_selected_but_primary_bcv2_terminal_rows = 0
legacy_buy_selected_and_precheck_uses_legacy_account_set_rows = 0
legacy_buy_selected_and_simulation_uses_legacy_account_set_rows = 0

legacy_buy_route_ready_rows = 28
fallback_route_success_rows = 28
successful_probe_entry_rows = 0
post_simulation_account_not_found_rows = 0
```

Execution feasibility:

```text
probe_selected_rows = 35
route_executable_rows = 31
route_non_executable_rows = 346
execution_feasibility_reject_rows = 6
active_buy_execution_infeasible_rows = 6
successful_entry_rows = 0
lifecycle_eligible_rows = 0
lifecycle_labeled_rows = 2
execution_feasibility_rate = 0.8857142857142857
entry_materialization_rate = 0.0
```

## Active Evidence

The critical row shape is:

```text
selected_route_kind = legacy_buy
route_resolution_status = fallback_route_ready
primary_route_not_ready_reason = bonding_curve_v2_observed_meta_missing_on_rpc
fallback_route_kind = legacy_buy
fallback_route_ready = true
legacy_buy_route_ready = true
execution_feasibility_status = executable
execution_feasibility_reason = fallback_route_ready
precheck_failure_reason = no_executable_route_account_set:primary_route_bcv2_missing:bonding_curve_v2:<pubkey>
```

That is the exact E4RS failure condition: the resolver chooses `legacy_buy`, but terminal precheck still reports primary `routed_exact_sol_in` / BCV2 failure.

## Probe Evidence

Probe side shows the same handoff gap in a different form:

```text
selected_route_kind = legacy_buy: 28
route_resolution_status = fallback_route_ready: 28
fallback_route_ready = true: 28
legacy_buy_route_ready = true: 28
selected_fallback_route_handoff_applied_rows = 0
successful_probe_entry_rows = 0
```

Probe does not show `legacy_buy_selected_but_primary_bcv2_terminal_rows`, but it also never records the selected fallback handoff as applied and never materializes entries.

## Interpretation

E4RS confirms:

```text
route resolver / feasibility side: improved and sees executable legacy fallback rows
execution handoff side: still not consuming selected fallback route
post-simulation AccountNotFound: still 0
IWIM overflow: still 0
```

This is not a Gatekeeper problem, not a threshold problem, and not a new route-selection problem. It is still the same E4R class: `selected_route_kind` is not the source of truth for the execution request/precheck/simulation manifest.

## Decision

```text
E4RS = FAIL
E4R code-level = not runtime-valid
legacy_buy = not declared unsupported yet
next = fix E4R handoff, specifically request/variant/account-set propagation
no E5
no Gatekeeper changes
no thresholds
no L2D2
no Phase B / P2 / live
```

Minimum next fix target:

```text
If selected_route_kind = legacy_buy and fallback_route_ready = true:
  PreparedBuyRequest / BuyAccountOverrides / active precheck / probe dispatch
  must carry legacy buy variant and selected fallback account set.

The primary_route_not_ready_reason may remain telemetry, but cannot remain
terminal execution reason for selected fallback rows.
```
