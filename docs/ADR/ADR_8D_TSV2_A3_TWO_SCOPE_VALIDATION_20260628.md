# ADR-8D: TSV2 A3 two-scope fixed-cell validation

Status: NO_TWO_SCOPE_FIXED_POLICY / INCONCLUSIVE_RESEARCH / NO_RUNTIME
Typ: ADR-8D / offline research evidence
Data: 2026-06-28
Autor/Agent: Codex
Zakres: PR-TSV2-A3 fixed-cell two-scope validation
Poziom ryzyka: MEDIUM

Powiazane scope:
- R49: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- R50: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

## 1. Decyzja

A3 compares already generated A2 CSV reports from R49 and R50. It does not add masks, thresholds, runtime behavior, active close, `shadow_close_only`, selector runtime policy, Gatekeeper policy, `alpha_31100`, XGBoost, or TX/Jito/live path changes.

## 2. Fixed cells

| cell | fixed tuple | pass R49 | pass R50 | passing both | R49 precision | R49 Wilson | R49 target-cut ratio | R50 precision | R50 Wilson | R50 target-cut ratio |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| canonical_fixed | `M0_ALL / 6000 / -6000 / 120000` | False | False | False | 0.7295158286778398 | 0.7103330535808028 | 0.40633133215612643 | 0.7597701149425288 | 0.7391381027103509 | 0.3766901769759775 |
| r49_selected_tested_on_r50 | `M4_CONFIRM_2_WINDOWS / 10000 / -6000 / 120000` | False | False | False | 0.7351377477042049 | 0.7157011038976295 | 0.22391198215115862 | 0.7645296584781306 | 0.7435808538283399 | 0.3295397323051803 |
| r50_selected_tested_on_r49 | `M7_CLASS_RESTRICTED / 10000 / -6000 / 60000` | False | True | False | 0.6524459613196815 | 0.6298755265892709 | 0.21134851617433772 | 0.7136986301369863 | 0.6899746900640238 | 0.12227115724957699 |

## 3. Result

- `fixed_cell_passing_both_count = 0`
- `canonical_cell_passing_both = false`
- `R49_selected_cell_validated_by_R50 = false`
- `R50_selected_cell_validated_by_R49 = false`
- `absolute_profitability_proven = false`
- `runtime_approval = false`
- `shadow_close_only_approval = false`

## 4. Passing fixed cells

| fixed tuple | R49 delta sum | R49 precision | R49 target-cut ratio | R50 delta sum | R50 precision | R50 target-cut ratio | absolute profitable both cost100/200 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| none |  |  |  |  |  |  |  |

## 5. Consequences

Final verdict: `NO_TWO_SCOPE_FIXED_POLICY / INCONCLUSIVE_RESEARCH / NO_RUNTIME`

This is not runtime approval and not `shadow_close_only` approval. A3 did not find a fixed-cell hypothesis to carry into R51. The active TSV2 exit direction remains rejected for runtime; TSV2 can remain diagnostic/logging-only.
