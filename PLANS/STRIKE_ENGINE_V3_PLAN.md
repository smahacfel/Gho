# PLAN REFACTORU: GATEKEEPER V2 → GHOST STRIKE V3

> Data: 2026-04-29  
> Autor: Ghost Father (AI Agent)  
> Cel: Zastąpienie statycznego, 6-fazowego pipeline'u z jednorazową decyzją po 8001 ms adaptacyjnym, dynamicznym oknem obserwacji z scoringiem 4-modułowym i wczesnym wejściem dla pul o wysokiej pewności. Cel inwestycyjny: minimum +30% zysku przy winratio ≥65%.

---

## Filary bezpieczeństwa (SSOT i kontrakty)

1. **Nowy komponent jako feature-flag** – stary Gatekeeper V2 pozostaje nietknięty; nowy silnik `StrikeEngine` działa obok, aktywowany konfiguracją `gatekeeper.mode = "strike"`.
2. **Wspólne struktury wejścia/wyjścia** – `MaterializedFeatureSet` (już istnieje) pozostaje SSOT dla danych; `GatekeeperDecision` zostaje rozszerzone o nowe pole `strike_verdict: Option<StrikeVerdict>`, nie modyfikując istniejących pól.
3. **Decyzje logowane w tym samym formacie JSONL** – schema wersjonowana, pole `gatekeeper_version = "v3.0-strike"`.
4. **Wszystkie nowe progi w konfiguracji TOML** – sekcja `[gatekeeper_v2.strike]` z wartościami domyślnymi, które przechodzą testy regresji.
5. **Nowy ADR** – dokumentujący decyzje architektoniczne.

---

## Plan krok po kroku (8 faz)

### Faza 0: Rozpoznanie i zamrożenie kontraktów
**Cel:** Ustalenie niezmienników, które nowy kod musi respektować.

| Krok | Akcja | Pliki |
|------|-------|-------|
| 0.1 | Spis wszystkich pól `GatekeeperDecision` używanych przez konsumentów | `gatekeeper.rs`, `decision_logger.rs`, `shadow_ledger/*`, `trigger/*` |
| 0.2 | Identyfikacja funkcji wywołujących `evaluate_policy()` | `oracle_runtime.rs`, `gatekeeper_commit_loop.rs` |
| 0.3 | Spis testów integracyjnych Gatekeepera | `ghost-brain/tests/*gatekeeper*`, `ghost-launcher/tests/*` |
| 0.4 | Potwierdzenie pól `MaterializedFeatureSet` | `ghost-core/src/checkpoint/types.rs` |
| 0.5 | Stworzenie brancha `refactor/strike-engine-v3` | git |

**Rezultat:** Dokument `STRIKE_SSOT_CONTRACTS.md` z listą niezmienników.

---

### Faza 1: Nowa konfiguracja (TOML + Rust struct)

**Cel:** Rozszerzenie `GatekeeperV2Config` o parametry Strike Engine.

| Krok | Akcja | Pliki |
|------|-------|-------|
| 1.1 | Dodać sekcję `[gatekeeper_v2.strike]` do `ghost_brain_config.toml` | `ghost-brain/ghost_brain_config.toml` |
| 1.2 | Dodać struct `StrikeConfig` w Rust | `ghost-brain/src/config/ghost_brain_config.rs` |
| 1.3 | Dodać `#[serde(default)]` dla bezpieczeństwa parsowania | j/w |
| 1.4 | Testy deserializacji konfiguracji | `ghost-brain/tests/config_wiring_test.rs` |

**Rezultat:** Nowe pola w konfiguracji, wstecznie kompatybilne.

---

### Faza 2: Definicja nowych typów (bez logiki)

**Cel:** Stworzenie kontraktów danych dla Strike Engine.

| Krok | Akcja | Pliki |
|------|-------|-------|
| 2.1 | Stworzyć `strike_types.rs` | NOWY: `ghost-launcher/src/components/strike_types.rs` |
| 2.2 | Dodać `StrikeAssessment` | j/w |
| 2.3 | Rozszerzyć `GatekeeperDecision` | `gatekeeper.rs` |
| 2.4 | Dodać tag wersji `"v3.0-strike"` | `gatekeeper.rs` |
| 2.5 | Nowe warianty `GatekeeperVerdictType` | `gatekeeper.rs` |

