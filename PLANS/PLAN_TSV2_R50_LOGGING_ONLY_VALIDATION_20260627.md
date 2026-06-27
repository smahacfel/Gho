# PLAN: R50 TSV2 logging-only validation run

Data: 2026-06-27

Status: PLAN_READY / LOGGING_ONLY / NO_RUNTIME_DECISION

Scope name:

`shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

Config-prep files:

- `configs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r50_tsv2_logging_only_target60_stop60_exit_replay_maxwait66000_fsc_off.toml`

## 1. Goal

Collect one independent full TSV2-window scope for validation of the already committed A2 target-cut attribution proof.

This is not A3, not a runtime promotion plan, not a new mask search, and not a new threshold tuning pass. The only allowed post-run analysis is to rerun the existing A2 report unchanged on the new R50 scope.

## 2. Current research boundary

- ORG-A0: `REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`
- A1: `INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME`
- A2: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`
- R48/R2 matrix: global exit grid is negative after realistic costs and has negative median.
- No runtime change is justified.
- No `shadow_close_only` is justified.
- No further R48/R49 tuning is allowed.

## 3. Runtime non-goals

R50 must explicitly keep:

- no active close
- no `shadow_close_only`
- no live close
- no BUY/REJECT change
- no Gatekeeper policy change
- no selector runtime change
- no `alpha_31100`
- no `v25_confidence` change
- no V3 promotion change
- no TX builder / sender / Jito path change
- no A2 mask additions
- no A2 threshold tuning
- no R48/R49 backfit

## 4. Required TimeStop V2 window emission knobs

Config file:

`configs/rollout/ghost_brain_selector_dataset_sampler_r50_tsv2_logging_only_target60_stop60_exit_replay_maxwait66000_fsc_off.toml`

Required block:

```toml
[post_buy_guardian.time_stop_v2]
enabled = true
mode = "observe_only"
first_check_ms = 3000
window_ms = 4000
failed_windows_to_signal = 3
min_age_before_signal_ms = 11000
min_price_delta_pct_alive = 3.0
min_mcap_delta_pct_alive = 3.0
min_bonding_delta_pct_alive = 0.75
min_volume_delta_sol_alive = 1.0
min_price_delta_pct_for_volume_alive = 1.0
min_tx_delta_for_heartbeat = 1
max_avg_volume_per_tx_sol_heartbeat = 0.05
max_abs_price_delta_pct_heartbeat = 1.0
max_abs_mcap_delta_pct_heartbeat = 1.0
max_bonding_delta_pct_heartbeat = 0.25
emit_window_records = true
```

Interpretation:

- `mode = "observe_only"` is mandatory; it may emit `time_stop_v2_window` evidence but must not close positions.
- `emit_window_records = true` is mandatory for A2 coverage.
- These values are carried forward from R49 for independent validation, not retuned on R49/R48 outcomes.

## 5. Required shadow exit replay knobs

Config file:

`configs/rollout/ghost_brain_selector_dataset_sampler_r50_tsv2_logging_only_target60_stop60_exit_replay_maxwait66000_fsc_off.toml`

Required block:

```toml
[post_buy_guardian.exit_replay_v1]
enabled = true
flush_on_shutdown = false
shutdown_flush_budget_ms = 3000
levels_bps = [
  -6000, -5000, -3000, -2000, -1500, -1000, -700, -500,
  -300, -200, -100, 100, 200, 300, 400, 500, 700, 1000,
  1500, 2000, 3000, 5000, 6000, 7500, 10000,
]
```

Launcher/runtime wiring expectation:

- `[execution.shadow].lifecycle_log_path` points at `logs/shadow_run/<scope>/shadow_lifecycle.jsonl`.
- The launcher derives `shadow_exit_replay_v1.jsonl` next to the shadow lifecycle path.
- `[p37_shadow_probe].lifecycle_log_path` points at `logs/shadow_run/<scope>/probe_shadow_lifecycle.jsonl`.

## 6. Shadow-only execution knobs

