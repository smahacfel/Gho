# AUDYT PIPELINE'U DECYZYJNEGO — GATEKEEPER V2 / V2.5
## Architektura SSOT, przepływ danych i kategoryzacja BUY/REJECT

> **Data audytu (kodu w repo):** 2026-05-07
> **Zakres:** `ghost-launcher`, `ghost-launcher/src/session`, `ghost-core`, `ghost-brain` (konfiguracja)
> **Konfiguracja referencyjna:** `ghost-brain/ghost_brain_config.toml` (version=11, mode=long, max_wait_time_ms=8001)
> **Metoda:** pełny trace z kodu źródłowego — każda ścieżka, każda funkcja, każda struktura danych

---

## SPIS TREŚCI

1. [Architektura SSOT — źródła prawdy o stanie pooli](#1-architektura-ssot)
2. [Pełny przepływ danych — od ingestu do werdyktu](#2-przeplyw-danych)
3. [Struktury danych — co, skąd, w jakiej formie](#3-struktury-danych)
4. [Ingestia — Seer, gRPC, Yellowstone](#4-ingestia)
5. [Sesja obserwacji — materializacja cech](#5-sesja-obserwacji)
6. [GatekeeperBuffer — akumulacja transakcji](#6-gatekeeperbuffer)
7. [Pipeline decyzyjny — 8 warstw kategoryzacji BUY/REJECT](#7-pipeline-decyzyjny)
8. [V2.5 Shadow — DOW, TAS, PDD, APS](#8-v25-shadow)
9. [Krzywa wiązania — curve gate i latch](#9-krzywa-wiazania)
10. [Commit, LivePipeline, post-Gatekeeper](#10-commit)
11. [Mapa plików — dokładne ścieżki i zakresy linii](#11-mapa-plikow)

---

## 1. ARCHITEKTURA SSOT

### 1.1. Co jest SSOT i dlaczego

System ma JEDNO kanoniczne źródło prawdy dla stanu każdego poolu:

```
MaterializedFeatureSet (ghost-core/src/checkpoint/types.rs:100-112)
```

To jest struktura-agregat, która łączy dane z **pięciu niezależnych źródeł** w jeden snapshot użyty do decyzji:

| Pole | Typ | Źródło danych |
|------|-----|--------------|
| `account_features` | `AccountStateFeatures` | Yellowstone gRPC → AccountStateCore reducer |
| `tx_intel_features` | `TxIntelFeatures` | Bufor transakcji → TxIntelligenceEngine |
| `checkpoint_features` | `CheckpointDerivedFeatures` | CheckpointEngine (trajektorie, impacty) |
| `curve_readiness` | `CurveReadinessFeatures` | ShadowLedger + AccountStateCore |
| `sybil_resistance` | `SybilResistanceFeatures` | CrossPoolVelocityIndex + FundingSourceIndex |
| `alpha_fingerprint` | `AlphaFingerprintFeatures` | Seer EarlyFingerprintAggregator |
| `risk_flags` | `Vec<RiskFlag>` | TxIntelligenceEngine |
| `session_metadata` | `SessionMetadata` | PoolObservationSession |

**Zasada:** żadna cecha nie jest liczona dwukrotnie z różnych źródeł. `MaterializedFeatureSet` jest budowany raz, w `PoolObservationSession::materialize_features()` (`ghost-launcher/src/session/observation.rs:368`), a następnie przekazywany jako niemutowalny snapshot do wszystkich warstw decyzyjnych.

### 1.2. Drugie źródło cen — GatekeeperBuffer.price_history

Równolegle do `MaterializedFeatureSet`, bufor Gatekeepera utrzymuje własną historię cen:

```rust
// ghost-launcher/src/components/gatekeeper.rs:433-446
pub struct PricePoint {
    pub timestamp_ms: u64,
    pub price_sol_per_token: f64,
    pub v_sol_in_curve: f64,        // rezerwy SOL w krzywej (lamports → SOL)
    pub v_tokens_in_curve: f64,     // rezerwy tokenów
    pub market_cap_sol: f64,
    pub is_buy: bool,               // czy punkt pochodzi z buy transakcji
    pub curve_data_known: bool,     // czy parser potwierdził dane
    pub curve_finality: CurveFinality,
}
```

`price_history` (`Vec<PricePoint>`) zasila:
- `compute_bonding_curve_dynamics()` → `BondingCurveDynamics` (Phase 6)
- `detect_entry_drift()` w `evaluate_pdd()` (PDD)
- `detect_flash_crash()` w `evaluate_pdd()`
- `current_curve_dynamics()` → uzupełnienie `MaterializedFeatureSet` w sesji

### 1.3. Trzecie źródło — AccountStateCore (on-chain state)

```rust
// ghost-core/src/account_state_core/types.rs:111-135
pub struct CanonicalPoolState {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub virtual_sol_reserves: u64,     // surowe lamporty
    pub virtual_token_reserves: u64,   // surowe tokeny (1 token = 10^6 unitów w Pump.fun)
    pub real_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub bonding_curve_progress: f64,   // 0.0-1.0
    pub price_sol: f64,                // SOL/token (znormalizowane)
    pub market_cap_sol: f64,           // SOL (znormalizowane)
    pub token_total_supply: u64,
    pub is_complete: bool,             // bonding curve ukończona?
    pub last_update_slot: u64,
    pub last_update_ts_ms: u64,
    pub curve_finality: CurveFinality,
    pub state_phase: StatePhase,       // Bootstrap → PendingConfirmation → Canonical → Migrated
    pub update_count: u64,
    pub initial_price_sol: f64,
    pub price_change_since_t0_pct: f64,
    pub reserve_velocity_sol_per_sec: f64,
}
```

Reducer: `AccountStateReducer` (`ghost-core/src/account_state_core/reducer.rs`) przyjmuje `AccountStateUpdate`, waliduje monotoniczność slotów i `receive_seq`, aktualizuje `CanonicalPoolState`. Maszyna stanów: `Bootstrap → PendingConfirmation → Canonical → Migrated`.

Z `CanonicalPoolState` wyprowadzany jest `AccountStateFeatures` (lekka wersja do decyzji):

```rust
// ghost-core/src/account_state_core/types.rs:164-175
pub struct AccountStateFeatures {
    pub current_reserves: (u64, u64),  // (sol_reserves, token_reserves) — SUROWE
    pub price_sol: f64,
    pub market_cap_sol: f64,
    pub bonding_progress: f64,         // 0.0-1.0
    pub price_change_since_t0_pct: f64,
    pub reserve_velocity_sol_per_sec: f64,
    pub is_bootstrap: bool,
    pub curve_finality: CurveFinality,
    pub state_phase: StatePhase,
    pub update_count: u64,
}
```

---

## 2. PRZEPŁYW DANYCH

### 2.1. Pełen trace: od Yellowstone do werdyktu

```
KROK 1: Yellowstone gRPC stream
  Plik: off-chain/components/seer/src/grpc_connection.rs
  Seer odbiera AccountUpdate + Transaction events przez gRPC

KROK 2: Seer → Event Bus → SnapshotListener
  Plik: ghost-launcher/src/components/seer.rs
  Plik: ghost-launcher/src/components/snapshot_listener.rs
  Normalizacja eventów, deduplikacja, routing do AccountStateReducer

KROK 3: SnapshotListener → AccountStateReducer
  Plik: ghost-core/src/account_state_core/reducer.rs
  Aktualizacja CanonicalPoolState, walidacja monotoniczności

KROK 4: OracleRuntime wykrywa nowy pool
  Plik: ghost-launcher/src/oracle_runtime.rs
  Tworzy PoolObservationSession per pool

KROK 5: PoolObservationSession.start() → tokio::select! pętla
  Plik: ghost-launcher/src/session/observation.rs + oracle_runtime.rs
  Dla każdego TX: session.ingest_transaction(tx)

KROK 6: session.ingest_transaction()
  Plik: ghost-launcher/src/session/observation.rs:211
  1. Enrichment TX: AccountStateCore → cena, bonding_progress
  2. Fingerprint: EarlyFingerprintAggregator → alpha_fingerprint
  3. gatekeeper_buffer.ingest_transaction_tracking_only(tx)
     → akumulacja TX, shadow checkpoints (Long mode)
  4. try_checkpoint() → CheckpointEngine

KROK 7: Przy deadlinie (mode=Long) lub triggerze (mode=Standard):
  resolve_feature_trigger_outcome()
    → evaluate_feature_driven_terminal_verdict()
      → session.materialize_features()
        → MaterializedFeatureSet (SSOT)
      → buffer.evaluate_from_features(features)
        → build_assessment_from_features()  [policy.rs]
        → evaluate_policy_from_assessment() [policy.rs]
        → evaluate_curve_gate()             [policy.rs]
      → GatekeeperVerdict::Buy / Reject / Timeout

KROK 8: Po BUY:
  → IWIM Veto Gate (opcjonalnie)
  → LauncherCommitCoordinator → gatekeeper_commit_loop
  → LivePipeline.init_for_mint()
  → Trigger / shadow_run / post_buy_runtime
```

### 2.2. Dwie ścieżki ewaluacji (świadomy dualizm)

Ścieżka kanoniczna (feature-driven, produkcja):
```
session.materialize_features()
  → GatekeeperBuffer::evaluate_from_features()
    → build_assessment_from_features()     // policy.rs:372
    → evaluate_policy_from_assessment()    // policy.rs:863
    → evaluate_curve_gate()                // policy.rs:1239
```

Ścieżka buforowa (shadow checkpoints, testy):
```
GatekeeperBuffer::run_assessment()         // gatekeeper.rs:4829
  → compute_decision()                     // gatekeeper.rs:4165
  → try_shadow_evaluate()                  // gatekeeper.rs:5192
```

Obie ścieżki są utrzymywane równolegle. Feature-driven używa `MaterializedFeatureSet` (SSOT z sesji). Buforowa liczy fazy inline na strukturach bufora.

### 2.3. Jak cena płynie przez system

```
Yellowstone AccountUpdate
  → sol_reserves: u64, token_reserves: u64  (surowe lamporty/tokeny)
  → AccountStateReducer.update_account_state()
    → price_sol = (virtual_sol_reserves / LAMPORTS_PER_SOL) / virtual_token_reserves
    → CanonicalPoolState.price_sol
    → AccountStateFeatures.price_sol
      → 1. MaterializedFeatureSet.account_features.price_sol
           → Phase 6: bonding_curve_from_features() → BondingCurveDynamics
      → 2. GatekeeperBuffer.price_history.push(PricePoint{...})
           → compute_bonding_curve_dynamics() → BondingCurveDynamics
           → detect_entry_drift() → entry_drift_pct
           → current_curve_dynamics() → uzupełnienie MaterializedFeatureSet

Dodatkowo:
  PoolObservationSession.materialize_features() (observation.rs:368)
    → curve_dynamics = gatekeeper_buffer.current_curve_dynamics()
    → materialized.checkpoint_features.single_tx_max_price_impact_pct
        .max(curve_dynamics.max_single_tx_price_impact_pct)
    → materialized.checkpoint_features.price_change_from_first_checkpoint_pct
        = (curve_dynamics.price_change_ratio - 1.0) * 100.0  [jeśli checkpoint pusty]
```

---

## 3. STRUKTURY DANYCH

### 3.1. MaterializedFeatureSet — SSOT decyzji

Lokalizacja: `ghost-core/src/checkpoint/types.rs:100-112`

```rust
pub struct MaterializedFeatureSet {
    pub account_features: AccountStateFeatures,        // z AccountStateCore
    pub tx_intel_features: TxIntelFeatures,            // z TxIntelligenceEngine
    pub checkpoint_features: CheckpointDerivedFeatures, // z CheckpointEngine
    pub risk_flags: Vec<RiskFlag>,                     // z TxIntelligenceEngine
    pub session_metadata: SessionMetadata,              // czas, id sesji
    pub curve_readiness: CurveReadinessFeatures,        // z ShadowLedger + AccountStateCore
    pub sybil_resistance: SybilResistanceFeatures,      // CPV + FSC + SFD + DBIA + DES + FTDI
    pub alpha_fingerprint: AlphaFingerprintFeatures,    // z Seer EarlyFingerprint
}
```

### 3.2. TxIntelFeatures — cechy z transakcji

Lokalizacja: `ghost-core/src/tx_intelligence/types.rs:49-96`

```rust
pub struct TxIntelFeatures {
    // Quantity (Phase 1)
    pub tx_count: u64,           pub buy_count: u64,        pub sell_count: u64,
    pub unique_signers: u64,     pub dust_tx_count: u64,    pub failed_tx_count: u64,

    // Volume (Phase 4)
    pub buy_ratio: f64,          pub sol_buy_ratio: f64,    pub avg_tx_sol: f64,
    pub volume_cv: f64,          pub total_volume_sol: f64,
    pub min_tx_sol: f64,         pub max_tx_sol: f64,
    pub max_consecutive_buys: u64,

    // Diversity (Phase 3)
    pub hhi: f64,                pub volume_gini: f64,
    pub unique_signer_ratio: f64, pub avg_tx_per_signer: f64,
    pub same_ms_tx_ratio: f64,   pub top3_volume_pct: f64,
    pub max_tx_per_signer: u64,

    // Velocity (Phase 2)
    pub interval_cv: f64,        pub timing_entropy: f64,
    pub avg_interval_ms: f64,    pub burst_ratio: f64,

    // Dev (Phase 5)
    pub dev_wallet_known: bool,  pub dev_buy_sol: f64,
    pub dev_volume_ratio: f64,   pub dev_tx_ratio: f64,
    pub dev_has_sold: bool,      pub dev_is_first_buyer: bool,
    pub dev_initial_buy_tokens: Option<f64>, pub dev_tx_count: u64,
}
```

### 3.3. CheckpointDerivedFeatures — dane z checkpointów

Lokalizacja: `ghost-core/src/checkpoint/types.rs:40-55`

```rust
pub struct CheckpointDerivedFeatures {
    pub price_trajectory: Vec<f64>,             // historia cen z checkpointów
    pub reserve_trajectory: Vec<(u64, u64)>,    // historia rezerw
    pub buy_pressure_trend: TrendDirection,     // Rising/Falling/Stable/Insufficient
    pub signer_diversity_trend: TrendDirection,
    pub risk_flag_count_trend: TrendDirection,
    pub trajectory_checkpoint_count: u32,
    pub price_change_from_first_checkpoint_pct: f64,  // zmiana % od pierwszego checkpointu
    pub single_tx_max_price_impact_pct: f64,          // max % impact pojedynczego TX
    pub max_single_sell_impact_pct: f64,              // max % impact pojedynczej sprzedaży
    pub bonding_progress: f64,                        // 0.0-1.0
}
```

### 3.4. GatekeeperAssessment — wynik 6-fazowej oceny

Lokalizacja: `ghost-launcher/src/components/gatekeeper.rs:1208-1279`

```rust
pub struct GatekeeperAssessment {
    // 6 faz
    pub phase1_passed: bool,        pub phase2_velocity: Option<VelocityProfile>,
    pub phase2_passed: bool,        pub phase3_diversity: Option<SignerDiversityProfile>,
    pub phase3_passed: bool,        pub phase4_volume: Option<VolumeSanityProfile>,
    pub phase4_passed: bool,        pub phase5_dev: Option<DevBehaviorProfile>,
    pub phase5_passed: bool,        pub phase6_curve: Option<BondingCurveDynamics>,
    pub phase6_passed: bool,        pub phases_passed: u8,

    // Decyzja
    pub hard_reject_reason: Option<String>,
    pub decision: Option<GatekeeperDecision>,

    // V2.5
    pub trajectory: Option<TrajectoryAssessment>,
    pub pdd_assessment: Option<PddDiagnostics>,
    pub aps_diagnostics: Option<ApsDiagnostics>,
    pub observation_stage: Option<ObservationStage>,
    pub entry_drift_pct: Option<f64>,
    pub v25_confidence: Option<f64>,
    pub v25_shadow_decisions: Vec<ShadowV25Decision>,

    // Metadane
    pub total_tx_evaluated: usize,   pub observation_duration_ms: u64,
    pub buy_count: usize,            pub checkpoint_count: u32,

    // SSOT feature bundle (dla replay/loggera)
    pub feature_snapshot: MaterializedFeatureSet,
}
```

### 3.5. GatekeeperDecision — wynik warstw decyzyjnych

```rust
pub struct GatekeeperDecision {
    pub hard_fail_reason: Option<String>,
    pub core1_passed: bool,         pub core2_passed: bool,     pub core3_passed: bool,
    pub dev_unknown: bool,
    pub soft_signals: SoftSignals,  pub soft_points: u8,
    pub sybil_policy: SybilPolicyDiagnostics,
    pub alpha_gate: AlphaGateDiagnostics,
    pub prosperity_filter: ProsperityFilterDiagnostics,
    pub total_soft_points: u16,
    pub verdict_type: GatekeeperVerdictType,
    pub verdict_buy: bool,
    pub reason_chain: String,       // pełny łańcuch powodów decyzji
    pub gatekeeper_strength: Option<GatekeeperStrength>,  // Strong/Borderline dla IWIM
}
```

---

## 4. INGESTIA

### 4.1. Seer — Yellowstone gRPC ingestion

```
off-chain/components/seer/src/grpc_connection.rs  →  połączenie gRPC
off-chain/components/seer/src/lib.rs              →  start serwisu, konfiguracja
off-chain/components/seer/src/ipc.rs              →  IPC między procesami
off-chain/components/seer/src/types.rs            →  typy eventów
```

Seer łączy się z Yellowstone gRPC (Chainstack: `yellowstone-solana-mainnet.core.chainstack.com:443`), subskrybuje:
- `SubscribeUpdate::Account` — aktualizacje kont bonding curve Pump.fun
- `SubscribeUpdate::Transaction` — transakcje na poolach

Eventy są normalizowane do wewnętrznego formatu i wysyłane przez event bus.

### 4.2. SnapshotListener — jedyny writer do AccountStateCore

Plik: `ghost-launcher/src/components/snapshot_listener.rs`

SnapshotListener odbiera eventy z Seer/event bus i:
1. Parsuje AccountUpdate → `AccountStateUpdate { sol_reserves, token_reserves, slot, ... }`
2. Wywołuje `AccountStateReducer::update_account_state(update)`
3. Reducer waliduje: monotoniczność slotów (`last_update_slot`), monotoniczność `receive_seq`
4. Aktualizuje `CanonicalPoolState` z nowymi rezerwami, ceną, market capem
5. Emituje zaktualizowany `AccountStateFeatures`

**Kontrakt:** SnapshotListener jest JEDYNYM kanonicznym writerem do silnika snapshotów. Runtime nie może dublować `handle_tx_event`.

### 4.3. Wzbogacanie TX w sesji

`pool_observation_task` (runtime.rs) → dla każdego TX:
1. `maybe_materialize_canonical_state_from_observed_tx` — pobiera stan z AccountStateCore
2. Enrichment z AccountStateCore + fallback ShadowLedger
3. `fingerprint_aggregator.observe_transaction()` — wczesne metryki (sell_buy_ratio, jito_tip_intensity, itd.)
4. `session.ingest_transaction(tx)` → GatekeeperBuffer

---

## 5. SESJA OBSERWACJI

### 5.1. PoolObservationSession — stan per pool

Lokalizacja: `ghost-launcher/src/session/observation.rs:34-63`

```rust
pub struct PoolObservationSession {
    pub session_id: SessionId,
    pub pool_amm_id: Pubkey,          pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,        pub dev_wallet: Option<Pubkey>,
    pub created_at_wall_ms: u64,      pub deadline_wall_ms: u64,
    pub status: SessionStatus,
    pub tx_buffer: VecDeque<Arc<PoolTransaction>>,
    pub tx_keys_seen: HashSet<TxKey>,
    pub highest_seen_ts_ms: u64,
    pub account_state_core: Arc<AccountStateReducer>,    // stan on-chain
    pub account_features: AccountStateFeatures,          // ostatni snapshot
    pub gatekeeper_buffer: GatekeeperBuffer,             // bufor transakcji
    pub tx_intelligence: TxIntelligenceEngine,           // cechy TX
    pub tx_intel_features: TxIntelFeatures,              // ostatni snapshot TX
    pub cross_pool_velocity_index: Arc<CrossPoolVelocityIndex>,
    pub funding_source_index: Arc<FundingSourceIndex>,
    pub checkpoint_engine: CheckpointEngine,             // checkpointy
    pub feature_builder: ObservationFeatureBuilder,      // materializator
    pub checkpoints: Vec<SessionCheckpoint>,
    pub active_risk_flags: Vec<RiskFlag>,
}
```

### 5.2. materialize_features() — budowanie SSOT

Lokalizacja: `ghost-launcher/src/session/observation.rs:368-490`

```rust
pub fn materialize_features(&self) -> MaterializedFeatureSet {
    // 1. Podstawowa materializacja z feature_buildera
    let account_features = self.current_account_features();
    let mut materialized = self.feature_builder.materialize(
        account_features.clone(),
        self.tx_intel_features.clone(),     // TxIntelFeatures
        &self.checkpoints,                  // CheckpointDerivedFeatures
        self.active_risk_flags.clone(),     // RiskFlag
        self.session_metadata(),            // SessionMetadata
    );

    // 2. Uzupełnienie z GatekeeperBuffer (curve dynamics)
    let curve_dynamics = self.gatekeeper_buffer.current_curve_dynamics();
    materialized.checkpoint_features.single_tx_max_price_impact_pct
        = materialized.checkpoint_features.single_tx_max_price_impact_pct
            .max(curve_dynamics.max_single_tx_price_impact_pct);
    materialized.checkpoint_features.max_single_sell_impact_pct
        = materialized.checkpoint_features.max_single_sell_impact_pct
            .max(curve_dynamics.max_single_sell_impact_pct);

    // 3. CurveReadiness z AccountStateCore
    materialized.curve_readiness = self.current_curve_readiness();

    // 4. Price change — fallback z gatekeeper_buffer
    if materialized.checkpoint_features.price_change_from_first_checkpoint_pct.abs() <= f64::EPSILON
        && curve_dynamics.price_data_points >= 2
    {
        materialized.checkpoint_features.price_change_from_first_checkpoint_pct
            = (curve_dynamics.price_change_ratio - 1.0) * 100.0;
    }

    // 5. Bonding progress fallback
    if materialized.account_features.update_count == 0 {
        let fallback_bonding_progress = self.candidate_snapshot.bonding_curve_progress
            .or_else(|| self.candidate_snapshot.shadow_bonding_progress.map(|p| p as f64 / 100.0))
            .unwrap_or_else(|| if curve_dynamics.curve_data_known { curve_dynamics.bonding_progress_pct / 100.0 } else { 0.0 });
        materialized.account_features.bonding_progress = fallback_bonding_progress;
    }

    // 6. Alpha fingerprint z EarlyFingerprintAggregator
    if let Some(fingerprint) = self.fingerprint_metrics() {
        materialized.alpha_fingerprint = AlphaFingerprintFeatures {
            avg_inner_ix_count_50tx: fingerprint.avg_inner_ix_count_50tx,
            sell_buy_ratio: fingerprint.sell_buy_ratio,
            compute_unit_cluster_dominance: fingerprint.compute_unit_cluster_dominance,
            static_fee_profile_ratio: fingerprint.static_fee_profile_ratio,
            jito_tip_intensity: fingerprint.jito_tip_intensity,
            early_slot_volume_dominance_buy: fingerprint.early_slot_volume_dominance_buy,
            early_top3_buy_volume_pct_3s: fingerprint.early_top3_buy_volume_pct_3s,
            fixed_size_buy_ratio: fingerprint.fixed_size_buy_ratio,
            flipper_presence_ratio: fingerprint.flipper_presence_ratio,
        };
    }

    // 7. Sybil resistance
    let sybil = compute_sybil_resistance(self.tx_buffer.iter().map(AsRef::as_ref), sybil_dev_wallet.as_deref());
    materialized.sybil_resistance.fee_topology_diversity_index = sybil.fee_topology_diversity_index;
    materialized.sybil_resistance.dev_buyer_infrastructure_affinity = sybil.dev_buyer_infrastructure_affinity;
    materialized.sybil_resistance.spend_fraction_divergence = sybil.spend_fraction_divergence;
    materialized.sybil_resistance.demand_elasticity_score = sybil.demand_elasticity_score;
    materialized.sybil_resistance.degraded_reasons = sybil.degraded_reasons;

    // 8. Cross-pool velocity
    let cpv = self.cross_pool_velocity_index.compute_for_transactions(
        self.pool_amm_id.to_string().as_str(),
        self.tx_buffer.iter().map(AsRef::as_ref),
        Some(cpv_anchor_ts_ms),
        &self.cross_pool_velocity_config,
    );
    materialized.sybil_resistance.signer_cross_pool_velocity = cpv.signer_cross_pool_velocity;

    // 9. Funding source (jeśli aktywne)
    // ... FSC computation ...

    materialized  // ← to jest SSOT dla decyzji
}
```

---

## 6. GATEKEEPERBUFFER

### 6.1. Struktura bufora

Lokalizacja: `ghost-launcher/src/components/gatekeeper.rs:2302+`

```rust
pub struct GatekeeperBuffer {
    pub pool_id: Pubkey,
    pub config: GatekeeperV2Config,
    pub state: PoolState,                     // Tracked → Approved → Committed

    // Akumulacja transakcji
    pub buffered_txs: Vec<GatekeeperBufferedTx>,
    pub tx_keys_seen: HashSet<TxKey>,
    pub total_tx_count: usize,
    pub buy_count: usize,       pub sell_count: usize,
    pub total_volume_sol: f64,  pub buy_volume_sol: f64,
    pub max_consecutive_buys: u64,
    pub unique_signers: HashSet<String>,
    pub signer_stats: HashMap<String, SignerStats>,
    pub dust_filtered_count: u64,
    pub failed_tx_count: u64,

    // Śledzenie czasu
    pub registered_wall_ts_ms: u64,
    pub deadline_wall_ts_ms: u64,
    pub first_tx_ts: Option<u64>,
    pub highest_seen_ts: u64,
    pub curve_t0_event_ts_ms: Option<u64>,
    pub curve_t0_clock_source: Option<&'static str>,

    // Historia cen (dla PDD i Phase 6)
    pub price_history: Vec<PricePoint>,

    // Dev tracking
    pub dev_wallet: Option<String>,
    pub dev_buy_total_sol: f64,
    pub dev_has_sold: bool,
    pub dev_tx_count: u64,

    // Stan decyzyjny
    pub phase1_passed: bool,
    pub rejected: bool,
    pub eval_count: usize,
    pub curve_ready: bool,

    // V2.5 shadow
    pub v25_shadow_decisions: Vec<ShadowV25Decision>,
    pub early_shadow_fired: bool,
    pub normal_shadow_fired: bool,
    pub window_stage: ObservationStage,
}
```

### 6.2. Ingestia transakcji (Long mode)

`ingest_transaction_tracking_only()` (gatekeeper.rs:3508) → `ingest_long_transaction_tracking_only()` (gatekeeper.rs:3412):

1. **Dust filter:** TX z `volume_sol < min_sol_threshold` → dust_filtered_count++
2. **Dedup:** TxKey w `tx_keys_seen`
3. **Akumulacja:** update_tracking() → signer_stats, volume, buy/sell count, price_history
4. **V2.5 shadow checkpoints:** przy `v25.shadow_enabled && dow.enabled`:
   - Early (2-5s od rejestracji): `try_shadow_evaluate(ObservationStage::Early)`
   - Normal (5-7s od rejestracji): `try_shadow_evaluate(ObservationStage::Normal)`
5. **Deadline check:** jeśli `now_ms >= deadline_wall_ts_ms` → `GatekeeperIngressOutcome::DeadlineElapsed`

### 6.3. run_assessment() — pełna ocena buforowa

Lokalizacja: `gatekeeper.rs:4829-5085`

1. **Hard Reject (inline):** dev_has_sold, extreme_bot (hardcoded: interval_cv < 0.08 && avg < 30ms), extreme_hhi (hardcoded: > 0.5), extreme_price_manipulation (hardcoded: impact > 50%), failed_tx_ratio
2. **Phase 2:** `compute_velocity_profile(tx_timestamps_sorted)` → VelocityProfile
3. **Phase 3:** `compute_signer_diversity(signer_stats)` → SignerDiversityProfile
4. **Phase 4:** `compute_volume_sanity(...)` → VolumeSanityProfile
5. **Phase 5:** `compute_dev_behavior(...)` → DevBehaviorProfile
6. **Phase 6:** `compute_bonding_curve_dynamics(price_history)` → BondingCurveDynamics
7. **Trajektoria:** `materialize_trajectory(tas_config)` → TrajectoryAssessment
8. **PDD:** `evaluate_pdd(self, pdd_config, None)` → PddDiagnostics (pełne 6/6 sygnałów)

---

## 7. PIPELINE DECYZYJNY

### 7.1. build_assessment_from_features() — mapowanie MaterializedFeatureSet → GatekeeperAssessment

Lokalizacja: `ghost-launcher/src/components/gatekeeper_policy.rs:372-536`

**Phase 1 — Quantity Gate** (policy.rs:377-379):
```
tx_count >= min_tx_count (12)
&& unique_signers >= min_unique_signers (8)
&& buy_count >= min_buy_count (6)
```

**Phase 2 — Velocity** (policy.rs:381-394):
```
velocity_profile_from_features(&features)
  → interval_cv in [min_interval_cv, max_interval_cv]
  → burst_ratio <= max_burst_ratio
  → avg_interval_ms in [min_avg_interval_ms, max_avg_interval_ms]
  → timing_entropy in [min_timing_entropy, max_timing_entropy]
  → dust_tx_count >= min_dust_filtered_count
```

**Phase 3 — Signer Diversity** (policy.rs:396-400):
```
signer_diversity_from_features(&features)
  → unique_ratio in [min_unique_ratio, max_unique_ratio]
  → hhi <= max_hhi
  → max_tx_per_signer <= max_tx_per_signer
  → volume_gini in [min_volume_gini, max_volume_gini]
  → top3_volume_pct <= max_top3_volume_pct
  → same_ms_tx_ratio <= max_same_ms_tx_ratio
```

**Phase 4 — Volume Sanity** (policy.rs:402-408):
```
volume_sanity_from_features(&features)
  AND alpha_fingerprint_phase4_passes(&features.alpha_fingerprint)
    → buy_ratio in [min_buy_ratio, max_buy_ratio]
    → avg_tx_sol in [min_avg_tx_sol, max_avg_tx_sol]
    → volume_cv in [min_volume_cv, max_volume_cv]
    → total_volume_sol in [min_total_volume_sol, max_total_volume_sol]
    → sol_buy_ratio in [min_sol_buy_ratio, max_sol_buy_ratio]
    → max_consecutive_buys >= min_consecutive_buys
    → alpha_fingerprint thresholds passes (8 osobnych progów dla fingerprint metryk)
```

**Phase 5 — Dev Behavior** (policy.rs:410-431):
```
dev_behavior_from_features(&features)
  → jeśli dev_wallet_known:
      dev_buy_total_sol in [min_dev_buy_sol, max_dev_buy_sol]
      dev_tx_ratio in [min_dev_tx_ratio, max_dev_tx_ratio]
      dev_volume_ratio in [min_dev_volume_ratio, max_dev_volume_ratio]
      (!dev_has_sold || !reject_on_dev_sell)
  → jeśli !dev_wallet_known: auto-pass
```

**Phase 6 — Bonding Curve Dynamics** (policy.rs:432-451):
```
bonding_curve_from_features(&features)
  → jeśli price_data_points < 2: auto-pass
  → price_change_ratio <= max_price_change_ratio
  → max_single_tx_price_impact_pct <= max_single_tx_price_impact_pct
  → max_single_sell_impact_pct <= max_single_sell_impact_pct
  → jeśli curve_data_known:
      bonding_progress_pct in [min_bonding_progress_pct, max_bonding_progress_pct]
      current_market_cap_sol >= min_market_cap_sol
```

Następnie (policy.rs:453-535):
- `phases_passed = count(true)` z 6 faz
- `hard_reject_reason = evaluate_hard_filters_from_assessment()`
- `pdd_assessment = materialize_pdd_diagnostics_from_features()` (feature-driven, 3/6 PDD)
- `aps_diagnostics = evaluate_aps()` (shadow)
- `observation_stage = derive_observation_stage_from_features()` (Early/Normal/Extended)

### 7.2. evaluate_policy_from_assessment() — 8-warstwowa decyzja

Lokalizacja: `ghost-launcher/src/components/gatekeeper_policy.rs:863-1140`

**WARSTWA 0: Hard Fails** (policy.rs:870-893)
```
evaluate_hard_filters_from_assessment(assessment, config)
  → HF-1: DevSold         — reject_on_dev_sell && dev_has_sold
  → HF-2: SellImpact      — max_single_sell_impact > config
  → HF-3: TxPriceImpact   — max_single_tx_price_impact > config
  → HF-4: PriceChange     — price_change_ratio > max_price_change_ratio
  → HF-5: MarketCapTooLow — market_cap < min_market_cap_sol
  → HF-6: ExtremeHhi      — hhi > hard_fail_hhi
  → HF-7: ExtremeBundling — same_ms_tx_ratio > hard_fail_same_ms_tx_ratio
  → HF-8: ExtremeTop3     — top3_volume_pct > hard_fail_top3_volume_pct
  → HF-9: ExtremeBotTiming — interval_cv < 0.08 && avg_interval < 30ms && tx >= hard_fail_bot_min_tx
  → HF-10: FailedTxRatio   — failed_ratio > min_failed_tx_ratio_for_bot_flag
  → HF-11: SlowPool        — avg_interval_ms > max_avg_interval_ms
```

**WARSTWA 1: PDD Live Veto** (policy.rs:896-947)
```
Warunek: pdd.enabled && v25.live_execution_enabled
Dla każdego PDD hard fail: sprawdź *_promoted_to_live flag
  → EntryDrift → entry_drift_promoted_to_live → RejectEntryDrift
  → Spike      → spike_promoted_to_live      → RejectPumpAndDump
  → Ramping    → ramping_promoted_to_live    → RejectRamping
  → Whale      → whale_promoted_to_live      → RejectPumpAndDump
  → Reserve    → reserve_promoted_to_live    → RejectPumpAndDump
  → FlashCrash → flash_crash_promoted_to_live → RejectFlashCrash
Nielive (niepromowane) → pomijane w live, nadal widoczne w shadow
```

**WARSTWA 2: Core Fail** (policy.rs:949-963)
```
Core1 = Phase1 (Quantity Gate)
Core2 = Phase4 (Volume Sanity)
Core3 = złożenie Phase5 + Phase6:
  → dev_unknown: auto-pass Phase5, zaostrzone Phase6
     (używa dev_unknown_max_single_tx_price_impact_pct,
      dev_unknown_min_market_cap_sol)
  → dev_known: Phase5 && Phase6
```

**WARSTWA 3: Sybil Combo Veto** (policy.rs:964-971)
```
sybil_combo_veto_reason(&diagnostics.sybil_policy, config)
  → HighDbiaLowFtdiLowSfd
  → LowDesLowSfd + (HighDbia || LowFtdi)
  → HighFscHighCpv + (LowDes || LowSfd)
```

**WARSTWA 4: Sybil Soft Excess** (policy.rs:972-990)
```
sybil_policy.soft_points > effective_max_sybil_soft_points
```

**WARSTWA 5: Legacy Soft Excess** (policy.rs:991-1003)
```
diagnostics.soft_points > effective_max_soft_points
gdzie soft_points = compute_soft_signals().weighted_score(weights)
```

**WARSTWA 6: Alpha Gate** (policy.rs:1005-1022)
```
evaluate_alpha_gate(features, config) — selektor pozytywny
  1. momentum = 0.36*norm(burst) + 0.34*norm(interval) + 0.20*norm(entropy) + 0.10*norm(buys)
     × jito_boost × dominance_boost
  2. demand = 0.35*norm(buys) + 0.35*norm(signers) + 0.30*norm(buy_ratio)
     × fixed_size_penalty × flipper_penalty
  3. joint = momentum * demand

Odrzucenie gdy:
  → momentum < min_momentum       (0.55)
  → demand < min_demand           (0.55)
  → joint < min_alpha_joint       (0.35)
  → buy_count < min_alpha_sample  (15)
  → missing alpha_inputs (jito_tip, fixed_size, flipper — None)
```

**WARSTWA 7: Prosperity Filter** (policy.rs:1024-1078)
```
evaluate_prosperity_filter(assessment, config) — 3 gałęzie:

B1: conviction_clean_sells
  → block0_sniped_supply_pct >= prosperity_branch1_min_block0_sniped_supply_pct
  → sell_buy_ratio <= prosperity_branch1_max_sell_buy_ratio

B2: large_cap_buy_dominance
  → market_cap >= prosperity_branch2_min_market_cap_sol
  → early_slot_volume_dominance_buy >= prosperity_branch2_min_early_slot_volume_dominance_buy

B3: organic_structure
  → hhi <= prosperity_branch3_max_hhi
  → fee_topology_diversity_index >= prosperity_branch3_min_fee_topology_diversity_index

Dodatkowo: prosperity overlay (opcjonalny, enable_prosperity_overlay)
  → price_change, bonding_progress, fee_topology, branch23_sell_buy, branch2_price

Przejście wymaga: market_cap_floor_pass && cpv_pass && minimum 1 base branch
  oraz (jeśli overlay): wszystkie overlay checks
```

### 7.3. TAS modulacja (po przejściu wszystkich warstw)

Lokalizacja: `gatekeeper_policy.rs:1080-1119`

Po przejściu wszystkich warstw, dla werdyktu BUY:
1. Oblicz `tas_score` z `assessment.trajectory` (jeśli dostępne)
2. Ustal `GatekeeperStrength`:
   - **Strong:** soft_points <= effective_max - iwim_veto_strong_margin (margines 3)
     && manipulation_flag_count <= iwim_veto_strong_max_manip_flags (0 flag)
   - **Borderline:** w przeciwnym razie
3. TAS demotion: jeśli `is_strong && tas_score < 0.45` → zdegraduj do Borderline

Ta klasyfikacja jest używana przez IWIM Veto Gate: Strong → IWIM tylko blokuje na HIGH confidence VETO, Borderline → IWIM timeout/unknown = REJECT.

### 7.4. evaluate_curve_gate() — krzywa wiązania

Lokalizacja: `gatekeeper_policy.rs:1239-1312`

Decyduje czy krzywa jest gotowa przed wykonaniem BUY:
```
curve_readiness.is_ready || freshness == Fresh/Committed
  → Ready: kontynuuj
  → Pending: czekaj do curve_wait_ms
  → Reject:
      - Unknown + !curve_require_for_buy → odrzuć
      - Unknown + wait_elapsed > curve_wait_ms → timeout
      - Stale + Reject fallback → odrzuć
```

### 7.5. evaluate_from_features() — finalny werdykt

Lokalizacja: `gatekeeper.rs:3221-3305`

```rust
pub fn evaluate_from_features(&mut self, features: MaterializedFeatureSet,
    config: &GatekeeperV2Config) -> GatekeeperVerdict
{
    let mut assessment = build_assessment_from_features(features, config, ctx);
    let decision = evaluate_policy_from_assessment(&assessment, config);
    assessment.decision = Some(decision);
    assessment.v25_shadow_decisions = self.v25_shadow_decisions.clone();

    match evaluate_curve_gate(&assessment.feature_snapshot, config) {
        Ready => {},
        Pending => return GatekeeperVerdict::PendingCurve,
        Reject => return GatekeeperVerdict::Reject { ... },
    }

    if !verdict_buy {
        return GatekeeperVerdict::Reject { assessment, reason };
    }

    self.state = PoolState::Approved;
    GatekeeperVerdict::Buy { buffered_txs, assessment }
}
```

---

## 8. V2.5 SHADOW

### 8.1. try_shadow_evaluate() — shadow checkpointy

Lokalizacja: `gatekeeper.rs:5192-5466`

Wywoływane z `ingest_long_transaction_tracking_only()` przy:
- **Early (2-5s):** `elapsed >= early_entry_min_ms && elapsed <= early_entry_max_ms`
- **Normal (5-7s):** `elapsed >= normal_window_ms`
- **Extended (7-10s):** obsługiwane w `check_long_deadline`

Flow:
```
1. Guard: total_tx_count >= min_data_tx
   (Early: early_entry_min_tx_count, Normal/Extended: min_tx_count)

2. Phase 1 guard: tx >= min_tx_count, signers >= min_unique_signers, buys >= min_buy_count

3. run_assessment() → GatekeeperAssessment (pełne 6 faz + trajectory + full PDD)

4. compute_decision() → GatekeeperDecision (hard fail + core + sybil + alpha + prosperity)

5. APS: evaluate_aps() → ApsDiagnostics
   → HighVolatility regime → re-check PDD drift z 3% threshold

6. Confidence proxy:
   confidence = 1.0 - soft_points/max_possible
   → TAS modulation (skip dla Early)
   → PDD: confidence *= pdd.pdd_score (0.0 jeśli hard_fail)

7. Stage-specific criteria:
   Early:
     → all_phases_passed (6/6)
     → enough_tx (>= early_entry_min_tx_count)
     → high_conf (>= early_entry_min_confidence)
     → sybil_clean (<= early_entry_max_sybil_points)
     → low_drift (<= early_entry_max_entry_drift_pct)
     → has_momentum (>= early_entry_min_momentum)
   Normal:
     → verdict_buy && confidence >= normal_window_min_confidence

8. Zapis ShadowV25Decision → v25_shadow_decisions
```

### 8.2. Dynamic Observation Window (DOW)

Trzy okna decyzyjne (shadow-first, live wymaga ADR + promotion):

| Okno | Czas | Confidence min | Dodatkowe warunki |
|------|------|---------------|-------------------|
| Early | 2-5s | 0.85 | 6/6 faz, drift < 3%, momentum > 0.40, sybil <= 1, tx >= 15 |
| Normal | 5-7s | 0.65 | BUY verdict z compute_decision |
| Extended | 7-10s | 0.55 | PDD w pełni czyste (pdd_clean) |

### 8.3. PDD — Pump & Dump Detector

Lokalizacja: `ghost-launcher/src/components/gatekeeper_pdd.rs:80-188`

**Pełny PDD (bufor, `evaluate_pdd`):** 6 sygnałów:
1. **Entry drift** — 4-poziomowa hierarchia kotwicy ceny:
   - Level 1: InitPoolEvent proxy (curve_data_known && v_sol_in_curve > 0) → "strong"
   - Level 2: AccountStateCore proxy (v_sol_in_curve > 0) → "strong"
   - Level 3: Parser-authoritative (curve_data_known) → "strong"
   - Level 4: Fallback (pierwszy punkt w historii) → "weak"
   - Drift = ((current / anchor) - 1.0) * 100

2. **Spike** — porównanie recent volume rate (3s okno) vs earlier rate (próg >2×)

3. **Ramping** — 4 consecutive same-size buys (±15% tolerance)

4. **Whale** — top3 volume pct > 60% (config `whale_top3_max_pct`) lub single > 35%

5. **Reserve** — reserve_sol >= 30.0 && reserve/market_cap >= 0.15

6. **Flash crash** — single sell impact > 15% lub 2+ selli w 500ms z cenowym impactem

**Feature-driven PDD (`materialize_pdd_diagnostics_from_features`, policy.rs:598-656):** 3/6 sygnałów:
- Entry drift (z checkpoint/account features)
- Whale (z tx_intel_features.top3_volume_pct)
- Reserve health (z account_features)

### 8.4. TAS — Trajectory Aware Scoring

Lokalizacja: `ghost-launcher/src/components/gatekeeper_trajectory.rs`

Segmentacja okna obserwacji na 3 równe segmenty (T0/T1/T2). Dla każdego:
- `build_segment(txs)` → TrajectorySegment { tx_count, buy_ratio, avg_interval_ms, total_volume_sol, hhi }
- Wymagane: `min_tx_per_segment >= 3` na segment, `total_duration >= 3000ms`

5-wymiarowy scoring:
```
momentum_score    = f(T2.tx_count / T0.tx_count)     [accel > 1.15 → 1.0, decel < 0.85 → 0.0]
hhi_score         = f(T2.hhi / T0.hhi)                [decline < 0.85 → 1.0]
volume_score      = 1.0 - (vol_cv / volume_cv_max)    [stabilność wolumenu]
interval_score    = f(T2.avg_interval / T0.avg_interval) [shortening < 0.80 → 1.0]
buy_ratio_score   = T2.buy_ratio / buy_ratio_stability_min [stabilność buy ratio]

overall_tas_score = Σ(w_i * score_i), clamped [0.0, 1.0]
```

TAS modulator: `confidence *= [0.75 + tas_score * 0.50]` (zakres 0.75-1.25)
TAS hard reject: `tas_score < 0.30` → confidence = 0.0 (tylko w shadow path)

### 8.5. APS — Adaptive Prosperity

Lokalizacja: `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`

Shadow-only. Wykrywa reżim rynkowy i sugeruje progi:
- **detect_regime():** HHI spike (>0.6), price spike (ratio > 3.0), volume spike (PDD spike) → HighVolatility
- **has_sufficient_history = false** (hardcodowane) → zawsze zwraca Normal (kalibracja wymaga cross-pool outcome trackera)
- Shadow thresholds: entry_drift_max, confidence_min, prosperity_mcap, branch1_sniped, branch3_hhi — różne per reżim

---

## 9. KRZYWA WIĄZANIA

### 9.1. CurveReadiness — skąd wiemy że krzywa jest gotowa

```rust
// ghost-core/src/checkpoint/types.rs:58-70
pub struct CurveReadinessFeatures {
    pub is_ready: bool,                  // krzywa gotowa do użycia
    pub freshness: CurveFreshnessState,  // Fresh | Committed | Unknown | Stale
    pub finality: CurveFinality,         // Speculative | Provisional | Finalized
    pub curve_data_known: bool,          // parser potwierdził dane
    pub price_sample_count: u32,         // liczba punktów cenowych
    pub t0_event_ts_ms: Option<u64>,     // timestamp pierwszego eventu krzywej
    pub wait_elapsed_ms: Option<u64>,    // ile ms minęło od t0
}
```

CurveReadiness jest budowane z:
- **AccountStateCore:** `curve_finality`, `state_phase` (Canonical/Migrated = final)
- **ShadowLedger:** `curve_data_known`, `price_sample_count`
- **GatekeeperBuffer:** `curve_t0_event_ts_ms`, `wait_elapsed_ms`

### 9.2. GatekeeperBuffer.current_curve_dynamics()

Lokalizacja: `gatekeeper.rs:3195-3211`

Zwraca `BondingCurveDynamics` używany do uzupełnienia `MaterializedFeatureSet` w sesji:
```rust
pub fn current_curve_dynamics(&self) -> BondingCurveDynamics {
    compute_bonding_curve_dynamics(&self.price_history)
}
```

### 9.3. compute_bonding_curve_dynamics()

Lokalizacja: `gatekeeper.rs:2582-2615`

Z `price_history` (Vec<PricePoint>):
1. `initial_price` = pierwszy punkt z ceną
2. `current_price` = ostatni punkt z ceną
3. `max_price` = maksimum z trajektorii
4. `price_change_ratio` = current / initial
5. `max_single_tx_price_impact_pct` = max różnica między sąsiednimi punktami (tylko buy)
6. `max_single_sell_impact_pct` = max różnica między sąsiednimi punktami (gdy przynajmniej jeden sell)
7. `current_market_cap_sol` = market_cap ostatniego punktu
8. `bonding_progress_pct` = bonding_progress ostatniego punktu
9. `price_data_points` = liczba punktów w historii

---

## 10. COMMIT, LIVEPIPELINE, POST-GATEKEEPER

### 10.1. LauncherCommitCoordinator

Lokalizacja: `ghost-launcher/src/components/gatekeeper.rs:193-260`

Po BUY: transakcje przechodzą przez commit pipeline:
```
Pending → Committing → PersistedAwaitingRuntime
```

- `LauncherCommitBuffer` per mint: `buffered_history`, `tx_keys_seen`, `pending_live`
- `add_tx()` → zależnie od fazy: `BufferedHistory` | `PendingLive` | `RouteToLive`

### 10.2. gatekeeper_commit_loop

Lokalizacja: `ghost-launcher/src/components/gatekeeper_commit_loop.rs`

Okresowo (`process_ready_commits`):
1. Bufory w fazie `Pending` → commit do ShadowLedger (`shadow_ledger.commit_history`)
2. Emisja `GhostEvent::gatekeeper_committed`
3. `live_pipeline.init_for_mint(base_mint)` — inicjalizacja LivePipeline
4. Replay `pending_live` TX

### 10.3. Post-Gatekeeper

Po werdykcie BUY, bez sprzężenia zwrotnego:
- **Trigger:** konstrukcja transakcji, symulacja, wysyłka (lub shadow)
- **Post-buy runtime / Guardian / AEM:** monitoring pozycji, exit strategy
- **HyperPrediction Oracle:** scoring post-Gatekeeper (niezależny konsument stanu)
- **DecisionLogger:** JSONL schema v16, logowanie wszystkich decyzji

---

## 11. MAPA PLIKÓW

### Launcher — Gatekeeper i sesja

| Plik | Kluczowe funkcje/struktury | Zakres linii |
|------|---------------------------|-------------|
| `ghost-launcher/src/components/gatekeeper.rs` | `GatekeeperBuffer`, `GatekeeperAssessment`, `GatekeeperDecision`, `GatekeeperVerdictType`, `PricePoint`, `BondingCurveDynamics`, `ShadowV25Decision`, `run_assessment()`, `compute_decision()`, `try_shadow_evaluate()`, `evaluate_from_features()`, `check_long_deadline()`, `materialize_trajectory()`, `v25_confidence()`, `LauncherCommitCoordinator`, `ingest_transaction_tracking_only()` | 1-11879 |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | `build_assessment_from_features()`, `evaluate_policy_from_assessment()`, `evaluate_hard_filters_from_assessment()`, `evaluate_alpha_gate()`, `evaluate_prosperity_filter()`, `evaluate_curve_gate()`, `materialize_pdd_diagnostics_from_features()`, `build_policy_diagnostics()`, `compute_soft_signals()`, `build_sybil_policy_diagnostics()`, `sybil_combo_veto_reason()`, `compute_momentum()`, `compute_demand()`, `compute_core3_pass()` | 1-2473 |
| `ghost-launcher/src/components/gatekeeper_pdd.rs` | `evaluate_pdd()`, `detect_entry_drift()`, `detect_spike()`, `detect_ramping()`, `detect_whale_concentration()`, `check_reserve_health()`, `detect_flash_crash()`, `PddDiagnostics`, `PddHardFail` | 1-566 |
| `ghost-launcher/src/components/gatekeeper_trajectory.rs` | `score_trajectory()`, `build_segment()`, `compute_tas_modulator()`, `TrajectoryAssessment`, `TrajectorySegment` | 1-207 |
| `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs` | `evaluate_aps()`, `detect_regime()`, `compute_shadow_prosperity_pass()`, `ApsDiagnostics`, `MarketRegime` | 1-397 |
| `ghost-launcher/src/components/gatekeeper_commit_loop.rs` | `run()`, `process_ready_commits()` | cały plik |
| `ghost-launcher/src/components/iwim_veto.rs` | IWIM Veto Gate (poza zakresem tego audytu) | cały plik |
| `ghost-launcher/src/oracle_runtime.rs` | `pool_observation_task()`, `evaluate_feature_driven_terminal_verdict()`, `resolve_feature_trigger_outcome()`, `OracleRuntime`, `RuntimeContext` | 1-22997 |
| `ghost-launcher/src/session/observation.rs` | `PoolObservationSession`, `materialize_features()`, `ingest_transaction()`, `try_checkpoint()`, `current_curve_readiness()` | 1-729 |
| `ghost-launcher/src/components/seer.rs` | Seer komponent, konfiguracja | cały plik |
| `ghost-launcher/src/components/snapshot_listener.rs` | Jedyny writer do AccountStateCore | cały plik |

### ghost-core — struktury danych SSOT

| Plik | Kluczowe struktury |
|------|-------------------|
| `ghost-core/src/checkpoint/types.rs` | `MaterializedFeatureSet`, `CheckpointDerivedFeatures`, `CurveReadinessFeatures`, `AlphaFingerprintFeatures`, `SessionCheckpoint` |
| `ghost-core/src/checkpoint/feature_builder.rs` | `ObservationFeatureBuilder::materialize()` |
| `ghost-core/src/checkpoint/engine.rs` | `CheckpointEngine` |
| `ghost-core/src/account_state_core/types.rs` | `CanonicalPoolState`, `AccountStateUpdate`, `AccountStateFeatures`, `StatePhase`, `BootstrapHints`, `UpdateSource` |
| `ghost-core/src/account_state_core/reducer.rs` | `AccountStateReducer::update_account_state()` |
| `ghost-core/src/tx_intelligence/types.rs` | `TxIntelFeatures`, `SybilResistanceFeatures`, `FundingSourceDiagnostics`, `RiskFlag` |
| `ghost-core/src/shadow_ledger/types.rs` | `MarketSnapshot`, `PriceState`, `PriceReason`, `BuySimulationResult`, `SnapshotBuffer` |
| `ghost-core/src/shadow_ledger/ledger.rs` | `ShadowLedger` |
| `ghost-core/src/shadow_ledger/live_pipeline.rs` | `LivePipeline` |
| `ghost-core/src/shadow_ledger/commit_types.rs` | `CommitResult`, `CommitHistoryStatus` |

### ghost-brain — konfiguracja

| Plik | Kluczowe struktury |
|------|-------------------|
| `ghost-brain/src/config/ghost_brain_config.rs` | `GatekeeperV2Config` (wszystkie progi faz 1-6, hard fails, soft signals, alpha, prosperity, sybil, V2.5 sub-structs), `GatekeeperMode` (Standard/Long) |
| `ghost-brain/src/config/gatekeeper_v25_config.rs` | `GatekeeperV25RolloutConfig`, `DynamicObservationWindowConfig`, `TrajectoryAwareScoringConfig`, `PumpAndDumpDetectorConfig`, `AdaptiveProsperityConfig` |
| `ghost-brain/ghost_brain_config.toml` | Kalibracja produkcyjna v11 — wszystkie wartości progowe |

### off-chain — Seer

| Plik | Funkcja |
|------|---------|
| `off-chain/components/seer/src/grpc_connection.rs` | Połączenie Yellowstone gRPC |
| `off-chain/components/seer/src/ipc.rs` | IPC między procesami |
| `off-chain/components/seer/src/types.rs` | Typy eventów Seer |
| `off-chain/components/seer/src/lib.rs` | Start serwisu Seer |

---

## Podsumowanie dla operatora

1. **SSOT:** `MaterializedFeatureSet` budowany w `PoolObservationSession::materialize_features()` (observation.rs:368) z 5+ źródeł: AccountStateCore, TxIntelligenceEngine, CheckpointEngine, EarlyFingerprint, CrossPoolVelocityIndex. Żadna cecha nie jest liczona dwukrotnie.

2. **Cena płynie trzema ścieżkami:** (a) Yellowstone → AccountStateReducer → AccountStateFeatures.price_sol → MaterializedFeatureSet, (b) GatekeeperBuffer.price_history → BondingCurveDynamics, (c) ShadowLedger → MarketSnapshot.price_sol_per_token (dla scoringu post-Gatekeeper).

3. **Produkcyjna ścieżka terminacji:** `evaluate_feature_driven_terminal_verdict()` → `session.materialize_features()` → `buffer.evaluate_from_features()` → `build_assessment_from_features()` → `evaluate_policy_from_assessment()`.

4. **Decyzja to 8 warstw:** Hard Fails → PDD Live Veto → Core Fail → Sybil Combo Veto → Sybil Soft Excess → Legacy Soft Excess → Alpha Gate → Prosperity Filter. Po przejściu: TAS modulacja GatekeeperStrength.

5. **V2.5 shadow-first:** `live_execution_enabled = false` — DOW, TAS, PDD, APS działają w shadow checkpointach (try_shadow_evaluate, check_long_deadline). Live wymaga ADR + promotion.

6. **Dwie ścieżki ewaluacji są świadomym dualizmem:** feature-driven (kanoniczna, używa MaterializedFeatureSet) i buforowa (shadow, używa run_assessment). Testy regresji pilnują synchronizacji.

7. **PDD feature-driven obsługuje 3/6 sygnałów** (drift, whale, reserve) — spike, ramping, flash crash wymagają bufora (pełne PDD w `evaluate_pdd`). Świadome ograniczenie przy obecnym `live_execution_enabled=false`.

---

*Koniec dokumentu — pełny trace z kodu źródłowego, każda funkcja, każda struktura, każda ścieżka.*
