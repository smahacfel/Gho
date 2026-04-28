# PLAN WYKONAWCZY: Event-Triggered Session Hybrid — Pełny Refaktor Architektury

> **Model docelowy**: `tx-bootstrap / account-state core / tx-intelligence sidecar / Gatekeeper policy verdict`
>
> **Zasada naczelna**: Każdy PR musi kompilować się, przechodzić istniejące testy i być deployowalny niezależnie. Nowe ścieżki działają równolegle z istniejącymi, aż do momentu migracji.

---

## A. Docelowe invariants architektury

Poniższe reguły opisują **stan końcowy po zakończeniu migracji (po PR 8)**. W trakcie migracji obowiązują reguły przejściowe z sekcji B.

| # | Invariant docelowy | Weryfikacja po PR 8 |
|---|--------|-------------|
| 1 | `AccountStateCore` jest jedynym source of truth dla canonical market state | Żadna inna struktura nie może być odpytywana jako kanoniczny stan rynku |
| 2 | `TxIntelligenceEngine` nigdy nie zapisuje bezpośrednio do canonical state | Brak `&mut AccountStateCore` w sygnaturze jakiejkolwiek metody TxIntelligence |
| 3 | Gatekeeper policy layer nie ma bezpośredniego dostępu do surowych tx ani account updates; konsumuje wyłącznie `MaterializedFeatureSet` | Gatekeeper nie importuje `PoolTransaction` w nowej ścieżce |
| 4 | `ShadowLedger` nigdy nie jest używany jako live market truth | Żaden hot-path query nie odpytuje ShadowLedger o kanoniczny stan |
| 5 | Każda pool od narodzin dostaje własną `PoolObservationSession` | 1 pool = 1 sesja; brak rozlanego per-pool state |
| 6 | Każda decyzja Gatekeepera musi być odtwarzalna z checkpointów i feature snapshotów | Checkpoint + FeatureSet persisted przy każdym werdykcie |
| 7 | Stan ma być redukowany przyrostowo, nie przeliczany od zera | `AccountStateCore` akumuluje delty, nie rebuild-uje |
| 8 | `FeatureBuilder`/`CheckpointEngine` budują cechy; Gatekeeper interpretuje je wyłącznie jako politykę i progi | Gatekeeper nie wywołuje żadnej funkcji obliczającej surowe feature'y |
| 9 | Canonical state aktualizuje się tylko według jawnego monotonicznego porządku zdarzeń | `MonotonicUpdateGuard` w `AccountStateCore` |
| 10 | Bootstrap/pending state nigdy nie może być mylony z canonical live state | Osobne typy/flagi: `BootstrapState` vs `CanonicalState` |
| 11 | Simulation state nigdy nie może nadpisywać canonical state | `ShadowLedger` i simulation paths operują na kopiach, nie na referencji do AccountStateCore |

## B. Reguły przejściowe migracji

Te reguły obowiązują **od momentu ich wprowadzenia** w ramach migracji równoległej i definiują granicę między dozwolonym legacy-debt a niedozwolonym rozszerzaniem tego długu.

| Obowiązuje od | Reguła przejściowa |
|---|---|
| PR 1 | Każdy nowy typ docelowy musi respektować granice crate'ów (ghost-core nie zależy od ghost-launcher) |
| PR 2 | AccountStateCore działa **równolegle** z istniejącą ścieżką reconciliation. Istniejący kod odpytujący ShadowLedger jako truth nadal działa, ale żaden **nowy** canonical state query nie może iść do ShadowLedger |
| PR 3 | Istniejące `PerPoolOracleState` i `GatekeeperBuffer` mogą nadal współistnieć jako embedded adaptery w sesji |
| PR 4 | Od tego momentu nie wolno dodawać **nowych** zależności Gatekeeper policy path od surowych tx/account updates. Istniejące legacy accesses są dozwolonym długiem do usunięcia w PR 6–8 |
| PR 5 | Gatekeeper legacy path może nadal liczyć features wewnętrznie, ale nowa ścieżka `evaluate_from_features()` nie może tego rozszerzać |
| PR 6 | Legacy path Gatekeepera oznaczony `#[deprecated]`. Nowy policy path jest preferowany. Od tego momentu policy path **nie może** dotykać raw tx |
| PR 7 | ShadowLedger nie może być queried as truth w nowych ścieżkach kodu. Legacy hotpath z ShadowLedger enrichment oznaczony deprecated. ReconciliationRuntime przechodzi na diagnostic-only (bez repair actions) |
| PR 8 | Usunięcie wszystkich deprecated legacy paths. Wszystkie invariants docelowe egzekwowane |

---

## Mapa komponentów: Stan obecny → Cel docelowy

```
OBECNY KOMPONENT              →   ROLA DOCELOWA
─────────────────────────────────────────────────────────────────
Seer (tx path)                →   BirthDetector + TransactionIngress (trigger + tx feed)
Seer (account path)           →   AccountUpdateIngress (primary canonical state feed)
OracleRuntime                 →   SessionManager + event router
PerPoolOracleState            →   → wchłonięty przez PoolObservationSession
GatekeeperBuffer              →   → wchłonięty przez PoolObservationSession
pool_observation_task          →   → PoolObservationSession lifecycle executor
ShadowLedger                  →   Simulation / WAL / Replay / Forensics substrate
SnapshotEngine                →   Derived snapshot / downstream staging engine (stopniowa degradacja z source-of-truth, nie natychmiastowa)
SnapshotListener              →   Staging/forwarding layer dla tx snapshot pipeline
LivePipeline                  →   Post-commit live append layer (bez zmian roli)
ReconciliationRuntime         →   Drift detection / health monitoring (do PR 7 legacy semantics, od PR 7 diagnostic-only)
GatekeeperCommitLoop          →   Commit / handoff layer (bez zmian roli)
Gatekeeper (scoring + policy) →   → rozbity na: FeatureBuilder + HardFilterEngine + VerdictEngine
WAL                           →   Bez zmian roli (durability layer)
```

---

## Granice crate'ów — decyzja architektoniczna

Kluczowy constraint: **ghost-core nie może zależeć od typów z ghost-launcher**.

Weryfikacja obecnego stanu:
- `GatekeeperAssessment` → `ghost-launcher/src/components/gatekeeper.rs:834`
- `PoolTransaction` → `ghost-launcher/src/events.rs:114`
- `DetectedPool` → `ghost-launcher/src/events.rs:82`
- `EnhancedCandidate` → `ghost-core/src/enhanced_candidate.rs:15` (already in ghost-core)

**Decyzja**: Podział odpowiedzialności między crate'y:

| Moduł | Crate | Uzasadnienie |
|-------|-------|-------------|
| `account_state_core/` | **ghost-core** | Nie zależy od ghost-launcher typów. Operuje na `Pubkey`, `u64`, `CurveFinality` (wszystkie w ghost-core) |
| `session/types.rs` (czyste typy: SessionId, SessionStatus, SessionMetadata, diagnostics) | **ghost-core** | Niezależne od launcher typów |
| `session/lifecycle.rs` (SessionManager, PoolObservationSession z tx_buffer) | **ghost-launcher** | Zależy od `PoolTransaction`, `GatekeeperBuffer` |
| `tx_intelligence/types.rs` (TxIntelligenceState, TxIntelFeatures, RiskFlag) | **ghost-core** | Czyste data types, zero zależności od launcher |
| `tx_intelligence/engine.rs` (TxIntelligenceEngine z on_transaction) | **ghost-launcher** | Zależy od `PoolTransaction` (input) |
| `checkpoint/types.rs` (SessionCheckpoint, MaterializedFeatureSet, CheckpointDerivedFeatures) | **ghost-core** | Czyste data types |
| `checkpoint/engine.rs, feature_builder.rs` | **ghost-core** | Operują na ghost-core typach (AccountStateFeatures, TxIntelFeatures) |
| `gatekeeper_policy.rs` (HardFilterEngine, VerdictEngine) | **ghost-launcher** | Blisko istniejącej logiki Gatekeepera |

