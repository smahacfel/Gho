# ADR-8D: R50 TimeStop V2 logging-only validation run start

Status: PLAN_READY / LOGGING_ONLY / NO_RUNTIME_CHANGE
Typ: ADR-8D / rollout config preparation and validation plan
Data: 2026-06-27
Autor/Agent: Codex
Repo/branch: `/root/Gho-tsv2-a1-a2-clean`, `research/tsv2-a1-a2-clean`
HEAD podczas pracy: `081c0260ba0301b0ef19e48ba95a815dd1e590e0`
Commit/PR: not committed at ADR creation time
Zakres: R50 independent TSV2 logging-only validation scope
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `PLANS/PLAN_TSV2_R50_LOGGING_ONLY_VALIDATION_20260627.md`
- `configs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r50_tsv2_logging_only_target60_stop60_exit_replay_maxwait66000_fsc_off.toml`
- `docs/ADR/ADR_8D_R50_TSV2_LOGGING_ONLY_RUN_START_20260627.md`

Powiazany scope:
- `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty w repo.

## 1. Kontekst

Aktualny stan research:

- ORG-A0: `REJECTED_FOR_RUNTIME / INCONCLUSIVE_RESEARCH`
- A1: `INCONCLUSIVE_RESEARCH / REJECTED_FOR_RUNTIME`
- A2: `TARGET_CUT_RISK_UNRESOLVED / INCONCLUSIVE_RESEARCH`
- R48/R2 matrix: global exit grid is negative after realistic costs and has negative median.

Wniosek przed R50:
Nie ma podstaw do runtime change, `shadow_close_only`, Gatekeeper policy change, selector change, `alpha_31100`, XGBoost ani dalszego strojenia R48/R49.

## 2. Decyzja

Przygotowac niezalezny R50 logging-only validation scope dla TimeStop V2.

R50 ma zebrac drugi pelny scope zawierajacy jednoczesnie:

- `time_stop_v2_window`
- `shadow_lifecycle.jsonl`
- `probe_shadow_lifecycle.jsonl`
- `shadow_exit_replay_v1.jsonl`

Po zebraniu danych nalezy uruchomic istniejacy A2 bez zmian masek, progow i logiki. R50 nie jest A3. R50 jest tylko warunkiem wstepnym do ewentualnego `PR-TSV2-A3: two-scope validation summary`.

## 3. Safety boundary

R50 nie moze zmieniac:

- Gatekeeper BUY/REJECT
- Gatekeeper policy
- selector runtime
- `alpha_31100`
- `v25_confidence`
- V3 promotion
- TX builder / sender / Jito / live execution
- active close
- `shadow_close_only`
- A2 masks
- A2 thresholds

Config musi pozostac shadow/logging-only:

```toml
[trigger]
entry_mode = "shadow_only"

[execution]
execution_mode = "shadow"

[post_buy_guardian.time_stop_v2]
enabled = true
mode = "observe_only"
emit_window_records = true

[post_buy_guardian.exit_replay_v1]
enabled = true
```

## 4. Config knobs

Wrapper config:

`configs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1.toml`

Key settings:

- `ghost_brain_config_path = "../../configs/rollout/ghost_brain_selector_dataset_sampler_r50_tsv2_logging_only_target60_stop60_exit_replay_maxwait66000_fsc_off.toml"`
- `[trigger].entry_mode = "shadow_only"`
- `[trigger.shadow_run].enabled = true`
- `[execution].execution_mode = "shadow"`
- `[execution.shadow].lifecycle_log_path = "../../logs/shadow_run/<scope>/shadow_lifecycle.jsonl"`
- `[p37_shadow_probe].enabled = true`
- `[p37_shadow_probe].lifecycle_log_path = "../../logs/shadow_run/<scope>/probe_shadow_lifecycle.jsonl"`
- `[oracle].decision_log_path = "../../logs/rollout/<scope>/decisions"`

Brain config:

`configs/rollout/ghost_brain_selector_dataset_sampler_r50_tsv2_logging_only_target60_stop60_exit_replay_maxwait66000_fsc_off.toml`

Key settings:

- `[gatekeeper_v2].mode = "long"`
- `[gatekeeper_v2].max_wait_time_ms = 66000`
- `[post_buy_guardian.time_stop_v2].enabled = true`
- `[post_buy_guardian.time_stop_v2].mode = "observe_only"`
- `[post_buy_guardian.time_stop_v2].emit_window_records = true`
- `[post_buy_guardian.exit_replay_v1].enabled = true`
- `[post_buy_guardian.exit_replay_v1].levels_bps` includes `-6000` and `6000`

## 5. Expected artifacts

Raw artifacts:

- `logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/probe_shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_exit_replay_v1.jsonl`
- `record_type = "time_stop_v2_window"` rows in lifecycle logs
- `logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/decisions/`

Post-run reports:

- A1 report outputs under `reports/selector/<scope>/`
- A2 report outputs under `reports/selector/<scope>/`

## 6. Minimum data acceptance

R50 is usable only if:

- `positions_with_exit_replay >= 3000`
- `positions_with_tsv2_windows >= 3000`
- `exact_join_rate >= 98%`
- `candidate_positions >= 2500`
- `path_approximate_rows = 0` preferred

## 7. Post-run analysis command

Run unchanged A2:

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

## 8. Success criteria

R50 passes only if unchanged A2 shows:

- `action_precision >= 0.70`
- Wilson lower 95% `>= 0.65`
- `paired_delta_sum_bps > 0`
- `paired_delta_avg_bps > 0`
- `paired_delta_median_bps >= 0`
- aggregate `target_cut_damage_ratio <= 0.25`
- each tercile `target_cut_damage_ratio <= 0.25`
- `target_cut_count <= saved_stop_count + 0.10 * timeout_improved_count`
- absolute TSV2 PnL cost100/cost200 reported
- grid-neighborhood positive
- `exact_join_rate >= 98%`

If R50 passes, the next step is only `PR-TSV2-A3: two-scope validation summary`.

## 9. Failure criteria

R50 fails for runtime consideration if any acceptance gate fails, if coverage is insufficient, if target-cut damage breaches aggregate or segment guard, if grid-neighborhood is fragile, or if results depend on approximate joins/stale/no-action leakage.

If R50 fails, active TimeStop V2 exit direction remains rejected for runtime and TSV2 stays diagnostic logging only.

## 10. Decision impact

This ADR does not approve runtime changes. It only prepares an independent logging-only validation scope.

No `shadow_close_only` plan is justified until at least two independent full TSV2-window scopes pass the same unchanged A2 gates.
