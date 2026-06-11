# ADR-0151: PR1 Reason-Code Contract + Config Plumbing Re-audit

Status: conditional-pass (PR1 gate-complete, z jawnie odseparowanym follow-up PR2)
Typ: corrective re-audit / scope-control
Data: 2026-06-10
Autor/Agent: Codex
Repo/branch: /root/Gho (main)
Commit/PR: not committed in this pass
Zakres: PR1 (Quality Contract, Reason Codes I Config Plumbing)
Dotknięte moduły/pliki:
- `/root/Gho/PLANS/PLAN_NAPRAWY_METRYK.md`
- `/root/Gho/ghost-core/src/tx_intelligence/types.rs`
- `/root/Gho/ghost-brain/src/config/ghost_brain_config.rs`
- `/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- `/root/Gho/ghost-core/tests/pr1_contracts_foundations.rs`
- `/root/Gho/ghost-launcher/src/tx_intelligence/funding_source.rs`
Powiązane runy/logi/raporty:
- `PLANS/PLAN_NAPRAWY_METRYK.md` (`Summary`, `Publiczne Kontrakty I Typy`, `PR1`)
- `cargo test -p ghost-brain test_gatekeeper_v2_from_toml_file_partial_override -- --nocapture`
- `cargo test -p ghost-brain test_fsc_v2_defaults_are_capture_inert -- --nocapture`
- `cargo test -p ghost-core pr1_public_quality_reason_codes_contract_is_complete -- --nocapture`
Poziom ryzyka: niski (kontrakty typów/konfiguracji), średni dla semantyki FSC coverage evidence (przekazana do PR2)

## 1. Przygotowanie i działania wstępne

Plan początkowy:
- Utrzymać PR1 jako wyłącznie kontrakt/config plumbing, bez zmian algorytmicznych policy/algorytmów metryk.
- Zweryfikować plan i DoD PR1.
- Dodać/utrzymać pełny kontrakt reason-code bez regresji PR1.

Rzeczywisty przebieg:
- Przeczytano wymagane sekcje planu (`Summary`, `Publiczne Kontrakty I Typy`, `PR1`, `DoD`).
- Sprawdzono git status, zakres zmian i to, że zmiany funkcjonalnie należą do PR1.
- Zweryfikowano aktualny stan PR1 i obecność wszystkich nowych reason-code.

Odchylenia od planu:
- Brak odchyleń funkcjonalnych w obrębie PR1.
- Semantyka `FscV2Evidence.coverage_window_*` jest celowo odroczona do PR2 mimo istnienia `coverage_window_status_locked()` w kodzie, aby nie emitować fałszywego canonical evidence.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
- Brak dedykowanych skilli uruchomionych operacyjnie w tym kroku; zakres był lokalny (PR1 kontraktowy).

Powód użycia:
- Zmiany dotyczyły typów, konfiguracji, testów kontraktowych i oznaczeń semantycznych.

Zakres użycia:
- `PLANS/PLAN_NAPRAWY_METRYK.md`, `types.rs`, `ghost_brain_config.rs`, `sybil_metrics.rs`, `pr1_contracts_foundations.rs`, `funding_source.rs`.

Wynik:
- PR1 reason-code/config został potwierdzony jako kompletny.

Ograniczenia:
- Workspace jest nadal szeroko brudny; wiele zmian niezwiązanych z PR1 pozostaje poza tym krokiem.

## 3. Opis problemu — 3W2H

What:
- Weryfikacja kompletności PR1 reason-code oraz zgodności konfiguracji.

Where:
- `ghost-core/src/tx_intelligence/types.rs`, `ghost-core/tests/pr1_contracts_foundations.rs`, `ghost-brain/src/config/ghost_brain_config.rs`, `ghost-launcher/src/tx_intelligence/funding_source.rs`.

Why it matters:
- PR1 musi dostarczyć stabilny kontrakt publicznych reason-code i serde/default config przed przejściem do PR2.

How observed:
- `rg` i testy wskazały braków we wcześniejszych sprawdzeniach; po ponownej weryfikacji braków nie było.

How many / scale:
- Dotyczy 9 reason-code’ów z listy PR1.

Evidence:
- `ghost-core/tests/pr1_contracts_foundations.rs` i `types.rs` zawierają kompletny zestaw 9 kodów.

## 4. Przyczyna źródłowa

Root cause:
- Fragmentaryczna synchronizacja informacji o stanie roboczym przy porównywaniu wyników re-audytu do bieżącej wersji kodu.

Mechanizm błędu:
- Wcześniejsze sprawdzenie obejmowało starszy snapshot i/lub częściowo niezaktualizowany kontekst.

Miejsce:
- `types.rs` (definicje reason-code), `pr1_contracts_foundations.rs` (test kontraktowy), `funding_source.rs` (`coverage_window_*`).

Skutek:
- Fałszywy alarm o braku `DBIA_PARTIAL_FINGERPRINT_COVERAGE`.

Dowód:
- Dzisiaj `rg` i test `pr1_public_quality_reason_codes_contract_is_complete` przechodzą.

Odrzucone hipotezy:
- Nieodnotowane zmiany BUY/REJECT/TIMEOUT w policy — PR1 nie modyfikuje logiki policy.

## 5. Strategia naprawy

Przyjęta strategia:
- Nie zmieniać runtime/policy.
- Utrzymać PR1 kontrakt i jednocześnie jawnie oznaczyć ograniczenie readiness FSC jako PR2-owned.

Zakres ingerencji:
- Kontrakty typów, config wiring, test kontraktowy.

Czego nie zmieniano:
- Algorytmy metryk, policy, TX builder, sender, live execution, legacy ścieżki.

Ryzyka:
- Semantyczne przejęcie `coverage_window_*` w PR1 mogłoby zostać uznane za canonical evidence.

Odrzucone alternatywy:
- Wprowadzanie pełnego `coverage_window_status` do evidence w PR1.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `/root/Gho/ghost-core/src/tx_intelligence/types.rs`
- Co zmieniono:
  - Dodano / potwierdzono `DBIA_PARTIAL_FINGERPRINT_COVERAGE`, `DES_PARTIAL_SEQUENCE_COVERAGE`, `DES_NO_COMPARABLE_PAIRS`, `SFD_NEGATIVE_BALANCE_DELTA_SKIPPED`, `SFD_BUY_AMOUNT_UNAVAILABLE`, `CPV_COVERAGE_WINDOW_UNAVAILABLE`, `FSC_V2_STATUS_NOT_CLEAN`, `FSC_COVERAGE_WINDOW_UNAVAILABLE`.
  - Dodano additive fieldy FSC coverage w `FscV2Evidence` i nowe quality fields w `SybilResistanceFeatures`.
- Efekt:
  - Publiczny kontrakt reason-code jest kompletny.

Zmiana 2:
- Plik/moduł: `/root/Gho/ghost-brain/src/config/ghost_brain_config.rs`
- Co zmieniono:
  - Dodano pola quality config w `GatekeeperV2Config` z `serde(default)` i walidacją 0..=1.
- Efekt:
  - Backward compatibility dla starych TOML-i i domyślny behavior.

Zmiana 3:
- Plik/moduł: `/root/Gho/ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- Co zmieniono:
  - Dodano `SybilMetricQualityConfig` z mapowaniem z `GatekeeperV2Config`.
