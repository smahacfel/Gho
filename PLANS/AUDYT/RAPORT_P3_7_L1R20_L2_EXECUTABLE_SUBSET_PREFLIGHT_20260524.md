# P3.7-L1R20 L2 Executable-Subset Preflight

## Verdict

- source_l1r19_decision: `GO_L2_EXECUTABLE_SUBSET`
- preflight_status: `pass`
- final_decision: `GO_L2_EXECUTABLE_SUBSET_LOCKED`
- override_used: `False`

## Scope Lock

- L2 input universe is restricted to historical executable lifecycle-labeled namespaces.
- This report does not change scoring, thresholds, route fallback, live/P2, IWIM, or Gatekeeper policy.
- Full R16 route universe remains blocked unless explicitly overridden outside this default preflight.

## Allowed L2 Namespaces

- `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1`
- `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1`

## Input Totals

- total_rows: `4322`
- executable_eligible_rows: `87`
- excluded_non_executable_rows_within_allowed_runs: `2280`
- buy_quality_denominator_rows: `85`
- buy_quality_bad: `81`
- buy_quality_dirty_good: `4`
- buy_quality_good: `0`
- buy_quality_not_executable: `0`
- lifecycle_labeled_rows: `85`
- feature_join_executable_labeled_rows: `81`
- dirty_good_rate: `0.0471`
- usable_label_rate: `1.0000`

## Excluded Totals

- excluded_runs: `12`
- excluded_decision_rows_total: `6106`
- excluded_non_executable_rows: `3956`
- excluded_unsupported_route_rows: `11`
- excluded_buy_quality_denominator_rows: `0`
- excluded_dirty_good_rows: `0`

## Requested L2 Inputs

| namespace | decisions | route_exec | route_non_exec | lifecycle_labels | buy_denominator | bad | dirty_good | good | not_exec |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | 2344 | 44 | 1205 | 42 | 42 | 42 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | 1978 | 43 | 1075 | 43 | 43 | 39 | 4 | 0 | 0 |

## Excluded Runs

| namespace | class | decisions | route_non_exec | unsupported_route | buy_denominator | dirty_good |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2` | `excluded_no_buy_quality_denominator` | 850 | 554 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3` | `hard_blocked_unsupported_route_universe` | 451 | 275 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution` | `hard_blocked_unsupported_route_universe` | 427 | 300 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing` | `hard_blocked_unsupported_route_universe` | 641 | 452 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r6-bcv2-contract` | `hard_blocked_unsupported_route_universe` | 934 | 568 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r7-active-shadow-attribution` | `hard_blocked_unsupported_route_universe` | 1159 | 673 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution` | `hard_blocked_unsupported_route_universe` | 223 | 134 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck` | `hard_blocked_unsupported_route_universe` | 81 | 57 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r10-route-bcv2-source` | `hard_blocked_unsupported_route_universe` | 455 | 332 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness` | `hard_blocked_unsupported_route_universe` | 416 | 263 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r12-bcv2-provenance` | `hard_blocked_unsupported_route_universe` | 399 | 285 | 0 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver` | `hard_blocked_unsupported_route_universe` | 70 | 63 | 11 | 0 | 0 |

## Guardrails

- GO_L2_EXECUTABLE_SUBSET is not GO_LIVE_POLICY.
- GO_L2_EXECUTABLE_SUBSET is not GO_FULL_R16_ROUTE_UNIVERSE.
- Non-executable rows remain outside buy-quality denominators.
- Hard-blocked R16-r3..r13 namespaces cannot enter L2 without explicit override.