**Efekt**: ghost-core zawiera czyste domain types i AccountStateCore reducer. ghost-launcher zawiera runtime logic wymagający launcher typów (PoolTransaction, session lifecycle, tx intelligence engine).

---

## PR 1: Kontrakty, typy i fundamenty nowej architektury

### Cel
Wprowadzenie wszystkich nowych typów, traitów i struktur danych wymaganych przez docelową architekturę. Czysto addytywny PR — zero zmian w istniejącym kodzie, zero łamania kompilacji. Respektuje granice crate'ów.

### Zakres prac

#### 1.1 AccountStateCore — typy i trait (ghost-core)

**Plik: `ghost-core/src/account_state_core/mod.rs`**

```rust
pub mod types;
pub mod reducer;
pub mod monotonic_guard;
```

**Plik: `ghost-core/src/account_state_core/types.rs`**

Definicja struktury `CanonicalPoolState`:
- `pool_amm_id: Pubkey`
- `base_mint: Pubkey`
- `bonding_curve: Pubkey`
- `virtual_sol_reserves: u64`
- `virtual_token_reserves: u64`
- `real_sol_reserves: u64`
- `real_token_reserves: u64`
- `bonding_curve_progress: f64` (0.0–1.0)
- `price_sol: f64` (derived: v_sol / v_token)
- `market_cap_sol: f64`
- `token_total_supply: u64`
- `is_complete: bool` (curve migrated)
- `last_update_slot: u64`
- `last_update_ts_ms: u64`
- `curve_finality: CurveFinality`
- `state_phase: StatePhase` {`Bootstrap`, `PendingConfirmation`, `Canonical`, `Migrated`}
- `update_count: u64` (monotonic counter)

Definicja `AccountStateUpdate` (input event):
- `pool_amm_id: Pubkey`
- `base_mint: Pubkey`
- `bonding_curve: Pubkey`
- `sol_reserves: u64`
- `token_reserves: u64`
- `is_complete: u8`
- `slot: u64`
- `receive_ts_ms: u64`
- `receive_seq: u64` (local monotonic counter — tie-breaker for same-slot updates)
- `curve_finality: CurveFinality`
- `source: UpdateSource` {`GeyserAccountUpdate`, `RpcPoll`, `WalReplay`}

Definicja `StatePhase` enum:
- `Bootstrap` — stan spekulatywny z CREATE tx, nie potwierdzone on-chain
- `PendingConfirmation` — zainicjalizowany, oczekuje na pierwszy account update
- `Canonical` — potwierdzony przez account update (live truth)
- `Migrated` — pool zakończył bonding curve, nie do obserwacji

Definicja `AccountStateFeatures` (output features for Gatekeeper):
- `current_reserves: (u64, u64)`
- `price_sol: f64`
- `market_cap_sol: f64`
- `bonding_progress: f64`
- `price_change_since_t0_pct: f64`
- `reserve_velocity_sol_per_sec: f64`
- `is_bootstrap: bool`
- `curve_finality: CurveFinality`
- `state_phase: StatePhase`
- `update_count: u64`

**Plik: `ghost-core/src/account_state_core/monotonic_guard.rs`**

`MonotonicUpdateGuard`:
- `last_accepted_slot: u64`
- `last_accepted_recv_seq: u64`
- `fn accept(&mut self, slot: u64, recv_seq: u64) -> bool`:
  - Akceptuje jeśli `slot > last_accepted_slot`
  - Jeśli `slot == last_accepted_slot`: akceptuje jeśli `recv_seq > last_accepted_recv_seq`
  - Odrzuca w pozostałych przypadkach
- **Minimalna wersja (PR 1-2)**: `(slot, recv_seq)` composite key
- **Docelowa wersja (PR 8+)**: jeśli Geyser zacznie dostarczać `write_version`, rozszerzenie na `(slot, write_version, recv_seq)` — dziś `write_version` nie jest parsowane w Seer

#### 1.2 Session — czyste typy (ghost-core)

**Plik: `ghost-core/src/session/mod.rs`**

```rust
pub mod types;
```

**Plik: `ghost-core/src/session/types.rs`**

Definicja `SessionId` (newtype: `u64` — monotonic counter).

Definicja `SessionStatus` enum:
- `Created` — sesja założona, oczekuje na dane
- `Accumulating` — zbiera tx i account updates
- `Evaluating` — okno obserwacyjne zamknięte, trwa ewaluacja
- `Decided(VerdictOutcome)` — werdykt wydany
- `Closed` — sesja zakończona, stan zarchiwizowany

Definicja `VerdictOutcome` enum (niezależny od GatekeeperAssessment — to jest ghost-core typ):
- `Pass { reason: String }`
- `Fail { reason: String }`
- `Timeout { reason: String }`

> **Uwaga**: `VerdictOutcome` celowo NIE referencuje `GatekeeperAssessment` (ghost-launcher type). Mapowanie verdict → assessment odbywa się po stronie ghost-launcher.

Definicja `SessionMetadata`:
- `session_id: SessionId`
- `pool_amm_id: Pubkey`
- `base_mint: Pubkey`
- `observation_duration_ms: u64`
- `is_dev_known: bool`

Definicja `SessionDiagnostics`:
- `total_tx_seen: u64`
- `total_account_updates: u64`
- `checkpoint_count: u32`
- `first_tx_ts_ms: Option<u64>`
- `last_tx_ts_ms: Option<u64>`
- `reject_reasons: Vec<String>`

#### 1.3 TxIntelligence — czyste typy (ghost-core)

**Plik: `ghost-core/src/tx_intelligence/mod.rs`**

```rust
pub mod types;
```

**Plik: `ghost-core/src/tx_intelligence/types.rs`**

Definicja `TxIntelligenceState` (per-session state):
- `total_buys: u64`
- `total_sells: u64`
- `total_tx: u64`
- `unique_signers: HashSet<Pubkey>`
- `buy_volume_sol: f64`
- `sell_volume_sol: f64`
- `dev_buy_lamports: u64`
- `dev_has_sold: bool`
- `dev_tx_count: u64`
- `signer_volume_map: HashMap<Pubkey, f64>` (SOL volume per signer)
- `tx_intervals_ms: Vec<u64>` (interwały między kolejnymi tx)
- `burst_windows: Vec<BurstWindow>`
- `bundle_suspicion_count: u64`
- `same_ms_tx_count: u64`
- `dust_tx_count: u64` (tx < threshold)

Definicja `TxIntelFeatures` (output features):
- `tx_count: u64`
- `buy_count: u64`
- `sell_count: u64`
- `unique_signers: u64`
- `buy_ratio: f64`
- `sol_buy_ratio: f64`
- `avg_tx_sol: f64`
- `volume_cv: f64`
- `hhi: f64`
- `volume_gini: f64`
- `unique_signer_ratio: f64`
- `avg_tx_per_signer: f64`
- `same_ms_tx_ratio: f64`
- `bundle_suspicion_ratio: f64`
- `top3_volume_pct: f64`
- `dev_buy_sol: f64`
- `dev_volume_ratio: f64`
- `dev_tx_ratio: f64`
- `dev_has_sold: bool`
- `interval_cv: f64`
- `timing_entropy: f64`
- `avg_interval_ms: f64`
- `burst_ratio: f64`
- `dust_ratio: f64`

Definicja `BurstWindow`:
- `start_ts_ms: u64`
- `end_ts_ms: u64`
- `tx_count: u64`

Definicja `RiskFlag`:
- `flag_id: &'static str`
- `severity: RiskSeverity` {`Hard`, `Soft(u8)`}
- `detected_at_ms: u64`
- `detail: String`

