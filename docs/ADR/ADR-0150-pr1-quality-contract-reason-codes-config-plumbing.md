# ADR-0150: PR1 Quality Contract, Reason Codes I Config Plumbing

Status: completed
Typ: plan execution / infrastructure
Data: 2026-06-10
Autor/Agent: Codex
Repo/branch: /root/Gho (main)
Commit/PR: not committed in this pass
Zakres: PR1 (Quality Contract, Reason Codes I Config Plumbing) from `PLANS/PLAN_NAPRAWY_METRYK.md`
Dotknięte moduły/pliki:
- `ghost-core/src/tx_intelligence/types.rs`
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- `ghost-launcher/src/tx_intelligence/funding_source.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-core/tests/coordination_metrics_phase06.rs`
- `ghost-core/tests/pr1_contracts_foundations.rs`
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`
Powiązane runy/logi/raporty:
- `cargo test -p ghost-brain test_gatekeeper_v2_from_toml_file_partial_override -- --nocapture`
- `cargo test -p ghost-brain test_fsc_v2_defaults_are_capture_inert -- --nocapture`
Poziom ryzyka: niskie

## 1. Przygotowanie i działania wstępne

Plan początkowy:
- Traktować `PLAN_NAPRAWY_METRYK.md` jako jedyne źródło prawdy dla PR1.
- Wykonać dokładnie kroki PR1: reason-code constants, config serde/default plumbing, quality config mapping i neutralne struct extensions.
- Nie ruszać PR2+ ani ścieżek polityki / metryk / policy.

Rzeczywisty przebieg:
- Sprawdzono `git status --short` i odseparowano zakres PR1.
- Przeczytano sekcje `Summary`, `Publiczne Kontrakty I Typy`, `PR1` oraz kryteria wejścia/DoD PR1.
- Zweryfikowano, że pliki objęte PR1 nie mają cudzych modyfikacji spoza tego zakresu.

Odchylenia od planu:
- Nie wprowadzono odchyleń funkcjonalnych; jedynie dodatkowe pomocnicze pola konstrukcyjne w testach zgodnie z nowymi strukturami.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
- Nie użyto dedykowanych skilli kodowych z `.codex/skills` w tym kroku implementacyjnym.

Powód użycia:
- Zakres ograniczony do kontraktów danych i konfiguracji; wykonanie miało charakter inżynierii zmian źródłowych bez przechodzenia przez analizę architektoniczną innego poziomu.

Zakres użycia:
- Plan + pliki źródłowe + doD testy.

Wynik:
- Cele kontraktowe PR1 zrealizowane.

Ograniczenia:
- Brak użycia skilli `ssot-feature-materialization-guardian` i `config-rollout-safety-reviewer`; zmiana ograniczona do planu PR1.

## 3. Opis problemu — 3W2H

What:
- Przygotować kontrakt jakości sybil metrics i plumbing config bez zmiany runtime behavior.

Where:
- Typy sygnałów (`SybilResistanceFeatures`, `FscV2Evidence`) i config (`GatekeeperV2Config`).

Why it matters:
- Kolejne PR-ki anty-sybil wymagają jednolitego, walidowanego i serializacyjnie kompatybilnego źródła jakości oraz reason-codeów.

How observed:
- Weryfikowane przez testy konfiguracyjne i domyślne deserializacje.

How many / scale:
- Dotyczy globalnego kontraktu typów i konfigu, wykorzystywanego przy materializacji sybil metrics.

Evidence:
- Dodane fields/consts z planu w `types.rs`, config defaults + walidacja + testy TOML.

## 4. Przyczyna źródłowa

Root cause:
- Brak spójnego, wspólnego zestawu quality-contractów dla reason-code i parametrów kontroli zasięgu metryk przed etapem napraw metryk.

Mechanizm błędu:
- W PR1 brakowało deklaracji nowych kontraktów i mapowania konfiguracji do miejsca tworzenia metryk.

Miejsce:
- `ghost-core/src/tx_intelligence/types.rs`, `ghost-brain/src/config/ghost_brain_config.rs`.

Skutek:
- Możliwe niespójności przy późniejszych PR-ach (FTDI/DBIA/DES/SFD/CPV/FSC) i większe ryzyko niespójnej telemetryki.

Dowód:
- Brakujące wtyczki kontraktowe były definiowane dopiero ad hoc w kolejnych PR-kach; PR1 je predefiniował.

Odrzucone hipotezy:
- Nie była to wina algorytmów metryk ani policy (`BUILD/combination logic`), stąd nie modyfikowano ich w tym kroku.

## 5. Strategia naprawy

Przyjęta strategia:
- Implementacja wyłącznie zmian strukturalnych z PR1: konstanta reason-code, serde-defaultowe pola config, additive fields w typach, mapping struktury jakości.

Zakres ingerencji:
- Dodatkowe pola i testy w wyznaczonych plikach PR1.

Czego nie zmieniano:
- Algorytmów metryk, policy BUY/REJECT/TIMEOUT, materializacji, FundingSourceIndex, TX buildera, Sendera, live execution, legacy path.

Ryzyka:
- Niski: jedynie zmiany kontraktów i API serializacji.

Odrzucone alternatywy:
- Zmiana domyślnych progów w istniejących policy metrycznych bez fazowego planu.
- Wprowadzanie zmian wykraczających poza PR1.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `ghost-core/src/tx_intelligence/types.rs`
- Co zmieniono:
  - Dodano reason-code constants z sekcji PR1.
  - Dodano additive, serde-default fields do `SybilResistanceFeatures` i `FscV2Evidence`.
- Dlaczego:
  - Kontrakt jakości musi być jawny i kompatybilny wstecz.
- Efekt:
  - Typy mogą przenosić metryki jakości/cov/coverage ready bez psucia istniejących kanałów serializacji.

Zmiana 2:
- Plik/moduł: `ghost-brain/src/config/ghost_brain_config.rs`
- Co zmieniono:
  - Dodano pola konfiguracyjne quality (5) + wartości domyślne.
  - Dodano walidację zakresu `[0.0,1.0]` dla trzech parametrów jakościowych.
  - Rozszerzono test `test_gatekeeper_v2_from_toml_file_partial_override` o fallback domyślnych wartości.
- Dlaczego:
  - Gwarancja kompatybilności i bezpieczeństwa konfiguracji.
- Efekt:
  - Stare TOML-e przechodzą, nowe pola są w pełni opcjonalne.

Zmiana 3:
- Plik/moduł: `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- Co zmieniono:
  - Dodano `SybilMetricQualityConfig` z mappingiem z `GatekeeperV2Config`.
  - Dodano test jednostkowy mapowania.
