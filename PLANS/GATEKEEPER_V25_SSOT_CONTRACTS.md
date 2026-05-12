# GATEKEEPER V2.5 — SSOT KONTRAKTÓW I NIEZMIENNIKÓW

> **Data:** 2026-05-05
> **Faza:** 0 + repair stream WS0-WS4
> **Status:** Aktywny SSOT po naprawach boundary/logging/invariants. Dokument łączy baseline kontraktów z repair deltas; tam gdzie stare sekcje były "planowane", poniższe opisy odzwierciedlają już stan zaimplementowany.
> **Branch implementacyjny:** `refactor/gatekeeper-v25`

---

## SPIS TREŚCI

1. [Niezmienniki architektoniczne (N1-N16)](#1-niezmienniki-architektoniczne)
2. [GatekeeperDecision — pełna specyfikacja](#2-gatekeeperdecision--pełna-specyfikacja)
3. [GatekeeperVerdictType — wszystkie warianty](#3-gatekeeperverdicttype--wszystkie-warianty)
4. [GatekeeperAssessment — pełna specyfikacja](#4-gatekeeperassessment--pełna-specyfikacja)
5. [GatekeeperBuffer — pełna specyfikacja](#5-gatekeeperbuffer--pełna-specyfikacja)
6. [GatekeeperV2Config — pełna specyfikacja](#6-gatekeeperv2config--pełna-specyfikacja)
7. [MaterializedFeatureSet — SSOT feature'ów](#7-materializedfeatureset--ssot-featureów)
8. [Ścieżki produkcji GatekeeperDecision](#8-ścieżki-produkcji-gatekeeperdecision)
9. [Konsumenci GatekeeperDecision](#9-konsumenci-gatekeeperdecision)
10. [evaluate_policy_from_assessment() — pełna mapa wywołań](#10-evaluate_policy_from_assessment--pełna-mapa-wywołań)
11. [Testy — pełny inwentarz](#11-testy--pełny-inwentarz)
12. [JSONL / Decision Logger — kontrakt](#12-jsonl--decision-logger--kontrakt)
13. [WAL — Write-Ahead Log](#13-wal--write-ahead-log)
14. [IWIM Veto Gate — kontrakt](#14-iwim-veto-gate--kontrakt)
15. [Miejsca bez GatekeeperDecision (ważne odkrycia)](#15-miejsca-bez-gatekeeperdecision)
16. [Mapa plików do modyfikacji](#16-mapa-plików-do-modyfikacji)
17. [Reguły implementacyjne V2.5](#17-reguły-implementacyjne-v25)

---

## 1. NIEZMIENNIKI ARCHITEKTONICZNE

### N1: MaterializedFeatureSet jest SSOT
**Plik:** `ghost-core/src/checkpoint/types.rs:99-112`
**Reguła:** Wszystkie nowe pola dodawane jako optional z `#[serde(default)]`. Struktura ma 8 pól (account_features, tx_intel_features, checkpoint_features, risk_flags, session_metadata, curve_readiness, sybil_resistance, alpha_fingerprint). Żadne istniejące pole nie może zostać usunięte ani zmienić typu.

### N2: GatekeeperDecision rozszerzane, nie modyfikowane
**Plik:** `ghost-launcher/src/components/gatekeeper.rs:1143-1180`
**Reguła:** 17 istniejących pól (16 bazowych + `gatekeeper_strength` opcjonalne) pozostaje bez zmian. Nowe pola dodawane jako `Option<T>` na końcu struktury. Nowe warianty `GatekeeperVerdictType` DODAWANE (nie zastępują istniejących).

### N3: JSONL schema wersjonowana
**Plik:** `ghost-brain/src/oracle/decision_logger.rs:75-77`
**Reguła:** Historyczny bump `schema=16` / `gatekeeper_version="v2.5"` zamraża wyłącznie pierwszy rollout V2.5. Naprawa semantyki decision-plane wymaga osobnego bumpu schema (docelowo `v17`) i nie może nadpisywać znaczenia istniejącego kontraktu `v16`.

### N4: Konfiguracja TOML — wsteczna kompatybilność
**Plik:** `ghost-brain/src/config/ghost_brain_config.rs:833-1487`
**Reguła:** Wszystkie nowe pola konfiguracyjne z `#[serde(default)]`. Nowe sekcje `[gatekeeper_v2.v25]`, `[gatekeeper_v2.dow]`, `[gatekeeper_v2.tas]`, `[gatekeeper_v2.pdd]`, `[gatekeeper_v2.aps]` w TOML.

### N5: Feature-flag V2.5
**Reguła:** `shadow_enabled = true`, `live_execution_enabled = false` domyślnie. Rollback przez `shadow_enabled = false`.

### N6: Kontrakt 8-9s obserwacji
**Reguła:** Live execution NIE skraca okna obserwacji przed ADR. Wszystkie decyzje 2-7s są shadow-only.

### N7: Yellowstone gRPC jedynym źródłem on-chain state
**Reguła:** V2.5 nie dodaje RPC do ścieżki decyzyjnej Gatekeepera.

### N8: Testy regresji — wszystkie istniejące muszą przechodzić
**Reguła:** 313 testów w 25 plikach (169 w ghost-launcher/tests/ + 142 w mod tests + 2 gatekeeper-related w ghost-core). Z tego 228 testów jest bezpośrednio gatekeeperowych. Każdy nowy kod w osobnych plikach/modułach.

### N9: compute_decision() i evaluate_policy_from_assessment() — dwie niezależne ścieżki
**Reguła:** `compute_decision()` (runtime, z GatekeeperBuffer::run_assessment()) i `evaluate_policy_from_assessment()` (policy, z MaterializedFeatureSet) to dwie niezależne ścieżki produkujące ten sam typ `GatekeeperDecision`. V2.5 modyfikuje obie.

### N10: Kolejność warstw w pipeline decyzyjnym
**Reguła:** Obecna kolejność to: HardFails → CoreFail → SybilComboVeto → SybilSoftExcess → LegacySoftExcess → AlphaGate → ProsperityFilter → BUY. PDD w V2.5 wchodzi jako pierwsza nowa warstwa PO HardFails, PRZED CoreFail.

### N11: Dev-unknown zaostrzenia
**Reguła:** Gdy `dev_unknown = true`, system stosuje zaostrzone progi (market cap, sol_buy_ratio, soft_points, single_tx_price_impact, sybil_soft_points). Ten mechanizm pozostaje nienaruszony.

### N12: IWIM Veto Gate
**Reguła:** IWIM działa po Gatekeeperze, mutując `verdict_buy`, `verdict_type`, i `reason_chain` w `GatekeeperDecision`. Jego pozycja w pipeline pozostaje bez zmian.

### N13: Legacy live plane i V2.5 shadow plane są rozdzielone
**Reguła:** Dopóki `gatekeeper_v2.v25.live_execution_enabled = false`, legacy Gatekeeper pozostaje jedynym źródłem live semantics, a V2.5 produkuje wyłącznie shadow semantics. V2.5 shadow verdict nie może po cichu nadpisywać legacy `verdict_buy`, `verdict_type` ani `reason_chain` używanych przez live/runtime consumers.

### N14: Parity Path A / Path B jest availability-aware, nie syntetyczna
**Reguła:** `compute_decision()` i `evaluate_policy_from_assessment()` mają raportować ten sam kontrakt typu, ale pola V2.5 wolno wypełniać tylko wtedy, gdy upstream naprawdę dostarcza wymagane dane. Zabronione jest syntetyczne rekonstruowanie `v25_confidence`, `entry_drift_pct`, verdictów PDD/TAS/DOW albo innych shadow pól tylko po to, by wymusić pozorną parity.

### N15: Logger musi jawnie nieść decision plane
**Reguła:** Po naprawie logi i routing muszą rozróżniać co najmniej `legacy_live` vs `v25_shadow` (oraz przyszły `v25_live`, jeśli kiedyś zostanie promowany). Mieszanie obu semantyk w jednym zestawie pól lub jednym znaczeniu `decision_verdict_*` jest złamaniem SSOT.

### N16: Ten repair stream nie wprowadza `GatekeeperMode::V25`
**Reguła:** Aktualne tryby operacyjne pozostają `Standard` i `Long`. Stan rolloutu V2.5 jest kodowany przez sekcje `gatekeeper_v2.v25.*`, jawne decision planes i kontrakt loggera, a nie przez dodanie nowego `GatekeeperMode`.

---

## 2. GATEKEEPERDECISION — PEŁNA SPECYFIKACJA

**Plik:** `ghost-launcher/src/components/gatekeeper.rs:1143-1180`
**Derive:** `Debug, Clone`
**Pola: 17 łącznie** (16 pól bazowych + 1 opcjonalne pole klasyfikacji siły BUY)

| # | Pole | Typ | Opis |
|---|------|-----|------|
| 1 | `hard_fail_reason` | `Option<String>` | Powód hard faila (natychmiastowy REJECT) |
| 2 | `core1_passed` | `bool` | Core-1 (Quantity Gate / Faza 1) |
| 3 | `core2_passed` | `bool` | Core-2 (Capital Dominance / Faza 4) |
| 4 | `core3_passed` | `bool` | Core-3 (Dev + Curve Safety / Fazy 5+6) |
| 5 | `soft_signals` | `SoftSignals` | Legacy soft signal flags (13 pól bool) |
| 6 | `soft_points` | `u8` | Ważone legacy soft points (grupowe) |
| 7 | `max_soft_points_possible` | `u8` | Maksymalne możliwe legacy soft points |
| 8 | `effective_max_soft_points` | `u8` | Efektywny próg (może być inny dla dev_unknown) |
| 9 | `dev_unknown` | `bool` | Czy dev wallet jest nieznany |
| 10 | `sybil_policy` | `SybilPolicyDiagnostics` | Sybil Interference diagnostics |
| 11 | `alpha_gate` | `AlphaGateDiagnostics` | Alpha gate diagnostics (tylko po przejściu wcześniejszych warstw) |
| 12 | `prosperity_filter` | `ProsperityFilterDiagnostics` | Prosperity selector diagnostics (tylko po alpha gate) |
| 13 | `total_soft_points` | `u16` | Legacy + sybil points (tylko telemetria) |
| 14 | `verdict_type` | `GatekeeperVerdictType` | Jawny typ werdyktu |
| 15 | `verdict_buy` | `bool` | Finalny werdykt: true = BUY, false = REJECT |
| 16 | `reason_chain` | `String` | Łańcuch przyczyn (HARD_FAIL > CORE_FAIL > SOFT_EXCESS > BUY) |
| 17 | `gatekeeper_strength` | `Option<GatekeeperStrength>` | Klasyfikacja siły BUY — **opcjonalne**, ustawiane tylko gdy `verdict_buy == true` |

**Uwaga:** Pole nr 17 (`gatekeeper_strength`) jest polem opcjonalnym wzbogacanym na ścieżce IWIM (`oracle_runtime.rs:4349`) i NIE jest bezpośrednio mapowane przez `to_buy_log()` — zamiast tego IWIM zapisuje je osobno jako `iwim_gatekeeper_strength` w JSONL.

---

## 3. GATEKEEPERVERDICTTYPE — WSZYSTKIE WARIANTY

**Plik:** `ghost-launcher/src/components/gatekeeper.rs:870-921`
**Derive:** `Debug, Clone, Copy, PartialEq, Eq`

| # | Wariant | tag() | Znaczenie |
|---|---------|-------|-----------|
| 1 | `Buy` | `"BUY"` | Wszystkie checki przeszły |
| 2 | `RejectHardFail` | `"REJECT_HARD_FAIL"` | Layer 1 kill-switch |
| 3 | `RejectCoreFail` | `"REJECT_CORE_FAIL"` | Layer 2 core check nie przeszedł |
| 4 | `RejectSoftExcess` | `"REJECT_SOFT_EXCESS"` | Layer 3 soft points przekroczone |
| 5 | `RejectSybilSoftExcess` | `"REJECT_SYBIL_SOFT_EXCESS"` | Sybil soft points przekroczone |
| 6 | `RejectSybilInterference` | `"REJECT_SYBIL_INTERFERENCE"` | Sybil combo-veto dopasowane |
| 7 | `RejectLowAlpha` | `"REJECT_LOW_ALPHA"` | Alpha gate nie przeszedł |
| 8 | `RejectLowProsperity` | `"REJECT_LOW_PROSPERITY"` | Prosperity filter odrzucił |
| 9 | `TimeoutPhase1` | `"TIMEOUT_PHASE1"` | Faza 1 nie osiągnięta w deadline |
| 10 | `TimeoutNoData` | `"TIMEOUT_NO_DATA"` | Brak danych w deadline |
| 11 | `RejectIwimVeto` | `"REJECT_IWIM_VETO"` | IWIM veto: dev history wykryło rug/sybil/scam |
| 12 | `RejectIwimLowConf` | `"REJECT_IWIM_LOW_CONF"` | IWIM low confidence + BORDERLINE gatekeeper |
| 13 | `RejectIwimUnknownStrict` | `"REJECT_IWIM_UNKNOWN_STRICT"` | IWIM timeout/error + BORDERLINE gatekeeper |

**Zaimplementowane warianty V2.5 / repair stream (dodane, nie zastępujące):**
- `RejectPumpAndDump` — PDD hard fail (po promocji progu)
- `RejectLowTrajectory` — TAS score zbyt niski (po promocji progu)
- `RejectEntryDrift` — Entry drift > max (po promocji progu)
- `RejectFlashCrash` — Flash crash protection (po promocji progu)
- `RejectRamping` — Ramping pattern detected (po promocji progu)
- `EarlyBuy` — Live early entry (tylko po ADR/promocji)

---

## 4. GATEKEEPERASSESSMENT — PEŁNA SPECYFIKACJA

**Plik:** `ghost-launcher/src/components/gatekeeper.rs:1182-1229`
**Derive:** `Debug, Clone`

| # | Pole | Typ |
|---|------|-----|
| 1 | `phase1_passed` | `bool` |
| 2 | `phase2_velocity` | `Option<VelocityProfile>` |
| 3 | `phase2_passed` | `bool` |
| 4 | `phase3_diversity` | `Option<SignerDiversityProfile>` |
| 5 | `phase3_passed` | `bool` |
| 6 | `phase4_volume` | `Option<VolumeSanityProfile>` |
| 7 | `phase4_passed` | `bool` |
| 8 | `phase5_dev` | `Option<DevBehaviorProfile>` |
| 9 | `phase5_passed` | `bool` |
| 10 | `phase6_curve` | `Option<BondingCurveDynamics>` |
| 11 | `phase6_passed` | `bool` |
| 12 | `phases_passed` | `u8` |
| 13 | `hard_reject_reason` | `Option<String>` |
| 14 | `total_tx_evaluated` | `usize` |
| 15 | `unique_tx_evaluated` | `usize` |
| 16 | `unique_signers_evaluated` | `usize` |
| 17 | `observation_duration_ms` | `u64` |
| 18 | `finalize_lag_ms` | `u64` |
| 19 | `dust_filtered_count` | `u64` |
| 20 | `eval_count` | `usize` |
| 21 | `buy_count` | `usize` |
| 22 | `decision` | `Option<GatekeeperDecision>` |
| 23 | `early_fingerprint` | `Option<EarlyFingerprintMetrics>` |
| 24 | `curve_t0_event_ts_ms` | `Option<u64>` |
| 25 | `curve_t0_clock_source` | `Option<&'static str>` |
| 26 | `curve_wait_elapsed_ms` | `Option<u64>` |
| 27 | `feature_snapshot` | `MaterializedFeatureSet` |
| 28 | `checkpoint_count` | `u32` |
| 29 | `trajectory_available` | `bool` |

**Zaimplementowane rozszerzenia V2.5:**
- `trajectory: Option<TrajectoryAssessment>` — pełna ocena trajektorii
- `pdd_assessment: Option<PddDiagnostics>` — wynik detekcji P&D
- `aps_diagnostics: Option<ApsDiagnostics>` — diagnostyka adaptacyjnych progów
- `observation_stage: Option<ObservationStage>` — Early/Normal/Extended
- `entry_drift_pct: Option<f64>` — procent driftu ceny
- `entry_drift_anchor_quality: Option<EntryDriftAnchorQuality>` — jakość kotwicy ceny
- `v25_confidence: Option<f64>` — confidence score V2.5
- `v25_shadow_decisions: Vec<ShadowV25Decision>` — jawne decyzje shadow plane
- `adaptive_thresholds_applied: bool` — czy zastosowano adaptacyjne progi

---

## 5. GATEKEEPERBUFFER — PEŁNA SPECYFIKACJA

**Plik:** `ghost-launcher/src/components/gatekeeper.rs:1963-2065`

Główne grupy pól:
- **Tożsamość puli:** `pool_id`, `pool_creator`, `pool_create_signature`, `pool_initial_liquidity_sol`
- **Stan:** `state` (Tracked/Approved/Committed)
- **Buffer TX:** `buffered_txs`, `tx_keys_seen`, `tx_signatures_seen`, `tx_keys_fifo`
- **Timing:** `highest_seen_ts`, `first_tx_ts`, `created_at_ms`, `registered_wall_ts_ms`, `deadline_wall_ts_ms`
- **Faza 1 (Quantity):** `unique_signers`, `total_tx_count`, `buy_count`, `sell_count`
- **Faza 2 (Velocity):** `tx_timestamps_sorted`
- **Faza 3 (Diversity):** `signer_stats`
- **Faza 4 (Volume):** `tx_volumes`, `total_volume_sol`, `buy_volume_sol`, `sell_volume_sol`
- **Faza 5 (Dev):** `dev_wallet`, `dev_buy_total_sol`, `dev_sell_total_sol`, `dev_tx_count`, `dev_has_sold`, itd.
- **Faza 6 (Curve):** `price_history` (Vec<PricePoint>)
- **Ewaluacja:** `phase1_passed`, `last_eval_at_count`, `eval_count`
- **Curve Latch:** `curve_t0_event_ts_ms`, `curve_ready`, `curve_quality`, `curve_finality_state`
- **Telemetria:** `dust_filtered_count`, `failed_tx_count`, `max_consecutive_buys`

**Zaimplementowane rozszerzenia V2.5 (w GatekeeperBuffer):**
- `early_deadline_ms: u64` — deadline dla okna Early (2-5s)
- `normal_deadline_ms: u64` — deadline dla okna Normal (5-7s)
- `extended_deadline_ms: u64` — deadline dla okna Extended (7-10s)
- `window_stage: ObservationStage` — aktualne aktywne okno
- `early_shadow_fired: bool` — early checkpoint emitowany dokładnie raz
- `normal_shadow_fired: bool` — normal checkpoint emitowany dokładnie raz
- `v25_shadow_decisions: Vec<ShadowV25Decision>` — zebrane decyzje V2.5 shadow plane
- `window_stage: ObservationStage` — aktualny etap obserwacji

---

## 6. GATEKEEPERV2CONFIG — PEŁNA SPECYFIKACJA

**Plik:** `ghost-brain/src/config/ghost_brain_config.rs:833-1487`
**Derive:** `Debug, Clone, Serialize, Deserialize`

Główne sekcje (każda z wieloma polami — szczegóły w pliku źródłowym):

| Sekcja | Opis | Liczba pól |
|--------|------|------------|
| Mode | `mode: GatekeeperMode` (Standard/Long) | 1 |
| Pre-filter | `min_sol_threshold` | 1 |
| Phase 1: Quantity Gate | `min_tx_count`, `min_unique_signers`, `min_buy_count`, `max_wait_time_ms` | 4 |
| Phase 2: Velocity | `min/max_interval_cv`, `max_burst_ratio`, `min/max_avg_interval_ms`, `min/max_timing_entropy`, `min_dust_filtered_count` | 8 |
| Phase 3: Diversity | `min/max_unique_ratio`, `max_hhi`, `max_tx_per_signer`, `min/max_volume_gini`, `max_top3_volume_pct`, `max_same_ms_tx_ratio` | 8 |
| Phase 4: Volume | `min/max_buy_ratio`, `min/max_avg_tx_sol`, `min/max_volume_cv`, `min/max_total_volume_sol`, `min/max_sol_buy_ratio`, `min_consecutive_buys` | 11 |
| Phase 5: Dev | `min/max_dev_buy_sol`, `min/max_dev_tx_ratio`, `min/max_dev_volume_ratio`, `reject_on_dev_sell` | 7 |
| Phase 6: Curve | `min/max_price_change_ratio`, `max_single_tx_price_impact_pct`, `min/max_single_sell_impact_pct`, `min/max_bonding_progress_pct`, `min_market_cap_sol` | 9 |
| Decision | `min_phases_to_pass`, `re_eval_tx_interval` | 2 |
| Three-Layer Decision | `use_three_layer_decision`, `hard_fail_hhi`, `hard_fail_same_ms_tx_ratio`, `hard_fail_top3_volume_pct`, `max_soft_points`, wagi soft (4), `max_soft_score` (deprecated) | 12 |
| Alpha Gate | `enable_alpha_gate`, `min_momentum`, `min_demand`, `min_alpha_joint`, `min_alpha_sample` | 5 |
| Prosperity Filter | `enable_prosperity_filter`, `prosperity_min_market_cap_sol`, `prosperity_max_signer_cross_pool_velocity`, progi branchy B1/B2/B3, overlay | 15 |
| Dev Unknown | `dev_unknown_min_market_cap_sol`, `dev_unknown_min_sol_buy_ratio`, `dev_unknown_max_soft_points`, `dev_unknown_max_single_tx_price_impact_pct` | 4 |
| Hybrid Fingerprint | `max/min_sell_buy_ratio`, `max/min_compute_unit_cluster_dominance`, `max/min_static_fee_profile_ratio`, `max/min_fixed_size_buy_ratio`, `max_fixed_size_buy_ratio_1e4`, `max_flipper_presence_ratio`, `max/min_jito_tip_intensity`, `max_early_slot_volume_dominance_buy`, `max_early_top3_buy_volume_pct_3s`, `min/max_avg_inner_ix_count_50tx`, whale reversal ratios, `min_dev_paperhand_latency_ms` | 22 |
| Sybil Resistance | `min_fee_topology_diversity_index`, `max_dev_buyer_infrastructure_affinity`, `min_spend_fraction_divergence`, `min_demand_elasticity_score`, `max_signer_cross_pool_velocity`, `max_funding_source_concentration` | 6 |
| Sybil Soft Penalties | 10 pól `soft_penalty_*` | 10 |
| Sybil Interference | `enable_sybil_interference_layer`, `max_sybil_soft_points`, `dev_unknown_max_sybil_soft_points`, `enable_sybil_combo_veto`, `emit_sybil_meta_score`, `require_ready_fsc_for_combo_veto` | 6 |
| Sybil Rolling State | `cpv_lookback_window_s`, `funding_lookback_window_s`, `funding_dust_threshold_lamports`, `cpv_per_signer_cap`, `cpv_global_signer_cap`, `fsc_per_recipient_cap`, `fsc_global_recipient_cap`, `neutral_funding_sources` | 8 |
| Hard Fail Bot Detection | `hard_fail_bot_min_tx`, `hard_fail_bot_min_observation_ms` | 2 |
| IWIM Classification | `iwim_veto_strong_margin`, `iwim_veto_strong_max_manip_flags` | 2 |
| Yellowstone-only | `min_failed_tx_ratio_for_bot_flag`, `use_slot_ordering` | 2 |
| Curve Latch | `curve_wait_ms`, `curve_require_for_buy`, `stale_fallback` | 3 |

**Łącznie: ~150 pól konfiguracyjnych.**

---

## 7. MATERIALIZEDFEATURESET — SSOT FEATURE'ÓW

**Plik:** `ghost-core/src/checkpoint/types.rs:99-112`
**Derive:** `Debug, Clone, Default, PartialEq, Serialize, Deserialize`

| # | Pole | Typ | Serde |
|---|------|-----|-------|
| 1 | `account_features` | `AccountStateFeatures` | required |
| 2 | `tx_intel_features` | `TxIntelFeatures` | required |
| 3 | `checkpoint_features` | `CheckpointDerivedFeatures` | required |
| 4 | `risk_flags` | `Vec<RiskFlag>` | required |
| 5 | `session_metadata` | `SessionMetadata` | required |
| 6 | `curve_readiness` | `CurveReadinessFeatures` | `#[serde(default)]` |
| 7 | `sybil_resistance` | `SybilResistanceFeatures` | `#[serde(default)]` |
| 8 | `alpha_fingerprint` | `AlphaFingerprintFeatures` | `#[serde(default)]` |

**Sub-struktury (wszystkie w `ghost-core`):**

- `AccountStateFeatures` — `ghost-core/src/account_state_core/types.rs:163-175`
- `TxIntelFeatures` — `ghost-core/src/tx_intelligence/types.rs:48-96`
- `CheckpointDerivedFeatures` — `ghost-core/src/checkpoint/types.rs:39-55`
- `RiskFlag` — `ghost-core/src/tx_intelligence/types.rs:20-26`
- `SessionMetadata` — `ghost-core/src/session/types.rs:38-45`
- `CurveReadinessFeatures` — `ghost-core/src/checkpoint/types.rs:57-83`
- `SybilResistanceFeatures` — `ghost-core/src/tx_intelligence/types.rs:153-175`
- `AlphaFingerprintFeatures` — `ghost-core/src/checkpoint/types.rs:86-97`

**Konstruktor:** `ObservationFeatureBuilder::materialize()` w `ghost-core/src/checkpoint/feature_builder.rs:81-105`. Post-populacja w `PoolObservationSession::materialize_features()` w `ghost-launcher/src/session/observation.rs:368-509`.

**Pola używane przez `build_assessment_from_features()`:** ~30 pól z wszystkich sub-struktur (oprócz `risk_flags` i `sybil_resistance` które są używane później w pipeline).

---

## 8. ŚCIEŻKI PRODUKCJI GATEKEEPERDECISION

Istnieją DWIE niezależne ścieżki produkcji `GatekeeperDecision`:

### Ścieżka A: Runtime (compute_decision)

```
GatekeeperBuffer::evaluate_phases() [gatekeeper.rs:4025]
  └─ GatekeeperBuffer::run_assessment()      → buduje GatekeeperAssessment z bufferu TX
  └─ GatekeeperBuffer::compute_decision()    [gatekeeper.rs:3620] → GatekeeperDecision
     (three-layer: hard_fails → core_pass → soft_signals → sybil)
     Uwaga: alpha_gate i prosperity_filter zawsze jako not_run() w tej ścieżce!

GatekeeperBuffer::check_long_deadline() [gatekeeper.rs:4831]
  └─ ten sam pattern: run_assessment() → compute_decision()

Warunek: self.config.use_three_layer_decision == true
```

### Ścieżka B: Policy (evaluate_policy_from_assessment)

```
evaluate_policy_from_assessment() [gatekeeper_policy.rs:707]
  └─ build_policy_diagnostics()              → core1/2/3, soft_signals, sybil
  └─ evaluate_hard_filters_from_assessment() → HardFailReason?
  └─ sybil_combo_veto_reason()              → SybilInterferencePattern?
  └─ evaluate_alpha_gate()                   → AlphaGateDiagnostics (real!)
  └─ evaluate_prosperity_filter()            → ProsperityFilterDiagnostics (real!)
  └─ compute gatekeeper_strength             → Strong/Borderline

Wywoływana przez:
  - evaluate_policy() [policy.rs:695] — wrapper (testy)
  - evaluate_from_features() [gatekeeper.rs:2750] — feature-driven path (produkcja)
  - build_timeout_decision_from_assessment() [policy.rs:913] — timeout (produkcja)
```

### Ścieżka C: Timeout (build_timeout_decision_from_assessment)

```
build_timeout_decision_from_assessment() [policy.rs:913]
  └─ Buduje GatekeeperDecision z verdict_type = TimeoutPhase1 / RejectCoreFail
  └─ Alpha gate i prosperity filter = not_run()
  └─ Używane w oracle_runtime.rs:4613 dla timeout assessment
```

---

## 9. KONSUMENCI GATEKEEPERDECISION

### 9.1 Producent → Konsument (wewnątrz gatekeeper.rs)

**compute_decision() → evaluate_phases() / check_long_deadline()**
- `gatekeeper.rs:4034-4039`: odczyt `soft_points`, `max_soft_points_possible`, `verdict_buy`, `verdict_type.tag()`, `reason_chain`
- `gatekeeper.rs:4879+`: ten sam pattern w `check_long_deadline()`

**evaluate_from_features() → caller**
- `gatekeeper.rs:2750-2756`: odczyt `reason_chain`, `verdict_buy`, `verdict_type.tag()`, `soft_points`, `max_soft_points_possible`

### 9.2 GatekeeperAssessment::to_buy_log()

**Plik:** `gatekeeper.rs:1327-1914`

Mapuje 17 pól `GatekeeperDecision` na `GatekeeperBuyLog` (JSONL):
- `hard_fail_reason` → `hard_fail_reason`
- `core1_passed`, `core2_passed`, `core3_passed` → odpowiednie pola
- `dev_unknown` → `dev_unknown`
- `soft_signals` → `soft_score`, `soft_flags`, `legacy_soft_flags`
- `soft_points` → `soft_points`, `legacy_soft_points`
- `effective_max_soft_points` → `effective_max_soft_points`, `legacy_soft_threshold`
- `sybil_policy.*` → `sybil_soft_*`, `sybil_interference_patterns`, `sybil_lead_signal`, `sybil_meta_score`
- `alpha_gate.*` → `alpha_*`
- `prosperity_filter.*` → `prosperity_*`
- `total_soft_points` → `total_soft_points`
- `verdict_type.tag()` → `verdict_type`
- `verdict_buy` → `decision_verdict_buy`
- `reason_chain` → `decision_reason`

**Nie mapowane przez to_buy_log():** `gatekeeper_strength` (wzbogacane przez IWIM), `max_soft_points_possible`

### 9.3 GatekeeperAssessment::decision_summary()

**Plik:** `gatekeeper.rs:1237-1292`

Formatuje jednoliniowe podsumowanie dla `info!()`. Odczytuje większość pól oprócz `prosperity_filter`. Używane w `oracle_runtime.rs` (4 miejsca: REJECT, TIMEOUT, IWIM REJECT, BUY).

### 9.4 oracle_runtime.rs — IWIM Veto Gate

**Plik:** `ghost-launcher/src/oracle_runtime.rs`

- **Odczyt `gatekeeper_strength` (linia 8333):** do przekazania do `run_iwim_veto_gate()`
- **Mutacja na IWIM reject (linie 8373-8380):** zmienia `verdict_buy`, `verdict_type`, `reason_chain`
- **Odczyt `reason_chain` (linie 4406, 8218, 8237, 8544):** do WAL append i logów
- **Odczyt `hard_fail_reason` (linia 4617):** kopiowanie do assessmentu timeoutowego
- **Enrichment IWIM (linia 4349):** `iwim_gatekeeper_strength` w buy logu

### 9.5 emit_gatekeeper_decision_event()

**Plik:** `oracle_runtime.rs:4461-4489`

Odczytuje `soft_signals.format_flags()` do events JSONL. Werdykt przekazywany jako string przez caller.

### 9.6 WAL (Write-Ahead Log)

**Plik:** `ghost-core/src/wal.rs:257-262`

WAL ma własny typ `GatekeeperDecision` (enum: Buy/Reject/Wait/Timeout) — zupełnie niezależny od struktury z `gatekeeper.rs`. Most przez `reason_chain` przy append.

### 9.7 Python Scripts

- `scripts/shadow_onchain_lifecycle_report.py`: odczytuje `decision_verdict_buy`, `verdict_type`, `decision_reason` z JSONL

### 9.8 Testy

- `gatekeeper_policy_tests.rs`: ~42 testy odczytujące `verdict_buy`, `verdict_type`, `reason_chain`, `hard_fail_reason`, `sybil_policy.*`
- `oracle_runtime.rs` (testy): asercje na `verdict_type`, `verdict_buy`
- `full_pipeline_integration.rs`: asercje na `verdict_type`

---

## 10. EVALUATE_POLICY_FROM_ASSESSMENT() — PEŁNA MAPA WYWOŁAŃ

**Definicja:** `ghost-launcher/src/components/gatekeeper_policy.rs:707-911`

### Produkcja — direct call-site (1 miejsce):

| # | Plik | Linia | Kontekst | Ścieżka |
|---|------|-------|----------|---------|
| 1 | `gatekeeper.rs` | 2750 | `evaluate_from_features()` — feature-driven policy path. Buduje assessment z `MaterializedFeatureSet`, wywołuje `evaluate_policy_from_assessment()`, zapisuje decyzję do `assessment.decision`. | Policy |

Jest to **jedyny produkcyjny direct call-site** funkcji `evaluate_policy_from_assessment()`.

### Produkcja — wywołanie pośrednie przez evaluate_policy() wrapper (0 miejsc):

```rust
// gatekeeper_policy.rs:695-705
pub fn evaluate_policy(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> GatekeeperDecision {
    let assessment = build_assessment_from_features(
        features.clone(), config, PolicyEvaluationContext::default());
    evaluate_policy_from_assessment(&assessment, config)
}
```

`evaluate_policy()` jest convenience wrapperem, który **nie jest wywoływany w żadnej ścieżce produkcyjnej**. Jest używany wyłącznie przez testowy helper `evaluate()` w `gatekeeper_policy_tests.rs:315-320`, który z kolei jest wołany 29 razy w testach. W mapie wywołań produkcyjnych ten wrapper jest **martwym kodem produkcyjnym** — nie jest to direct call-site.

### Produkcja — ścieżka siostrzana: build_timeout_decision_from_assessment() (1 miejsce):

Funkcja `build_timeout_decision_from_assessment()` (policy.rs:913) NIE wywołuje `evaluate_policy_from_assessment()`, ale jest jej ścieżką siostrzaną — buduje `GatekeeperDecision` dla timeoutu:

| # | Plik | Linia | Kontekst |
|---|------|-------|----------|
| 1 | `oracle_runtime.rs` | 4613 | `build_timeout_assessment_from_policy_context()` → `build_timeout_decision_from_assessment()` |

### Produkcja — ścieżka alternatywna: compute_decision() (2 miejsca):

Równoległa ścieżka runtime, która NIE przechodzi przez `evaluate_policy_from_assessment()`, tylko buduje `GatekeeperDecision` bezpośrednio z `GatekeeperBuffer::run_assessment()`:

| # | Plik | Linia | Kontekst |
|---|------|-------|----------|
| 1 | `gatekeeper.rs` | 4034 | `evaluate_phases()` — runtime trigger evaluation |
| 2 | `gatekeeper.rs` | 4879 | `check_long_deadline()` — deadline/finalize evaluation |

**Uwaga:** W ścieżce `compute_decision()`, pola `alpha_gate` i `prosperity_filter` są zawsze ustawiane na `not_run()`. Tylko ścieżka policy (`evaluate_policy_from_assessment()`) produkuje realne alpha gate i prosperity diagnostics.

### Testy — direct call-site'y evaluate_policy_from_assessment() (24 miejsca):

**W `gatekeeper_policy.rs` (mod tests) — 6 testów:**

| Linia | Test |
|-------|------|
| 2003 | `alpha_gate_rejects_low_momentum` |
| 2029 | `alpha_gate_rejects_low_demand` |
| 2072 | `alpha_gate_rejects_low_joint_when_scalars_individually_pass` |
| 2096 | `alpha_gate_skips_when_sample_is_too_small` |
| 2113 | `alpha_gate_skips_when_required_inputs_are_missing` |
| 2135 | `disabled_alpha_gate_preserves_buy_path` |

**W `gatekeeper_policy_tests.rs` — 17 testów:**

| Linia | Test |
|-------|------|
| 1063 | `hard_fail_decision_preserves_phase_diagnostics` |
| 1168 | `fingerprint_thresholds_can_downgrade_preliminary_buy` |
| 1223 | `early_top3_fingerprint_threshold_can_downgrade_preliminary_buy` |
| 1306 | `prosperity_filter_accepts_branch_b1_conviction_clean_sells` |
| 1360 | `prosperity_filter_accepts_branch_b2_large_cap_buy_dominance` |
| 1413 | `prosperity_filter_accepts_branch_b3_organic_structure` |
| 1465 | `prosperity_filter_rejects_when_no_balanced_branch_matches` |
| 1519 | `prosperity_filter_rejects_high_cpv_before_branch_match` |
| 1579 | `prosperity_overlay_accepts_large_cap_branch_when_overlay_passes` |
| 1640 | `prosperity_overlay_accepts_organic_branch_when_overlay_passes` |
| 1697 | `prosperity_overlay_rejects_large_cap_branch_on_branch2_price_extension` |
| 1753 | `prosperity_overlay_rejects_matched_branch_on_high_bonding_progress` |
| 1809 | `prosperity_overlay_rejects_matched_branch_on_low_fee_topology_diversity` |
| 1865 | `prosperity_overlay_rejects_organic_branch_on_high_sell_buy_ratio` |
| 1945 | `bidirectional_fingerprint_bounds_can_fail_core2` |
| 2024 | `verdict_engine_buys_when_core_pass_holds_and_soft_signals_stay_within_limit` |
| 2052 | `verdict_engine_rejects_soft_excess_while_core_pass_still_holds` |

**W `gatekeeper.rs` — 1 martwy test-only call-site:**

| Linia | Test |
|-------|------|
| 2839 | `evaluate_compat_from_features` — `#[cfg(test)]` + `#[allow(dead_code)]`, nieużywany |

### Testy — wywołania pośrednie przez evaluate_policy() wrapper:

Helper `evaluate()` w `gatekeeper_policy_tests.rs:315-320` wywołuje `evaluate_policy()`, który wywołuje `evaluate_policy_from_assessment()`. Ten helper jest wołany **29 razy** w testach (linie: 359, 367, 376, 383, 401, 414, 448, 462, 490, 499, 533, 557, 586, 611, 654, 655, 697, 726, 753, 782, 783, 812, 1040, 1257, 1882, 1963, 1977, 1978).

### Podsumowanie mapy wywołań:

| Kategoria | Liczba |
|-----------|--------|
| Produkcja — direct call-site `evaluate_policy_from_assessment()` | **1** |
| Produkcja — `evaluate_policy()` wrapper | **0** (tylko testy) |
| Produkcja — ścieżka siostrzana `build_timeout_decision_from_assessment()` | **1** |
| Produkcja — ścieżka alternatywna `compute_decision()` | **2** |
| Testy — direct call-site | **24** |
| Testy — pośrednie przez `evaluate_policy()` wrapper | **29** |

---

## 11. TESTY — PEŁNY INWENTARZ (ZWERYFIKOWANY 2026-05-02)

Wszystkie liczby uzyskane przez `grep -c '#\[test\]'` + `grep -c '#\[tokio::test'` na każdym pliku.

---

### 11.1 Wszystkie pliki testowe w `ghost-launcher/tests/` — 20 plików, 169 testów

| # | Plik | `#[test]` | `#[tokio::test]` | Razem | Powiązanie z Gatekeeperem |
|---|------|:---------:|:----------------:|:-----:|---------------------------|
| 1 | `gatekeeper_policy_tests.rs` | 44 | — | **44** | **direct** — testuje `evaluate_policy()` / `evaluate_policy_from_assessment()` |
| 2 | `seer_shadow_ledger_bridge_tests.rs` | 22 | — | **22** | infra — testuje Seer↔ShadowLedger bridge |
| 3 | `session_lifecycle_tests.rs` | 22 | — | **22** | **direct** — używa `GatekeeperV2Config`, `GatekeeperBuffer`, `GatekeeperAssessment` |
| 4 | `snapshot_engine_integration.rs` | — | 11 | **11** | **adjacent** — importuje `GatekeeperV2Config`, `GatekeeperVerdict` |
| 5 | `refactor_invariants_tests.rs` | 10 | — | **10** | **direct** — 4/10 testują kontrakty Gatekeepera |
| 6 | `seer_connection_mode_test.rs` | 7 | — | **7** | infra — testuje Seer connection mode |
| 7 | `oracle_event_bus_integration.rs` | — | 7 | **7** | infra — OracleRuntime event bus integration |
| 8 | `tx_intelligence_tests.rs` | 7 | — | **7** | **adjacent** — testuje `TxIntelFeatures` |
| 9 | `full_pipeline_integration.rs` | 6 | — | **6** | **direct** — testuje `GatekeeperBuffer`, `build_assessment_from_features` |
| 10 | `wal_startup_recovery.rs` | 5 | — | **5** | infra — WAL startup recovery |
| 11 | `gatekeeper_v2_pipeline_integration.rs` | — | 4 | **4** | **direct** — pełen event flow Gatekeepera |
| 12 | `gatekeeper_events_emission_test.rs` | 4 | — | **4** | **direct** — testuje `emit_gatekeeper_decision_event()` |
| 13 | `oracle_transaction_gathering.rs` | — | 4 | **4** | infra — Oracle transaction gathering |
| 14 | `post_buy_runtime_integration.rs` | — | 4 | **4** | infra — PostBuyRuntime (za Gatekeeperem) |
| 15 | `event_bus_subscription_order.rs` | — | 3 | **3** | **adjacent** — waliduje fix subskrypcji po emisji Seer |
| 16 | `oracle_continuous_sampling.rs` | — | 3 | **3** | infra — Oracle continuous sampling |
| 17 | `time_contract_bridge.rs` | 2 | — | **2** | infra — time contract bridge |
| 18 | `oracle_logging_demo.rs` | — | 2 | **2** | infra — Oracle logging demo |
| 19 | `genesis_repro_check.rs` | 1 | — | **1** | infra — genesis repro check |
| 20 | `log_separation_test.rs` | — | 1 | **1** | infra — log separation |
| | **SUMA** | **130** | **39** | **169** | |

**Klasyfikacja:**
- **direct** (6 plików): `gatekeeper_policy_tests.rs`(44), `session_lifecycle_tests.rs`(22), `full_pipeline_integration.rs`(6), `gatekeeper_v2_pipeline_integration.rs`(4), `gatekeeper_events_emission_test.rs`(4), `refactor_invariants_tests.rs`(4 z 10) = **84 testy bezpośrednio gatekeeperowe**
- **adjacent** (4 pliki): `snapshot_engine_integration.rs`(11), `tx_intelligence_tests.rs`(7), `refactor_invariants_tests.rs`(6 z 10), `event_bus_subscription_order.rs`(3) = **27 testów w bezpośrednim sąsiedztwie**
- **infra** (11 plików): seer_shadow_ledger_bridge(22), seer_connection_mode(7), oracle_event_bus(7), wal_startup_recovery(5), oracle_transaction_gathering(4), post_buy_runtime(4), oracle_continuous_sampling(3), time_contract_bridge(2), oracle_logging_demo(2), genesis_repro_check(1), log_separation(1) = **58 testów infrastrukturalnych**

---

### 11.2 Testy jednostkowe w plikach źródłowych (mod tests) — 3 pliki, 142 testy

| Plik | `#[test]` | `#[tokio::test]` | Razem | Co testuje |
|------|:---------:|:----------------:|:-----:|------------|
| `gatekeeper.rs` | 128 | — | **128** | GatekeeperBuffer: fazy 1-6, velocity, diversity, volume, dev, curve latch, buy log, time resolution, dedup, gini, downsample, window vectors, fingerprint metrics |
| `gatekeeper_policy.rs` | 9 | — | **9** | Sybil degradation + alpha gate rejects/skips |
| `gatekeeper_commit_loop.rs` | 2 | 3 | **5** | Commit loop start/stop, runtime gatekeeper commits, live pipeline bootstrap, pending live survival, failure recovery |

Wszystkie 142 testy są **bezpośrednio gatekeeperowe** — testują `GatekeeperBuffer`, `GatekeeperDecision`, `evaluate_policy_from_assessment()`, `compute_decision()` lub CommitLoop zależny od Gatekeepera.

---

### 11.3 Testy w ghost-core — 2 testy gatekeeper-related

| Plik | Testy ogółem | Gatekeeper-related | Co testuje |
|------|:------------:|:------------------:|------------|
| `health.rs` | 11 | 1 (`test_mark_gatekeeper_decision`) | Weryfikuje `last_gatekeeper_decision_ts_ms` |
| `wal.rs` | 7 | 1 (użycie `GatekeeperDecision::Buy` w `WalRecord::Decision`) | WAL append/replay z decyzją Gatekeepera |

Pozostałe testy w tych plikach nie dotykają struktur Gatekeepera.

---

### 11.4 Podsumowanie liczbowe

| Kategoria | Plików | Testów ogółem | Gatekeeper-direct |
|-----------|:------:|:-------------:|:-----------------:|
| `ghost-launcher/tests/` | 20 | **169** | 84 (6 plików) |
| `ghost-launcher/src/components/` (mod tests) | 3 | **142** | 142 (3 pliki) |
| `ghost-core/src/` (gatekeeper-related) | 2 | **2** (z 18) | 2 |
| **Łącznie** | **25** | **313** | **228** |

**Testy bezpośrednio gatekeeperowe (228):** wszystkie z `gatekeeper.rs`(128), `gatekeeper_policy.rs`(9), `gatekeeper_commit_loop.rs`(5), `gatekeeper_policy_tests.rs`(44), `session_lifecycle_tests.rs`(22), `full_pipeline_integration.rs`(6), `gatekeeper_v2_pipeline_integration.rs`(4), `gatekeeper_events_emission_test.rs`(4), 4 z `refactor_invariants_tests.rs`(4), `health.rs`(1), `wal.rs`(1).

**Testy w sąsiedztwie (adjacent, 27):** `snapshot_engine_integration.rs`(11), `tx_intelligence_tests.rs`(7), 6 z `refactor_invariants_tests.rs`(6), `event_bus_subscription_order.rs`(3).

**Testy infrastrukturalne (58):** pozostałe 11 plików — nie testują Gatekeepera, ale są częścią tego samego projektu i muszą przechodzić.

---

### 11.5 Testy V2.5 do dodania (Faza 7)

**PDD (6 testów):**
- `test_entry_drift_shadow_hard_reject`
- `test_entry_drift_soft_pass`
- `test_spike_pattern_detection`
- `test_ramping_detection`
- `test_whale_concentration_shadow_veto`
- `test_reserve_health`

**TAS (4 testy):**
- `test_momentum_acceleration_positive`
- `test_momentum_deceleration_negative`
- `test_hhi_decline_during_observation`
- `test_volume_spike_detection`

**Backtest/regresja (2 testy):**
- `test_v25_vs_historical_losing_pools`
- `test_v25_vs_historical_winning_pools`

**Pliki testowe do utworzenia:**
- `ghost-launcher/tests/gatekeeper_pdd_tests.rs`
- `ghost-launcher/tests/gatekeeper_tas_tests.rs`
- `ghost-launcher/tests/gatekeeper_v25_regression.rs`

---

### 11.6 Test helper functions i fixture builders

**Konfiguracja:**
- `test_gk_v2_config()` — `gatekeeper_v2_pipeline_integration.rs:31`
- `policy_test_config()` — `gatekeeper_policy_tests.rs:34`
- `stage_b_policy_config()` — `gatekeeper_policy_tests.rs:178`
- `stage_c_policy_config()` — `gatekeeper_policy_tests.rs:198`
- `balanced_prosperity_config()` — `gatekeeper_policy_tests.rs:213`
- `strict_prosperity_overlay_config()` — `gatekeeper_policy_tests.rs:219`
- `pipeline_config()` — `full_pipeline_integration.rs:16`
- `v2_default_config()` — `gatekeeper.rs:5091`
- `alpha_config()` — `gatekeeper_policy.rs:1824`

**Feature'y i fixture'y:**
- `base_feature_set()` — `gatekeeper_policy_tests.rs:225`
- `assessment_with_sybil()` — `gatekeeper_policy.rs:1787`
- `alpha_ready_assessment()` — `gatekeeper_policy.rs:1836`
- `phase1_incomplete_feature_snapshot()` — `full_pipeline_integration.rs:363`

**Buildery transakcji:**
- `organic_tx()` — `gatekeeper_v2_pipeline_integration.rs:198`
- `bot_tx()` — `gatekeeper_v2_pipeline_integration.rs:260`
- `curve_tx()` — `gatekeeper_policy_tests.rs:828` / `full_pipeline_integration.rs:169`
- `create_v2_mock_tx()` — `gatekeeper.rs:4989`

**Buildery sesji:**
- `make_detected_pool()` — `gatekeeper_v2_pipeline_integration.rs:265`
- `setup_runtime()` — `gatekeeper_v2_pipeline_integration.rs:283`
- `candidate()` — `gatekeeper_policy_tests.rs:819`
- `account_update()` — `gatekeeper_policy_tests.rs:890`
- `seed_session_tx()` — `gatekeeper_policy_tests.rs:914`
- `evaluate_feature_policy()` — `gatekeeper_policy_tests.rs:922`
- `canonical_ready_terminal_verdict()` — `full_pipeline_integration.rs:257`
- `open_session()` — `session_lifecycle_tests.rs:264`

**Buildery sybilowe (session_lifecycle_tests.rs):**
- `ftdi_tx()`:78, `dbia_fingerprint()`:102, `dbia_tx()`:122, `sfd_tx()`:144, `des_tx()`:167, `funding_transfer()`:197

**Wrappery ewaluacji:**
- `evaluate()` — `gatekeeper_policy_tests.rs:315` (29 użyć)
- `evaluate_feature_policy()` — `gatekeeper_policy_tests.rs:922`
- `make_test_emitter()` — `gatekeeper_events_emission_test.rs:13

---

## 12. JSONL / DECISION LOGGER — KONTRAKT

**Plik:** `ghost-brain/src/oracle/decision_logger.rs`

### Stałe (obecny stan):
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 15`
- `GATEKEEPER_VERSION = "v2.2"`

### Historyczny V2.5 bump:
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 16`
- `GATEKEEPER_VERSION = "v2.5"`

### Repair-stream contract:
- `schema=16` pozostaje znacznikiem starego, mieszanego kontraktu rolloutowego
- naprawa plane separation wymaga nowego schema bump (`>=17`)
- logger/routing musi odróżniać `legacy_live` i `v25_shadow`
- `decision_verdict_buy` bez jawnej informacji o plane nie może już oznaczać obu semantyk naraz

### Routing:
- Wszystkie decyzje → `{gatekeeper_log_dir}/{rollout_profile}/{gatekeeper_version}/{decision_plane}/{config_hash}/gatekeeper_v2_decisions.jsonl`
- Tylko BUY → analogiczny plik `gatekeeper_v2_buys.jsonl` w tym samym katalogu routingu
- Dedup jest plane-aware (`ab_record_id + decision_plane`)
- Rekord mieszany bez jawnego plane może zostać rozbity na osobny wpis `legacy_live` i osobny wpis `v25_shadow`

### Nowe pola JSONL v17 (repair stream; wszystkie optional/additive):
- `rollout_profile`, `decision_plane`, `config_hash`
- `legacy_live_reason_chain`, `legacy_live_verdict_buy`, `legacy_live_verdict_type`
- `v25_shadow_verdict_type`, `v25_shadow_reason_chain`, `v25_shadow_confidence`, `v25_shadow_observation_stage`
- `v25_promotion_state`
- `tas_available`, `tas_unavailable_reason`
- `pdd_sequence_signals_available`, `pdd_price_anchor_available`
- `v25_confidence_available`, `v25_confidence_unavailable_reason`
- pola `decision_reason`, `decision_verdict_buy`, `verdict_type` pozostają jako alias zgodny wstecznie dla `legacy_live`
- historyczne pola shadow telemetry (`shadow_*`, `pdd_*`, `tas_*`, `aps_*`) pozostają zachowane, ale nie zastępują jawnego `decision_plane`
- dla Path B / feature-driven parity pola sekwencyjne mogą pozostać `None`, ale brak musi być jawnie opisany przez availability fields, a `v25_confidence` nie może być liczone z partial fiction

### GatekeeperBuyLog — struktura
Rozbudowana struktura z setkami pól (linie 197-1095 w `decision_logger.rs`). Mapowanie z `GatekeeperDecision` przez `GatekeeperAssessment::to_buy_log()`.

---

## 13. WAL — WRITE-AHEAD LOG

**Plik:** `ghost-core/src/wal.rs:257-262`

```rust
pub enum GatekeeperDecision {
    Buy,
    Reject,
    Wait,
    Timeout,
}
```

Niezależny typ od `ghost_launcher::components::gatekeeper::GatekeeperDecision`. Mapowanie:
- `verdict_buy == true` → `WalGatekeeperDecision::Buy`
- `verdict_buy == false` (z przyczyną) → `WalGatekeeperDecision::Reject`
- Timeout → `WalGatekeeperDecision::Timeout`
- `reason_chain` → `WalRecord::Decision.reason`

Punkty zapisu w `oracle_runtime.rs`: linie 8133 (Reject), 8214 (Timeout), 8392 (IWIM Reject), 8540 (Buy).

---

## 14. IWIM VETO GATE — KONTRAKT

**Plik:** `ghost-launcher/src/components/iwim_veto.rs`

### IwimVetoResult
- `gatekeeper_strength: GatekeeperStrength` — sklasyfikowana siła decyzji Gatekeepera
- `dev_known: bool` — czy dev jest znany

### Policy matrix (linie 253-431):
- **Strong + dev_known:** IWIM tylko blokuje na HIGH-confidence VETO, timeout = BUY
- **Strong + dev_unknown:** BUY (skip IWIM, dev_unknown strict cap active)
- **Borderline + dev_known:** IWIM jako "required confirmation", timeout/unknown = REJECT
- **Borderline + dev_unknown:** REJECT (najwyższy wektor ryzyka)

### Mutacja GatekeeperDecision (oracle_runtime.rs:8373-8380):
```rust
decision.verdict_buy = false;
decision.verdict_type = iwim_verdict_type;
decision.reason_chain = format!("{} → IWIM_REJECT: {}", decision.reason_chain, iwim_res.summary());
```

---

## 15. MIEJSCA BEZ GATEKEEPERDECISION

### 15.1 shadow_ledger/*
**Ścieżka:** `ghost-core/src/shadow_ledger/` (19 plików)
**Status:** ZERO referencji do `GatekeeperDecision` lub jego pól.
**Rola:** ShadowLedger operuje na własnych typach (`canonical_tx.rs`, `ledger.rs`, `simulation.rs`, `reconciliation.rs`, itd.) i nie konsumuje struktury `GatekeeperDecision`. Jest to osobny komponent do śledzenia on-chain lifecycle pozycji, nie do podejmowania decyzji.

### 15.2 trigger/*
**Ścieżka:** `ghost-launcher/src/components/trigger/` (5 plików: `mod.rs`, `component.rs`, `shadow_run.rs`, `safety.rs`, `tip_guard.rs`)
**Status:** ZERO referencji do `GatekeeperDecision` lub jego pól.
**Rola:** Trigger odpowiada za wykonanie transakcji kupna (live buy execution), a nie za podejmowanie decyzji. Operuje na `PoolTransaction`, `DetectedPool`, `ShadowLedger`, nie na `GatekeeperDecision`.

### 15.3 outcome_tracker.rs
**Plik:** `ghost-brain/src/oracle/outcome_tracker.rs`
**Status:** Tylko referencja dokumentacyjna do `verdict_type == BUY`. Nie odczytuje pól `GatekeeperDecision`.

---

## 16. MAPA PLIKÓW DO MODYFIKACJI

### NOWE pliki (7):

| Plik | Zawartość |
|------|-----------|
| `ghost-launcher/src/components/gatekeeper_pdd.rs` | Pump & Dump Detector |
| `ghost-launcher/src/components/gatekeeper_trajectory.rs` | Trajectory Aware Scoring |
| `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs` | Adaptive Prosperity |
| `ghost-brain/src/config/gatekeeper_v25_config.rs` | V2.5 config structs |
| `ghost-launcher/tests/gatekeeper_pdd_tests.rs` | Testy PDD |
| `ghost-launcher/tests/gatekeeper_tas_tests.rs` | Testy TAS |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | Testy regresji vs V2 |

### ISTNIEJĄCE pliki do modyfikacji / już zmodyfikowane w repair stream (8):

| Plik | Zmiana |
|------|--------|
| `ghost-brain/ghost_brain_config.toml` | Sekcje `[gatekeeper_v2.v25/dow/tas/pdd/aps]` aktywne; strict shadow guardrails przywrócone |
| `ghost-brain/src/config/ghost_brain_config.rs` | `GatekeeperV2Config` ma pola `v25`, `dow`, `tas`, `pdd`, `aps`; parser testy pilnują wartości rolloutowych |
| `ghost-launcher/src/components/gatekeeper.rs` | Rozszerzone `GatekeeperAssessment`, `GatekeeperVerdictType`, `GatekeeperBuffer`; invariants shadow plane egzekwowane |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | Live-path promocja PDD/TAS pozostaje jawnie gated przez `live_execution_enabled` |
| `ghost-launcher/src/oracle_runtime.rs` | Shadow routing wzbogacony o `rollout_profile` i `config_hash`; BUY-log enrichment plane-aware |
| `ghost-brain/src/oracle/decision_logger.rs` | Schema `v17`, plane-separated routing, rollout/config hash, dedup plane-aware |
| `ghost-launcher/src/components/mod.rs` | Eksportuje `gatekeeper_pdd`, `gatekeeper_trajectory`, `gatekeeper_adaptive_prosperity` |
| `ghost-brain/src/oracle/followup_scoring.rs` | Testowe `DecisionLoggerConfig` zaktualizowane o nowe pola routingu |

---

## 17. REGUŁY IMPLEMENTACYJNE V2.5

### R1: Wszystkie nowe struktury z `#[serde(default)]`
Każde nowe pole w configu, structach decyzyjnych i JSONL musi mieć `#[serde(default)]` dla wstecznej kompatybilności.

### R2: Nowe warianty enuma DODAWANE, nie zastępowane
`GatekeeperVerdictType` dostaje nowe warianty. Żaden istniejący wariant nie jest usuwany ani zmieniany.

### R3: Shadow-first
Wszystkie nowe mechanizmy (PDD, TAS, DOW, APS) domyślnie działają w trybie shadow/telemetry. Live execution tylko po promocji przez ADR.

### R4: Feature-flag
`v25.shadow_enabled = true`, `v25.live_execution_enabled = false`. Rollback: `shadow_enabled = false`.

### R5: Niezależne moduły
Każdy z czterech komponentów (PDD, TAS, DOW, APS) w osobnym pliku. Moduły nie importują się wzajemnie.

### R6: Testy regresji
Wszystkie 313 istniejących testów musi przechodzić po każdej fazie implementacji (z czego 228 to bezpośrednie testy Gatekeepera). Nowe testy V2.5 w osobnych plikach.

### R7: Kontrakt 8-9s nienaruszony
Live execution NIE skraca okna obserwacji. Wszystkie decyzje < 8s są shadow-only do czasu ADR.

### R8: Yellowstone jedynym źródłem
V2.5 nie dodaje RPC do ścieżki decyzyjnej.

---

## PODSUMOWANIE STANU WYJŚCIOWEGO

| Element | Stan obecny | Target V2.5 |
|--------|-------------|-------------|
| `GATEKEEPER_VERSION` | `"v2.2"` | `"v2.5"` |
| `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` | `15` | `16` |
| Tryb operacyjny | `mode = "long"` | `mode = "long"` (bez zmian) |
| `use_three_layer_decision` | `true` | `true` (bez zmian) |
| Feature flag V2.5 | brak | `v25.shadow_enabled = true` |
| Live V2.5 execution | brak | `v25.live_execution_enabled = false` |
| Pól `GatekeeperDecision` | 17 (16 bazowych + 1 opcjonalne strength) | 17+ (rozszerzone optional) |
| Wariantów `GatekeeperVerdictType` | 13 | 13+6 = 19 |
| Pól `GatekeeperAssessment` | 29 | 29+8 = 37 |
| Plików w shadow_ledger/ | 19 | 19 (bez zmian) |
| Plików w trigger/ | 5 | 5 (bez zmian) |
| Testów | 313 (169 w tests/ + 142 w mod tests + 2 ghost-core; z czego 228 direct) | 313 + ~12 nowych |
| Branch | `main` | `refactor/gatekeeper-v25` |

---

**Koniec dokumentu GATEKEEPER_V25_SSOT_CONTRACTS.md**

*Dokument został sporządzony na podstawie szczegółowej eksploracji kodu źródłowego na branchu `main` (commit `eaecac4`). Wszystkie ścieżki, numery linii i nazwy pól zostały zweryfikowane przez bezpośredni odczyt plików źródłowych.*
