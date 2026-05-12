# PLAN PRZEBUDOWY GATEKEEPER V2.5 "PRECISION STRIKE" -- ODZYSKIWANIE REALNYCH ZYSKÓW

> **Data:** 2026-05-02
> **Autor:** Ghost Father (AI Agent)
> **Wersja:** 2.1 (shadow-first hardening — ograniczenie obietnic, źródła ceny, rollout bez naruszenia kontraktu 8-9s)
> **Status:** Plan implementacyjny po recenzji krytycznej. V2.5 startuje jako **shadow-first**; live early-entry wymaga osobnego ADR i walidacji.
> **Cel:** Przebudowa pipeline'u decyzyjnego Gatekeepera redukująca 6 root causes strat (-52.26% avg na 57.7% pozycji) poprzez środkowe podejście między config-only tuning a pełnym Strike Engine V3 rewrite.
> **Zakres:** Gatekeeper V2.5 -- chirurgiczna przebudowa 4 kluczowych warstw z zachowaniem architektonicznych kontraktów (SSOT, format JSONL, struktury decyzyjne).
> **Target walidacyjny:** Win rate >= 65%, avg loss <15%, profit >= 30% są hipotezami do shadow-burnin, nie gwarancją implementacji.

---

## SPIS TREŚCI

1. [Diagnoza -- Sześć Root Causes](#1-diagnoza----sześć-root-causes)
   - [1.5. Fundamentalna Asymetria -- Co 10s może, a czego nie może](#15-fundamentalna-asymetria----co-10s-może-a-czego-nie-może)
   - [1.6. Czego V2.5 NIE rozwiąże (jawne ograniczenia)](#16-czego-v25-nie-rozwiąże-jawne-ograniczenia)
   - [1.7. Założenia walidacyjne i guardraile](#17-założenia-walidacyjne-i-guardraile)
2. [Architektura Rozwiązania -- Cztery Moduły V2.5](#2-architektura-rozwiązania----cztery-moduły-v25)
3. [Plan Implementacji -- 8 Faz](#3-plan-implementacji----8-faz)
4. [Mapowanie Plików do Zmian](#4-mapowanie-plików-do-zmian)
5. [Diagram Przepływu Decyzji V2.5](#5-diagram-przepływu-decyzji-v25-v21)
6. [Matryca Ryzyka i Kontrola](#6-matryca-ryzyka-i-kontrola)
7. [Harmonogram](#7-harmonogram)
8. [Oczekiwany Wpływ](#8-oczekiwany-wpływ)
9. [Sprawdzenie Kontraktów](#9-sprawdzenie-kontraktów)

---

## 1. DIAGNOZA -- SZEŚĆ ROOT CAUSES

### Kontekst

System Ghost Brain PREDATOR v11 operuje na Pump.fun (Solana), ingestując dane przez Seer (gRPC Yellowstone) i podejmując decyzje BUY/REJECT przez Gatekeeper V2. Mimo zaawansowanej 6-fazowej analizy, 3-warstwowego systemu decyzyjnego, Alpha Gate, Sybil Interference Layer i Prosperity Filter, **57.7% pozycji przynosi stratę, średnia strata wynosi -52.26%, a najgorsza -79.87%** (dane z `INVESTIGATION_REPORT_LOSING_POOLS.md`, 2026-04-26).

**Ograniczenie statystyczne diagnozy:** raport źródłowy obejmuje 26 transakcji i 15 stratnych pozycji. To wystarcza do wskazania mocnych hipotez inżynierskich (szczególnie ekstremalny drift 26%+), ale nie wystarcza do bezpiecznego promowania wszystkich progów do live execution bez shadow-burnin, replay i walidacji walk-forward.

### RC1: Statyczny single-point decision

System czeka pełne 10s (`max_wait_time_ms=10000`), zbiera TX, robi **JEDNĄ** ewaluację na deadline. Tryb `"long"` jest jedynym aktywnym. Efekt:
- Potencjalne okno wcześniejszego wejścia (pierwsze 2-7s od kreacji pool) nie jest wykorzystywane
- System kupuje po pełnym pumpie, nie przed nim
- Brak możliwości wczesnego wejścia przy wysokiej pewności
- 10s w meme coin tradingu to wieczność -- rynek już zdążył wypompować i zacząć dump

### RC2: Brak dedykowanej detekcji Pump & Dump

System doskonale selekcjonuje "jakościowe" tokeny (fazy 1-6), ale **nie ma negatywnej warstwy detekcji sztucznych pumpów**:
- Sybil layer łapie wzorce botowe, nie schematy pump & dump
- Brak analizy: czy wzrost ceny jest organiczny czy sztucznie napompowany?
- Losing pools mają entry drift +4.32% vs +3.37% u winning pools
- 2 z 5 największych strat miały drift 26%+ (ekstremalny spike przed entry)

### RC3: Entry drift blind spot

System nie ma pojęcia "o ile cena już wzrosła zanim kupiliśmy":
- `BondingCurveDynamics.price_change_ratio` istnieje, ale `max_price_change_ratio = 9999.0` -- praktycznie wyłączone
- Drift 26.27% przechodzi bez problemu przez wszystkie filtry!
- Brak jednoznacznego `initial_pool_price` jako kotwicy do porównania. `BondingCurveDynamics.initial_price` bierze pierwszy punkt z `price_history`, który może być już po pierwszym skoku. V2.5 musi doprecyzować źródło kotwicy: preferowane `InitPoolEvent.initial_price_quote` / pierwszy authoritative Yellowstone AccountUpdate / AccountStateCore, z jasnym fallbackiem i flagą jakości.

### RC4: Prosperity Filter overfit

Trzy gałęzie skalibrowane na historycznych winnerach (look-ahead bias):
- Branch 1: sniped >= 28%, sell/buy <= 16% -- ultra selektywny
- Branch 2: mcap >= 55 SOL, dom >= 90% -- bardzo wysoko
- Branch 3: HHI <= 0.0416, FTDI >= 0.0909 -- ekstremalnie wąskie
- Rynek ewoluuje, statyczne gałęzie nie adaptują się
- Overlay (aktywny) dodatkowo zawęża przejście

### RC5: Brak analizy trajektorii momentum

Wszystkie metryki to point-in-time snapshoty. System nie analizuje:
- Czy momentum **akceleruje** (organiczny wzrost) czy **deceleruje** (gasnący pump)?
- Jak zmienia się HHI, volume, interval **w czasie** obserwacji?
- Czy aktywność jest równomierna czy spike'owa?
- To co wygląda dobrze na snapshotcie może być już w fazie spadkowej

### RC6: Brak rozważania exit risk przy entry

Gatekeeper decyduje BUY nie wiedząc:
- Przy jakiej cenie wchodzi względem trajektorii puli?
- Jaki jest potencjalny exit target?
- Czy pool ma wystarczającą rezerwę żeby nie zostać zdumpowanym?
- HyperPrediction Oracle działa jako **osobny** system post-Gatekeeper, bez sprzężenia zwrotnego

---

### 1.5. FUNDAMENTALNA ASYMETRIA -- Co 10s może, a czego nie może

To rozróżnienie jest **kluczowe** dla zrozumienia, dlaczego V2.5 jest zaprojektowany tak, a nie inaczej.

**10s WYSTARCZA na negatywną selekcję (wykrywanie pułapek):**

| Sygnał | Czas do wykrycia | Pewność | Dlaczego |
|--------|------------------|---------|----------|
| Entry drift (cena vs cena inicjalna) | natychmiast | **wysoka** | Fakt, nie estymacja |
| Ramping (4 consecutive same-size buy) | ~2-4s | **wysoka** | Wzorzec sekwencyjny, niski szum |
| Spike (volume rate 3s vs reszta) | ~5-8s | **średnia** | Wymaga kontrastu między oknami |
| Avg interval | ~3-5s | **wysoka** | Primary survival signal (p=1.2e-90) |
| Whale concentration (top3%) | ~5-8s | **średnia** | Przy małej liczbie TX może być przypadkowe |

**10s NIE WYSTARCZA na pozytywną predykcję (który pool zrobi 40%+):**

| Sygnał | Problem |
|--------|---------|
| HHI trajectory (T0→T1→T2) | Przy 5-10 TX na segment, HHI jest **bardzo noisy**. Jeden dodatkowy signer potrafi przestawić je o 0.1. |
| Volume consistency (CV między segmentami) | Pojedyncza duża transakcja generuje fałszywy sygnał spike'a przy małym samplu |
| True community vs cabal | Cabal potrafi **symulować decentralizację** przez pierwsze 15-20s |
| Graduation trajectory | Niemożliwe do oceny — potrzeba min. 30-60s |
| Który pool zrobi 10x vs 2x | Poza zasięgiem 10s — wymaga analizy społeczności, virality, trendów |

**Konsekwencja dla architektury V2.5:**

System **nie próbuje przewidzieć** przyszłości. System **eliminuje oczywiste pułapki** i wchodzi **wystarczająco wcześnie**, żeby zysk był realny. To nie jest "słabość" — to jest **świadoma decyzja projektowa**:

- **PDD działa jako kandydat na HARD VETO** — bo jego sygnały są mierzalne i dostępne w czasie decyzji. Nie znaczy to jednak, że interpretacja "pump & dump" jest pewna bez walidacji false-positive. Hard veto w live wolno aktywować dopiero po shadow-promocji.
- **TAS działa jako SOFT MODULATOR** — bo jego sygnały są częściowo noisy przy małym samplu. Nie blokuje samodzielnie (chyba że trajektoria jest ekstremalnie negatywna, score < 0.30).
- **DOW najpierw symuluje okno 3-7s w shadow** — bo live early-entry zmienia dotychczasowy kontrakt Gatekeepera (8-9s obserwacji po pool creation). Promocja do live wymaga ADR, shadow-burnin i potwierdzenia, że nie pogarsza precision.

### 1.6. Czego V2.5 NIE rozwiąże (jawne ograniczenia)

| Ograniczenie | Dlaczego | Czy V3 by to rozwiązał? |
|--------------|---------|--------------------------|
| Nie przewidzi, który pool zrobi 10x vs 2x | Wymaga 30-60s+ danych, analizy społecznościowej, virality | Częściowo — ciągłe okno daje więcej danych |
| Nie wyeliminuje wszystkich strat | Dev nieznany może zdumpować w T+12s — żadna analiza <10s tego nie przewidzi | Nie — to fundamentalne ograniczenie rynku |
| Nie zastąpi ciągłego monitoringu | V2.5 ma max 3 punkty decyzyjne, nie ciągły monitoring | Tak — Strike Engine V3 z ciągłym oknem |
| Nie oceni graduation potential | Potrzeba min. 30-60s na ocenę trajektorii graduacji | Tak — dłuższe okno to umożliwia |
| Nie wyeliminuje fałszywych sygnałów przy małym samplu | 10s to fizycznie za mało TX na statystycznie odporne metryki | Częściowo — więcej danych = lepsze estymacje |

### 1.7. Założenia walidacyjne i guardraile

V2.5 nie może być wdrożone jako natychmiastowa zmiana live execution. Kolejność promocji:

1. **Telemetry-only:** nowe pola JSONL v16, PDD/TAS/DOW/APS liczone i logowane, ale nie zmieniają werdyktu.
2. **Shadow decision:** dla każdej puli logować, co zrobiłby V2.5 w oknach 2-5s, 5-7s i 7-10s, przy zachowaniu live werdyktu V2/`long`.
3. **Replay + walk-forward:** walidacja na historycznych i nowych oknach czasowych, z raportem false rejects, false accepts, driftu progu i stabilności reżimowej.
4. **Partial live:** najpierw tylko PDD dla ekstremów o najwyższej separacji (np. bardzo wysoki drift / flash crash), potem ewentualnie DOW.
5. **Live early-entry:** dopiero po ADR aktualizującym kontrakt Gatekeepera, bo przejście z 8-9s obserwacji na 3-7s decyzję jest zmianą architektoniczną.

Minimalne kryteria promocji:

- brak pogorszenia precision względem V2 na shadow-burnin,
- jawny raport false-positive dla winnerów,
- osobna ablation dla PDD, TAS, DOW i APS,
- confidence intervals dla win rate / avg loss / profit,
- brak nowego źródła danych poza Yellowstone gRPC,
- brak RPC w ścieżce Gatekeepera,
- brak blokowania i brak alokacji w Revolver hot path.

---

## 2. ARCHITEKTURA ROZWIĄZANIA -- CZTERY MODUŁY V2.5

### Zasada wiodąca: asymetria selekcji + shadow-first rollout

System NIE próbuje być "wyrocznią przewidującą przyszłość". System działa jak **sito o dwóch oczkach**:
- **Grube oczko (PDD)**: wykrywa mierzalne, negatywne wzorce. W shadow loguje wszystkie flagi; w live hard veto tylko po promocji progów.
- **Drobne oczko (TAS + V2)**: wzmacnia lub osłabia confidence na podstawie kierunku zmian, ale nie powinno samodzielnie decydować przy małym samplu.
- **APS**: startuje jako warstwa offline/shadow, bo online adaptacja progów może wprowadzić feedback loop i overfit do ostatnich BUY.

```
                  ┌─────────────────────────────────────────────────┐
                  │         GATEKEEPER V2.5 DECISION FLOW           │
                  │                                                │
  Pool Events ───┤  0. PUMP & DUMP DETECTOR (pdd)  ← PIERWSZA!    │
                  │     ├─ Entry drift hard limit (< 5%)            │
                  │     ├─ Ramping pattern detection                │
                  │     ├─ Spike volume detection                   │
                  │     ├─ Whale concentration veto                 │
                  │     ├─ Reserve health check                     │
                  │     └─ Flash crash protection                   │
                  │     Shadow-first → live hard veto po promocji   │
                  │                                                │
                  │  1. DYNAMIC OBSERVATION WINDOW (dow)            │
                  │     ├─ Early: 2-5s → shadow decision point      │
                  │     ├─ Normal: 5-7s → shadow decision point     │
                  │     └─ Extended: 7-10s → live-compatible point  │
                  │                                                │
                  │  2. TRAJECTORY AWARE SCORING (tas)              │
                  │     ├─ SOFT modulator confidence score           │
                  │     ├─ Hard reject TYLKO przy ekstremalnie       │
                  │     │  negatywnej trajektorii (score < 0.30)     │
                  │     └─ Normalnie: obniża/zawiesza confidence     │
                  │                                                │
                  │  3. ADAPTIVE PROSPERITY (aps)                   │
                  │     ├─ Market regime detection                  │
                  │     ├─ Shadow threshold suggestions             │
                  │     └─ Offline branch calibration               │
                  │                                                │
                  │  EXISTING (unchanged, sprawdzone):              │
                  │  - 6-Phase analysis pipeline                    │
                  │  - 3-Layer decision (hard/core/soft)            │
                  │  - Sybil interference layer                     │
                  │  - IWIM Veto Gate                               │
                  └─────────────────────────────────────────────────┘
```

**Kluczowa zmiana względem v1.0 planu:** PDD jest pierwszą nową warstwą sprawdzaną po Hard Fails — przed Core, przed Soft, przed Sybil — ale jego wpływ na live verdict jest stopniowany: telemetry-only → shadow → hard veto dla ekstremów → pełna promocja. TAS pozostaje **modulatorem confidence score**, bo jego sygnały są częściowo noisy. Poprzedni alternatywny diagram kolejności został usunięty, żeby nie mieszać DOW/PDD/TAS w sprzecznych pozycjach.

### Zasady nienaruszalne:

1. **MaterializedFeatureSet** pozostaje SSOT -- NOWE pola dodawane jako optional, serializowane
2. **GatekeeperDecision** rozszerzane o nowe opcjonalne pola (v2.2 -> v2.5)
3. **JSONL logging** -- schema wersjonowana bump z v15 na v16, stare pola zachowane
4. **Wszystkie nowe progi** w sekcjach `[gatekeeper_v2.dow]`, `[gatekeeper_v2.tas]`, `[gatekeeper_v2.pdd]`, `[gatekeeper_v2.aps]` w TOML
5. **Nowy komponent** jako feature-flag obok istniejącego gatekeepera. Domyślnie `[gatekeeper_v2.v25].shadow_enabled = true`, `[gatekeeper_v2.v25].live_execution_enabled = false`.
6. **Testy regresji** -- wszystkie istniejące testy muszą przechodzić
7. **Kontrakt 8-9s obserwacji Gatekeepera** pozostaje live invariantem do czasu osobnego ADR. Decyzje 2-7s są najpierw wyłącznie shadow.
8. **Yellowstone gRPC pozostaje jedynym źródłem on-chain state**. V2.5 nie dodaje RPC do ścieżki decyzyjnej.

### Dlaczego nie pełny Strike Engine V3?

STRIKE_ENGINE_V3_PLAN.md to kompletny rewrite (estymacja 20+ dni). V2.5 to **chirurgiczna interwencja**:
- Zachowuje sprawdzone elementy V2: 6 faz, 3-layer decision, Sybil layer, IWIM
- Dodaje 4 brakujące moduły jako osobne komponenty
- Feature-flag umożliwia natychmiastowy rollback
- ~15 dni roboczych vs 20+ dla pełnego rewrite
- Większa przewidywalność -- mniej kodu do przetestowania

---

## 3. PLAN IMPLEMENTACJI -- 8 FAZ

### Faza 0: Rozpoznanie i zamrożenie kontraktów (1 dzień)

| Krok | Akcja | Pliki |
|------|-------|-------|
| 0.1 | Spisanie wszystkich pól `GatekeeperDecision` używanych przez konsumentów | `gatekeeper.rs`, `decision_logger.rs`, `shadow_ledger/*`, `trigger/*` |
| 0.2 | Spisanie ścieżek wywołania `evaluate_policy_from_assessment()` | `oracle_runtime.rs`, `gatekeeper_commit_loop.rs` |
| 0.3 | Spisanie testów integracyjnych Gatekeepera | `ghost-launcher/tests/*` |
| 0.4 | Potwierdzenie struktury `MaterializedFeatureSet` i dostępnych pól | `ghost-core/src/checkpoint/types.rs` |
| 0.5 | Stworzenie brancha `refactor/gatekeeper-v25` | git |
| 0.6 | Dokument `GATEKEEPER_V25_SSOT_CONTRACTS.md` z listą niezmienników | `PLANS/` |

**Output:** `PLANS/GATEKEEPER_V25_SSOT_CONTRACTS.md`

---

### Faza 1: Rozszerzenie konfiguracji TOML + Rust struct (1 dzień)

#### Krok 1.1: Nowe sekcje w `ghost_brain_config.toml`

```toml
# ══════════════════════════════════════════════════════════════════════════════
# GATEKEEPER V2.5: Rollout guardrails
# ══════════════════════════════════════════════════════════════════════════════
[gatekeeper_v2.v25]
shadow_enabled = true
live_execution_enabled = false
require_promotion_adr = true
emit_shadow_decisions = true
emit_ablation_fields = true

# ══════════════════════════════════════════════════════════════════════════════
# GATEKEEPER V2.5: Dynamic Observation Window (dow)
# ══════════════════════════════════════════════════════════════════════════════
[gatekeeper_v2.dow]
enabled = true
# Early entry: ultra-restrykcyjny shadow verdict (2-5s)
early_entry_enabled = true
early_entry_min_ms = 2000
early_entry_max_ms = 5000
early_entry_min_confidence = 0.85        # wyższy niż w v1.0 (było 0.80)
early_entry_min_tx_count = 15
early_entry_min_phases_passed = 6        # wszystkie 6/6 (było 5)
early_entry_min_momentum = 0.40          # wyraźnie powyżej progu alpha
early_entry_max_sybil_points = 1         # praktycznie czysto
early_entry_max_entry_drift_pct = 3.0    # ostrzej niż normalny PDD (5%)
# Normal window: główna hipoteza skrócenia decyzji (5-7s)
normal_window_ms = 7000                  # skrócone z 8000ms
normal_window_min_confidence = 0.65
# Extended window: borderline live-compatible point (7-10s)
extended_window_ms = 10000
extended_window_min_confidence = 0.55
extended_require_pdd_clean = true        # extended tylko jeśli PDD czysty

# ══════════════════════════════════════════════════════════════════════════════
# GATEKEEPER V2.5: Trajectory Aware Scoring (tas)
# UWAGA: TAS jest SOFT MODULATOREM confidence, nie hard gate'm.
# Ekstremalnie negatywna trajektoria (< tas_hard_reject_threshold)
# jest najpierw shadow reject; live reject dopiero po promocji.
# ══════════════════════════════════════════════════════════════════════════════
[gatekeeper_v2.tas]
enabled = true
# Shadow hard reject TYLKO przy ekstremalnie negatywnej trajektorii
tas_hard_reject_threshold = 0.30         # było 0.60 jako soft gate
# Modulacja confidence: range [0.75, 1.25]
tas_confidence_modulator_min = 0.75      # przy tas_score = 0.30
tas_confidence_modulator_max = 1.25      # przy tas_score = 1.00
# Minimalna liczba TX na segment, żeby TAS był liczony
tas_min_tx_per_segment = 3
tas_min_total_duration_ms = 3000
# Wagi wymiarów trajektorii (bez zmian)
momentum_trajectory_weight = 0.25
momentum_accel_min_ratio = 1.15
momentum_decel_max_ratio = 0.85
hhi_trajectory_weight = 0.20
hhi_decline_min_ratio = 0.85
volume_trajectory_weight = 0.20
volume_cv_max = 0.60
interval_trajectory_weight = 0.15
interval_shortening_min_ratio = 0.80
buy_ratio_trajectory_weight = 0.20
buy_ratio_stability_min = 0.55

# ══════════════════════════════════════════════════════════════════════════════
# GATEKEEPER V2.5: Pump & Dump Detector (pdd)
# ══════════════════════════════════════════════════════════════════════════════
[gatekeeper_v2.pdd]
enabled = true
# Entry drift -- CORE PROTECTION
entry_drift_max_pct = 5.0
entry_drift_soft_max_pct = 3.0
entry_drift_soft_weight = 2
# Spike pattern detection
spike_detection_enabled = true
spike_observation_window_ms = 3000
spike_ratio_threshold = 2.0
spike_hard_veto = true
spike_soft_penalty = 3
# Reserve health
reserve_min_sol = 30.0
reserve_min_ratio = 0.15
# Whale concentration
whale_top3_max_pct = 60.0
whale_top3_size_max_sol = 15.0
whale_single_max_pct = 35.0
# Ramping detection
ramping_detection_enabled = true
ramping_min_consecutive_buys = 4
ramping_size_tolerance_pct = 15.0
ramping_hard_veto = true
# Flash crash protection
flash_crash_protection_enabled = true
flash_crash_max_price_impact_pct = 15.0
flash_crash_sell_cluster_max_ms = 500

# ══════════════════════════════════════════════════════════════════════════════
# GATEKEEPER V2.5: Adaptive Prosperity (aps)
# UWAGA: APS startuje jako shadow/offline. Nie wolno automatycznie zmieniać
# progów live bez promocji po walk-forward i osobnego ADR.
# ══════════════════════════════════════════════════════════════════════════════
[gatekeeper_v2.aps]
enabled = true
adaptive_enabled = false
shadow_suggestions_enabled = true
adaptation_interval_buys = 50
calibration_lookback_buys = 100
min_calibration_samples = 30
branch_weight_adaptation = true
branch_weight_learning_rate = 0.05
branch_min_weight = 0.15
regime_detection_enabled = true
regime_lookback_hours = 4
regime_high_volatility_threshold = 0.6
regime_high_vol_entry_drift_max_pct = 3.0
regime_high_vol_confidence_min = 0.70
regime_normal_entry_drift_max_pct = 5.0
regime_normal_confidence_min = 0.60
```

#### Krok 1.2: Nowe struktury w `ghost_brain_config.rs`

W `GatekeeperV2Config` dodać:
```rust
#[serde(default)]
pub v25: GatekeeperV25RolloutConfig,
#[serde(default)]
pub dow: DynamicObservationWindowConfig,
#[serde(default)]
pub tas: TrajectoryAwareScoringConfig,
#[serde(default)]
pub pdd: PumpAndDumpDetectorConfig,
#[serde(default)]
pub aps: AdaptiveProsperityConfig,
```

Każdy struct definiowany w nowym pliku `ghost-brain/src/config/gatekeeper_v25_config.rs` z `#[derive(Debug, Clone, Serialize, Deserialize)]` i `#[serde(default)]` na każdym polu.

#### Krok 1.3: Testy deserializacji

```rust
#[test]
fn parse_gatekeeper_v25_config() {
    let config: GatekeeperV2Config = toml::from_str(include_str!("../../ghost_brain_config.toml"))
        .expect("ghost_brain_config.toml should deserialize with v2.5 sections");
    assert!(config.dow.enabled);
    assert!(config.pdd.entry_drift_max_pct == 5.0);
}
```

**Rezultat:** Nowe pola w konfiguracji, wstecznie kompatybilne (wszystko ma `default`).

---

### Faza 2: Dynamiczne Okno Obserwacji (dow) -- shadow-first (2 dni)

#### Koncepcja "złotego okna" (3-7s)

Z danych raportu:
- Losing pools: średni czas pozycji 139s, dump trwa 15-30s
- Winning pools: średni czas pozycji 194s
- Typowy pump.fun cykl: kreacja → 2-3s ciszy → 3-6s pump → 8-15s dump (jeśli scam)

**Hipoteza złotego okna 3-7s**: wystarczająco dużo TX, żeby część metryk była użyteczna, a jednocześnie przed typowym oknem dumpu. To wymaga walidacji, bo obecny hard invariant projektu zakłada obserwację Gatekeepera przez 8-9s po pool creation. Dlatego DOW w pierwszym wdrożeniu loguje shadow decyzje, ale nie skraca live observation window.

#### Krok 2.1: Trzy deadline'y w `GatekeeperBuffer` (rekalibracja)

```
Okno        Czas    Status v2.5       Przeznaczenie
─────────────────────────────────────────────────────
Early       2-5s    shadow-only       Ultra-restrykcyjny kandydat early entry
Normal      5-7s    shadow-only       Główna hipoteza skrócenia decyzji
Extended    7-10s   live-compatible   Borderline + zgodność z obecnym kontraktem
```

**Uzasadnienie:** celem nie jest założenie, że 85% BUY musi zapadać przed 7s, tylko sprawdzenie w shadow, ile historycznych i bieżących BUY mogłoby bezpiecznie zapaść przed 7s bez utraty precision.

```rust
// W GatekeeperBuffer (rekalibracja):
early_deadline_ms: u64,    // registered_wall_ts_ms + 5000ms
normal_deadline_ms: u64,   // registered_wall_ts_ms + 7000ms
extended_deadline_ms: u64, // registered_wall_ts_ms + 10000ms
window_stage: ObservationStage, // Early, Normal, Extended
```

#### Krok 2.2: Early Entry -- ultra-restrykcyjny shadow verdict (2-5s)

Przy 2-5s mamy ekstremalnie mało danych. Trajektoria nie ma sensu (segmenty się nie uformowały). Dlatego early entry wymaga **wszystkiego** poniższego:

1. **Wszystkie PDD checki PASS** — drift < 3% (nie 5% — ostrzej!), brak ramping, brak spike
2. **Wszystkie 6 faz PASS** — nie 5/6, pełne 6/6
3. **Minimum 15 TX** — żeby metryki nie były czystym szumem
4. **Confidence >= 0.85** — wyższy próg niż w v1.0 planu (było 0.80)
5. **Alpha Gate PASS z momentum > 0.40** — wyraźnie powyżej progu
6. **Sybil clean** — max 1 sybil soft point

**Filozofia:** Early entry to najpierw kontrfaktyczny werdykt: "czy system miałby wystarczające przesłanki, żeby wejść wcześniej?". Live system kontynuuje obserwację do obecnego punktu decyzyjnego, dopóki shadow nie udowodni stabilnej przewagi.

```rust
// W oracle_runtime.rs, po każdym TX gdy elapsed >= 2000ms && elapsed <= 5000ms:
if elapsed >= config.dow.early_entry_min_ms && elapsed <= config.dow.early_entry_max_ms 
    && buffer.tx_count >= 15 
{
    let assessment = build_assessment_from_features(&features, config, context);
    let pdd = evaluate_pdd(&buffer, &assessment, pool_data, &config.pdd);
    
    // Early entry: PDD musi być całkowicie czysty
    if pdd.hard_fail_reason.is_some() || pdd.entry_drift_pct > Some(3.0) {
        // Nie spełnia ultra-restrykcyjnych warunków -> kontynuuj
        return ContinueObservation;
    }
    
    let decision = evaluate_policy_from_assessment(&assessment, config);
    let confidence = compute_v25_confidence(&assessment, &pdd, None, config);
    
    // Wszystkie 6 faz + confidence >= 0.85 + momentum > 0.40
    if assessment.phases_passed >= 6 
        && confidence >= 0.85 
        && decision.alpha_momentum > 0.40
        && decision.sybil_soft_points <= 1
    {
        record_shadow_v25_decision(ShadowDecisionKind::EarlyBuy, &assessment, &pdd);
        return ContinueObservation;
    }
    // else: kontynuuj obserwację
}
```

#### Krok 2.3: Normal Window -- główna ścieżka (5-7s)

To jest główna hipoteza skrócenia decyzji, którą trzeba zmierzyć w shadow. W tym oknie:
- Mamy wystarczająco dużo TX na wiarygodną trajektorię (min. 4-5 TX na segment)
- Jesteśmy przed typowym oknem dumpu
- TAS działa jako modulator confidence, nie hard gate

```rust
// Przy deadline 7000ms:
let trajectory = materialize_trajectory(&buffer);
let tas = evaluate_trajectory(&trajectory, &config.tas);
let confidence = compute_v25_confidence(&assessment, &pdd, Some(&tas), config);
// confidence >= 0.65 -> BUY
```

#### Krok 2.4: Extended Window -- borderline (7-10s)

Tylko dla pooli, które:
- Mają za mało TX w 7s, żeby podjąć decyzję
- Nie wykazują **żadnych** sygnałów PDD
- Mają przynajmniej neutralną trajektorię

```rust
// Przy deadline 10000ms:
// Niższy próg confidence (>= 0.55), ale PDD musi być czysty
if pdd.hard_fail_reason.is_none() 
    && pdd.pdd_score > 0.7 
    && confidence >= 0.55 
{
    return BUY;
}
```

**Rezultat fazy 2:** JSONL v16 zawiera `shadow_early_verdict`, `shadow_normal_verdict`, `shadow_extended_verdict`, `shadow_decision_elapsed_ms` i reason-chain dla każdego punktu. Live BUY przed 7s pozostaje wyłączony do czasu promocji.

---

### Faza 3: Trajectory Aware Scoring (tas) (2 dni)

#### Dlaczego TAS jako SOFT modulator, nie hard gate

Trajektoria dzieli okno na 3 segmenty czasowe (T0, T1, T2). Przy 10-20 TX w całym oknie, każdy segment ma 3-7 TX. To wystarcza na **sygnał kierunkowy**, ale NIE na precyzyjny pomiar statystyczny:

- HHI przy 5 TX w segmencie T2: jeden dodatkowy signer może przestawić HHI o 0.1 — czyli o 65% progu fazy 3
- Buy ratio przy 3 TX: jedna transakcja to 33%
- Volume CV: pojedyncza duża transakcja (np. 5 SOL buy) generuje fałszywy sygnał spike'a

Dlatego TAS **nie blokuje samodzielnie** decyzji BUY w pierwszej fazie rollout. Zamiast tego **moduluje confidence score** w dół, proporcjonalnie do tego jak bardzo trajektoria odbiega od ideału. Ekstremalnie negatywna trajektoria `< 0.30` jest najpierw logowana jako `shadow_reject_low_trajectory`; live hard reject wymaga osobnej promocji po walidacji false-positive.

#### Krok 3.1: Minimalne wymagania dla segmentów

```rust
pub const MIN_TX_PER_SEGMENT: usize = 3; // minimum, żeby segment był wiarygodny

pub fn materialize_trajectory(buffer: &GatekeeperBuffer) -> Option<TrajectorySegments> {
    let total_duration = buffer.highest_seen_ts - buffer.registered_wall_ts_ms;
    if total_duration < 3000 {
        return None; // za krótko na sensowną trajektorię
    }
    
    // ... podział na T0/T1/T2 ...
    
    // Trajektoria tylko jeśli KAŻDY segment ma >= MIN_TX_PER_SEGMENT
    if segments.t0.tx_count < MIN_TX_PER_SEGMENT 
        || segments.t1.tx_count < MIN_TX_PER_SEGMENT 
        || segments.t2.tx_count < MIN_TX_PER_SEGMENT 
    {
        return None; // za mało danych — TAS nie ingeruje
    }
    
    Some(segments)
}
```

Jeśli trajektoria jest `None` (za mało danych) — TAS **nie wpływa** na decyzję. Confidence score opiera się wyłącznie na pozostałych modułach.

#### Krok 3.2: Pięć wymiarów scoringu trajektorii (bez zmian względem v1.0)

1. **Momentum acceleration** (waga 0.25): T2/T0 TX count ratio. > 1.15 = akceleracja. < 0.85 = deceleracja.
2. **HHI trajectory** (waga 0.20): T2/T0 HHI ratio. < 0.85 = decentralizacja (dobrze). > 1.0 = cabal (źle).
3. **Volume consistency** (waga 0.20): CV volume między T0/T1/T2. Niskie CV = równomierne.
4. **Interval trajectory** (waga 0.15): T2/T0 avg_interval ratio. < 0.80 = skracanie (dobrze).
5. **Buy ratio stability** (waga 0.20): min(buy_ratio w T2, 0.55). Stabilny = zdrowy popyt.

```rust
overall_tas_score = 0.25 * momentum + 0.20 * hhi_traj 
    + 0.20 * volume_cons + 0.15 * interval_traj + 0.20 * buy_ratio_stab
```

#### Krok 3.3: TAS jako modulator confidence score (ZMIANA WZGLĘDEM v1.0)

```rust
pub fn compute_v25_confidence(
    assessment: &GatekeeperAssessment,
    pdd: &PumpAndDumpAssessment,
    tas: Option<&TrajectoryAssessment>,
    config: &GatekeeperV2Config,
) -> f64 {
    // Bazowa jakość: ile faz przeszło (0-1), jak czysto przeszedł core
    let base_quality = assessment.phases_passed as f64 / 6.0;
    
    // Sygnał z Alpha Gate
    let alpha_quality = clamp01(
        assessment.alpha_momentum * 0.4 
        + assessment.alpha_demand * 0.35 
        + assessment.alpha_joint * 0.25
    );
    
    // Modulator PDD: (1 - PDD_penalty), gdzie penalty = 1 - pdd_score
    // pdd_score = 1.0 (czysty) → brak kary
    // pdd_score = 0.0 (hard fail) → już by było REJECT, tu nie dochodzi
    let pdd_modulator = 0.7 + 0.3 * pdd.pdd_score; // zakres [0.7, 1.0]
    
    // Modulator TAS: jeśli trajectory dostępna, moduluje ±25%
    let tas_modulator = if let Some(tas) = tas {
        if tas.overall_tas_score < 0.30 {
            // Ekstremalnie negatywna trajektoria → shadow reject candidate.
            // Live hard reject wymaga promocji po walidacji false-positive.
            return 0.0; // → shadow_reject_low_trajectory
        }
        // Normalny zakres: TAS score 0.30-1.0 → modulator 0.75-1.25
        0.75 + 0.50 * tas.overall_tas_score
    } else {
        1.0 // brak danych trajektorii → nie wpływa
    };
    
    // Modulator Sybil: czysty → 1.0, max sybil points → 0.80
    let sybil_modulator = 1.0 - 0.20 * (assessment.sybil_soft_points as f64 
        / config.max_sybil_soft_points.max(1) as f64);
    
    let confidence = base_quality 
        * alpha_quality 
        * pdd_modulator 
        * tas_modulator 
        * sybil_modulator;
    
    clamp01(confidence)
}
```

**Kluczowa zmiana:** TAS nie jest osobnym gate'em z progiem `tas_min_score = 0.60`. Jest **modulatorem** — obniża confidence o max 25% przy niskim TAS score, podnosi o max 25% przy wysokim. Tylko ekstremalnie negatywna trajektoria (< 0.30) może zostać kandydatem do hard reject, ale początkowo pozostaje shadow-only.

**Dlaczego to jest lepsze:** Pool z wysokim base_quality (5/6 faz), czystym PDD i dobrym alpha, ale słabą trajektorią (TAS = 0.45) — przejdzie z confidence ~0.55 zamiast ~0.68. Może to być nadal dobry BUY, tylko z niższą pewnością. Pool z TAS = 0.25 dostaje `shadow_reject_low_trajectory`, ale live decyzja pozostaje zgodna z dotychczasowym pipeline'em do czasu promocji.

#### Krok 3.4: Integracja do pipeline'u

```rust
// W evaluate_policy_from_assessment(), PO alpha gate, PRZED prosperity:
let tas = if let Some(traj) = assessment.trajectory.as_ref() {
    evaluate_trajectory(traj, &config.tas)
} else {
    None
};

// TAS hard reject pozostaje shadow-only do czasu promocji
if let Some(ref tas_result) = tas {
    if tas_result.overall_tas_score < 0.30 {
        record_shadow_v25_decision(
            ShadowDecisionKind::RejectLowTrajectory,
            format!("TAS_EXTREME: score={:.3} < 0.30", tas_result.overall_tas_score),
        );
    }
}

// Confidence obliczane z użyciem TAS jako modulatora (nie gate'a)
let confidence = compute_v25_confidence(
    &assessment, &pdd, tas.as_ref(), config
);
```

**Rezultat:** System rozumie KIERUNEK zmian, ale nie podejmuje decyzji na podstawie noisy danych. Trajektoria wzmacnia lub osłabia pewność, zamiast zero-jedynkowo blokować.

---

### Faza 4: Pump & Dump Detector (pdd) -- PIERWSZA WARSTWA (2 dni)

#### Dlaczego PDD jest pierwszą warstwą (przed Core)

PDD mierzy **obserwowalne fakty**, ale ich interpretacja jako pump & dump nadal wymaga walidacji:
- Entry drift 26% to fakt, że cena już wystrzeliła względem kotwicy, ale próg live musi udowodnić niski false-positive.
- Ramping 5 consecutive same-size buyów to silna sygnatura automatyzacji, ale trzeba odróżnić bot-ramping od normalnego flow przez agregatory.
- Top 3 walletów > 60% volume to matematyczny fakt koncentracji, ale przy małym `n` może być artefaktem pierwszych transakcji.

Nie ma sensu ignorować takich sygnałów do końca pipeline'u — każdy z późniejszych gate'ów może zobaczyć "dobry" snapshot i przepuścić kandydat. **Kolejność ma znaczenie:** PDD jest liczony przed Core i logowany natychmiast. Wpływ na live verdict zależy od poziomu promocji: telemetry-only, shadow reject, hard veto dla ekstremów, pełny hard veto.

#### Krok 4.1: Nowy plik `gatekeeper_pdd.rs`

Sześć mechanizmów detekcji:

##### 4.1.1 Entry Drift Detection (NAJWAŻNIEJSZY)

```rust
drift_pct = ((current_price / initial_pool_price) - 1.0) * 100.0
```

- > 5% -> **shadow hard reject**, a po promocji potencjalny live veto (kupowalibyśmy już po pumpie)
- 3-5% -> soft penalty (obniża pdd_score)
- W reżimie HighVolatility: > 3% -> shadow hard reject; live dopiero po promocji

**Źródło ceny inicjalnej:** nie używać ślepo `BondingCurveDynamics.initial_price`, jeśli pierwszy punkt `price_history` może pochodzić już po skoku. Preferowana kolejność kotwicy:

1. `InitPoolEvent.initial_price_quote` z Yellowstone-derived pool init,
2. pierwszy authoritative AccountUpdate z AccountStateCore,
3. pierwszy fresh `curve_data_known=true` punkt z `price_history`,
4. fallback do pierwszego punktu `price_history` tylko z flagą `entry_drift_anchor_quality = "weak"`.

**Uzasadnienie z danych:** 2 z 5 największych strat miały drift 26%+. To nie jest subtelny sygnał. To jest czerwona flaga widoczna gołym okiem, ale twierdzenie "żaden legitny pool" należy zastąpić walidacją: shadow musi pokazać, ile winnerów miałoby drift > 5%, > 7% i > 10%.

##### 4.1.2 Spike Pattern Recognition

Analizuje ostatnie 3s vs resztę okna. Jeśli volume rate (volume/duration) w ostatnich 3s > 2x volume rate wcześniej -> spike detected.
- Spike + hard_veto = shadow hard reject; live dopiero po promocji
- Spike bez hard_veto = silna penalizacja pdd_score (-0.25)

##### 4.1.3 Ramping Detection

Szuka sekwencji consecutive buyów o podobnym rozmiarze (tolerancja 15%). >= 4 consecutive same-size buys = bot ramping.
- Ramping + hard_veto = shadow hard reject; live dopiero po promocji
- Ramping bez hard_veto = silna penalizacja pdd_score (-0.30)

**Dlaczego ramping jest unikalny:** Sybil layer patrzy na strukturalne wzorce (FTDI, DBIA, DES), ale nie na **sekwencyjny pattern transakcji**. Organiczny trader nie kupuje 4 razy pod rząd dokładnie 0.5 SOL. Robi to bot pompujący cenę przed dystrybucją.

##### 4.1.4 Whale Concentration Check

Top 3 walletów > 60% volume lub single wallet > 35% = shadow hard reject; live dopiero po promocji.

##### 4.1.5 Reserve Health Check

Szacowana rezerwa bonding curve < 30 SOL lub reserve/mcap < 15% = shadow hard reject; live dopiero po promocji.

**Uzasadnienie:** Mała rezerwa = łatwy dump. Dumper potrzebuje zaledwie 10-20% pozycji żeby zbić kurs o 70% przy rezerwie < 50 SOL.

##### 4.1.6 Flash Crash Protection

Pojedynczy sell impact > 15% lub 2+ selle w 500ms = shadow hard reject; live dopiero po promocji.

#### Krok 4.2: Integracja PDD jako PIERWSZEJ warstwy (ZMIANA WZGLĘDEM v1.0)

```rust
// W evaluate_policy_from_assessment(), NATYCHMIAST po hard-fail check:
// PDD jest PIERWSZĄ nową warstwą — przed Core, przed Soft, przed Sybil.
// Domyślnie loguje shadow reject; live hard reject tylko po promocji progu.

if config.pdd.enabled {
    if let Some(ref pdd) = assessment.pdd_assessment {
        // PDD Hard Fail — najpierw shadow reject, potem opcjonalny live veto
        if let Some(ref fail_reason) = pdd.hard_fail_reason {
            record_shadow_v25_decision(
                ShadowDecisionKind::RejectPumpAndDump,
                format!("PDD_HARD_FAIL: {:?}", fail_reason),
            );
            if !config.v25.live_execution_enabled || !pdd.threshold_promoted_to_live {
                continue_existing_v2_pipeline();
            }
            return GatekeeperDecision {
                verdict_type: GatekeeperVerdictType::RejectPumpAndDump,
                verdict_buy: false,
                reason_chain: format!("PDD_HARD_FAIL: {:?} entry_drift={:?} spike={} ramping={} whale={}", 
                    fail_reason,
                    pdd.entry_drift_pct,
                    pdd.spike_detected,
                    pdd.ramping_detected,
                    pdd.whale_top3_pct,
                ),
                // ...
            };
        }
    }
}

// Potem dopiero: Core Layer, Soft, Sybil, Alpha, TAS, Prosperity...
```

**Rezultat fazy 4:** wszystkie PDD flagi są widoczne w JSONL i w ablation report. Live PDD blokuje tylko progi awansowane po shadow; reszta pozostaje obserwacją diagnostyczną.

---

### Faza 5: Adaptive Prosperity (aps) -- shadow/offline (2 dni)

APS jest najbardziej podatną na overfit częścią V2.5. Nie wolno zaczynać od online auto-dostosowania progów, bo system uczyłby się na własnych zaakceptowanych BUY i łatwo wpadłby w feedback loop. Pierwsza wersja APS generuje **sugestie progów** oraz `market_regime`, ale nie zmienia live verdict.

#### Krok 5.1: Nowy plik `gatekeeper_adaptive_prosperity.rs`

##### Reżim rynkowy

```rust
enum MarketRegime {
    HighVolatility,  // >60% pooli ma HHI spike lub anomalne zachowanie
    Normal,
    LowVolatility,   // rynek spokojny
}
```

Detekcja: analiza ostatnich N decyzji i shadow outcomes pod kątem HHI spike (>2x historyczne) i price spike (>3x baseline). Reżim musi być wykrywalny w czasie rzeczywistym; jeśli `min_calibration_samples` nie jest spełnione, fallback to `Normal` i brak live modyfikacji progów.

##### Adaptacyjne progi

| Próg | HighVol | Normal | LowVol |
|------|---------|--------|--------|
| Entry drift max | 3.0% | 5.0% | 5.0% |
| Prosperity mcap floor | +30% | bazowy | -15% |
| Confidence min | 0.70 | 0.60 | 0.60 |
| Branch 1 sniped | +20% | bazowy | -15% |
| Branch 3 HHI max | -20% | bazowy | +30% |

#### Krok 5.2: Sliding-window kalibracja gałęzi -- bez live mutation

Co 50 BUY decyzji, analizuj ostatnie 100 pod kątem:
- Która gałąź dała najwięcej zyskownych pozycji?
- W shadow wylicz sugerowane zwiększenie wagi skutecznych gałęzi i zmniejszenie nieskutecznych
- Tempo symulowanej adaptacji: 5% na iterację
- Nie mutować progów live bez osobnego pliku kalibracji zaakceptowanego po walk-forward

#### Krok 5.3: Integracja z `evaluate_prosperity_filter()`

Przed ewaluacją gałęzi, pobierz adaptacyjne progi zależne od reżimu i zapisz je w telemetry jako `aps_shadow_thresholds`. Dopóki `adaptive_enabled=false`, `evaluate_prosperity_filter()` używa statycznych wartości z configu, a APS tylko raportuje kontrfaktyczny verdict.

**Rezultat fazy 5:** APS dostarcza raport regime/threshold suggestion i ablation, ale nie wpływa na live do czasu osobnej promocji.

---

### Faza 6: Nowe struktury danych i rozszerzenia SSOT (1 dzień)

#### Krok 6.1: Rozszerzenie `GatekeeperAssessment`

```rust
pub struct GatekeeperAssessment {
    // ... istniejące pola BEZ ZMIAN ...
    
    // ── V2.5 extensions ──
    pub trajectory: Option<TrajectoryAssessment>,
    pub pdd_assessment: Option<PumpAndDumpAssessment>,
    pub observation_stage: Option<ObservationStage>,
    pub entry_drift_pct: Option<f64>,
    pub entry_drift_anchor_quality: Option<EntryDriftAnchorQuality>,
    pub v25_confidence: Option<f64>,
    pub v25_shadow_decisions: Vec<ShadowV25Decision>,
    pub adaptive_thresholds_applied: bool,
}
```

#### Krok 6.2: Nowe warianty `GatekeeperVerdictType`

```rust
RejectPumpAndDump,        // PDD hard fail (po promocji progu)
RejectLowTrajectory,      // TAS score too low (po promocji progu)
RejectEntryDrift,         // Entry drift > max (po promocji progu)
RejectFlashCrash,         // Flash crash protection (po promocji progu)
RejectRamping,            // Ramping pattern detected (po promocji progu)
EarlyBuy,                 // Live early entry tylko po ADR/promocji
```

W shadow nie dodawać osobnych live wariantów do `GatekeeperVerdictType`. Kontrfaktyczne decyzje zapisywać w `ShadowV25Decision`, żeby nie mylić realnego verdict path z symulacją.

#### Krok 6.3: Rozszerzenie `GatekeeperBuyLog`

Nowe pola w schema v16 JSONL:
- `entry_drift_pct`, `pdd_hard_fail`, `pdd_soft_flags`
- `entry_drift_anchor_source`, `entry_drift_anchor_quality`
- `tas_momentum_score`, `tas_overall_score`
- `observation_stage`, `v25_confidence`, `market_regime`
- `shadow_early_verdict`, `shadow_normal_verdict`, `shadow_extended_verdict`
- `shadow_pdd_reject_reason`, `shadow_tas_reject_reason`, `aps_shadow_thresholds`

Bump `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` do `16`.
Bump `GATEKEEPER_VERSION` do `"v2.5"`.

---

### Faza 7: Testy regresji i integracyjne (2 dni)

#### Testy jednostkowe PDD

- `test_entry_drift_shadow_hard_reject` -- drift 10% -> shadow reject
- `test_entry_drift_soft_pass` -- drift 4% -> soft flag
- `test_spike_pattern_detection` -- spike volume w ostatnich 3s
- `test_ramping_detection` -- 5 kolejnych buyów o tym samym rozmiarze
- `test_whale_concentration_shadow_veto` -- top3 > 60% volume
- `test_reserve_health` -- mała rezerwa -> REJECT

#### Testy jednostkowe TAS

- `test_momentum_acceleration_positive` -- T2 więcej TX niż T0
- `test_momentum_deceleration_negative` -- T2 mniej TX niż T0
- `test_hhi_decline_during_observation` -- HHI maleje
- `test_volume_spike_detection` -- wysokie CV między segmentami

#### Backtest na historycznych danych

```rust
#[test]
fn test_v25_vs_historical_losing_pools() {
    // 26.27% drift -> shadow_reject_entry_drift_extreme
    // 26.28% drift -> shadow_reject_entry_drift_extreme
    // Raportuje coverage: ile losing pools byłoby odrzuconych przez PDD
}

#[test]
fn test_v25_vs_historical_winning_pools() {
    // Raportuje false-positive: ilu winnerów PDD/TAS odrzuciłby w shadow
    // Test nie zakłada z góry 0 false-positive; wymaga jawnego threshold report
}
```

#### Wszystkie istniejące testy muszą przechodzić

```bash
cargo test -p ghost-launcher --lib -- gatekeeper
cargo test -p ghost-launcher -- test_v25
cargo test --workspace
```

---

### Faza 8: Dokumentacja, shadow-burnin, rollout (2 dni)

#### Feature flag i rollback

```toml
# W ghost_brain_config.toml:
mode = "long"  # live bez zmian na etapie shadow

[gatekeeper_v2.v25]
shadow_enabled = true
live_execution_enabled = false
```

Rollback: `shadow_enabled = false` -- natychmiastowy powrót do czystego starego zachowania bez zmiany `mode`.

#### Shadow-burnin

1. Uruchomić system w trybie shadow (`mode = "long"`, `v25.shadow_enabled = true`) na 2000 pulach
2. Porównać metryki vs istniejący Gatekeeper V2
3. Analiza logów JSONL -- które losing pools V2.5 odrzuciłby, a które winning pools fałszywie odrzuciłby?
4. Ablacja PDD/TAS/DOW/APS osobno, bez mieszania efektów
5. Walk-forward na kolejnym oknie danych, bez ponownego strojenia progów
6. Iteracyjna kalibracja progów tylko w pliku konfiguracyjnym / planie kalibracji

#### Kryteria akceptacji

| Metryka | V2 (obecny) | V2.5 shadow target |
|---------|-------------|---------------|
| Win Rate | ~42% | >= 65% |
| Avg Loss | -52.26% | < -15% |
| Avg Profit | +50-100% | +50-110% |
| False Positive | niepewne | raportowane z CI; live max po akceptacji |
| Entry Drift Avg (losing) | +4.32% | < +2% (all BUY) |
| Czas do decyzji (avg) | 10s | 5-8s |
| Czas do decyzji (early) | N/A | 2-5s |

Te wartości są kryteriami promocji po shadow, nie obietnicą wpływu samego wdrożenia telemetry.

---

## 4. MAPOWANIE PLIKÓW DO ZMIAN

### Pliki do stworzenia (NOWE):

| Plik | Zawartość |
|------|-----------|
| `ghost-launcher/src/components/gatekeeper_pdd.rs` | Pump & Dump Detector |
| `ghost-launcher/src/components/gatekeeper_trajectory.rs` | Trajectory Aware Scoring |
| `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs` | Adaptive Prosperity |
| `ghost-brain/src/config/gatekeeper_v25_config.rs` | V2.5 config structs |
| `ghost-launcher/tests/gatekeeper_pdd_tests.rs` | Testy PDD |
| `ghost-launcher/tests/gatekeeper_tas_tests.rs` | Testy TAS |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | Testy regresji vs V2 |

### Pliki do modyfikacji (ISTNIEJĄCE):

| Plik | Zmiana |
|------|--------|
| `ghost-brain/ghost_brain_config.toml` | Dodać sekcje `[gatekeeper_v2.v25/dow/tas/pdd/aps]`; na etapie shadow zostawić `mode = "long"` |
| `ghost-brain/src/config/ghost_brain_config.rs` | Dodać pola `v25`, `dow`, `tas`, `pdd`, `aps`; nie dodawać live wariantu trybu V25 przed ADR |
| `ghost-launcher/src/components/gatekeeper.rs` | Rozszerzyć `GatekeeperAssessment`, `GatekeeperVerdictType`, `GatekeeperBuffer` |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | Dodać warstwę PDD i TAS, zmodyfikować `evaluate_prosperity_filter()` |
| `ghost-launcher/src/oracle_runtime.rs` | Dodać shadow ścieżkę V2.5 z kontrfaktycznymi punktami ewaluacji; live tryb V2.5 dopiero po ADR |
| `ghost-brain/src/oracle/decision_logger.rs` | Nowe pola JSONL, bump schema do v16 |
| `ghost-launcher/src/components/mod.rs` | Dodać `pub mod gatekeeper_pdd/trajectory/adaptive_prosperity` |

---

## 5. DIAGRAM PRZEPŁYWU DECYZJI V2.5 (v2.1)

```
Pool Event (TX)
    │
    ▼
[GatekeeperBuffer.ingest_transaction_tracking_only()]
    │
    ├─ Mode "long" / "standard": istniejąca ścieżka live (UNCHANGED)
    │
    └─ v25.shadow_enabled: NOWA ŚCIEŻKA SHADOW
         │
         ├─ [HARD FAILS CHECK] (V2 legacy: MarketCapTooLow, SlowPool, ExtremeHhi)
         │    └─ FAIL → REJECT_HARD_FAIL
         │
         ├─ [PDD — PIERWSZA WARSTWA] (mierzalne negatywne sygnały)
         │    ├─ Entry drift > 5% → SHADOW_REJECT_PUMP_AND_DUMP
         │    ├─ Ramping detected → SHADOW_REJECT_PUMP_AND_DUMP
         │    ├─ Spike detected → SHADOW_REJECT_PUMP_AND_DUMP
         │    ├─ Whale > 60% → SHADOW_REJECT_PUMP_AND_DUMP
         │    ├─ Reserve too small → SHADOW_REJECT_PUMP_AND_DUMP
         │    └─ Flash crash risk → SHADOW_REJECT_PUMP_AND_DUMP
         │    PASS ↓
         │
         ├─ [CORE LAYER] (V2 legacy: Core1+Core2+Core3)
         │    └─ FAIL → REJECT_CORE_FAIL
         │
         ├─ [SYBIL COMBO VETO + SOFT EXCESS] (V2 legacy)
         │    └─ FAIL → REJECT_SYBIL_SOFT_EXCESS / REJECT_SYBIL_INTERFERENCE
         │
         ├─ [SOFT EXCESS] (V2 legacy)
         │    └─ FAIL → REJECT_SOFT_EXCESS
         │
         ├─ [ALPHA GATE] (V2 legacy)
         │    └─ FAIL → REJECT_LOW_ALPHA
         │
         ├─ [TAS — SOFT MODULATOR] (NIE live hard gate)
         │    ├─ Ekstremalnie negatywna (< 0.30) → SHADOW_REJECT_LOW_TRAJECTORY
         │    └─ W przeciwnym razie: moduluje confidence (±25%)
         │
         ├─ [ADAPTIVE PROSPERITY SHADOW]
         │    ├─ Wykryj reżim rynkowy (HighVol/Normal/LowVol)
         │    ├─ Wylicz sugerowane progi
         │    └─ Zapisz kontrfaktyczny verdict
         │
         └─ [DECISION GATE]
              │
              ├─ Early Window (2-5s): confidence >= 0.85 + wszystkie 6 faz + drift < 3%
              │    └─ YES → SHADOW_EARLY_BUY
              │
              ├─ Normal Window (5-7s): confidence >= 0.65
              │    └─ YES → SHADOW_BUY
              │
              └─ Extended Window (7-10s): confidence >= 0.55 + PDD czysty
                   └─ YES → SHADOW_BUY / NO → SHADOW_TIMEOUT / SHADOW_REJECT
```
```
Hipoteza złotego okna (3-7s)
├─ 2-3s: za mało TX, szum
├─ 3-7s: kandydat na wcześniejsze entry — do potwierdzenia w shadow
├─ 8-15s: typowe okno dumpu (dla scamów)
└─ 15s+: za późno — albo pump się skończył, albo dump w toku
```

---

## 6. MATRYCA RYZYKA I KONTROLA

| Ryzyko | P-stwo | Impact | Mitigacja |
|--------|--------|--------|-----------|
| False positive wzrost (odrzucenie legitnych tokenów przez PDD) | Średnie | Wysoki | Shadow-first; raport false rejects na winnerach; live tylko po promocji progów |
| Zbyt agresywny entry drift limit (5%) | Średnie | Wysoki | Analiza progów 5/7/10% w shadow; wymagana jakość kotwicy ceny |
| Fałszywy sygnał TAS przy małym samplu (< 3 TX/segment) | Niskie | Niski | TAS liczony tylko gdy każdy segment ma >= 3 TX; w przeciwnym razie nie wpływa na decyzję |
| Zbyt agresywny early entry — wejście w szum | Średnie | Wysoki | Early entry shadow-only; live dopiero po ADR i walk-forward |
| Normal window 7s — za krótko dla wolniejszych pooli | Niskie | Niski | Extended window (7-10s) jako catch-all dla pooli z małą liczbą TX |
| Extended window — wejście w oknie dumpu (8-15s) | Średnie | Średni | Extended tylko gdy PDD całkowicie czysty + confidence >= 0.55; udział BUY przed 7s mierzony w shadow |
| Regime detection niedokładna przy małym sample | Średnie | Średni | APS shadow/offline; default to Normal regime jeśli sample < 30; brak live mutation progów |
| Zwiększona latency decyzji | Niskie | Średni | Shadow scoring nie blokuje live path; live early entry dopiero po promocji |
| Konflikt z istniejącymi testami | Niskie | Wysoki | Wszystkie nowe struktury `#[serde(default)]`; enum warianty dodawane |
| Naruszenie kontraktu Gatekeeper 8-9s | Średnie | Wysoki | Na etapie V2.5 live kontrakt bez zmian; skrócenie wymaga ADR |
| Feedback loop przez APS | Średnie | Wysoki | APS nie mutuje progów live; wyłącznie ablation i sugestie offline |

---

## 7. HARMONOGRAM

| Faza | Opis | Czas | Zależności |
|------|------|------|------------|
| Faza 0 | Rozpoznanie, kontrakty, branch | 1 dzień | brak |
| Faza 1 | Konfiguracja TOML + Rust struct | 1 dzień | Faza 0 |
| Faza 2 | Dynamiczne Okno Obserwacji | 2 dni | Faza 1 |
| Faza 3 | Trajectory Aware Scoring | 2 dni | Faza 1 (niezależnie od Fazy 2) |
| Faza 4 | Pump & Dump Detector | 2 dni | Faza 1 |
| Faza 5 | Adaptive Prosperity | 2 dni | Faza 1 |
| Faza 6 | Nowe struktury, rozszerzenia SSOT | 1 dzień | Fazy 2+3+4+5 |
| Faza 7 | Testy regresji i integracyjne | 2 dni | Fazy 0-6 |
| Faza 8 | Dokumentacja, shadow-burnin, rollout | 2 dni | Faza 7 |

**Total: ~15 dni roboczych** (Fazy 2, 3, 4, 5 mogą być realizowane równolegle)

---

## 8. OCZEKIWANY WPŁYW

Poniższa tabela opisuje **hipotezy walidacyjne**, nie gwarantowany efekt wdrożenia. Wersja shadow ma najpierw potwierdzić, czy progi faktycznie poprawiają precision i avg loss bez nadmiernego odrzucania winnerów.

| Metryka | V2 (obecny) | V2.5 shadow target | Status |
|---------|-------------|---------------|--------|
| **Win Rate** | ~42% | >= 65% | wymaga CI + walk-forward |
| **Avg Loss** | -52.26% | < -15% | wymaga outcome labeling |
| **Avg Profit** | +50-100% | +50-110% | nie zakładać poprawy bez testu |
| **Czas do decyzji (early)** | N/A (tylko 10s) | 2-5s shadow | live wyłączone |
| **Czas do decyzji (średni)** | 10s | 5-7s shadow | live bez zmian do ADR |
| **% BUY w oknie 3-7s** | 0% live | mierzone w shadow | brak targetu przed replay |
| **Entry Drift avg (wszystkie BUY)** | +4.32% | < +2% | zależy od jakości kotwicy ceny |
| **Największa strata** | -79.87% | redukcja ekstremów | tylko hipoteza PDD |
| **False Positive** | nieustalone | raportowane z CI | krytyczne kryterium promocji |
| **False Negative (timeouty)** | wysokie | mierzone w shadow | nie optymalizować kosztem precision |

---

## 9. SPRAWDZENIE KONTRAKTÓW

| Kontrakt | Status w V2.5 |
|----------|---------------|
| `MaterializedFeatureSet` | BEZ ZMIAN -- dalej SSOT. Nowe pola opcjonalne. |
| `GatekeeperDecision` | NOWE OPCJONALNE POLA. Stare pola bez zmian. Nowe warianty enuma DODANE. |
| `DecisionLogger` JSONL | Nowe pola w schema v16. Stare pola zachowane. |
| `GatekeeperV2Config` | Rozszerzone o sub-structs. Wszystko `#[serde(default)]`. |
| Mode `long` / `standard` | Działa BEZ ZMIAN. V2.5 startuje jako `v25.shadow_enabled=true`, bez zmiany live mode. |
| `ShadowLedger` / `WAL` | Nowy wariant enuma, stary zapis bez zmian. |
| Testy istniejące | Wszystkie muszą przechodzić. Nowy kod w osobnych plikach. |
| `GatekeeperBuyLog` | Stare pola zachowane, nowe dodane jako optional. |
| Curve gate (`evaluate_curve_gate`) | Nienaruszony. Entry drift używa jawnej kotwicy ceny i flagi jakości, nie zastępuje curve gate. |
| IWIM Veto Gate | Nienaruszony. Działa po PDD check. |
| Sybil Interference | Nienaruszony. Działa w tej samej pozycji w pipeline. |

---

**Koniec planu GATEKEEPER V2.5 "PRECISION STRIKE".**
