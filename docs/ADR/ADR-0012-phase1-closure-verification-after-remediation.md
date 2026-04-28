# ADR-0012: Weryfikacja domknięcia Fazy 1 po remediacji luk audytu

**Date:** 2026-03-20  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Po audycie zapisanym w `docs/ADR/ADR-0011-phase1-storage-arbitration-audit.md` pozostały trzy jawnie nazwane luki blokujące formalne domknięcie Fazy 1:

1. `commit_history()` i `append_live()` nie przechodziły przez ten sam wspólny contract dispatch co arbitration curve write'ów.
2. `apply_curve_write(...)` wykonywało `register_curve_alias(...)` przed końcowym werdyktem precedence, więc część rejectów nie była side-effect free na alias-plane.
3. Legacy helper surface pozostawał zbyt szeroki i semantycznie zbyt łatwo dostępny.

Użytkownik zgłosił, że luki zostały domknięte i wskazał konkretne zmiany w:

- `ghost-core/src/shadow_ledger/ledger.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/wal_recovery.rs`

oraz uruchomione walidacje kompilacyjne i testowe.

Niniejsza decyzja dokumentuje zimną weryfikację po remediacji — przeciwko literalnym wymaganiom Fazy 1 z `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`.

## Decision

Uznano, że **Faza 1 jest po tych poprawkach formalnie domknięta w swoim zakresie**.

### Potwierdzone domknięcie trzech luk

#### 1. Wspólny contract dispatch dla realnych write pathów

W `ghost-core/src/shadow_ledger/ledger.rs` wszystkie produkcyjnie istotne write role Fazy 1 są teraz prowadzone przez wspólny dispatch oparty o:

- `ShadowLedgerWriteRequest`
- `ShadowLedgerWriteOutcome`
- `ShadowLedger::apply_write(...)`

To obejmuje:

- curve writes przez `apply_curve_write(...)`,
- canonical commit przez `commit_history_with_source(...)`,
- live append przez `append_live_with_source(...)`.

Oznacza to, że write pathy, które plan Fazy 1 modeluje jako role `P0/P1/P2/P3/P4`, nie są już rozszczepione na „arbitraż curve obok osobnych ścieżek commit/live”, tylko są spięte przez jeden wspólny storage contract dispatch.

#### 2. Rejecty precedence są side-effect free na alias-plane

W `apply_curve_write_inner(...)` rejestracja aliasu następuje po wyliczeniu wyniku write'u.

W efekcie wyniki:

- `RejectedWeakerWrite`
- `RejectedOutOfOrder`
- `RejectedMissingMetadata`

nie wykonują już mutacji `curve_keys_by_base_mint`. To domyka dokładnie ten dług semantyczny, który poprzedni audit nazwał jako residual alias-plane side effect.

`NoOpExistingEqualOrStronger` nadal może utrzymać alias binding dla tej samej zaakceptowanej prawdy storage i nie narusza kontraktu Fazy 1.

#### 3. Legacy surface został zredukowany do helperów wewnętrznych

W `ghost-core/src/shadow_ledger/ledger.rs` helpery:

- `insert_with_slot_known*`
- `insert_seed_curve*`

zostały zredukowane do `pub(crate)`.

W `off-chain/components/seer/src/lib.rs`:

- `store_curve_with_snapshots(...)` nie jest już publicznym runtime helperem,
- aktywne callery runtime idą przez jawne role semantyczne:
  - `store_bootstrap_seed(...)`
  - `store_confirmed_bootstrap(...)`
  - `store_repair_curve(...)`

W `ghost-launcher/src/wal_recovery.rs` replay curve updates przechodzi przez ten sam storage contract przez `apply_curve_write(...)`.

To nie usuwa wszystkich helperów pomocniczych z kodu, ale domyka wymóg Fazy 1: precedence nie jest już pozostawione callerom, a aktywny runtime nie ma szerokiego, publicznego backdoora do omijania jawnych ról `P0/P1/P2`.

### Dodatkowe ustalenie walidacyjne

Podczas weryfikacji ujawniono ważny proceduralny szczegół: część komend testowych podanych początkowo przez użytkownika nie wykonywała realnych testów przy danym filtrze i kończyła się `running 0 tests`.

Dotyczyło to trzech testów `ghost-core`, gdy były uruchamiane po samej krótkiej nazwie funkcji z `--exact`.

Dlatego walidacja została skorygowana i powtórzona po pełnych nazwach modułowych:

- `shadow_ledger::ledger::tests::test_duplicate_bootstrap_seed_is_storage_level_noop`
- `shadow_ledger::ledger::tests::commit_history_reports_noop_for_existing_history`
- `shadow_ledger::ledger::tests::test_append_live_rejects_slot_zero`
- `wal_recovery::tests::replay_duplicate_bootstrap_seed_is_noop_under_storage_arbitration`

Po tej korekcie wszystkie cztery testy zostały faktycznie wykonane i przeszły.

## Architectural Impact

Ta decyzja zmienia status Fazy 1 z:

- „merytorycznie mocna, ale jeszcze nieformalnie domknięta”