#### 1.4 Checkpoint — typy i traity (ghost-core)

**Plik: `ghost-core/src/checkpoint/mod.rs`**

```rust
pub mod types;
pub mod traits;
```

**Plik: `ghost-core/src/checkpoint/types.rs`**

Definicja `SessionCheckpoint`:
- `checkpoint_id: u32` (monotonic per session)
- `timestamp_ms: u64`
- `trigger: CheckpointTrigger` {`TimeBased(u64)`, `EventBased(String)`}
- `account_state_snapshot: AccountStateFeatures`
- `tx_intel_snapshot: TxIntelFeatures`
- `risk_flags: Vec<RiskFlag>`

Definicja `MaterializedFeatureSet` (input do Gatekeeper):
- `account_features: AccountStateFeatures`
- `tx_intel_features: TxIntelFeatures`
- `checkpoint_features: CheckpointDerivedFeatures`
- `risk_flags: Vec<RiskFlag>`
- `session_metadata: SessionMetadata`

Definicja `CheckpointDerivedFeatures` (trajectory analysis):
- `price_trajectory: Vec<f64>`
- `reserve_trajectory: Vec<(u64, u64)>`
- `buy_pressure_trend: TrendDirection` {`Rising`, `Falling`, `Stable`, `Insufficient`}
- `signer_diversity_trend: TrendDirection`
- `risk_flag_count_trend: TrendDirection`
- `trajectory_checkpoint_count: u32`

**Plik: `ghost-core/src/checkpoint/traits.rs`**

```rust
pub trait CheckpointProducer {
    fn should_checkpoint(&self, now_ms: u64, last_checkpoint_ms: u64) -> bool;
    fn create_checkpoint(
        &self,
        account_features: &AccountStateFeatures,
        tx_intel_features: &TxIntelFeatures,
        risk_flags: &[RiskFlag],
    ) -> SessionCheckpoint;
}

pub trait FeatureMaterializer {
    fn materialize(
        &self,
        account_features: AccountStateFeatures,
        tx_intel_features: TxIntelFeatures,
        checkpoints: &[SessionCheckpoint],
        risk_flags: Vec<RiskFlag>,
        metadata: SessionMetadata,
    ) -> MaterializedFeatureSet;
}
```

#### 1.5 Reeksporty i integracja z ghost-core/src/lib.rs

- Dodanie `pub mod account_state_core;`
- Dodanie `pub mod session;`
- Dodanie `pub mod tx_intelligence;`
- Dodanie `pub mod checkpoint;`

#### 1.6 Testy jednostkowe

- `MonotonicUpdateGuard` — test monotoniczności z same-slot tie-breaking
- `StatePhase` — test poprawności przejść
- Serializacja/deserializacja nowych typów
- `MaterializedFeatureSet` — test kompletności danych wejściowych

### Pliki nowe (ghost-core)
- `ghost-core/src/account_state_core/mod.rs`
- `ghost-core/src/account_state_core/types.rs`
- `ghost-core/src/account_state_core/reducer.rs` (placeholder)
- `ghost-core/src/account_state_core/monotonic_guard.rs`
- `ghost-core/src/session/mod.rs`
- `ghost-core/src/session/types.rs`
- `ghost-core/src/tx_intelligence/mod.rs`
- `ghost-core/src/tx_intelligence/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/traits.rs`

### Pliki modyfikowane
- `ghost-core/src/lib.rs` — dodanie nowych modułów

### Weryfikacja reguł
- Reguła przejściowa PR 1: Wszystkie nowe typy w ghost-core nie zależą od ghost-launcher
- Invariant #9: `MonotonicUpdateGuard` zdefiniowany z `(slot, recv_seq)` composite key
- Invariant #10: `StatePhase::Bootstrap` vs `StatePhase::Canonical` jawnie rozróżnione

---

## PR 2: AccountStateCore — implementacja reducera i wejście account path

### Cel
Pełna implementacja `AccountStateCore` jako przyrostowego reducera stanu rynku. Account updates z Seera zaczynają zasilać AccountStateCore **równolegle** z istniejącą ścieżką reconciliation (dual-write). Istniejąca logika nie jest jeszcze zmieniana — nowa ścieżka działa obok.

### Zakres prac

#### 2.1 AccountStateReducer — implementacja (ghost-core)

**Plik: `ghost-core/src/account_state_core/reducer.rs`**

`AccountStateReducer`:
- `states: DashMap<Pubkey, CanonicalPoolState>` — per-mint kanoniczny stan
- `update_guards: DashMap<Pubkey, MonotonicUpdateGuard>` — per-mint monotoniczność
- `bootstrap_states: DashMap<Pubkey, BootstrapPoolState>` — stan tymczasowy z CREATE tx
- `recv_seq_counter: AtomicU64` — globalny monotoniczny counter dla `receive_seq`

