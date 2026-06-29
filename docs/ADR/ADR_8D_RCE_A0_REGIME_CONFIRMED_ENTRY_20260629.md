# ADR-8D: PR-RCE-A0 Regime-Confirmed Entry

Status: SPEC_AND_LOGGING_SURFACE_PREP / OFFLINE_ONLY / NO_RUNTIME
Typ: ADR-8D / research logging surface
Data: 2026-06-29
Zakres: PR-RCE-A0
Poziom ryzyka: LOW runtime decision risk / MEDIUM logging-schema risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Przygotowano `PR-RCE-A0: Regime-Confirmed Entry` jako ostatnia, ograniczona hipoteze edge-search.

RCE-A0 nie jest:

- runtime change,
- `shadow_close_only`,
- active close,
- kolejnym ORG/TSV2/RTP/RUG pass,
- kolejnym mask/grid/threshold tuning pass.

Hipoteza RCE:

> Wejscie jest dozwolone dopiero po decision-time potwierdzeniu kontrolowanej kontynuacji w pre-entry tape.

To wymaga swiezego logging-only scope, chyba ze istniejace logi zawieraja juz nowa surface.

## 2. Additive evidence surface

Dodano addytywne pola do `MaterializedFeatureSet`, czyli do kanonicznego decision-time snapshotu:

- `pre_entry_path_summary_v1`
- `session_regime_snapshot_v1`

DecisionLogger nie staje sie nowym wlascicielem feature. Logger tylko serializuje `materialized_feature_snapshot`, tak jak dotychczas.

Nie dodano sidecara.

Nie zmieniono Gatekeeper policy.

Nie zmieniono BUY/REJECT.

## 3. `pre_entry_path_summary_v1`

Pola:

- `pre_entry_ret_5s`
- `pre_entry_ret_10s`
- `pre_entry_ret_20s`
- `pre_entry_ret_30s`
- `pre_entry_ret_45s`
- `pre_entry_mfe_10s`
- `pre_entry_mfe_20s`
- `pre_entry_mfe_30s`
- `pre_entry_mfe_45s`
- `pre_entry_mae_10s`
- `pre_entry_mae_20s`
- `pre_entry_mae_30s`
- `pre_entry_mae_45s`
- `pullback_depth_bps`
- `reclaim_bps`
- `reclaim_fraction`
- `higher_low_count`
- `above_0bps_dwell_ms`
- `above_300bps_dwell_ms`
- `above_600bps_dwell_ms`

Zrodlo: `PoolObservationSession::materialize_features()` na podstawie decision-time series z bufora sesji.

## 4. `session_regime_snapshot_v1`

Pola:

- `same_ms_tx_ratio_recent`
- `same_ms_tx_ratio_decay`
- `burst_ratio_recent`
- `burst_ratio_decay`
- `unique_ratio_recent`
- `unique_ratio_drift`
- `top3_signer_volume_ratio_recent`
- `top3_signer_volume_ratio_drift`
- `buy_sell_ratio_recent`
- `session_pool_rate_5m`
- `session_pool_rate_10m`
- `session_followthrough_rate_10m_optional`
- `template_reason_code`
- `veto_reason_code`

`session_pool_rate_5m`, `session_pool_rate_10m` i `session_followthrough_rate_10m_optional` pozostaja nullable, dopoki runtime nie ma jednoznacznego SSOT wlasciciela tych metryk. Nie wolno ich proxy-rekonstruowac z outcome.

## 5. Offline proof

Dodano `scripts/rce_a0_offline_proof.py`.

Skrypt moze testowac tylko:

- `T1_BREAKOUT_RETEST_RECLAIM`
- `T2_STAIRSTEP_CONTINUATION`
- `T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY`

Fixed grid:

- target: `600`, `900`, `1200` bps,
- stop: `-250`, `-400`, `-600` bps,
- max hold: `10000`, `20000`, `30000` ms,
- cost: `100`, `200` bps.

## 6. R51 logging-only config

Przygotowano config dla:

`shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1`

Run nie zostal wystartowany.

R51 musi pozostac:

- `entry_mode = "shadow_only"`,
- `execution_mode = "shadow"`,
- no active close,
- no `shadow_close_only`,
- no BUY/REJECT change,
- no Gatekeeper policy change,
- no selector runtime change,
- no `alpha_31100`,
- no XGBoost,
- no TX/Jito/live path change.

## 7. Evidence retention

R51 wymaga:

- `pre_run_manifest.json`,
- `post_run_manifest.json`,
- `gatekeeper_v2_decisions.jsonl`,
- `materialized_feature_snapshot`,
- `shadow_exit_replay_v1.jsonl`,
- `shadow_lifecycle.jsonl`,
- `probe_shadow_lifecycle.jsonl`,
- launcher PASS report,
- no active log symlinks to archive volume,
- cleanup only via `scripts/guard_rollout_evidence_cleanup.py`.

## 8. Consequences

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

`run_started = false`

Existing R49/R50 logs do not prove RCE because they predate the new RCE feature surface. The next allowed action is one separately approved logging-only R51 scope. Without that approval, the trading edge search should be closed.

## 9. Files

- `PLANS/AUDYT/PLAN_RCE_A0_REGIME_CONFIRMED_ENTRY_20260629.md`
- `scripts/rce_a0_offline_proof.py`
- `configs/rollout/shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r51_rce_logging_only_target12_stop6_exit_replay_maxwait45000_fsc_off.toml`
- `PLANS/AUDYT/RAPORT_RCE_A0_OFFLINE_PROOF_20260629.md`
- `docs/ADR/ADR_8D_RCE_A0_RESULT_20260629.md`
