# PR-TSV2-A3: Two-scope fixed-cell validation summary

Date: `2026-06-28`

Status: `NO_TWO_SCOPE_FIXED_POLICY / INCONCLUSIVE_RESEARCH / NO_RUNTIME`

## Scope

- R49: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- R50: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

This is an offline fixed-cell intersection report. It does not introduce new masks, new thresholds, R48/R49/R50 retuning, runtime close behavior, `shadow_close_only`, Gatekeeper changes, selector runtime changes, `alpha_31100`, XGBoost, or TX/Jito/live path changes.

## Question

Does the same fixed combination `(mask, target_bps, stop_bps, max_hold_ms)` pass the A2 cost100 fixed-cell gate in both R49 and R50?

## Fixed-Cell Gate

A fixed cell passes a scope only if all conditions hold:

- `cost100_delta_sum_bps > 0`
- `cost100_delta_avg_bps > 0`
- `cost100_delta_median_bps >= 0`
- `cost100_exit_action_precision >= 0.70`
- `cost100_exit_action_precision_wilson95_lower >= 0.65`
- `cost100_aggregate_target_cut_damage_guard_pass = true`
- `cost100_segment_target_cut_damage_guard_pass = true`
- `cost100_target_cut_count_guard_pass = true`

## Required Cells

| cell | fixed tuple | pass R49 | pass R50 | passing both | R49 precision | R49 Wilson | R49 target-cut ratio | R50 precision | R50 Wilson | R50 target-cut ratio |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| canonical_fixed | `M0_ALL / 6000 / -6000 / 120000` | False | False | False | 0.7295158286778398 | 0.7103330535808028 | 0.40633133215612643 | 0.7597701149425288 | 0.7391381027103509 | 0.3766901769759775 |
| r49_selected_tested_on_r50 | `M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000` | False | False | False | 0.7351377477042049 | 0.7157011038976295 | 0.22391198215115862 | 0.7645296584781306 | 0.7435808538283399 | 0.3295397323051803 |
| r50_selected_tested_on_r49 | `M7_CLASS_RESTRICTED / 10000 / -6000 / 60000` | False | True | False | 0.6524459613196815 | 0.6298755265892709 | 0.21134851617433772 | 0.7136986301369863 | 0.6899746900640238 | 0.12227115724957699 |

## Intersection Result

- `fixed_cell_passing_both_count = 0`
- `canonical_cell_passing_both = false`
- `R49_selected_cell_validated_by_R50 = false`
- `R50_selected_cell_validated_by_R49 = false`
- `absolute_profitability_proven = false`
- `runtime_approval = false`
- `shadow_close_only_approval = false`

## Passing Fixed Cells

| fixed tuple | R49 delta sum | R49 precision | R49 target-cut ratio | R50 delta sum | R50 precision | R50 target-cut ratio | absolute profitable both cost100/200 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| none |  |  |  |  |  |  |  |

## Interpretation

R50 confirms that selective TSV2 masks can reduce damage versus a losing baseline, but selected rows differ between R49 and R50. A3 therefore checks only fixed-cell intersection and does not promote any runtime behavior.

No fixed cell passed both scopes. Therefore R49+R50 do not establish a stable fixed TSV2 close-policy candidate.

## Final Verdict

`NO_TWO_SCOPE_FIXED_POLICY / INCONCLUSIVE_RESEARCH / NO_RUNTIME`

No runtime approval.
No `shadow_close_only` approval.

Next step: close active TSV2 exit direction for runtime and keep TSV2 as diagnostic/logging-only. R51 predeclared validation would require an explicitly accepted fixed-cell hypothesis, which A3 did not find.
