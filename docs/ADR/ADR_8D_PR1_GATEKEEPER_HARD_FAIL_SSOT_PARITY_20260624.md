# ADR-8D: PR1 Gatekeeper hard-fail SSOT parity and policy-only authority

Status: IMPLEMENTED / STATIC_VALIDATION_COMPLETED
Typ: ADR-8D / gatekeeper policy authority and parity hardening
Data: 2026-06-24
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `6ad066c`
Commit/PR: local working tree, not committed at ADR update time
Zakres: implementacja PR1 z `PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`; usuniecie drugiego zrodla prawdy dla hard-fail verdictow i domkniecie parity runtime/feature/legacy bez zmiany configu rolloutowego
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`
- `ghost-launcher/tests/gatekeeper_v25_regression.rs`

Powiazane plany:
- `PLANS/PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR1 z planu Gatekeeper policy SSOT and availability, czyli doprowadzic do stanu, w ktorym aktywny hard-fail verdict ma tylko jednego wlasciciela: warstwe decision/policy. `run_assessment()` i feature-path assessment maja pozostac evidence-only.

Rzeczywisty przebieg:
- Potwierdzono, ze aktywny runtime nadal utrzymywal dwa zrodla prawdy:
  - typed decision z `compute_decision()` / `evaluate_policy_from_assessment()`,
  - kompatybilnosciowe `assessment.hard_reject_reason`, ktore bywalo ustawiane juz na etapie assessment.
- Potwierdzono, ze ten dual-authority surface przecieka do kilku miejsc:
  - runtime `run_assessment()`,
  - feature path `build_assessment_from_features()` / `refresh_assessment_thresholds()`,
  - legacy branch `use_three_layer_decision = false`,
  - export/log fallbacki czytajace `assessment.hard_reject_reason`.
