# AUDYT PIPELINE'U DECYZYJNEGO – GATEKEEPER V2

> **Data audytu**: 2026-04-29  
> **Wersja systemu**: Ghost Brain PREDATOR v11  
> **Konfiguracja**: `ghost-brain/ghost_brain_config.toml` (v11)  
> **Tryb**: `long` (akumulacyjny – czeka pełne 8001 ms, jedna finalna ewaluacja)  
> **Cel**: Pełna mapa pipeline'u odbierającego dane z pooli pump.fun, analizującego je i wydającego werdykt BUY lub REJECT

---

## SPIS TREŚCI

1. [Przegląd architektury](#1-przegląd-architektury)
2. [Strumień danych – od eventu do decyzji](#2-strumień-danych--od-eventu-do-decyzji)
3. [Gatekeeper V2 – 6-fazowa analiza](#3-gatekeeper-v2--6-fazowa-analiza)
4. [Trójwarstwowy system decyzyjny (3-Layer Decision)](#4-trójwarstwowy-system-decyzyjny-3-layer-decision)
5. [Alpha Gate – pozytywny selektor](#5-alpha-gate--pozytywny-selektor)
6. [Sybil Interference Layer](#6-sybil-interference-layer)
7. [Prosperity Filter – finalny selektor](#7-prosperity-filter--finalny-selektor)
8. [IWIM Veto Gate (obecnie wyłączony)](#8-iwim-veto-gate-obecnie-wyłączony)
9. [Pełna ścieżka werdyktu](#9-pełna-ścieżka-werdyktu)
10. [Timeout i logowanie](#10-timeout-i-logowanie)
11. [Post-Gatekeeper: HyperPrediction Oracle](#11-post-gatekeeper-hyperprediction-oracle)
12. [Kompletna lista plików](#12-kompletna-lista-plików)

---

## 1. PRZEGLĄD ARCHITEKTURY

Pipeline decyzyjny systemu Ghost składa się z trzech głównych warstw:

```
┌─────────────────────────────────────────────────────────────┐
│                    WARSTWA INGESTYCJI                        │
│  Seer (gRPC Yellowstone / PumpPortal WS / Helius WS)        │
│  Binary Parser → GeyserEvent → IPC → ghost-launcher         │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA ANALIZY                           │
│  Gatekeeper V2 (ghost-launcher)                             │
│  ├── Buforowanie TX per pool (max 8001 ms)                  │
│  ├── Materializacja cech (tx_intel, alpha_fingerprint,      │
│  │   sybil_resistance, curve_readiness)                      │
│  ├── 6-fazowa analiza                                       │
│  ├── 3-warstwowy system decyzyjny                           │
│  └── Alpha Gate → Sybil Interference → Prosperity Filter    │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA DECYZYJNA                         │
│  GatekeeperPolicy → GatekeeperDecision → BUY / REJECT /     │
│  TIMEOUT                                                     │
├─────────────────────────────────────────────────────────────┤
│                    WARSTWA POST-DECYZYJNA                    │
│  HyperPrediction Oracle (SurvivorScore, cykle S1-S13)       │
│  Post-Buy Guardian (monitoring pozycji)                     │
│  Execution Layer (Trigger, Live/Shadow/Paper)               │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. STRUMIEŃ DANYCH – OD EVENTU DO DECYZJI

### 2.1. Ingestia zdarzeń (Seer)

| Komponent | Plik | Rola |
|-----------|------|------|
| **gRPC Yellowstone** | `off-chain/components/seer/src/grpc_connection.rs` | Główny kanał danych – subskrypcje kont bonding curve + slot updates |
| **PumpPortal WebSocket** | `off-chain/components/seer/src/pumpportal_connection.rs` | Alternatywne źródło – eventy trade i create token z PumpPortal |
| **Helius WebSocket** | `off-chain/components/seer/src/helius_websocket_adapter.rs` | Trzecie źródło fallback |
| **Binary Parser** | `off-chain/components/seer/src/binary_parser.rs` | Parsowanie surowych bajtów transakcji → `GeyserEvent` (< 5ms) |
| **Curve Parser** | `off-chain/components/seer/src/curve_parser.rs` | Parsowanie stanu bonding curve z kont Solana |
| **Early Fingerprint** | `off-chain/components/seer/src/early_fingerprint.rs` | Wczesne metryki fingerprint (gRPC-level) |
| **IPC** | `off-chain/components/seer/src/ipc.rs` | Transport eventów między procesami Seer ↔ Launcher |

### 2.2. Odbiór w Launcherze

```
Seer IPC → ghost-launcher (components/seer.rs)
  → snapshot_listener.rs (odbiera eventy)
  → oracle_runtime.rs (rejestruje pule, uruchamia obserwację)
```

**Kluczowe pliki:**

| Plik | Rola |
|------|------|
| `ghost-launcher/src/components/seer.rs` | Zarządzanie połączeniem z Seer |
| `ghost-launcher/src/components/snapshot_listener.rs` | Odbieranie snapshotów stanu |
| `ghost-launcher/src/oracle_runtime.rs` | Główna pętla runtime – uruchamianie obserwacji per pool |

### 2.3. Rejestracja nowej puli

Każda nowa pula pump.fun przechodzi przez:

1. **Wykrycie** – event `NewPoolDetected` (z curve parsera lub PumpPortal)
2. **Rejestracja** – `PoolIdentity` (pool_id, base_mint, bonding_curve key)
3. **Okno AB** (2000 ms) – pierwsze okno obserwacji zbierające wczesne TX
4. **Obserwacja właściwa** – Gatekeeper V2 przejmuje pulę na `max_wait_time_ms = 8001 ms`

---

## 3. GATEKEEPER V2 – 6-FAZOWA ANALIZA

### Pliki źródłowe

| Plik | Rola |
|------|------|
| `ghost-launcher/src/components/gatekeeper.rs` | Główny komponent Gatekeeper V2 (10226 linii) |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | Silnik polityki – ocena faz, decyzje, soft signals (2142 linii) |
| `ghost-launcher/src/components/gatekeeper_commit_loop.rs` | Pętla commitowania zbuforowanych TX do ShadowLedger |
| `ghost-launcher/src/tx_intelligence/analysis.rs` | Obliczenia metryk (velocity, diversity, volume, dev) |
| `ghost-launcher/src/tx_intelligence/engine.rs` | Silnik TX Intelligence |
| `ghost-launcher/src/tx_intelligence/sybil_metrics.rs` | Metryki sybil (FTDI, DBIA, SFD, DES) |
| `ghost-launcher/src/tx_intelligence/funding_source.rs` | Analiza źródła finansowania (FSC) |
| `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs` | Cross-pool velocity (CPV) |

### 3.1. Budowanie MaterializedFeatureSet

Przed oceną faz, wszystkie surowe dane są materializowane w jedną strukturę `MaterializedFeatureSet`, która zawiera:

```rust
MaterializedFeatureSet {
    tx_intel_features: TxIntelFeatures {
        tx_count: u64,            // liczba TX w oknie obserwacji
        unique_signers: u64,      // unikalni sygnatariusze
        buy_count: u64,           // liczba buy TX
        dust_tx_count: u64,       // TX odfiltrowane jako dust
        buy_ratio: f64,           // stosunek buy / wszystkie TX
        avg_interval_ms: f64,     // średni odstęp między TX
        burst_ratio: f64,         // współczynnik burst
        timing_entropy: f64,      // entropia timing
        // ... wiele więcej pól
    },
    alpha_fingerprint: AlphaFingerprintFeatures {
        avg_inner_ix_count_50tx: Option<f64>,
        sell_buy_ratio: Option<f64>,
        compute_unit_cluster_dominance: Option<f64>,
        static_fee_profile_ratio: Option<f64>,
        jito_tip_intensity: Option<f64>,
        early_slot_volume_dominance_buy: Option<f64>,
        early_top3_buy_volume_pct_3s: Option<f64>,
        fixed_size_buy_ratio: Option<f64>,
        flipper_presence_ratio: Option<f64>,
        // ... whale_reversal, dev_sold timing, etc.
    },
    sybil_resistance: SybilResistanceFeatures {
        fee_topology_diversity_index: Option<f64>,        // FTDI
        dev_buyer_infrastructure_affinity: Option<f64>,    // DBIA
        spend_fraction_divergence: Option<f64>,            // SFD
        demand_elasticity_score: Option<f64>,              // DES
        signer_cross_pool_velocity: Option<f64>,           // CPV
        funding_source_concentration: Option<f64>,         // FSC
        degraded_reasons: Vec<String>,                     // powody degradacji
    },
    curve_readiness: CurveReadinessFeatures {
        is_ready: bool,               // czy dane krzywej są gotowe
        freshness: CurveFreshnessState,  // Fresh/Committed/Stale/Unknown
        t0_event_ts_ms: Option<u64>,  // timestamp T0 (początek puli)
        wait_elapsed_ms: Option<u64>, // czas oczekiwania na krzywą
        price_sample_count: u64,      // liczba próbek ceny
        bonding_progress_pct: Option<f64>,
        current_market_cap_sol: Option<f64>,
    },
    checkpoint_features: ...,
    session_metadata: ...,
}
```

### 3.2. Sześć faz analizy

Funkcja `build_assessment_from_features()` w `gatekeeper_policy.rs:370` przeprowadza analizę w 6 fazach:

---

#### FAZA 1: Quantity Gate (Ilościowa)

**Plik**: `gatekeeper_policy.rs:375-377`

**Sprawdzane metryki:**

| Metryka | Próg (config) | Opis |
|---------|---------------|------|
| `min_tx_count` | **12** | Minimalna liczba transakcji |
| `min_unique_signers` | **8** | Minimalna liczba unikalnych sygnatariuszy |
| `min_buy_count` | **6** | Minimalna liczba transakcji BUY |

**Logika**: `phase1_passed = tx_count >= 12 AND unique_signers >= 8 AND buy_count >= 6`

**Znaczenie**: Podstawowy filtr aktywności – pool musi mieć wystarczający ruch, żeby w ogóle rozważać dalszą analizę. Bez tego → `TIMEOUT_PHASE1`.

---

#### FAZA 2: Velocity Profile (Profil prędkości)

**Plik**: `gatekeeper_policy.rs:379-392`

**Sprawdzane metryki:**

| Metryka | Min | Max | Opis |
|---------|-----|-----|------|
| `interval_cv` | 0.0 | **2.3** | Współczynnik zmienności odstępów |
| `burst_ratio` | – | **1.0** | Współczynnik burst (skupisk TX) |
| `avg_interval_ms` | 1.0 | **450.0** | Średni odstęp między TX |
| `timing_entropy` | **0.01** | 9999.0 | Entropia timing (musi być > 0.01) |
| `dust_filtered_count` | **0** | – | TX odfiltrowane jako dust |

**Znaczenie**: SURVIVAL FILTER. `avg_interval_ms ≤ 450` jest głównym detektorem momentum (p=1.2e-90 separacji między survival a early-death). Pool'e survival mają średnio 309ms, early-death 606ms.

---

#### FAZA 3: Signer Diversity (Różnorodność sygnatariuszy)

**Plik**: `gatekeeper_policy.rs:394-398`

**Sprawdzane metryki:**

| Metryka | Min | Max | Opis |
|---------|-----|-----|------|
| `unique_ratio` | 0.1 | 1.0 | Stosunek unikalnych sygnatariuszy do TX |
| `hhi` | – | **0.155** | Herfindahl-Hirschman Index (koncentracja) |
| `max_tx_per_signer` | – | 999999 | Max TX na jednego sygnatariusza |
| `volume_gini` | **0.53** | 11.0 | Współczynnik Giniego wolumenu |
| `top3_volume_pct` | – | **0.70** | Procent wolumenu top 3 sygnatariuszy |
| `same_ms_tx_ratio` | – | 1.0 | TX w tej samej milisekundzie |

**Znaczenie**: SURVIVAL FILTER. `HHI ≤ 0.155` predykuje survival (p=1.9e-13). Pool'e survival mają HHI=0.155, early-death 0.200.

---

#### FAZA 4: Volume Sanity (Zdrowy wolumen)

**Plik**: `gatekeeper_policy.rs:400-406`

**Sprawdzane metryki:**

| Metryka | Min | Max | Opis |
|---------|-----|-----|------|
| `buy_ratio` | **0.80** | 1.0 | Stosunek buy TX do wszystkich |
| `avg_tx_sol` | 0.01 | 9999.0 | Średni wolumen na TX |
| `volume_cv` | 0.01 | 9999.0 | Zmienność wolumenu |
| `total_volume_sol` | 1.0 | 9999.0 | Całkowity wolumen w SOL |
| `sol_buy_ratio` | 0.0 | 1.0 | Stosunek SOL buy do całkowitego |
| `min_consecutive_buys` | **3** | – | Minimalna liczba kolejnych buy TX |

**Znaczenie**: `buy_ratio ≥ 0.80` zostawia sentiment-positive bias. Sprawdza też dodatkowo `alpha_fingerprint_phase4_passes()` dla metryk gRPC-level.

---

#### FAZA 5: Dev Behavior (Zachowanie dewelopera)

**Plik**: `gatekeeper_policy.rs:408-428`

**Sprawdzane metryki (gdy dev_wallet_known):**

| Metryka | Min | Max | Opis |
|---------|-----|-----|------|
| `dev_buy_total_sol` | 0.0 | **2.0** | Całkowity buy dewelopera w SOL |
| `dev_tx_ratio` | 0.0 | **0.99** | Stosunek TX deva do wszystkich |
| `dev_volume_ratio` | 0.0 | **0.23** | Stosunek wolumenu deva do całkowitego |
| `reject_on_dev_sell` | – | – | **false** – NIE odrzucamy na dev sell |

**Znaczenie**: Sekcja permissive. `reject_on_dev_sell=false` – dev może sprzedawać bez automatycznego REJECT. `max_dev_buy_sol=2.0` – ogranicza wielkość buy deva.

Gdy `dev_wallet_known = false` → `dev_unknown = true` → uruchamiane są zaostrzone progi w Core3.

---

#### FAZA 6: Bonding Curve Dynamics (Dynamika krzywej wiązania)

**Plik**: `gatekeeper_policy.rs:430-449`

**Sprawdzane metryki (gdy price_data_points ≥ 2):**

| Metryka | Min | Max | Opis |
|---------|-----|-----|------|
| `price_change_ratio` | 0.0 | 9999.0 | Zmiana ceny |
| `max_single_tx_price_impact_pct` | – | 9999.0 | Max wpływ pojedynczej TX na cenę |
| `max_single_sell_impact_pct` | – | 9999.0 | Max wpływ pojedynczego sella |
| `bonding_progress_pct` | **40.0** | **99.0** | Postęp bonding curve |
| `current_market_cap_sol` | **60.0** | – | Kapitalizacja rynkowa w SOL |

**Znaczenie**: SURVIVAL FILTER. Market cap ≥ 60 SOL i bonding progress ≥ 40% korelują z survivalem.

---

### 3.3. Hard Fails – natychmiastowe odrzucenie

**Plik**: `gatekeeper_policy.rs:540-693`, funkcja `evaluate_hard_filters_from_assessment()`

Hard fail'e są sprawdzane PRZED główną logiką decyzyjną i powodują natychmiastowe `REJECT_HARD_FAIL`:

| Hard Fail | Warunek | Próg |
|-----------|---------|------|
| `DevSold` | dev sprzedał + `reject_on_dev_sell=true` | *(obecnie false)* |
| `SellImpact` | max pojedynczy sell impact > próg | 9999% *(wyłączone)* |
| `TxPriceImpact` | max pojedynczy tx price impact > próg | 9999% *(wyłączone)* |
| `PriceChange` | price_change_ratio > próg | 9999.0 *(wyłączone)* |
| **MarketCapTooLow** | market_cap < **60.0 SOL** | ✅ **AKTYWNE** |
| `ExtremeHhi` | HHI > 3.90 | ✅ **AKTYWNE** |
| `ExtremeBundling` | same_ms_tx_ratio > 9999 | *(wyłączone)* |
| `ExtremeTop3Dominance` | top3_volume_pct > 1.0 | *(wyłączone)* |
| `ExtremeBotTiming` | CV < 0.08 + avg < 30ms + min_tx > 999999 | *(wyłączone – HF-9)* |
| **SlowPool** | avg_interval_ms > **450.0 ms** | ✅ **AKTYWNE** |
| `FailedTxRatio` | failed_tx_ratio > próg (Yellowstone) | *(opcjonalne)* |

**Aktywne hard fail'e w obecnej konfiguracji:** `MarketCapTooLow` (< 60 SOL) i `SlowPool` (> 450ms avg interval).

---

## 4. TRÓJWARSTWOWY SYSTEM DECYZYJNY (3-LAYER DECISION)

**Plik**: `gatekeeper_policy.rs:707-911`, funkcja `evaluate_policy_from_assessment()`

Po przejściu hard fails, system przechodzi przez 3 warstwy:

### 4.1. CORE LAYER (muszą przejść wszystkie 3)

```
Core1 = Phase1 (Quantity Gate) – tx≥12, signers≥8, buys≥6
Core2 = Phase4 (Volume Sanity) – buy_ratio≥0.80, consecutive_buys≥3
Core3 = zależne od dev_unknown:
  - dev znany:    Phase5 (Dev) + Phase6 (Curve)
  - dev nieznany: Phase4 + price_ok + bonding_ok + market_cap ≥ 60 SOL
```

**Jeśli którykolwiek Core NIE przejdzie → `REJECT_CORE_FAIL`**

### 4.2. SOFT LAYER (punkty karne)

**Plik**: `gatekeeper_policy.rs:1187-1216`, funkcja `compute_soft_signals()`

Sygnały miękkie są liczone jako punkty karne. Każdy sygnał ma wagę:

| Kategoria | Wagi | Sygnały |
|-----------|------|---------|
| **Timing** (waga 1) | `soft_weight_timing = 1` | low_interval_cv, high_interval_cv, low_timing_entropy, high_timing_entropy, avg_interval_oor, high_burst_ratio |
| **Manipulation** (waga 3) | `soft_weight_manipulation = 3` | bundle_suspicion, cabal_suspicion, top3_dominance |
| **Diversity** (waga 2) | `soft_weight_diversity = 2` | high_volume_gini, unique_ratio_oor, high_tx_per_signer |
| **Ecosystem** (waga 1) | `soft_weight_ecosystem = 1` | low_dust_count |

**Limit**: `max_soft_points = 255` (❄ FROZEN – praktycznie wyłączone, soft scoring zneutralizowany)

```
Jeśli soft_points > effective_max_soft_points → REJECT_SOFT_EXCESS
```

### 4.3. DEV UNKNOWN – zaostrzone progi

Gdy `dev_wallet_known = false`:
- `effective_max_soft_points` zmienia się na `dev_unknown_max_soft_points = 255` (❄ FROZEN)
- Core3 wymaga: Phase4 + market_cap ≥ `dev_unknown_min_market_cap_sol` (60 SOL) + bonding OK

---

## 5. ALPHA GATE – POZYTYWNY SELEKTOR

**Plik**: `gatekeeper_policy.rs:125-167`, funkcja `evaluate_alpha_gate()`

**Włączony**: `enable_alpha_gate = true`

Alpha Gate działa jako filtr pozytywny PO przejściu core + soft. Oblicza trzy skalary:

### 5.1. Momentum (siła trendu)

```rust
momentum = 0.36 * norm(burst_ratio, 0.08, 0.45)
         + 0.34 * norm_down(avg_interval_ms, 90, 700)
         + 0.20 * norm(timing_entropy, 1.05, 2.35)
         + 0.10 * norm(buy_count, 10, 25)
```

### 5.2. Demand (popyt)

```rust
demand = 0.35 * norm(unique_signers, 10, 25)
       + 0.35 * norm(buy_ratio - sell_buy_ratio, -0.10, 0.50)
       + 0.20 * norm_down(fixed_size_buy_ratio, 0.75, 0.05)
       + 0.10 * (1.0 - flipper_presence_ratio)
```

### 5.3. Joint (łączny)

```rust
joint = clamp01(momentum * demand)
```

### 5.4. Progi

| Próg | Wartość | Opis |
|------|---------|------|
| `min_momentum` | **0.2** | Minimalny momentum |
| `min_demand` | **0.2** | Minimalny popyt |
| `min_alpha_joint` | **0.2** | Minimalny wynik łączny |
| `min_alpha_sample` | **10** | Minimalna liczba buy TX do oceny |

**Rezultat**: Jeśli momentum < 0.2, demand < 0.2 lub joint < 0.2 → `REJECT_LOW_ALPHA`

---

## 6. SYBIL INTERFERENCE LAYER

**Plik**: `gatekeeper_policy.rs:1282-1420` (soft signals) + `gatekeeper_policy.rs:1519+` (diagnostics)

**Włączony**: `enable_sybil_interference_layer = true`  
**Combo veto**: `enable_sybil_combo_veto = false` (wyłączone)

### 6.1. Sześć metryk Sybil

| Metryka | Próg | Kara | Opis |
|---------|------|------|------|
| **FTDI** (Fee Topology Diversity Index) | < **0.06** | 1 pkt | Niska różnorodność topologii fee |
| **DBIA** (Dev-Buyer Infrastructure Affinity) | > **0.60** | 1 pkt | Zbyt wysokie powinowactwo infrastruktury dev-buyer |
| **SFD** (Spend Fraction Divergence) | < **0.06** | 2 pkt | Niska dywergencja frakcji wydatków |
| **DES** (Demand Elasticity Score) | < **0.05** | 3 pkt | Nieelastyczny popyt (lead signal) |
| **CPV** (Cross-Pool Velocity) | > **0.50** | 1 pkt | Zbyt wysoka prędkość cross-pool |
| **FSC** (Funding Source Concentration) | > **0.60** | 0 pkt (❄ FROZEN) | Koncentracja źródła finansowania |

### 6.2. Combo patterns (wykrywanie złożonych wzorców)

| Pattern | Kara |
|---------|------|
| `HIGH_DBIA + LOW_FTDI` | 2 pkt |
| `LOW_DES + LOW_SFD` | 2 pkt |
| `HIGH_CPV + LOW_DES` | 0 pkt (❄ FROZEN) |
| `HIGH_FSC + HIGH_CPV` | 0 pkt (❄ FROZEN) |

### 6.3. Limity

| Limit | Wartość |
|-------|---------|
| `max_sybil_soft_points` | **6** |
| `dev_unknown_max_sybil_soft_points` | **5** |

```
Jeśli sybil_soft_points > max_sybil_soft_points → REJECT_SYBIL_SOFT_EXCESS
Jeśli sybil_combo_veto (wyłączone) → REJECT_SYBIL_INTERFERENCE
```

---

## 7. PROSPERITY FILTER – FINALNY SELEKTOR

**Plik**: `gatekeeper_policy.rs:169+`, funkcja `evaluate_prosperity_filter()`

**Włączony**: `enable_prosperity_filter = true`  
**Overlay**: `enable_prosperity_overlay = false` (wyłączony)

Prosperity Filter wchodzi dopiero po przejściu hard/core/soft/sybil/alpha. Ma odsiać BUY-e, które nie przypominają historycznie zyskownych gałęzi (branches) ze shadow outcome set.

### 7.1. Warunki wstępne (floor)

| Warunek | Próg |
|---------|------|
| Market cap floor | ≥ **45.0 SOL** |
| Cross-pool velocity (CPV) | ≤ **0.5** |

### 7.2. Trzy gałęzie (token musi pasować do ≥ 1)

#### Branch 1: "Sniper Interest + Clean Flow"
| Metryka | Próg |
|---------|------|
| `block0_sniped_supply_pct` | ≥ **28%** |
| `sell_buy_ratio` | ≤ **16%** |

#### Branch 2: "Strong Market Cap + Buy Dominance"
| Metryka | Próg |
|---------|------|
| Market cap | ≥ **55.0 SOL** |
| `early_slot_volume_dominance_buy` | ≥ **90%** |

#### Branch 3: "Extreme Decentralization"
| Metryka | Próg |
|---------|------|
| HHI | ≤ **0.0416** |
| `fee_topology_diversity_index` | ≥ **0.0909** |

```
Jeśli NIE pasuje do żadnej gałęzi → REJECT_LOW_PROSPERITY
```

---

## 8. IWIM VETO GATE (OBECNIE WYŁĄCZONY)

**Plik**: `ghost-launcher/src/components/iwim_veto.rs`  
**Status**: `iwim_veto_gate.enabled = false`  
**Tryb**: `mode = "pp"` (paper-paper – shadow only)

IWIM Veto Gate to post-Gatekeeper filtr, który dla BUY werdyktów fetchnąłby historię dev walleta przez RPC i sprawdzał wzorce rug/sybil. Obecnie wyłączony do zbierania danych.

**Macierz polityki**: `(dev_known × iwim_quality × fetch_status × gk_strength) → verdict`

---

## 9. PEŁNA ŚCIEŻKA WERDYKTU

### 9.1. Diagram decyzyjny

```
NOWA PULA (T0)
    │
    ▼
OKNO AB (2000 ms) – Early fingerprint
    │
    ▼
OBSERWACJA WŁAŚCIWA (max 8001 ms, tryb "long")
    │
    ├── Tx < min_tx_count (12) → TIMEOUT_PHASE1
    │
    ▼
BUDOWANIE MaterializedFeatureSet
    │
    ▼
6-FAZOWA ANALIZA (build_assessment_from_features)
    │
    ▼
HARD FAILS CHECK
    ├── MarketCap < 60 SOL → REJECT_HARD_FAIL
    ├── avg_interval > 450ms → REJECT_HARD_FAIL
    ├── HHI > 3.90 → REJECT_HARD_FAIL
    └── (inne hard fail'e wyłączone)
    │
    ▼ (brak hard fail)
CORE LAYER (3 warstwy)
    ├── Core1 (Phase1: Quantity) FAIL → REJECT_CORE_FAIL
    ├── Core2 (Phase4: Volume Sanity) FAIL → REJECT_CORE_FAIL
    └── Core3 (Phase5+6 lub zaostrzone dla dev_unknown) FAIL → REJECT_CORE_FAIL
    │
    ▼ (wszystkie core pass)
SYBIL COMBO VETO (wyłączone)
    │
    ▼
SYBIL SOFT EXCESS
    ├── sybil_soft_points > 6 → REJECT_SYBIL_SOFT_EXCESS
    └── (dla dev_unknown: > 5)
    │
    ▼
SOFT EXCESS
    └── soft_points > 255 (❄ FROZEN – praktycznie niemożliwe)
    │
    ▼
ALPHA GATE (pozytywny selektor)
    ├── momentum < 0.2 → REJECT_LOW_ALPHA
    ├── demand < 0.2 → REJECT_LOW_ALPHA
    └── joint < 0.2 → REJECT_LOW_ALPHA
    │
    ▼ (alpha pass)
PROSPERITY FILTER (finalny selektor)
    ├── market_cap < 45 SOL → REJECT_LOW_PROSPERITY
    ├── cpv > 0.5 → REJECT_LOW_PROSPERITY
    └── żadna gałąź nie pasuje → REJECT_LOW_PROSPERITY
    │
    ▼ (prosperity pass)
WERDYKT: ✅ BUY
```

### 9.2. Wszystkie typy werdyktów

**Plik**: `gatekeeper.rs:874-901`, enum `GatekeeperVerdictType`

| Werdykt | Tag | Znaczenie |
|---------|-----|-----------|
| **Buy** | `BUY` | Wszystkie filtry przeszły – POZYTYWNA DECYZJA |
| RejectHardFail | `REJECT_HARD_FAIL` | Hard fail (market cap, slow pool, extreme HHI) |
| RejectCoreFail | `REJECT_CORE_FAIL` | Nie przeszedł core layer (1, 2 lub 3) |
| RejectSoftExcess | `REJECT_SOFT_EXCESS` | Za dużo punktów karnych soft |
| RejectSybilSoftExcess | `REJECT_SYBIL_SOFT_EXCESS` | Za dużo punktów karnych sybil |
| RejectSybilInterference | `REJECT_SYBIL_INTERFERENCE` | Sybil combo veto |
| RejectLowAlpha | `REJECT_LOW_ALPHA` | Nie przeszedł Alpha Gate |
| RejectLowProsperity | `REJECT_LOW_PROSPERITY` | Nie przeszedł Prosperity Filter |
| TimeoutPhase1 | `TIMEOUT_PHASE1` | Za mało TX po pełnym czasie obserwacji |
| TimeoutNoData | `TIMEOUT_NO_DATA` | Brak jakichkolwiek danych |
| RejectIwimVeto | `REJECT_IWIM_VETO` | IWIM Veto (obecnie wyłączone) |
| RejectIwimLowConf | `REJECT_IWIM_LOW_CONF` | IWIM niska pewność (wyłączone) |
| RejectIwimUnknownStrict | `REJECT_IWIM_UNKNOWN_STRICT` | IWIM unknown strict (wyłączone) |

### 9.3. GatekeeperStrength (siła werdyktu BUY)

Gdy werdykt to BUY, system określa jego siłę:

- **Strong**: `soft_points ≤ (effective_max - strong_margin)` ORAZ `manipulation_flags ≤ strong_max_manip_flags`
- **Borderline**: BUY ze słabszymi parametrami

```rust
strong_margin = 3
strong_max_manipulation_flags = 0
```

---

## 10. TIMEOUT I LOGOWANIE

### 10.1. Tryb "long"

W trybie `long`:
- System czeka **pełne 8001 ms** (`max_wait_time_ms`)
- Zbiera **wszystkie TX** w oknie
- Jedna **finalna ewaluacja** na deadline
- **Brak wczesnych decyzji** (w przeciwieństwie do trybu `standard`)

### 10.2. Logowanie decyzji

**Plik**: `ghost-brain/src/oracle/decision_logger.rs`

Każda decyzja jest logowana jako JSONL z pełnym zestawem pól:

```json
{
  "log_schema_version": 15,
  "timestamp": "2026-04-27T19:20:05.719Z",
  "pool_id": "Eyjnwzqu7Q...",
  "base_mint": "BnL92KHwW...",
  "core_pass": false,
  "gatekeeper_version": "v2.2",
  "phases_passed": 1,
  "decision_reason": "TIMEOUT_PHASE1: tx=4/12 signers=4/8 buys=4/6",
  "decision_verdict_buy": false,
  "verdict_type": "TIMEOUT_PHASE1",
  "soft_score": 3,
  "soft_points": 7,
  "sybil_soft_points": 3,
  "total_soft_points": 10,
  "alpha_gate_enabled": true,
  "alpha_actionable": false,
  "prosperity_filter_enabled": true,
  "prosperity_actionable": false,
  // ... wszystkie metryki faz 1-6 ...
  "vectors_ts_offsets_ms": [0, 154, 186],
  "vectors_sol_amounts": [5.925, 7.169, 0.987],
  "vectors_prices": [4.009e-08, 5.769e-08, 6.037e-08]
}
```

**Ścieżki logów w runtime:**
```
logs/rollout/shadow-burnin/system.log.YYYY-MM-DD
logs/rollout/shadow-burnin/oracle.log.YYYY-MM-DD
logs/rollout/shadow-burnin/decisions/gatekeeper_v2_buys.jsonl
logs/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl
```

---

## 11. POST-GATEKEEPER: HYPERPREDICTION ORACLE

Po decyzji Gatekeepera, dla pul które przeszły BUY, uruchamiany jest HyperPrediction Oracle:

### 11.1. Główne komponenty

| Komponent | Plik | Rola |
|-----------|------|------|
| **Orchestrator** | `ghost-brain/src/oracle/hyper_prediction/orchestrator.rs` | Główny silnik scoringu |
| **SurvivorScore** | `ghost-brain/src/oracle/survivor_score.rs` | Obliczanie Survival × Momentum × Quality |
| **Confidence Model** | `ghost-brain/src/oracle/confidence_model.rs` | Model pewności C ∈ [0,1] |
| **Verdict** | `ghost-brain/src/oracle/hyper_prediction/verdict.rs` | FinalVerdict, OracleDecision, RiskLevel |

### 11.2. Cykle scoringowe (S1-S13)

HyperPrediction Oracle działa w 13 cyklach:

- **S1-S6** (Early Stage): 16-22 TX, analiza statyczna (LIGMA, IWIM, Chaos, MESA)
- **S7-S12** (Full Analysis): 23+ TX, wszystkie metryki (SCR, ULVF, POVC)
- **S13**: Final Verdict – decyzja końcowa

### 11.3. Sygnały używane przez Oracle

| Sygnał | Plik | Opis |
|--------|------|------|
| **SOBP** | `ultrafast/sobp.rs` | Slot-Over-Slot Buying Pressure (0-2s) |
| **MPCF** | `ultrafast/mpcf.rs` | Micro-Payload Cognitive Fingerprint (30-70μs) |
| **SSMI** | `ultrafast/ssmi.rs` | Sub-Slot Microentropy Index |
| **IWIM** | `ultrafast/iwim.rs` | Initial Wallet Intent Mapping |
| **QASS** | `ultrafast/qass_stub.rs` | Quantum Amplitude Scoring System |
| **CIR** | `ultrafast/cir.rs` | Causal Impact Ratio |
| **PANIC** | `ultrafast/panic.rs` | Panic signal detection |
| **BVA** | `oracle/bva.rs` | Behavioral Vacuum Analysis (0-7s) |
| **TCR** | `ultrafast/tcr_phi.rs` | Transaction Coherence Resonance |
| **FRE** | `ultrafast/fre/engine.rs` | Fractal Resonance Engine |
| **LIGMA** | `signals/ligma.rs` | Liquidity Impact Gauge |
| **MCI** | `mci.rs` | Market Coherence Index |
| **QEDD** | `qedd.rs` | Quality Early Death Detector |
| **TCF** | `oracle/tcf/` | Trend Cohesion Field |
| **SCR Extended** | `oracle/scr_extended.rs` | Bot detection via FFT |
| **ULVF Extended** | `oracle/ulvf_extended.rs` | Divergence/Curl analysis |

### 11.4. Formuła SurvivorScore

```
S = (survival)^0.35 × (momentum)^0.30 × (quality)^0.20 × (1 - risk_discount)
```

Gdzie:
- **survival** = QEDD (0.625) + Cluster (0.375)
- **momentum** = SOBP × 0.5 + Chaos × 0.6
- **quality** (Early Stage) = MPCF (0.44) + MESA (0.31) + Wallet (0.25)
- **quality** (Full Analysis) = MPCF (0.35) + MESA (0.25) + SCR (0.20) + Wallet (0.20)

---

## 12. KOMPLETNA LISTA PLIKÓW

### 12.1. Gatekeeper V2 (główny pipeline decyzyjny)

| Plik | Linie | Rola |
|------|-------|------|
| `ghost-launcher/src/components/gatekeeper.rs` | 10226 | Główny komponent – buforowanie, obserwacja, koordynacja |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | 2142 | Silnik polityki – fazy, decyzje, soft signals |
| `ghost-launcher/src/components/gatekeeper_commit_loop.rs` | 512 | Commitowanie do ShadowLedger |
| `ghost-launcher/src/components/iwim_veto.rs` | – | IWIM Veto Gate (obecnie wyłączony) |
| `ghost-launcher/src/oracle_runtime.rs` | 8000+ | Runtime orkiestrujący obserwację i decyzje |
| `ghost-launcher/src/tx_intelligence/analysis.rs` | – | Obliczenia metryk: velocity, diversity, volume, dev |
| `ghost-launcher/src/tx_intelligence/engine.rs` | – | Silnik TX Intelligence |
| `ghost-launcher/src/tx_intelligence/sybil_metrics.rs` | 1118 | Metryki Sybil: FTDI, DBIA, SFD, DES |
| `ghost-launcher/src/tx_intelligence/funding_source.rs` | – | Analiza FSC (Funding Source Concentration) |
| `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs` | – | Analiza CPV (Cross-Pool Velocity) |
| `ghost-launcher/src/tx_intelligence/config.rs` | – | Konfiguracja TX Intelligence |
| `ghost-launcher/src/tx_intelligence/mod.rs` | – | Re-eksporty |

### 12.2. Ingestia danych (Seer)

| Plik | Rola |
|------|------|
| `off-chain/components/seer/src/grpc_connection.rs` | Główny kanał gRPC Yellowstone (5600+ linii) |
| `off-chain/components/seer/src/pumpportal_connection.rs` | PumpPortal WebSocket |
| `off-chain/components/seer/src/helius_websocket_adapter.rs` | Helius WebSocket adapter |
| `off-chain/components/seer/src/binary_parser.rs` | Parser binarny TX → GeyserEvent |
| `off-chain/components/seer/src/curve_parser.rs` | Parser stanu bonding curve |
| `off-chain/components/seer/src/early_fingerprint.rs` | Wczesny fingerprint |
| `off-chain/components/seer/src/paradox_sensor/mod.rs` | Paradox Sensor – detekcja anomalii |
| `off-chain/components/seer/src/ipc.rs` | Transport IPC |
| `off-chain/components/seer/src/lib.rs` | Biblioteka Seer |
| `off-chain/components/seer/src/main.rs` | Binarka Seer |

### 12.3. Oracle (HyperPrediction + scoring)

| Plik | Rola |
|------|------|
| `ghost-brain/src/oracle/hyper_prediction/orchestrator.rs` | Główny orkiestrator scoringu (2838 linii) |
| `ghost-brain/src/oracle/hyper_prediction/verdict.rs` | Typy werdyktów (OracleDecision, FinalVerdict, RiskLevel) |
| `ghost-brain/src/oracle/hyper_prediction/config.rs` | Konfiguracja HyperPrediction |
| `ghost-brain/src/oracle/hyper_prediction/state.rs` | Stan analizy (AnalysisPhase) |
| `ghost-brain/src/oracle/hyper_prediction/scoring/mod.rs` | Łączenie scoringu |
| `ghost-brain/src/oracle/hyper_prediction/scoring/boosters.rs` | Boostery scoringu |
| `ghost-brain/src/oracle/hyper_prediction/scoring/penalties.rs` | Kary scoringu |
| `ghost-brain/src/oracle/hyper_prediction/scoring/weights.rs` | Wagi scoringu |
| `ghost-brain/src/oracle/hyper_prediction/signals/` | Kolektory sygnałów (LIGMA, QEDD, Cluster, MCI, Paradox) |
| `ghost-brain/src/oracle/survivor_score.rs` | SurvivorScore calculator |
| `ghost-brain/src/oracle/confidence_model.rs` | Model pewności |
| `ghost-brain/src/oracle/scoring.rs` | ScoringWeights + SimpleOracle |
| `ghost-brain/src/oracle/scoring_phase.rs` | Fazy scoringu (EarlyStage vs FullAnalysis) |
| `ghost-brain/src/oracle/followup_scoring.rs` | Follow-up scoring |
| `ghost-brain/src/oracle/decision_logger.rs` | Logowanie decyzji do JSONL |
| `ghost-brain/src/oracle/score_history.rs` | Historia scoringu |
| `ghost-brain/src/oracle/engine.rs` | Silnik Oracle |

### 12.4. Sygnały (ultrafast processing)

| Plik | Rola |
|------|------|
| `ghost-brain/src/oracle/ultrafast/sobp.rs` | SOBP – Buying Pressure |
| `ghost-brain/src/oracle/ultrafast/mpcf.rs` | MPCF – Fingerprinting |
| `ghost-brain/src/oracle/ultrafast/ssmi.rs` | SSMI – Microentropy |
| `ghost-brain/src/oracle/ultrafast/iwim.rs` | IWIM – Dev Intent Mapping |
| `ghost-brain/src/oracle/ultrafast/qass_stub.rs` | QASS – Amplitude Scoring |
| `ghost-brain/src/oracle/ultrafast/cir.rs` | CIR – Causal Impact |
| `ghost-brain/src/oracle/ultrafast/panic.rs` | PANIC – Panic Detection |
| `ghost-brain/src/oracle/ultrafast/praecog.rs` | PRAECOG – Adversarial Simulation |
| `ghost-brain/src/oracle/ultrafast/tcr_phi.rs` | TCR – Coherence Resonance |
| `ghost-brain/src/oracle/ultrafast/ecto.rs` | ECTO |
| `ghost-brain/src/oracle/ultrafast/fre/engine.rs` | FRE – Fractal Resonance Engine |
| `ghost-brain/src/oracle/bva.rs` | BVA – Behavioral Vacuum Analysis |

### 12.5. Pool state i pump.fun

| Plik | Rola |
|------|------|
| `ghost-brain/src/pool_state_ssot/mod.rs` | Pool State SSOT |
| `ghost-brain/src/pool_state_ssot/phase.rs` | Fazy puli (BondingCurve → Amm) |
| `ghost-brain/src/pool_state_ssot/snapshot.rs` | PoolSnapshot |
| `ghost-brain/src/pool_state_ssot/store.rs` | SnapshotStore |
| `ghost-brain/src/pool_state_ssot/yellowstone.rs` | Yellowstone subscriber dla SSOT |
| `ghost-brain/src/pool_state_ssot/quote_engine.rs` | QuoteEngine |
| `ghost-brain/src/pumpfun/mod.rs` | Pump.fun module |
| `ghost-brain/src/pumpfun/state.rs` | Pump.fun state |
| `ghost-core/src/pool_identity.rs` | PoolIdentity + PoolIdentityRegistry |
| `ghost-core/src/market_state.rs` | BondingCurve struct |

### 12.6. Shadow Ledger i storage

| Plik | Rola |
|------|------|
| `ghost-core/src/shadow_ledger/ledger.rs` | ShadowLedger – tracking stanu puli |
| `ghost-core/src/shadow_ledger/live_pipeline.rs` | Live pipeline |
| `ghost-core/src/shadow_ledger/mod.rs` | Module root |
| `ghost-core/src/shadow_ledger/reconciliation.rs` | Reconciliation |
| `ghost-core/src/wal.rs` | Write-Ahead Log + GatekeeperDecision enum |

### 12.7. Konfiguracja

| Plik | Rola |
|------|------|
| `ghost-brain/ghost_brain_config.toml` | **Główna konfiguracja** (1049 linii, v11) |
| `ghost-brain/src/config/ghost_brain_config.rs` | Struct `GatekeeperV2Config` w Rust |
| `config.toml` | Główna konfiguracja systemu Ghost |
| `configs/shadow-burnin.toml` | Konfiguracja shadow burnin |
| `configs/paper-burnin.toml` | Konfiguracja paper burnin |
| `configs/dual-micro-live.toml` | Konfiguracja dual-micro-live |

### 12.8. ADR-y

| ADR | Temat |
|-----|-------|
| `ADR-0050` | Gatekeeper V2 startup config SSOT |
| `ADR-0051` | Gatekeeper V2 config fail-closed on parse errors |
| `ADR-0052` | Gatekeeper Phase2 avg_interval config self-consistency |
| `ADR-0061` | Bootstrap market-cap fallback for feature-driven gatekeeper |
| `ADR-0095` | Sybil Interference Policy Architecture |
| `ADR-0097` | Final Sybil Policy Closure and Activation |
| `ADR-0105` | Gatekeeper opposite threshold bounds |
| `ADR-0115` | Strict Prosperity Overlay for shadow-burnin |

---

## PODSUMOWANIE

Pipeline decyzyjny Gatekeeper V2 to wielowarstwowy system analityczny, który:

1. **Zbiera** dane w czasie rzeczywistym z blockchaina Solana (gRPC Yellowstone + PumpPortal)
2. **Buforuje** transakcje per pool przez 8001 ms (tryb `long`)
3. **Materializuje** surowe dane w `MaterializedFeatureSet` (tx_intel + alpha_fingerprint + sybil_resistance + curve_readiness)
4. **Analizuje** w 6 fazach: Quantity → Velocity → Diversity → Volume → Dev → Curve
5. **Podejmuje decyzję** w 3 warstwach: Hard Fails → Core (1-2-3) → Soft
6. **Filtruje pozytywnie**: Alpha Gate (momentum + demand) → Sybil Interference → Prosperity Filter (3 gałęzie)
7. **Wydaje werdykt**: BUY lub REJECT (z 13 możliwych typów odrzucenia)
8. **Loguje** pełną diagnozę w JSONL do dalszej analizy offline

System działa w trybie **ultra-selektywnym**: tylko pule przechodzące wszystkie filtry otrzymują BUY. Hard fail'e `MarketCapTooLow` (< 60 SOL) i `SlowPool` (> 450ms) eliminują większość kandydatów. Alpha Gate i Prosperity Filter dodają dodatkowe sito pozytywnej selekcji.