**Rezultat:** Nowe typy, istniejące struktury rozszerzone bez łamania.

---

### Faza 3: Implementacja Strike Engine (scoring)

**Cel:** Nowy silnik scoringowy jako osobny moduł.

| Krok | Akcja | Pliki |
|------|-------|-------|
| 3.1 | `evaluate_strike()` | NOWY: `strike_engine.rs` |
| 3.2 | `compute_survival()` | j/w |
| 3.3 | `compute_momentum()` | j/w |
| 3.4 | `compute_demand()` | j/w |
| 3.5 | `compute_safety()` | j/w |
| 3.6 | `compute_confidence()` | j/w |
| 3.7 | Testy jednostkowe | `ghost-launcher/tests/strike_engine_tests.rs` |

---

### Faza 4: Dynamiczne okno obserwacji (runtime)

**Cel:** Modyfikacja `oracle_runtime.rs` dla adaptacyjnych ewaluacji.

| Krok | Akcja | Pliki |
|------|-------|-------|
| 4.1 | Gałąź `strike.enabled` obok istniejącej logiki | `oracle_runtime.rs` |
| 4.2 | Pętla ewaluacyjna co 500ms po AB Window | j/w |
| 4.3 | Wczesny BUY: score > 0.75 przez 3 okna | j/w |
| 4.4 | Standardowy BUY: score > 0.60 przez 2 okna | j/w |
| 4.5 | Późny BUY: przy max_wait_ms, score > 0.70 | j/w |
| 4.6 | W przeciwnym razie Reject / Timeout | j/w |
| 4.7 | Stara ścieżka bez zmian | j/w |

---

### Faza 5: Integracja z decision loggerem i shadow ledger

| Krok | Akcja | Pliki |
|------|-------|-------|
| 5.1 | Nowe pola w logach JSONL | `decision_logger.rs` |
| 5.2 | Bump `log_schema_version` do 16 | j/w |
| 5.3 | Backward compatibility | j/w |
| 5.4 | Zapis `strike_verdict` w WAL | `wal.rs` |

---

### Faza 6: Testy regresji i integracyjne

| Krok | Akcja | Pliki |
|------|-------|-------|
| 6.1 | Testy jednostkowe Strike Engine | `strike_engine_tests.rs` |
| 6.2 | Testy integracyjne vs Gatekeeper V2 na replay | `strike_vs_gatekeeper_regression.rs` |
| 6.3 | Wszystkie istniejące testy muszą przechodzić | `cargo test` |
| 6.4 | Benchmark wydajności (< 500μs na ewaluację) | `strike_bench.rs` |

---

### Faza 7: Dokumentacja ADR i konfiguracja rollout

| Krok | Akcja | Pliki |
|------|-------|-------|
| 7.1 | Utworzyć `ADR-0121-strike-engine-v3.md` | `docs/ADR/` |
| 7.2 | Aktualizacja `CONFIG_REFERENCE.md` | `ghost-brain/` |
| 7.3 | Plik konfiguracyjny `configs/shadow-strike.toml` | NOWY |
| 7.4 | Kryteria akceptacji shadow-burnin | PLANS |

---

### Faza 8: Shadow-burnin i iteracja

| Krok | Akcja |
|------|-------|
| 8.1 | Shadow-strike na 2000 pulach |
| 8.2 | Porównanie metryk vs Gatekeeper V2 |
| 8.3 | Kalibracja progów `min_confidence_*` |
| 8.4 | Decyzja: paper-burnin lub produkcyjne włączenie |

---

## Podsumowanie – co NIE zostanie naruszone

| Kontrakt | Gwarancja |
|----------|-----------|
| `MaterializedFeatureSet` | Bez zmian – dalej SSOT |
| `GatekeeperDecision` | Nowe opcjonalne pola, stare bez zmian |
| `DecisionLogger` JSONL | Nowe pola w schema v16, stare zachowane |
| `GatekeeperV2Config` | Rozszerzone, domyślnie wyłączone (`strike.enabled = false`) |
| Tryb `long` / `standard` | Działa bez zmian |
| `ShadowLedger` / `WAL` | Nowy wariant enuma, stary zapis bez zmian |
| Testy istniejące | Wszystkie muszą przechodzić |