- Potwierdzono, ze w long legacy path hard-fail mogl zostac pominiety, a curve gate mogl przykryc typed reject w standard path.
- Zakres utrzymano scisle w obrebie PR1: bez zmiany configu runtime, bez zmiany taxonomy typed reason codes, bez ruszania rolloutowego `ghost_brain_config.toml`.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT boundary, typed verdict authority, shadow/live separation.
- `rust-master`: lokalna implementacja Rust, helpery compat, testy jednostkowe i regresyjne.
- `trading-systems`: parity runtime/legacy/feature path i decyzja o tym, gdzie policy ma byc jedynym wlascicielem hard-fail.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/gatekeeper-policy-auditor.md`

Powod:
PR1 dotyka bezposrednio kontraktu miedzy assessment a policy oraz parity hard-fail semantics miedzy runtime path, feature path i legacy branch. Nie bylo potrzeby ladowania dokumentow execution/send path ani config rollout specialist, bo ta zmiana nie dotyka Solana execution ani aktywnej migracji configu.

## 3. Opis problemu - 3W2H

What:
Hard-fail verdicty Gatekeepera mialy dwa niezalezne nosniki:
- aktywna typed decision z policy layer,
- `assessment.hard_reject_reason`, ktore moglo byc ustawione przed lub obok policy evaluation.

Where:
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- runtime deadline/finalization path
- feature-driven policy path
- legacy `use_three_layer_decision = false`
- export/log fallbacki

Why it matters:
Dual authority lamie PR1 SSOT contract i dopuszcza rozjazd:
- runtime assessment moze sygnalizowac hard-fail zanim policy stworzy typed decision,
- legacy branch moze czytac compatibility field zamiast tej samej decision logic,
- log/export path moze wyeksportowac reason z innego zrodla niz final verdict,
- runtime moze roznie traktowac ten sam przypadek miedzy standard, long, feature i legacy path.

How observed:
W kodzie znaleziono aktywne przypisania do `assessment.hard_reject_reason` w runtime assessment oraz w feature assessment. Testy historyczne asserowaly niekiedy ten field zamiast typed decision. Dodatkowo legacy long path nie honorowal policy hard-fail precheck, a standard path mial kolejnosc, w ktorej curve gate mogl zaslonic reject verdict z policy.

How many / scale:
Zmiana dotyka aktywnego Gatekeeper runtime i test surface, ale jest lokalna do polityki V2/V2.5 i jej export/log parity. Nie dotyka execution handoff ani config payloads.

## 4. Przyczyna zrodlowa

Root cause:
Historycznie `GatekeeperAssessment` pelnil podwojna role:
- evidence snapshot dla policy/logowania,
- oraz nieformalny nosnik aktywnego hard-fail reason.

To spowodowalo trzy klasy problemow:
- assessment mogl tworzyc "quasi-decision" przed policy,
- legacy branch mogl polegac na compatibility field zamiast na tej samej decision logic,
- eksport/log fallback mogl czytac pole kompatybilnosciowe jak aktywna prawde.

Dodatkowy problem:
Long legacy deadline path byl oparty o `phases_passed`, ale bez jawnego policy precheck dla hard-fail, co przeczylo wymaganiu PR1: `use_three_layer_decision = false` ma uzywac tej samej policy dla hard-faili.

## 5. Strategia naprawy

Przyjeta strategia:
- Uczynic `run_assessment()` i feature-driven assessment evidence-only.
- Zostawic `assessment.hard_reject_reason` tylko jako compat/export field, wypelniany dopiero po uzyskaniu typed decision.
- Wprowadzic jeden helper do przypinania compat fields po decision.
- Przepisac wszystkie aktywne runtime reject paths tak, aby hard-fail wynikaly z `compute_decision()` lub `evaluate_policy_from_assessment()`, a nie z samego assessment.
- Domknac parity testami dla runtime, feature path, compat path oraz legacy `use_three_layer_decision = false`.

Granice:
- Brak zmian w configu repo.
- Brak zmian rolloutowych.
- Brak zmian w aktywnej taxonomii reason codes.
- Brak zmian w Solana execution path.
- Brak szerokiego refaktoru innych modulow Gatekeepera.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: assessment staje sie evidence-only
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `run_assessment()` przestal ustawiac aktywne hard-fail reasons.
- Zwracane `GatekeeperAssessment` ma teraz `hard_reject_reason: None` do momentu uzyskania typed decision.

Zmiana 2: feature path przestaje produkowac aktywna decyzje
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- `build_assessment_from_features()` i `refresh_assessment_thresholds()` nie ustawiaja juz `assessment.hard_reject_reason`.
- Hard-fail na feature path pozostaje wylacznie wynikiem `evaluate_policy_from_assessment()`.

Zmiana 3: compat/export field jest przypinany tylko po decision
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Dodano helper `attach_policy_decision_compat_fields(...)`.
- Helper:
  - zapisuje `assessment.decision`,
  - kopiuje `decision.hard_fail_reason` do compatibility field,
  - cache'uje V2.5 confidence tak jak wczesniej wymagaly downstream exporty.

Zmiana 4: runtime reject path czyta typed decision zamiast assessment field
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `evaluate_from_features()`, `evaluate_compat_from_features()`, `build_snapshot_assessment_for_current_state()` oraz deadline/timeouts zostaly przepiete na helper compat po policy decision.
- Standard path najpierw honoruje reject z typed decision, a dopiero potem curve gate na BUY path. Eliminuje to maskowanie rejectu przez curve latch.

Zmiana 5: legacy branch dostaje ten sam hard-fail precheck
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- W `evaluate_phases()` i `check_long_deadline()` dla `use_three_layer_decision = false` dodano jawny `compute_decision(&assessment)` precheck.
- Jesli policy zwraca `RejectHardFail`, legacy branch zwraca ten sam typed verdict/reason/reason_code zamiast czytac `assessment.hard_reject_reason`.

Zmiana 6: export/log fallback preferuje typed decision
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `to_buy_log()` i powiazane summary/export surfaces preferuja `assessment.decision.hard_fail_reason`, a compatibility field pozostaje tylko fallbackiem dla starych sciezek.

Zmiana 7: test surface PR1 zostal przepisany na typed decision
- Pliki:
  - `ghost-launcher/src/components/gatekeeper.rs`
  - `ghost-launcher/tests/gatekeeper_policy_tests.rs`
  - `ghost-launcher/tests/gatekeeper_v25_regression.rs`
- Stare testy nie asseruja juz aktywnego hard-fail na `run_assessment()`.
- Dodano macierz parity `hard_fail_ssot_parity_across_runtime_legacy_and_feature_paths` dla szesciu przypadkow:
  - dev sold,
  - extreme HHI,
  - extreme bot timing,
  - slow pool,
  - tx price impact,
  - failed tx ratio.
- Regresje timeoutowe zostaly zawerzone fixture'ami tak, aby testowaly timeout taxonomy, a nie przypadkowy hard-fail HHI po obecnych thresholdach branchowych.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Rustfmt touched files | `rustfmt --edition 2021 ghost-launcher/src/components/gatekeeper.rs ghost-launcher/src/components/gatekeeper_policy.rs ghost-launcher/tests/gatekeeper_policy_tests.rs ghost-launcher/tests/gatekeeper_v25_regression.rs` | passed | PASS |
| PR1 hard-fail parity matrix | `cargo test -p ghost-launcher --lib hard_fail_ssot_parity_across_runtime_legacy_and_feature_paths -- --nocapture` | 1 passed | PASS |
| Policy timeout and assessment tests | `cargo test -p ghost-launcher --test gatekeeper_policy_tests -- --nocapture` | 44 passed | PASS |
| Legacy hard-reject fixtures | `cargo test -p ghost-launcher --lib test_evaluate_hard_reject -- --nocapture` | 4 passed | PASS |
| V2.5 regression suite | `cargo test -p ghost-launcher --test gatekeeper_v25_regression -- --nocapture` | 41 passed | PASS |

### Korekta stalego testu regresyjnego

W trakcie finalnego audytu wyszlo, ze `gatekeeper_v25_regression` zawiera historyczny
assert `p3_config_has_legacy_drift_cap_1_50`, ktory byl juz niespojny z aktualnym
checkoutem:
- branchowy `ghost-brain/ghost_brain_config.toml` ma swiadomy collector-profile
  `max_price_change_ratio = 9.50`,
- `ghost-brain/src/config/ghost_brain_config.rs` ma juz odpowiadajacy temu test,
  ktory jawnie dokumentuje: "Collector profile keeps the legacy cap permissive
  for wide evidence gathering."

Naprawa byla test-only:
- regression test zaktualizowano tak, aby weryfikowal aktualny autorytatywny
  collector profile `9.50`,
- nie zmieniono runtime thresholdow,
- nie zmieniono polityki Gatekeepera,
- nie poszerzono scope PR1 poza potrzebna synchronizacje test surface z biezaca
  galazia.

### Co zostalo potwierdzone

- `run_assessment()` nie jest juz aktywnym zrodlem hard-fail authority.
- Feature path nie produkuje aktywnego hard-fail przed policy evaluation.
- Standard runtime, long runtime, feature path i compat path korzystaja z tej samej policy authority dla hard-fail.
- Legacy `use_three_layer_decision = false` nie obchodzi juz typed hard-fail semantics.
- Historyczne testy `test_evaluate_hard_reject*` sa zsynchronizowane z aktualnym
  kontraktem policy i nie polegaja juz na przypadkowych kolizjach z innymi
  hard-failami albo na brakujacym deadline/wall-clock context.
- Timeout regression tests nadal zachowuja typed timeout taxonomy po odizolowaniu ich od niezaleznych hard-fail thresholdow.

## 8. Ryzyka resztkowe / czego PR1 jeszcze nie robi

- PR1 nie usuwa compatibility field `assessment.hard_reject_reason`; zostawia go jako inert export surface dla kompatybilnosci.
- PR1 nie zmienia samej definicji hard-fail thresholdow.
- PR1 nie zmienia broader gate ordering poza miejscami niezbednymi do przywrocenia policy-only authority.
- Workspace nadal ma wiele istniejacych warningow kompilacji poza touched scope.

## 9. Scope out

Poza zakresem pozostaly:
- `ghost-brain/ghost_brain_config.toml` drift repair,
- rollout profile rewrites,
- DecisionLogger schema migration,
- Solana execution / trigger / sender path,
- PDD/TAS threshold tuning,
- szeroki cleanup warningow workspace,
- jakiekolwiek zmiany w legacy scoring poza koniecznym hard-fail parity precheck.
