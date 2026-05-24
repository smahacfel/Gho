# P3.7-L1R19 Executable Universe Discovery / Route Support Decision

## Verdict

- final_decision: `GO_L2_EXECUTABLE_SUBSET`
- reason: At least one existing run has executable lifecycle-labeled rows with a non-empty buy-quality denominator.
- reason: L2 remains scoped to that executable labeled subset; non-executable rows stay in execution-feasibility reporting.

## Scope

- Offline denominator audit only.
- No runtime, no threshold changes, no route fallback implementation, no collection, no P2/live.
- Successful entries/lifecycle labels in older artifacts are treated as inferred executable evidence when L1R18-native route fields are absent.

## Totals

- configs_considered: `14`
- configs_missing: `0`
- runs_with_usable_executable_labeled_subset: `2`
- runs_execution_route_support_blocked: `11`
- runs_executable_without_lifecycle_labels: `1`
- runs_with_audit_gap: `0`
- total_route_executable_rows: `89`
- total_lifecycle_labeled_rows: `85`
- total_buy_quality_denominator_rows: `85`
- total_buy_quality_dirty_good: `4`
- total_buy_quality_good: `0`
- total_execution_feasibility_reject_rows: `11`

## Per-Run Denominators

| namespace | status | decisions | probe_selected | active_buy | route_exec | route_non_exec | exec_reject | active_buy_infeasible | success_entry | sim_error_entry | lifecycle_eligible | lifecycle_labels | buy_denominator | bad | dirty_good | good | not_exec | feature_join_exec_labels | exec_rate | entry_rate | lifecycle_rate | usable_label_rate | evidence |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `usable_executable_labeled_subset` | 2344 | 151 | 2 | 44 | 1205 | 0 | 0 | 44 | 8 | 42 | 42 | 42 | 42 | 0 | 0 | 0 | 42 | 0.2876 | 1.0000 | 0.9545 | 1.0000 | `inferred_from_successful_entry_or_lifecycle_label` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `usable_executable_labeled_subset` | 1978 | 163 | 6 | 43 | 1075 | 0 | 0 | 43 | 15 | 43 | 43 | 43 | 39 | 4 | 0 | 0 | 39 | 0.2544 | 1.0000 | 1.0000 | 1.0000 | `inferred_from_successful_entry_or_lifecycle_label` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2` | `executable_without_lifecycle_labels` | 850 | 72 | 7 | 2 | 554 | 0 | 0 | 2 | 15 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0253 | 1.0000 | 0.0000 | n/a | `inferred_from_successful_entry_or_lifecycle_label` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3` | `execution_route_support_blocked` | 451 | 39 | 0 | 0 | 275 | 0 | 0 | 0 | 15 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution` | `execution_route_support_blocked` | 427 | 22 | 4 | 0 | 300 | 0 | 0 | 0 | 23 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing` | `execution_route_support_blocked` | 641 | 48 | 9 | 0 | 452 | 0 | 0 | 0 | 33 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r6-bcv2-contract` | `execution_route_support_blocked` | 934 | 39 | 12 | 0 | 568 | 0 | 0 | 0 | 24 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r7-active-shadow-attribution` | `execution_route_support_blocked` | 1159 | 73 | 27 | 0 | 673 | 0 | 0 | 0 | 54 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution` | `execution_route_support_blocked` | 223 | 15 | 1 | 0 | 134 | 0 | 0 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck` | `execution_route_support_blocked` | 81 | 3 | 2 | 0 | 57 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r10-route-bcv2-source` | `execution_route_support_blocked` | 455 | 47 | 6 | 0 | 332 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness` | `execution_route_support_blocked` | 416 | 16 | 7 | 0 | 263 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r12-bcv2-provenance` | `execution_route_support_blocked` | 399 | 22 | 3 | 0 | 285 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver` | `execution_route_support_blocked` | 70 | 2 | 3 | 0 | 63 | 11 | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.0000 | n/a | n/a | n/a | `native_execution_feasibility` |

## Interpretation

- `GO_L2_EXECUTABLE_SUBSET` does not promote full R16 route support.
- It only means historical artifacts contain a non-empty executable lifecycle-labeled denominator suitable for scoped L2 analysis.
- Rows with `no_executable_route_account_set` remain outside buy-quality denominators and must stay in execution-feasibility reporting.
