# PR-TSV2-EIX-A0: Offline entry+exit intersection proof

Date: `2026-06-28`

Status: `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`

## Runtime Boundary

This is offline-only research evidence. It does not approve runtime changes, `shadow_close_only`, active close, BUY/REJECT changes, Gatekeeper policy changes, selector runtime changes, `v25_confidence`, V3 promotion, `alpha_31100`, XGBoost, TX builder/sender/Jito/live path changes, new masks, new thresholds, or R50 retuning.

Raw JSONL logs are local evidence only and must not be committed.

No R51 is approved from this result.

## Scopes

- R49: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- R50: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

## Evidence Inventory

| scope | artifact | exists | size_bytes | path |
| --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | shadow_lifecycle | True | 280671496 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | probe_shadow_lifecycle | True | 27343702 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | shadow_exit_replay_v1 | True | 11742516 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | gatekeeper_v2_decisions | False | 0 |  |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | a2_mask_summary | True | 2110106 | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | a2_cost_sensitivity | True | 666003 | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | a2_stability | True | 4373516 | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | shadow_lifecycle | True | 212628141 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_lifecycle.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | probe_shadow_lifecycle | True | 38523937 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/probe_shadow_lifecycle.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | shadow_exit_replay_v1 | True | 9226688 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_exit_replay_v1.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | gatekeeper_v2_decisions | True | 1370423178 | /mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/decisions/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/v2.5/v25_shadow/ba77fb17e35a73ba5efe8f4640339922d4f1ccaf3735ea18931f8b0e65f11970/gatekeeper_v2_decisions.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | a2_mask_summary | True | 2146365 | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | a2_cost_sensitivity | True | 654251 | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | a2_stability | True | 4450505 | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv |

## Fixed Rule Space

- Entry cohorts: `S1_F5, C1, C2, C3, C4`
- Exit masks: `M4_CONFIRM_2_WINDOWS, M5_DELAY_4000MS_CONFIRM, M6_DELAY_8000MS_CONFIRM, M7_CLASS_RESTRICTED`
- Exit cells: `(7500, -6000, 60000); (10000, -6000, 60000); (7500, -6000, 120000); (10000, -6000, 120000)`
- Fixed entry+exit rules requested: `80`
- Evaluable entry+exit rows: `0`
- Passing fixed rules: `0`
- Best fixed rule: `none`

## Threshold Manifest Preview

| family | cohort | field_or_rule | operator | threshold | source | used_in_eix |
| --- | --- | --- | --- | --- | --- | --- |
| entry | S1_F5 | current_market_cap_sol | >= | 30.2 | ORG-A0 S1/F5 fixed floor | True |
| entry | S1_F5 | bonding_progress_pct | >= | 36.5 | ORG-A0 S1/F5 fixed floor | True |
| entry | S1_F5 | price_change_ratio | >= | 1.012 | ORG-A0 S1/F5 fixed floor | True |
| entry | S1_F5 | buy_count | >= | 8.0 | ORG-A0 S1/F5 fixed floor | True |
| entry | S1_F5 | sol_buy_ratio | >= | 0.52 | ORG-A0 S1/F5 fixed floor | True |
| entry | C1 | current_market_cap_sol | <= | 78.026285883 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C1 | bonding_progress_pct | <= | 60.0 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C1 | price_change_ratio | <= | 1.8295869672926404 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C2 | avg_cpi_depth_50tx | <= | 2.627906976744186 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C2 | max_single_tx_price_impact_pct_observed | <= | 66.50471802361244 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C2 | compute_unit_cluster_dominance | <= | 0.4453125 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C3 | unique_ratio | >= | 0.5 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C4 | hhi | <= | 0.06198347107438017 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C4 | top3_signer_volume_ratio | <= | 0.5368185671073352 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | True |
| entry | C4 | top3_volume_pct | <= | 0.5368185671073352 | ORG-A0 train_s1_distribution_cut; reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv | False |
| exit_mask | TSV2 | M4_CONFIRM_2_WINDOWS | fixed_mask |  | PR-TSV2-A2 predeclared mask | True |
| exit_mask | TSV2 | M5_DELAY_4000MS_CONFIRM | fixed_mask |  | PR-TSV2-A2 predeclared mask | True |
| exit_mask | TSV2 | M6_DELAY_8000MS_CONFIRM | fixed_mask |  | PR-TSV2-A2 predeclared mask | True |