na:

- „formalnie domknięta w zakresie nakazów i zakazów Fazy 1”.

Skutki architektoniczne:

1. Storage-level precedence jest teraz wymuszane dla wszystkich pięciu ról write'ów modelu Fazy 1 poprzez wspólny dispatch contract.
2. Bootstrap ma jednego semantycznego właściciela, a drugi bootstrap pozostaje storage-level `NoOp` albo upgrade `P0 -> P1`.
3. Replay curve update nie jest osobną heurystyczną ścieżką i korzysta z tego samego contractu co runtime.
4. Alias-plane nie przecieka już przez rejecty precedence.

To oznacza, że Warunek wejścia do Fazy 2 z planu:

- „Faza 1 ukończona”
- „storage arbitration działa i precedence jest testowane”

można teraz uznać za spełniony.

## Risk Assessment

**Rate: Low**

Ryzyko regresji względem samego celu Fazy 1 jest niskie, ponieważ:

- wspólny dispatch został potwierdzony w kodzie,
- alias-side-effect debt został zdjęty,
- replay korzysta z tego samego contractu,
- testy precedence i reject-pathów zostały faktycznie wykonane po skorygowanym doborze selektorów.

Pozostają ogólne ryzyka repo niezwiązane z closure Fazy 1:

- duża liczba istniejących warningów,
- starsze testy poza zakresem tej pracy,
- alias-only mutacje niezwiązane z precedence plane, zinwentaryzowane już wcześniej w Fazie 0.

Nie są to jednak luki blokujące formalne domknięcie Fazy 1.

## Consequences

Co stało się łatwiejsze:

- można przejść do Fazy 2 bez udawania, że Faza 1 „jest prawie gotowa”,
- storage contract Fazy 1 ma już spójną egzekucję dla `P0/P1/P2/P3/P4`,
- semantyka bootstrapu i replay curve update jest zamknięta bardziej rygorystycznie,
- odrzucone curve writes są czystsze semantycznie i bez ukrytych alias side effectów.

Co nadal nie wynika z tej decyzji:

- cały workspace nie jest wolny od warningów,
- Faza 4 recovery ordering nie jest jeszcze przez to sama z siebie ukończona,
- legacy side paths całego systemu nie są przez to automatycznie zamknięte — to są kolejne fazy planu.

## Alternatives Considered

### 1. Nadal nie uznawać Fazy 1 za domkniętą
Odrzucono, bo trzy wcześniej nazwane luki rzeczywiście zostały usunięte w kodzie, a nie tylko opisane.

### 2. Uznać domknięcie wyłącznie na podstawie deklaracji użytkownika
Odrzucono, bo część pierwotnie podanych komend testowych dawała fałszywe zielone wyniki `0 tests`, więc potrzebna była niezależna weryfikacja.

### 3. Uznać Fazę 1 za domkniętą dopiero po pełnym zielonym workspace
Odrzucono, bo plan Fazy 1 nie wymaga pełnego wyzerowania historycznego długu repo; wymaga domknięcia precedence, bootstrap semantics i testów kontraktowych dla tego zakresu.

## Validation Steps

Weryfikacja została wykonana przez:

1. odczyt aktualnych implementacji w:
	- `ghost-core/src/shadow_ledger/ledger.rs`
	- `off-chain/components/seer/src/lib.rs`
	- `ghost-launcher/src/wal_recovery.rs`
2. potwierdzenie, że `commit_history_with_source(...)` i `append_live_with_source(...)` przechodzą przez `apply_write(...)`,
3. potwierdzenie, że `register_curve_alias(...)` w `apply_curve_write_inner(...)` jest wykonywane dopiero po werdykcie i nie zachodzi dla rejectów `RejectedWeakerWrite`, `RejectedOutOfOrder`, `RejectedMissingMetadata`,
4. wyszukanie callerów legacy helperów i potwierdzenie, że aktywny runtime nie opiera się już na publicznym helper surface dla tych ról,
5. kompilację kontrolną:
	- `cargo test -p ghost-launcher --lib --no-run -q`
6. wykonanie testu launcherowego:
	- `cargo test -p ghost-launcher --lib wal_recovery::tests::replay_duplicate_bootstrap_seed_is_noop_under_storage_arbitration -- --exact`
7. ustalenie pełnych nazw modułowych testów `ghost-core` przez:
	- `cargo test -p ghost-core --lib -- --list`
8. wykonanie realnych testów `ghost-core` po pełnych nazwach:
	- `cargo test -p ghost-core --lib shadow_ledger::ledger::tests::test_duplicate_bootstrap_seed_is_storage_level_noop -- --exact`
	- `cargo test -p ghost-core --lib shadow_ledger::ledger::tests::commit_history_reports_noop_for_existing_history -- --exact`
	- `cargo test -p ghost-core --lib shadow_ledger::ledger::tests::test_append_live_rejects_slot_zero -- --exact`
9. potwierdzenie, że wszystkie powyższe testy wykonały się jako `running 1 test` i zakończyły się `ok`.