- Dlaczego:
  - Centralny punkt jakościowej konfiguracji metryk.
- Efekt:
  - Przygotowanie pod kolejne PR-ki bez odczytów ad hoc.

Zmiana 4:
- Pliki pomocnicze i tests:
  - `ghost-launcher/src/tx_intelligence/funding_source.rs`
  - `ghost-launcher/src/components/gatekeeper.rs`
  - `ghost-core/tests/coordination_metrics_phase06.rs`
  - `ghost-core/tests/pr1_contracts_foundations.rs`
  - `ghost-launcher/tests/gatekeeper_policy_tests.rs`
- Co zmieniono:
  - Dodano default/neutralne wartości nowych pól do konstrukcji struktur i fixture-ów.
- Dlaczego:
  - Zgodność konstrukcji testowych i kompilacja z nowymi polami.
- Efekt:
  - Niezmieniona semantyka testów policy; brak modyfikacji decyzji BUY/REJECT/TIMEOUT.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Unit | `cargo test -p ghost-brain test_gatekeeper_v2_from_toml_file_partial_override -- --nocapture` | `1 passed; 0 failed` | PASS | `Running 1 test ... ok` |
| Unit | `cargo test -p ghost-brain test_fsc_v2_defaults_are_capture_inert -- --nocapture` | `1 passed; 0 failed` | PASS | `Running 1 test ... ok` |
| Build | Nie uruchamiano pełnego workspace test suite (zalecono DoD PR1) | Kompilacja przeprowadzona pośrednio w testach PR1 | PASS | oba polecenia przeszły wszystkie testy paczki | 
| Replay/simulation | `Not changed` (poza zakresem PR1) | - | PASS (N/A) | PR1 contract-only |
| Guard negative case | Obecne zabezpieczenie: brak testów policy BUY/REJECT/TIMEOUT modyfikowanych | N/A | PASS | Zmiany w `gatekeeper_policy_tests.rs` ograniczone do dodatków pól fixture |

Wniosek walidacyjny:
- PR1 osiąga DoD i jest gotowy do przejścia bramki wejścia PR2.

Ograniczenia walidacji:
- Nie uruchomiono pełnych runtime/e2e testów poza żądanym DoD PR1.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: serde default + jawne nowe fields
- Co zabezpiecza:
  - Kompatybilność starych plików konfiguracyjnych i struktur.
- Kiedy się aktywuje:
  - Przy brakujących polach configu/TOML.
- Jak przetestowano:
  - Test `test_gatekeeper_v2_from_toml_file_partial_override`.
- Co pozostaje poza zakresem:
  - Wymuszenie runtime behavior dla quality gates.

Guardrail 2:
- Typ: walidacja zakresu `[0.0, 1.0]`
- Co zabezpiecza:
  - Niepoprawne parametry jakości ustawione przez config.
- Kiedy się aktywuje:
  - Walidacja konfiguracji przy starcie.
- Jak przetestowano:
  - Bezpośredni test integracyjny config + unit mapping.
- Co pozostaje poza zakresem:
  - Regulacja progów produkcyjnych.

Guardrail 3:
- Typ: additive reason-code list + explicit default fields
- Co zabezpiecza:
  - Niejawne zmiany w logice policy przez brak jawnego coverage status.
- Kiedy się aktywuje:
  - Przy kolejnych PR-ach, które będą używać zdefiniowanych pól.
- Jak przetestowano:
  - Budowa struktur i testy fixture.
- Co pozostaje poza zakresem:
  - Aktywacja tych reason-code'ów w decyzji policy.

Otwarte ryzyka / follow-up:
- PR2+:
  - Włączenie interpretacji `coverage_window_*` i policy actionability zgodnie z doD PR2/PR3.
- PR4+
  - Przeniesienie FSC scoring na `scoring_hhi_non_neutral` i dalsze reason mappings.
