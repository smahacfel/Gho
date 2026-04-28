# ADR-0053: Refactor Plan Completion Audit

**Date:** 2026-03-29
**Status:** Accepted
**Author:** Ghost Father

## Context

Przeprowadzono audyt repozytorium względem `PLANS/REFACTOR.md`, aby zweryfikować twierdzenie,
że wszystkie 8 PR-ów planu refaktoru zostało zrealizowanych kompletnie.

Plan definiuje stan końcowy po PR 8, w którym między innymi:
- `AccountStateCore` jest jedynym source of truth dla canonical market state,
- Gatekeeper policy layer konsumuje wyłącznie `MaterializedFeatureSet`,
- `ShadowLedger` nie jest queried jako live truth w hot-path,
- każda pula działa przez `PoolObservationSession`,
- legacy ścieżki zostają usunięte po PR 8.

Audyt miał odróżnić:
1. obecność artefaktów/modułów/doców/testów,
2. od rzeczywistego cutoveru production hot-path do nowej architektury.

## Decision

Uznano, że repozytorium **nie potwierdza** twierdzenia o "kompletnym zrealizowaniu wszystkich 8 PR-ów".

Stan faktyczny jest następujący:
- duża część docelowych modułów została dodana i kompiluje się,
- istnieją testy pokazujące nowy pipeline sesyjny,
- jednak production runtime w `ghost-launcher/src/oracle_runtime.rs` nadal używa legacy hot-path,
  a docelowe invariants z `PLANS/REFACTOR.md` nie są w pełni wyegzekwowane.

Kluczowe ustalenia audytu:
- `SessionManager` istnieje, ale nie znaleziono jego produkcyjnych use-site'ów poza modułem sesji i testami.
- `PoolObservationSession` istnieje, ale `oracle_runtime.rs` nie wywołuje jego ścieżek
  `try_checkpoint()` ani `materialize_features()`.
- `GatekeeperBuffer::evaluate_from_features()` istnieje, ale w praktyce jest używany w testach,
  a nie w głównym runtime hot-path.
- `PerPoolOracleState` nadal istnieje i pozostaje aktywnym runtime state w `OracleRuntime`.
- `oracle_runtime.rs` nadal używa `buffer.on_transaction(tx.clone())` w hot-path obserwacji puli.
- `oracle_runtime.rs` nadal odpytuje `shadow_ledger.get_curve(...)` dla danych wykorzystywanych
  do decyzji runtime.
- `OracleRuntime` nadal posiada aktywny `ReconciliationRuntime`.

Na tej podstawie przyjęto, że poprawny opis stanu to:
**"refaktor został zaimplementowany częściowo i równolegle, ale nie został w pełni przeniesiony
na production hot-path zgodnie ze stanem końcowym PR 8"**.

## Architectural Impact

To ustalenie zmienia interpretację aktualnej architektury:
- system jest nadal hybrydą nowego i legacy runtime,
- nowy model sesyjny nie jest jeszcze single-path execution model,
- `ShadowLedger` i legacy Gatekeeper scoring nadal wpływają na runtime behavior,
- deklaracje domknięcia migracji nie mogą być traktowane jako SSOT dla dalszych prac.

W praktyce oznacza to, że wszelkie kolejne decyzje projektowe, rolloutowe i audytowe muszą
zakładać współistnienie nowych modułów z aktywnym legacy hot-path, dopóki nie zostanie wykonany
rzeczywisty cutover callsite'ów i usunięcie deprecated paths.

## Risk Assessment

**Rate: High**

Ryzyka wynikające z błędnego uznania planu za zamknięty:
- fałszywe poczucie zgodności z invariants z `PLANS/REFACTOR.md`,
- dalsze decyzje operacyjne podejmowane na podstawie nieprawdziwego obrazu runtime,
- regresje wynikające z utrzymywania dwóch konkurencyjnych modeli stanu,
- błędne audyty faz zamknięcia i rolloutów,
- ryzyko, że test coverage nowej ścieżki zostanie błędnie uznany za dowód production cutoveru.

## Consequences

Korzyści:
- stan repo zostaje opisany zgodnie z rzeczywistością,
- dalsze prace mogą być planowane wobec realnych blockerów migracyjnych,
- rozdzielono "moduł istnieje" od "runtime używa modułu".

Koszty:
- wcześniejsze deklaracje ukończenia nie mogą być utrzymane bez korekty,
- wymagane będzie osobne domknięcie runtime cutoveru,
- część wcześniejszych statusów PR będzie wymagała rekategoryzacji na "partial" lub "artifact-complete, integration-incomplete".

## Alternatives Considered

### 1. Uznanie planu za ukończony na podstawie samych artefaktów i testów
Odrzucono, ponieważ obecność plików, eksportów i testów nie dowodzi, że production hot-path
został faktycznie przełączony.

### 2. Uznanie planu za ukończony na podstawie dokumentów i opisów PR
Odrzucono, ponieważ repozytorium zawiera aktywne legacy callsite'y sprzeczne ze stanem końcowym PR 8.

### 3. Ograniczenie audytu do PR1-PR3
Odrzucono, ponieważ użytkownik pytał o twierdzenie obejmujące wszystkie 8 PR-ów, a krytyczna
niezgodność ujawnia się właśnie na granicy integracji hot-path i cleanupu legacy.

## Validation Steps

Audyt oparto na bezpośredniej weryfikacji kodu i callsite'ów:

1. Przeczytano `PLANS/REFACTOR.md` i porównano stan końcowy PR 8 z kodem.
2. Zweryfikowano obecność nowych modułów:
	- `ghost-core/src/account_state_core/*`
	- `ghost-core/src/session/*`
	- `ghost-core/src/checkpoint/*`
	- `ghost-launcher/src/session/*`
	- `ghost-launcher/src/tx_intelligence/*`
3. Sprawdzono production callsite'y w `ghost-launcher/src/oracle_runtime.rs`.
4. Potwierdzono brak użycia w runtime dla:
	- `SessionManager` poza modułem/testami,
	- `try_checkpoint()` w hot-path,
	- `materialize_features()` w hot-path,
	- `evaluate_from_features()` w hot-path.
5. Potwierdzono aktywne legacy callsite'y w runtime:
	- `PerPoolOracleState`,
	- `buffer.on_transaction(tx.clone())`,
	- `shadow_ledger.get_curve(...)`,
	- `ReconciliationRuntime` jako własność `OracleRuntime`.
6. Potwierdzono, że test `ghost-launcher/tests/full_pipeline_integration.rs` dowodzi działania
	nowego pipeline'u sesyjnego w izolacji, ale nie dowodzi production cutoveru `oracle_runtime.rs`.