Full manifest: `reports/selector/tsv2_entry_exit_intersection_threshold_manifest.csv`

## Diagnostic Mask-Only Baseline

The rows below are existing A2 mask-only diagnostics without entry filtering. They are included as baselines only and are not entry+exit intersection proof.

| label | scope | mask_name | target_bps | stop_bps | max_hold_ms | retained_count | paired_delta_sum_bps | exit_action_precision | wilson_lower95 | target_cut_damage_ratio |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| r49_m4 | shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS | 10000 | -6000 | 120000 | 4748 | 442815 | 0.7351377477042049 | 0.7157011038976295 | 0.22391198215115862 |
| r49_m7 | shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | M7_CLASS_RESTRICTED | 10000 | -6000 | 60000 | 4748 | 67513 | 0.6524459613196815 | 0.6298755265892709 | 0.21134851617433772 |
| r50_m4 | shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M4_CONFIRM_2_WINDOWS | 10000 | -6000 | 120000 | 3656 | 439644 | 0.7645296584781306 | 0.7435808538283399 | 0.3295397323051803 |
| r50_m7 | shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | M7_CLASS_RESTRICTED | 10000 | -6000 | 60000 | 3656 | 498371 | 0.7136986301369863 | 0.6899746900640238 | 0.12227115724957699 |

## Blocking Evidence

`shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1:missing_gatekeeper_v2_decisions_jsonl`

The R49 scope has local lifecycle and exit replay evidence, but no local `gatekeeper_v2_decisions.jsonl` or equivalent materialized pre-entry feature rows were found in the clean worktree or the log volume. Lifecycle/replay rows do not contain the ORG-A0 pre-entry feature set required for S1_F5/C1/C2/C3/C4 filtering. The script therefore does not create proxy features from lifecycle fields and does not tune thresholds on R50.

Rescue audit also checked `/root/Gho`, `/root/Gho-tsv2-a1-a2-clean`, `/tmp`, `logs/rollout/**`, `logs/shadow_run/**`, `reports/selector/**`, `PLANS/AUDYT/**`, ORG-A0 intermediate artifacts, and joined/inventory/threshold CSV candidates. It found R49 lifecycle/entry/probe/event artifacts, but no R49 decision-time ORG-A0 feature surface. Existing R49 A2 attribution reports explicitly mark the pre-entry fields as `missing evidence: field unavailable`.

This means the EIX hypothesis is not numerically falsified. It is unevaluable in the current local evidence set because the R49 pre-entry feature surface is unavailable.

## Output Files

- `reports/selector/tsv2_entry_exit_intersection_summary.csv`
- `reports/selector/tsv2_entry_exit_intersection_stability.csv`
- `reports/selector/tsv2_entry_exit_intersection_cost_sensitivity.csv`
- `reports/selector/tsv2_entry_exit_intersection_tail_audit.csv`
- `reports/selector/tsv2_entry_exit_intersection_threshold_manifest.csv`
- `docs/ADR/ADR_8D_TSV2_ENTRY_EXIT_INTERSECTION_20260628.md`

## Final Verdict

`MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`

No runtime approval.
No `shadow_close_only` approval.
No active close.
No R51.

If the missing R49 pre-entry decision evidence cannot be recovered, active TSV2/ORG entry+exit intersection is closed as unevaluable for runtime in this evidence set. Any later retry must keep this fixed manifest and still cannot add masks, thresholds, or R49/R50 retuning.

## POST-EIX Contingency

- Do not start R51 from this result.
- Do not promote TSV2, ORG-A0, or an entry+exit intersection into runtime.
- Keep TSV2/ORG as diagnostic/logging-only evidence.
- The only rescue path is archival evidence recovery: recover the original R49 `gatekeeper_v2_decisions.jsonl` or an equivalent materialized pre-entry feature snapshot, then rerun this same fixed EIX script without changing masks, thresholds, or target/stop/hold cells.
- If the original R49 pre-entry evidence cannot be recovered, close EIX as `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`.