Config file:

`configs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1.toml`

Required settings:

```toml
[trigger]
entry_mode = "shadow_only"

[trigger.shadow_run]
enabled = true

[execution]
execution_mode = "shadow"

[p37_shadow_probe]
enabled = true
namespace = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"
sample_source = "v3_mfs_decision_rows"
sample_mode = "deterministic_hash_mod"
sample_modulus = 1
sample_threshold = 1
```

Operational note:

- Metrics port is set to `9137`.
- GUI backend remains disabled, with port `8837` reserved only to avoid collision if enabled accidentally.

## 7. Expected output files

Raw runtime outputs:

- `logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/probe_shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_exit_replay_v1.jsonl`
- `time_stop_v2_window` rows inside the lifecycle JSONL streams
- `logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/decisions/`

Post-run A1/A2 report outputs:

- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/TIME_STOP_V2_NOHARM_PROOF_A1.md`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_noharm_summary_v1.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_noharm_cost_sensitivity_v1.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_noharm_stability_v1.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_noharm_grid_neighborhood_v1.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/TIME_STOP_V2_TARGET_CUT_ATTRIBUTION_A2.md`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_target_cut_attribution_a2.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv`
- `reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_grid_neighborhood_a2.csv`

## 8. Minimum sample acceptance

R50 is analyzable only if all minimum coverage checks pass:

- `positions_with_exit_replay >= 3000`
- `positions_with_tsv2_windows >= 3000`
- `exact_join_rate >= 98%`
- `candidate_positions >= 2500`
- `path_approximate_rows = 0` preferred

If any minimum is missed, the run is coverage-inconclusive and must not be used for shadow-close planning.

## 9. Post-run A2 command

Run the existing A2 script unchanged. Do not add masks, thresholds, or R50-specific tuning.

```bash
SCOPE="shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"

python3 scripts/time_stop_v2_counterfactual_lab.py \
  --scope "$SCOPE" \
  --target-bps 6000 \
  --stop-bps -6000 \
  --max-hold-ms 120000 \
  --resurrection-windows-ms 4000,8000,12000 \
  --enable-noharm-a2
```

## 10. R50 success criteria

R50 passes only if the unchanged A2 report shows all of:

- `action_precision >= 0.70`
- Wilson lower 95% `>= 0.65`
- `paired_delta_sum_bps > 0`
- `paired_delta_avg_bps > 0`
- `paired_delta_median_bps >= 0`
- aggregate `target_cut_damage_ratio <= 0.25`
- each chronological tercile `target_cut_damage_ratio <= 0.25`
- `target_cut_count <= saved_stop_count + 0.10 * timeout_improved_count`
- absolute TSV2 PnL cost100/cost200 reported
- grid-neighborhood positive
- `exact_join_rate >= 98%`

Passing R50 does not authorize runtime changes. Passing R50 only unlocks:

`PR-TSV2-A3: two-scope validation summary`

## 11. R50 failure criteria

R50 fails for runtime consideration if any of the following occurs:

- missing `time_stop_v2_window` rows
- missing `shadow_exit_replay_v1.jsonl`
- coverage below minimum sample acceptance
- `exact_join_rate < 98%`
- aggregate target-cut guard fails
- any chronological tercile target-cut guard fails
- action precision or Wilson lower bound misses acceptance
- paired delta sum/avg/median misses acceptance
- grid-neighborhood is not positive
- target-cut count guard fails
- result depends on approximate joins or stale/no-action leakage

If R50 fails, active TimeStop V2 exit direction remains rejected for runtime and TSV2 stays diagnostic logging only.

## 12. No-runtime-decision statement

This plan does not recommend runtime changes, active close, `shadow_close_only`, Gatekeeper policy changes, selector changes, alpha changes, V3/v25 confidence changes, or TX/Jito/live path changes.

The only allowed next research step after a passing R50 is a separate A3 two-scope validation summary. If R50 fails, the active TSV2 exit direction should be closed as rejected for runtime.