- Efekt:
  - Przygotowanie konfiguracji jakości do kolejnych PR bez zmian algorytmiki.

Zmiana 4:
- Plik/moduł: `/root/Gho/ghost-core/tests/pr1_contracts_foundations.rs`
- Co zmieniono:
  - Test `pr1_public_quality_reason_codes_contract_is_complete` sprawdza komplet 9 publicznych reason-code’ów PR1 oraz ich niepustą postać.
- Efekt:
  - Wprost locki kontraktu jakościowego.

Zmiana 5:
- Plik/moduł: `/root/Gho/ghost-launcher/src/tx_intelligence/funding_source.rs`
- Co zmieniono:
  - `coverage_window_*` w `FscV2Evidence` pozostawione inert (`false/0/false`) z komentarzem semantycznym PR2-owned.
- Efekt:
  - Brak niejawnego wprowadzenia fake canonical readiness evidence w PR1.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Unit | `cargo test -p ghost-brain test_gatekeeper_v2_from_toml_file_partial_override -- --nocapture` | `1 passed; 0 failed` | PASS | `test config::ghost_brain_config::tests::test_gatekeeper_v2_from_toml_file_partial_override ... ok` |
| Unit | `cargo test -p ghost-brain test_fsc_v2_defaults_are_capture_inert -- --nocapture` | `1 passed; 0 failed` | PASS | `test config::ghost_brain_config::tests::test_fsc_v2_defaults_are_capture_inert ... ok` |
| Unit | `cargo test -p ghost-core pr1_public_quality_reason_codes_contract_is_complete -- --nocapture` | `1 passed; 0 failed` | PASS | `test pr1_public_quality_reason_codes_contract_is_complete ... ok` |
| Integration | `cargo test -p ghost-launcher --test gatekeeper_policy_tests sybil -- --nocapture` | `6 passed; 0 failed` | PASS | `running 6 tests` + 6/6 passed |

Wniosek walidacyjny:
- PR1 contract/config warunki wymagane przez plan są spełnione.

Ograniczenia walidacji:
- Brak pojedynczego dedykowanego smoke testu obejmującego wyłącznie BUY/REJECT/TIMEOUT; wykonano natomiast filtrowany zestaw `sybil` w `gatekeeper_policy_tests`, który przeszedł.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: contract test
- Co zabezpiecza: kompletność reason-code.
- Kiedy: każdorazowo w CI lokalnym PR1.
- Jak przetestowano: `pr1_public_quality_reason_codes_contract_is_complete`.
- Co pozostaje poza zakresem: runtime interpretacja readiness.

Guardrail 2:
- Typ: serde compatibility
- Co zabezpiecza: brak regresji parse starego TOML.
- Kiedy: ładowanie konfiguracji.
- Jak przetestowano: `test_gatekeeper_v2_from_toml_file_partial_override`.
- Co pozostaje poza zakresem: algorytmiczne użycie nowych pól.

Guardrail 3:
- Typ: jawne oznaczenie odroczenia PR2
- Co zabezpiecza: brak mylącego canonical interpretation dla `coverage_window_*`.
- Kiedy: podczas budowy `FscV2Evidence` PR1.
- Jak przetestowano: przegląd kodu i komentarze `PR2-owned` + test semantyczny reason-code.
- Co pozostaje poza zakresem: właściwy wiring `coverage_window_status_locked`.

## 9. Otwarte ryzyka / follow-up

- PR2: `coverage_window_ready`, `coverage_window_remaining_ms`, `authoritative_buy_ready` muszą zostać podpięte do realnego `decision-wall` w materiale `funding_source_v2`.
- PR2/PR3: actionability policy i FSC score transformations zgodnie z planem naprzemiennie.

## Decyzja końcowa

- PR1 jest zamknięty względem kontraktu reason-code/config; blokadą do wejścia w PR2 pozostaje celowo oznaczona i odroczona semantyka `FscV2Evidence.coverage_window_*` (PR2-owned).