Metody:
- `fn register_pool_from_bootstrap(&self, pool_id, base_mint, bonding_curve, hints: BootstrapHints) -> ()`
  - Tworzy `BootstrapPoolState` w fazie `Bootstrap`
  - NIE tworzy `CanonicalPoolState` — to czeka na account update (invariant #10)
  - `BootstrapHints`: opcjonalne dane z `EnhancedCandidate` (nie `DetectedPool` bezpośrednio)
- `fn apply_account_update(&self, update: AccountStateUpdate) -> AccountUpdateResult`
  - Sprawdza `MonotonicUpdateGuard` — odrzuca stare/duplicate eventy (invariant #9)
  - Jeśli `BootstrapState` istnieje ale nie ma jeszcze `CanonicalState` → promuje do `Canonical`
  - Aktualizuje reserves, recalkuluje price, market_cap, bonding_progress
  - Aktualizuje `update_count` (przyrostowo, invariant #7)
  - Zwraca `AccountUpdateResult` {`Applied`, `Rejected(reason)`, `PromotedFromBootstrap`}
- `fn get_canonical_state(&self, mint: &Pubkey) -> Option<CanonicalPoolState>`
- `fn get_features(&self, mint: &Pubkey) -> Option<AccountStateFeatures>`
- `fn is_canonical(&self, mint: &Pubkey) -> bool`
- `fn next_recv_seq(&self) -> u64` — atomowo inkrementuje i zwraca recv_seq
- `fn remove_pool(&self, mint: &Pubkey)`

`BootstrapPoolState` (oddzielna struktura — invariant #10):
- `pool_amm_id: Pubkey`
- `base_mint: Pubkey`
- `bonding_curve: Pubkey`
- `speculative_reserves: Option<(u64, u64)>` — opcjonalne, z `EnhancedCandidate` jeśli dostępne
- `token_total_supply: Option<u64>` — opcjonalne, z `EnhancedCandidate` jeśli dostępne
- `bonding_curve_progress: Option<f64>` — opcjonalne, z `EnhancedCandidate` jeśli dostępne
- `created_at_ms: u64`

`BootstrapHints` (opcjonalne dane z `EnhancedCandidate`):
- `speculative_reserves: Option<(u64, u64)>`
- `token_total_supply: Option<u64>`
- `bonding_curve_progress: Option<f64>`
- `initial_liquidity_sol: Option<f64>`

> **Uwaga**: `DetectedPool` (events.rs:82) nie zawiera `bonding_curve_progress` ani `token_total_supply`. Te dane są dostępne przez `EnhancedCandidate` (ghost-core/src/enhanced_candidate.rs:39,45) jako `Option<f64>` i `Option<u64>`. Bootstrap state musi traktować je jako opcjonalne hinty, nie wymagane pola. Bootstrap jest identity-first, optional-hints-second, canonical dopiero po pierwszym account update.

#### 2.2 Wejście account path — dual-write (ghost-launcher)

**Plik: `ghost-launcher/src/oracle_runtime.rs`** — modyfikacja

W sekcji obsługi `GhostEvent::AccountUpdate`:
- **Istniejąca ścieżka**: `oracle_runtime.process_account_update(...)` → `ReconciliationRuntime` (bez zmian)
- **Nowa ścieżka (równoległa)**: `account_state_core.apply_account_update(AccountStateUpdate { ..., recv_seq: account_state_core.next_recv_seq() })`
- Obie ścieżki działają jednocześnie
- Metryki: `counter!("account_state_core.updates_applied")`, `counter!("account_state_core.updates_rejected")`

W sekcji obsługi `GhostEvent::NewPoolDetected`:
- **Nowa ścieżka**: `account_state_core.register_pool_from_bootstrap(...)` z danymi z `EnhancedCandidate`
- `EnhancedCandidate` jest budowany z `DetectedPool` w momencie rejestracji (istniejący flow)
- Rejestruje bootstrap state, który będzie promowany po pierwszym account update

**Plik: `ghost-launcher/src/oracle_runtime.rs`** — dodanie pola

`OracleRuntime`:
- Nowe pole: `account_state_core: Arc<AccountStateReducer>`
- Inicjalizacja w konstruktorze

#### 2.3 Metryki i obserwabilność

- `account_state_core.pools_bootstrap` — gauge
- `account_state_core.pools_canonical` — gauge
- `account_state_core.slot_rejections` — counter
- `account_state_core.recv_seq_rejections` — counter (same-slot tie-break rejections)
- `account_state_core.bootstrap_promotions` — counter
- `account_state_core.update_latency_us` — histogram

#### 2.4 Testy

- Test monotoniczności: update ze starym slotem jest odrzucany
- Test same-slot tie-breaking: dwa updaty w tym samym slocie — drugi z wyższym recv_seq akceptowany
- Test promoci: bootstrap → canonical lifecycle
- Test separacji: bootstrap state nie jest widoczny przez `get_canonical_state`
- Test optional bootstrap hints: bootstrap z None na wszystkich hintach działa poprawnie
- Test concurrent access: wiele wątków aktualizuje różne poole jednocześnie
- Test feature computation: `get_features()` zwraca poprawne wartości pochodne
- Integration test: symulacja strumienia account updates

### Pliki nowe
- `ghost-core/src/account_state_core/reducer.rs` (pełna implementacja)
- `ghost-core/tests/account_state_core_tests.rs`

### Pliki modyfikowane
- `ghost-launcher/src/oracle_runtime.rs` — dodanie `account_state_core` + dual-write w event handlerach
- `ghost-core/src/account_state_core/types.rs` — ewentualne doprecyzowania typów

### Weryfikacja reguł
- Invariant #7: Reducer akumuluje delty, nie rebuild-uje
- Invariant #9: MonotonicUpdateGuard z `(slot, recv_seq)` aktywny
- Invariant #10: Bootstrap oddzielony od Canonical, hinty opcjonalne
- Reguła przejściowa PR 2: dual-write, istniejący ShadowLedger truth path nienaruszony

---

## PR 3: PoolObservationSession — zunifikowana sesja per-pool

### Cel
Wprowadzenie `PoolObservationSession` jako jednego spójnego kontenera per-pool, który zastąpi rozproszone `PerPoolOracleState`, `GatekeeperBuffer`, per-pool timing state i observation context. Sesja staje się „mózgiem operacyjnym" pojedynczej obserwacji (invariant #5).

### Zakres prac

#### Etap 3A: Definicja sesji i SessionManager (nowe struktury obok istniejących)

##### 3A.1 SessionManager (ghost-launcher)

**Plik: `ghost-launcher/src/session/mod.rs`** (nowy moduł w ghost-launcher)
**Plik: `ghost-launcher/src/session/manager.rs`**

`SessionManager`:
- `sessions: DashMap<Pubkey, Arc<RwLock<PoolObservationSession>>>`
- `session_counter: AtomicU64`
- `config: SessionConfig`

Metody:
- `fn open_session(&self, pool_id, base_mint, bonding_curve, dev_wallet, deadline_ms) -> SessionId`
- `fn get_session(&self, pool_id: &Pubkey) -> Option<Arc<RwLock<PoolObservationSession>>>`
- `fn close_session(&self, pool_id: &Pubkey, verdict: VerdictOutcome)`
- `fn remove_session(&self, pool_id: &Pubkey)`

##### 3A.2 PoolObservationSession (ghost-launcher)

**Plik: `ghost-launcher/src/session/observation.rs`**

`PoolObservationSession`:
- `session_id: SessionId`
- `pool_amm_id: Pubkey`
- `base_mint: Pubkey`
- `bonding_curve: Pubkey`
- `dev_wallet: Option<Pubkey>`
- `candidate_snapshot: EnhancedCandidate`
- `created_at_wall_ms: u64`
- `created_at_instant: Instant`
- `deadline_wall_ms: u64`
- `status: SessionStatus`
- `tx_buffer: Vec<Arc<PoolTransaction>>`
- `tx_keys_seen: HashSet<TxKey>`
- `highest_seen_ts_ms: u64`
- `gatekeeper_buffer: GatekeeperBuffer` (embedded, legacy compatibility)
- `diagnostics: SessionDiagnostics`
- `active_risk_flags: Vec<RiskFlag>`
- `verdict: Option<VerdictOutcome>`

Metody:
- `fn on_transaction(&mut self, tx: Arc<PoolTransaction>) -> bool`
- `fn on_account_update(&mut self, update: &AccountStateUpdate)`
- `fn elapsed_ms(&self) -> u64`
- `fn is_expired(&self, now_wall_ms: u64) -> bool`
- `fn get_status(&self) -> &SessionStatus`

##### 3A.3 Tabela migracji per-pool state

| Obecne źródło | Pole | Nowe miejsce w sesji |
|---------------|------|---------------------|
| `PerPoolOracleState.pool_amm_id` | identyfikator | `session.pool_amm_id` |
| `PerPoolOracleState.base_mint` | mint | `session.base_mint` |
| `PerPoolOracleState.candidate` | metadata | `session.candidate_snapshot` |
| `PerPoolOracleState.dev_wallet` | dev | `session.dev_wallet` |
| `PerPoolOracleState.scored` | flag | `session.status == Decided` |
| `PerPoolOracleState.start_time` | timing | `session.created_at_instant` |
| `GatekeeperBuffer.registered_wall_ts_ms` | t0 | `session.created_at_wall_ms` |
| `GatekeeperBuffer.deadline_wall_ts_ms` | deadline | `session.deadline_wall_ms` |
| `GatekeeperBuffer.buffered_txs` | tx buffer | `session.tx_buffer` |
| `GatekeeperBuffer.tx_keys_seen` | dedup | `session.tx_keys_seen` |
| `GatekeeperBuffer.highest_seen_ts` | timing | `session.highest_seen_ts_ms` |
| `GatekeeperBuffer.first_tx_ts` | timing | `session.diagnostics.first_tx_ts_ms` |

#### Etap 3B: Przepięcie pool_observation_task na sesje

##### 3B.1 Modyfikacja pool_observation_task

**Plik: `ghost-launcher/src/oracle_runtime.rs`** — modyfikacja `pool_observation_task()`

Zamiast tworzyć osobno `GatekeeperBuffer` i `PerPoolOracleState`, task:
1. Otwiera `SessionManager::open_session(...)` → dostaje `SessionId`
2. Pobiera `Arc<RwLock<PoolObservationSession>>`
3. W pętli accumulation:
   - `session.on_transaction(tx)` — deleguje do embedded `GatekeeperBuffer`
4. Przy werdykcie:
   - `session_manager.close_session(pool_id, verdict)`
5. Przy cleanup:
   - `session_manager.remove_session(pool_id)`

**Ważne**: W tym PR `GatekeeperBuffer` nadal istnieje wewnątrz sesji jako embedded struct i wykonuje scoring. Sesja jest kontenerem unifikującym, nie zmianą logiki decyzyjnej. Logika scoring/policy zostanie wyekstrahowana dopiero w PR 6.

##### 3B.2 Kompatybilność wsteczna

- `PerPoolOracleState` zostaje jako thin wrapper delegujący do sesji (deprecated, do usunięcia w PR 8)
- `GatekeeperBuffer` embedded w sesji, zachowuje API
- Testy istniejące przechodzą bez zmian

#### 3.3 Testy

- Test lifecycle: Created → Accumulating → Evaluating → Decided → Closed
- Test dedup: duplikaty tx nie zwiększają countera
- Test timing: `elapsed_ms()` i `is_expired()` działają poprawnie
- Test session isolation: dwie sesje nie interferują ze sobą
- Test cleanup: `remove_session` zwalnia zasoby

### Pliki nowe (ghost-launcher)
- `ghost-launcher/src/session/mod.rs`
- `ghost-launcher/src/session/manager.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/tests/session_lifecycle_tests.rs`

### Pliki modyfikowane
- `ghost-launcher/src/oracle_runtime.rs` — integracja SessionManager, modyfikacja pool_observation_task

### Weryfikacja reguł
- Invariant #5: Każda pool dostaje jedną `PoolObservationSession` ze wszystkim wewnątrz
- Reguła przejściowa PR 3: GatekeeperBuffer embedded, legacy compatibility zachowana

---

## PR 4: TxIntelligenceEngine — ekstrakcja warstwy semantyki zachowań

### Cel
Wyekstrahowanie z `GatekeeperBuffer` (i rozlanej logiki w OracleRuntime) całej analizy behawioralnej do osobnej warstwy `TxIntelligenceEngine`. Ta warstwa konsumuje tx feed i produkuje `TxIntelFeatures` oraz `RiskFlag`-i. **Nigdy nie dotyka canonical state** (invariant #2).

### Zakres prac

#### 4.1 TxIntelligenceEngine — implementacja (ghost-launcher)

**Plik: `ghost-launcher/src/tx_intelligence/mod.rs`** (nowy moduł w ghost-launcher)
**Plik: `ghost-launcher/src/tx_intelligence/engine.rs`**

> Żyje w ghost-launcher bo zależy od `PoolTransaction` (ghost-launcher type).

`TxIntelligenceEngine`:
- `state: TxIntelligenceState` (ghost-core type)
- `config: TxIntelligenceConfig`

Implementacja `on_transaction(&mut self, tx: &PoolTransaction)`:
1. **Counting & classification**: buys/sells/tx, volumes, dev detection, signers
2. **Timing analysis** (z Gatekeeper Phase 2): intervals, burst windows, entropy
3. **Manipulation detection** (z Gatekeeper Phase 3): same_ms, HHI, dust
4. **Dev behavior tracking** (z Gatekeeper Phase 5): dev_buy, dev_sold, dev_ratio
5. **Fingerprinting** (z OracleRuntime/Gatekeeper): FingerprintAggregator

`fn compute_features(&self) -> TxIntelFeatures` — oblicza derived features
`fn get_risk_flags(&self) -> Vec<RiskFlag>` — hard/soft risk flags

#### 4.2 Konfiguracja

**Plik: `ghost-launcher/src/tx_intelligence/config.rs`**

`TxIntelligenceConfig`: progi z `GatekeeperV2Config` Phase 2/3/5 przeniesione semantycznie.

#### 4.3 Integracja z PoolObservationSession

**Modyfikacja `PoolObservationSession`:**
- Nowe pole: `tx_intelligence: TxIntelligenceEngine`
- `on_transaction()` deleguje: `self.tx_intelligence.on_transaction(&tx)`

**Domknięcie długu z PR 3 (`tx_buffer`)**:
- `session.tx_buffer` nie może pozostać nieograniczonym `Vec<Arc<PoolTransaction>>` po wprowadzeniu `TxIntelligenceEngine`.
- Po PR 4 transakcje mają być konsumowane **strumieniowo** przez `TxIntelligenceEngine` i dalsze warstwy obserwacji, zamiast budować pełną historię in-memory per sesja.
- Jeśli bufor ostatnich tx nadal jest potrzebny do diagnostyki / replay krótkiego okna, należy zastąpić pełny `Vec` **bounded ring bufferem** (`last N tx`) z jawnym capem konfiguracyjnym lub stałą.
- Pełna retencja wszystkich tx sesji nie jest już akceptowalnym domyślnym zachowaniem po PR 4.

**Ważne**: W tym PR Gatekeeper nadal liczy te same metryki wewnętrznie (legacy compatibility). `TxIntelligenceEngine` liczy je niezależnie. W PR 6 Gatekeeper przestanie liczyć sam.

#### 4.4 Tabela ekstrakcji logiki z Gatekeeper

| Gatekeeper Phase | Logika | Nowe miejsce |
|-----------------|--------|-------------|
| Phase 1 (quantity gate) | tx_count, unique_signers, buy_count | `TxIntelligenceEngine.on_transaction()` |
| Phase 2 (velocity) | interval_cv, timing_entropy, burst_ratio | `TxIntelligenceEngine.on_transaction()` + `compute_features()` |
| Phase 3 (signer diversity) | HHI, Gini, unique ratio, bundle detection | `TxIntelligenceEngine.on_transaction()` + `compute_features()` |
| Phase 4 (volume sanity) | buy_ratio, avg_tx_sol, volume_cv, sol_buy_ratio | `TxIntelligenceEngine.compute_features()` |
| Phase 5 (dev behavior) | dev_buy, dev_sold, dev_ratio | `TxIntelligenceEngine.on_transaction()` + `compute_features()` |

**Pozostaje w Gatekeeper**: Phase 6 (bonding curve — zależy od AccountState), hard fail evaluation, core pass, soft weighting, final verdict.

#### 4.5 Testy

- Unit test: HHI, Gini, CV na syntetycznych danych
- Unit test: `dev_has_sold` flag
- Unit test: burst detection
- Unit test: risk flags
- **Invariant test**: `TxIntelligenceEngine` nie importuje `AccountStateCore`/`AccountStateReducer` — compile-time guarantee
- Integration test: 100 tx stream → porównanie features

### Pliki nowe (ghost-launcher)
- `ghost-launcher/src/tx_intelligence/mod.rs`
- `ghost-launcher/src/tx_intelligence/engine.rs`
- `ghost-launcher/src/tx_intelligence/config.rs`
- `ghost-launcher/tests/tx_intelligence_tests.rs`

### Pliki modyfikowane
- `ghost-launcher/src/session/observation.rs` — dodanie `tx_intelligence` field
- `ghost-launcher/src/oracle_runtime.rs` — sesja akumuluje tx intelligence

### Weryfikacja reguł
- Invariant #2: `TxIntelligenceEngine` NIE importuje `AccountStateCore` — compile-time guarantee
- Reguła przejściowa PR 4: od tego momentu żadne nowe zależności Gatekeeper → raw tx w policy path

---

## PR 5: CheckpointEngine i FeatureBuilder — materializacja obserwacji

### Cel
Implementacja `CheckpointEngine` (tworzenie snapshotów poznawczych w ustalonych punktach sesji) i `ObservationFeatureBuilder` (materializacja końcowego `MaterializedFeatureSet`). Po tym PR system ma pełną warstwę materializacji, niezależną od Gatekeepera (invariant #8).

### Zakres prac

#### 5.1 CheckpointEngine — implementacja (ghost-core)

**Plik: `ghost-core/src/checkpoint/engine.rs`**

`CheckpointEngine` implementuje `CheckpointProducer`:
- `config: CheckpointConfig`
- `checkpoint_counter: u32`

`CheckpointConfig`:
- `interval_ms: u64` (default: 2000)
- `min_tx_between_checkpoints: u64` (default: 5)
- `enable_event_checkpoints: bool` (default: true)
- `event_triggers: Vec<EventCheckpointTrigger>` {`DevSell`, `LargeTradeImpact(f64)`, `SignerCountMilestone(u64)`}

#### 5.2 ObservationFeatureBuilder — implementacja (ghost-core)

**Uwaga**: Istniejący `FeatureBuilder` w `ghost-brain/src/aem/feature_adapter.rs` to zupełnie inny komponent (AEM control adapter). Nowy builder nie koliduje.

**Plik: `ghost-core/src/checkpoint/feature_builder.rs`**

`ObservationFeatureBuilder` implementuje `FeatureMaterializer`:
- Account features — passthrough
- Tx intelligence features — passthrough
- Checkpoint-derived features (trajectory analysis): price/reserve trajectories, trends
- Risk flags — agregacja
- Session metadata — passthrough

#### 5.3 Phase 6 (Bonding Curve) feature extraction

Logika Phase 6 przenoszona do `ObservationFeatureBuilder`:
- `price_change_from_first_checkpoint_pct`
- `single_tx_max_price_impact_pct`
- `bonding_progress`

#### 5.4 Integracja z PoolObservationSession

Nowe pola w `PoolObservationSession`:
- `checkpoint_engine: CheckpointEngine`
- `feature_builder: ObservationFeatureBuilder`
- `checkpoints: Vec<SessionCheckpoint>`

Nowe metody:
- `fn try_checkpoint(&mut self, now_ms: u64)` — periodic/event-driven snapshots
- `fn materialize_features(&self) -> MaterializedFeatureSet` — gotowy feature set

#### 5.5 Integracja z pool_observation_task

W pętli accumulation:
- Po każdym `on_transaction()`: `session.try_checkpoint(now_ms)`
- Przed verdict: `session.materialize_features()`

#### 5.6 Testy

- Unit test: CheckpointEngine tworzy checkpointy co N ms
- Unit test: event-triggered checkpoints
- Unit test: trajectory trends
- Unit test: `materialize()` kompletność
- Integration test: pełna sesja z 4 checkpointami

### Pliki nowe
- `ghost-core/src/checkpoint/engine.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- `ghost-core/tests/checkpoint_engine_tests.rs`
- `ghost-core/tests/feature_builder_tests.rs`

### Pliki modyfikowane
- `ghost-core/src/checkpoint/mod.rs` — dodanie `engine`, `feature_builder`
- `ghost-launcher/src/session/observation.rs` — dodanie checkpoint/feature_builder fields
- `ghost-launcher/src/oracle_runtime.rs` — checkpointowanie w observation task

### Weryfikacja reguł
- Invariant #6: Checkpointy zapisywane systematycznie
- Invariant #8: `FeatureBuilder` buduje cechy; Gatekeeper jeszcze ich nie konsumuje (to PR 6)

---

## PR 6: Gatekeeper → Policy Engine — refaktoryzacja roli decyzyjnej

### Cel
Refaktoryzacja Gatekeepera z monolitu na czysty **policy engine** konsumujący gotowy `MaterializedFeatureSet`. Wydzielenie `HardFilterEngine` i `GatekeeperVerdictEngine`. Od tego PR nowy policy path NIE liczy surowych feature'ów. Legacy path oznaczony `#[deprecated]`.

### Zakres prac

#### 6.1 HardFilterEngine (ghost-launcher)

**Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`** (nowy)

```rust
pub fn evaluate_hard_filters(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> Option<HardFailReason> {
    // HF-1: Dev sold
    if features.tx_intel_features.dev_has_sold { return Some(HardFailReason::DevSold); }
    // HF-2..HF-11: przeniesione z Layer 1
    None
}
```

#### 6.2 GatekeeperVerdictEngine (ghost-launcher)

```rust
pub fn evaluate_policy(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> PolicyVerdict {
    // Hard filters → Core pass (1-3) → Soft signals → verdict
}
```

#### 6.3 Refaktoryzacja GatekeeperBuffer

Nowa metoda:
```rust
pub fn evaluate_from_features(&self, features: MaterializedFeatureSet, config: &GatekeeperV2Config) -> GatekeeperVerdict
```

**Deprecation**: Stara `on_transaction()` z inline scoring oznaczona `#[deprecated]`.

#### 6.4 Wiring w pool_observation_task

```
// Nowy flow:
tx → session.on_transaction(tx) → session.try_checkpoint(now_ms)
     ...deadline hit...
features = session.materialize_features()
verdict = evaluate_policy(&features, &config)
```

#### 6.5 GatekeeperAssessment refaktor

Rozszerzenie o:
- `feature_snapshot: MaterializedFeatureSet` — pełny feature set użyty do decyzji (invariant #6)
- `checkpoint_count: u32`
- `trajectory_available: bool`

Gwarantuje odtwarzalność: `feature_snapshot` + `config` = deterministic verdict replay.

#### 6.6 Usunięcie duplikacji

**Usuwane z nowego policy path**: Phase 1-5 inline computation, Phase 6 curve analysis, wewnętrzne countery.
**Zachowane w legacy path** (deprecated): pełna logika `on_transaction()` dla rollback safety.
**Zachowane zawsze**: timing logic, status machine, integration z `GatekeeperCommitLoop`.

#### 6.7 GatekeeperCommitLoop — bez zmian

Zachowuje obecną rolę commit/handoff. Nie wymaga zmian.

#### 6.8 Testy

- Unit test: HardFilterEngine — każdy hard fail w izolacji
- Unit test: VerdictEngine — core pass, soft signals
- Unit test: deterministic replay — ten sam `MaterializedFeatureSet` + config = ten sam verdict
- Integration test: pełny flow tx → session → features → verdict
- Regression test: porównanie verdicts starej i nowej ścieżki

### Pliki nowe
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`

### Pliki modyfikowane
- `ghost-launcher/src/components/gatekeeper.rs` — deprecation inline metrics, dodanie `evaluate_from_features()`
- `ghost-launcher/src/oracle_runtime.rs` — nowy flow w `pool_observation_task`
- `ghost-launcher/src/components/mod.rs` — reeksport

### Weryfikacja reguł
- Invariant #3: Nowy policy path nie rekonstruuje stanu
- Invariant #8: Features obliczane przez FeatureBuilder, interpretowane przez policy engine
- Invariant #6: `feature_snapshot` w assessment umożliwia replay
- Reguła przejściowa PR 6: legacy path deprecated, nowy policy path nie dotyka raw tx

---

## PR 7: Migracja truth source — AccountStateCore jako jedyna prawda

### Cel
AccountStateCore staje się **jedynym kanonicznym źródłem stanu rynku**. ShadowLedger zostaje zdemotowany do roli symulacyjno-diagnostycznej. Wszystkie hot-path queries o canonical state przekierowane na AccountStateCore.

### Zakres prac

#### 7.1 Przekierowanie canonical state queries

| Wywołanie | Plik | Nowe źródło |
|-----------|------|-------------|
| `shadow_ledger.get_curve(mint)` (enrichment) | `oracle_runtime.rs` | `account_state_core.get_canonical_state(mint)` (primary), ShadowLedger (fallback for bootstrap only) |
| `shadow_ledger.get_quote(mint)` (curve readiness) | `gatekeeper.rs` | `account_state_core.get_features(mint)` |
| `shadow_ledger.simulate_buy/sell(mint)` | Trigger | **Bez zmian** — simulation to zdrowa rola ShadowLedger |

Nowa funkcja `enrich_pool_tx_from_canonical_state()` z ShadowLedger jako fallback wyłącznie dla pooli w stanie Bootstrap.

#### 7.2 Curve readiness w Gatekeeper

- Primary: `account_state_core.is_canonical(mint)` → curve ready
- Fallback: brak Canonical → `PendingCurve`

#### 7.3 ShadowLedger — formalne zawężenie roli

**Dozwolone** (simulation/WAL/forensics): `simulate_buy/sell`, `commit_history`, `append_live`, `snapshot_to_disk/restore_from_disk`, `get_snapshots`

**Deprecated**: `get_curve()`, `get_quote()`, `insert()` z `LegacyCompat`

ShadowLedger nadal zasilany danymi (commit_history, append_live) dla simulation, WAL, forensics, disk snapshots.

#### 7.4 Reconciliation — zmiana semantyki

**Od tego PR**: `ReconciliationRuntime` przechodzi na **diagnostic-only** (drift monitoring między AccountStateCore a ShadowLedger). Nie wykonuje repair actions — AccountStateCore jest już truth.

> **Uwaga**: Do tego PR (PR 2–6) ReconciliationRuntime zachowywał legacy semantics (repair). Zmiana na diagnostic-only następuje tutaj, nie wcześniej.

#### 7.5 SnapshotEngine — clarification

SnapshotEngine zachowuje rolę derived snapshot engine. Nie jest canonical truth. Nie znika z runtime scoring overnight — stopniowa degradacja roli, ale w tym PR nie jest usuwany ze scoring/runtime path. Jest jedynie formalnie udokumentowany jako derived consumption layer.

#### 7.6 Testy

- Integration test: AccountStateCore odpowiada na canonical queries
- Test: enrichment z AccountStateCore (primary) i ShadowLedger (fallback)
- Test: Reconciliation nie repair-uje, tylko loguje drift
- Test: curve readiness latch odpytuje AccountStateCore
- **Invariant test**: grep codebase za bezpośrednimi get_curve/get_quote na hot-path

### Pliki modyfikowane
- `ghost-launcher/src/oracle_runtime.rs` — `enrich_pool_tx_from_canonical_state`, curve readiness
- `ghost-launcher/src/components/gatekeeper.rs` — curve readiness latch
- `ghost-core/src/shadow_ledger/ledger.rs` — deprecation annotations
- `ghost-core/src/shadow_ledger/reconciliation.rs` — drift monitoring only
- `ghost-core/src/shadow_ledger/reconciliation_runtime.rs` — zmiana semantyki

### Weryfikacja reguł
- Invariant #1: AccountStateCore jest jedynym source of truth — teraz egzekwowane
- Invariant #4: ShadowLedger deprecated jako live truth
- Invariant #11: simulation na kopiach, nie na canonical
- Reguła przejściowa PR 7: legacy ShadowLedger enrichment deprecated, Reconciliation diagnostic-only

---

## PR 8: Seer dual-ingest formalizacja, końcowa integracja i cleanup

### Cel
Formalizacja roli Seera jako dual-ingest layer. Pełna integracja end-to-end pipeline. Usunięcie legacy paths, deprecated code i compatibility shims.

### Zakres prac

#### 8.1 Seer account path — formalizacja jako primary

**Plik: `off-chain/components/seer/src/lib.rs`**

Zmiana semantyki `handle_account_update()`:
- **Dawniej**: secondary/repair — `DropNew` backpressure
- **Teraz**: primary canonical state feed — backpressure `Block` lub `DropOldest` z dużym buforem
- Metryki: `seer.account_updates.emitted`, `seer.account_updates.latency_us`

Zmiana semantyki `handle_trade_event()`:
- **Teraz**: trigger + intelligence feed
- Reserves z tx traktowane jako hint (bootstrap), nie canonical

> **Uwaga dotycząca `account_updates_enabled`**: Po zakończeniu migracji account-state path staje się obowiązkowym production path. Flaga `account_updates_enabled` przechodzi z feature-gate migracyjnego do wartości domyślnie wymaganej i docelowo może zostać usunięta. Nie jest to natychmiastowa zmiana — to kierunek, nie nakaz w tym PR.

#### 8.2 IPC priority zmiana

`SeerEvent::AccountUpdate` — zmiana backpressure na critical path.
`SeerEvent::Trade` — bez zmian backpressure.

#### 8.3 End-to-end pipeline verification

```
Faza A — Birth Detection:
  Seer tx path → parse CREATE → SeerEvent::PoolDetected → IPC → EventBus

Faza B — Session Start:
  OracleRuntime → GhostEvent::NewPoolDetected → SessionManager.open_session()
  AccountStateCore.register_pool_from_bootstrap() → BootstrapState

Faza C — Canonical State Tracking:
  Seer account path → SeerEvent::AccountUpdate → IPC → EventBus
  OracleRuntime → GhostEvent::AccountUpdate → AccountStateCore.apply_account_update()
  BootstrapState → Canonical promotion

Faza D — Behavioral Semantics:
  Seer tx path → SeerEvent::Trade → IPC → EventBus
  OracleRuntime → GhostEvent::PoolTransaction → session.on_transaction()
  session.tx_intelligence.on_transaction()

Faza E — Checkpoint + Feature Materialization:
  session.try_checkpoint() → periodic/event-driven snapshots
  session.materialize_features() → MaterializedFeatureSet

Faza F — Policy Evaluation:
  evaluate_policy(features, config) → PASS/FAIL

Faza G — Post-Verdict Routing:
  PASS → GatekeeperCommitLoop → ShadowLedger.commit_history() → LivePipeline
  FAIL → session.close(Fail) → WAL log → diagnostics → cleanup
```

#### 8.4 Usunięcie legacy paths

1. **`PerPoolOracleState`** — usunięcie definicji, `OracleRuntime.pools` → `SessionManager`
2. **Gatekeeper deprecated inline metrics** — usunięcie `#[deprecated]` methods i counterów Phase 1-5
3. **`enrich_pool_tx_from_shadow_ledger`** — usunięcie, zastąpiony przez canonical_state version
4. **Reconciliation repair path** — usunięcie `apply_curve_write` calls, pure monitoring
5. **ShadowLedger deprecated methods** — `#[doc(hidden)]`
6. **`LegacyCompat` write source** — usunięcie lub soft-block

#### 8.5 Configuration consolidation

```toml
[account_state_core]
enable = true  # feature gate, do usunięcia po stabilizacji

[session]
max_sessions = 1000
checkpoint_interval_ms = 2000
max_observation_window_ms = 10000

[tx_intelligence]
dust_threshold_sol = 0.001
burst_window_ms = 500
```

#### 8.6 Invariant enforcement — compile-time i runtime guards

**Compile-time:**
- `GatekeeperVerdictEngine` nie importuje `ShadowLedger` ani `PoolTransaction`
- `TxIntelligenceEngine` nie importuje `AccountStateCore`
- `AccountStateCore` nie importuje `ShadowLedger`

**Runtime:**
- Metrics: `account_state_core.canonical_hits` vs `shadow_ledger.fallback_hits` — fallback → ~0%
- Health check: `account_state_core.pools_canonical > 0` w produkcji

#### 8.7 Integration tests

- **E2E test**: pełny lifecycle Geyser event → verdict
- **Replay test**: odtworzenie werdyktu z `MaterializedFeatureSet` + config
- **Failover test**: brak account path → graceful fallback
- **Ordering test**: out-of-order updates odrzucane
- **Bootstrap isolation test**: Bootstrap nie widoczne jako Canonical
- **ShadowLedger isolation test**: simulation nie nadpisuje canonical

### Pliki nowe
- `ghost-launcher/tests/full_pipeline_integration.rs`

### Pliki modyfikowane
- `off-chain/components/seer/src/lib.rs` — formalizacja account path priority
- `off-chain/components/seer/src/ipc.rs` — backpressure zmiana
- `ghost-launcher/src/oracle_runtime.rs` — usunięcie legacy, SessionManager-only flow
- `ghost-launcher/src/components/gatekeeper.rs` — cleanup deprecated inline metrics
- `ghost-core/src/shadow_ledger/ledger.rs` — doc(hidden) na deprecated methods
- `ghost-core/src/shadow_ledger/reconciliation.rs` — pure monitoring mode
- `ghost-launcher/src/config.rs` — nowe sekcje config
- `config.toml` — nowe sekcje

### Weryfikacja reguł — PEŁNA WERYFIKACJA KOŃCOWA

| # | Invariant docelowy | Status po PR 8 |
|---|--------|----------------|
| 1 | AccountStateCore jedyny source of truth | Egzekwowane: canonical queries → AccountStateCore |
| 2 | TxIntelligenceEngine nie pisze do canonical state | Compile-time: brak importu AccountStateCore |
| 3 | Gatekeeper nie rekonstruuje stanu | Konsumuje MaterializedFeatureSet |
| 4 | ShadowLedger nie jest live truth | Deprecated, fallback only for bootstrap |
| 5 | Każda pool ma PoolObservationSession | SessionManager enforced |
| 6 | Werdykt odtwarzalny z checkpoints/features | feature_snapshot w GatekeeperAssessment |
| 7 | Stan redukowany przyrostowo | AccountStateReducer: delta-based |
| 8 | FeatureBuilder buduje, Gatekeeper interpretuje | Osobne moduły, osobne odpowiedzialności |
| 9 | Monotoniczna aktualizacja stanu | MonotonicUpdateGuard z (slot, recv_seq) |
| 10 | Bootstrap != canonical | StatePhase enum, osobne struktury, opcjonalne hinty |
| 11 | Simulation != canonical | ShadowLedger operuje na kopiach |

---

## Podsumowanie chronologiczne

```
PR 1  ──  Kontrakty i typy               ──  Fundament (zero breaking changes)
  │
PR 2  ──  AccountStateCore reducer        ──  Nowy source of truth (dual-write, parallel)
  │
PR 3  ──  PoolObservationSession          ──  Unified per-pool session container
  │       (3A: definicja + adaptery)
  │       (3B: cutover pool_observation_task)
  │
PR 4  ──  TxIntelligenceEngine            ──  Behavioral analysis extraction
  │
PR 5  ──  CheckpointEngine + FeatureBuilder ──  Observation materialization layer
  │
PR 6  ──  Gatekeeper → Policy Engine      ──  Policy-only refactor + feature consumption
  │       (legacy path deprecated, nie usunięty)
  │
PR 7  ──  Truth source migration          ──  AccountStateCore primary, ShadowLedger demotion
  │       (Reconciliation → diagnostic-only)
  │
PR 8  ──  Seer formalizacja + cleanup     ──  End-to-end integration, legacy removal
```

Każdy PR jest samodzielnie kompilowalny, testowalny i deployowalny. Nowe ścieżki działają równolegle z istniejącymi, aż do formalnej migracji w PR 7-8.

---

## Uwagi końcowe / hardening po głównym planie

### Uwaga A — `TxIntelligenceState`: bounded tracking signerów

W docelowej implementacji `TxIntelligenceEngine` pola:
- `unique_signers: HashSet<Pubkey>`
- `signer_volume_map: HashMap<Pubkey, f64>`

nie powinny rosnąć bez ograniczeń przez całą sesję.

**Zalecenie hardeningowe**:
- dodać `MAX_TRACKED_SIGNERS` (np. `512`)
- po przekroczeniu progu signer nadal liczy się do `total_tx` i agregatów volumenu, ale przestaje być trackowany indywidualnie
- dodać flagę diagnostyczną `signer_count_capped: bool`

Nie jest to blocker dla sekwencji PR 1–8, ale powinno zostać potraktowane jako jawny follow-up memory hardening po aktywacji nowej warstwy `TxIntelligenceEngine`.

### Uwaga B — `CheckpointDerivedFeatures`: bounded trajectories

Pola:
- `price_trajectory: Vec<f64>`
- `reserve_trajectory: Vec<(u64, u64)>`

nie powinny być semantycznie nieograniczonym storage zależnym wyłącznie od configu.

**Zalecenie hardeningowe**:
- dodać `MAX_TRAJECTORY_POINTS` (np. `20`)
- przechowywać trajectory jako bounded structure / circular overwrite
- utrzymać semantykę analizy trendów bez proporcjonalnego wzrostu pamięci przy agresywnych konfiguracjach checkpointowania

Nie jest to blocker dla bieżącego planu refaktoru, ale powinno zostać dopięte jako końcowe utwardzenie warstwy checkpointów.

---

## Diagram docelowej architektury

```
                          ┌─────────────────────────────────┐
                          │           S E E R               │
                          │  ┌───────────┐ ┌──────────────┐ │
                          │  │  TX PATH  │ │ ACCOUNT PATH │ │
                          │  │  (gRPC/   │ │ (Geyser      │ │
                          │  │  PumpPort)│ │  AccountUpd)  │ │
                          │  └─────┬─────┘ └──────┬───────┘ │
                          └────────┼──────────────┼─────────┘
                                   │              │
                          ┌────────▼──────────────▼─────────┐
                          │         EVENT BUS               │
                          │  NewPoolDetected | PoolTx |     │
                          │  AccountUpdate                  │
                          └────────┬──────────────┬─────────┘
                                   │              │
                ┌──────────────────▼──────────────▼──────────────────┐
                │              ORACLE RUNTIME (SessionManager)       │
                │                                                    │
                │  ┌──────────────────────────────────────────────┐  │
                │  │      POOL OBSERVATION SESSION (per pool)     │  │
                │  │                                              │  │
                │  │  ┌─────────────────┐ ┌────────────────────┐  │  │
                │  │  │ AccountStateCore│ │ TxIntelligenceEngine│  │  │
                │  │  │ (canonical      │ │ (behavioral        │  │  │
                │  │  │  state reducer) │ │  analysis)         │  │  │
                │  │  └────────┬────────┘ └─────────┬──────────┘  │  │
                │  │           │                    │              │  │
                │  │  ┌────────▼────────────────────▼──────────┐  │  │
                │  │  │        CHECKPOINT ENGINE                │  │  │
                │  │  │  (periodic + event snapshots)           │  │  │
                │  │  └────────────────┬───────────────────────┘  │  │
                │  │                   │                           │  │
                │  │  ┌────────────────▼───────────────────────┐  │  │
                │  │  │        FEATURE BUILDER                  │  │  │
                │  │  │  MaterializedFeatureSet                 │  │  │
                │  │  └────────────────┬───────────────────────┘  │  │
                │  └───────────────────┼──────────────────────────┘  │
                └──────────────────────┼─────────────────────────────┘
                                       │
                          ┌────────────▼───────────────┐
                          │   GATEKEEPER POLICY ENGINE  │
                          │                             │
                          │  HardFilterEngine           │
                          │  GatekeeperVerdictEngine    │
                          │  → PASS / FAIL              │
                          └──────┬─────────────┬───────┘
                                 │             │
                          ┌──────▼──────┐ ┌────▼────────────┐
                          │    PASS     │ │     FAIL        │
                          │             │ │                  │
                          │ CommitLoop  │ │ Close session    │
                          │ LivePipeline│ │ WAL log          │
                          │ Execution   │ │ Diagnostics      │
                          └─────────────┘ └─────────────────┘

        ┌─────────────────────────────────────────────────────┐
        │              SUPPORTING LAYERS                       │
        │                                                     │
        │  ShadowLedger ─── Simulation / WAL / Replay         │
        │  SnapshotEngine ── Derived snapshots / staging       │
        │                    (stopniowa degradacja roli)       │
        │  SnapshotListener ─ TX snapshot forwarding           │
        │  WAL ───────────── Durability / recovery             │
        │  Reconciliation ── Drift monitoring (diagnostic)     │
        └─────────────────────────────────────────────────────┘
```
