# ADR-0057: PR8 acceptance boundary — production cutover vs literal legacy deletion

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Po `ADR-0056` wymagany był niezależny re-audyt tego, czy PR8 można już uczciwie uznać za zamknięty względem dwóch źródeł SSOT jednocześnie:

- `PLANS/REFACTOR.md`
- `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md`

To rozróżnienie było konieczne, ponieważ oba dokumenty opisują ten sam etap, ale z różną ostrością semantyczną:

- `ADR-0054` definiuje **operacyjną checklistę zamknięcia hot-path**, skupioną na tym, czy legacy state/path nadal uczestniczą w produkcyjnym flow.
- `PLANS/REFACTOR.md` zawiera także **mocniejsze, literalne oczekiwania cleanupu/deletion**, np. usunięcie definicji `PerPoolOracleState` i pełne wycięcie compatibility shimów.

Re-audyt objął bieżący kod w:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/components/seer.rs`
- `off-chain/components/seer/src/config.rs`
- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/ipc.rs`
- `config.toml`

oraz celowane testy dowodowe uruchomione w tej sesji.

## Decision

PR8 jest **zamknięty operacyjnie jako production cutover**, ale **nie jest zamknięty w sensie pełnego literalnego cleanupu/deletion opisanego najszerzej w `PLANS/REFACTOR.md`**.

### 1. Werdykt względem `ADR-0054`

`ADR-0054` można uznać za spełnione w praktycznym sensie zamknięcia PR8:

- produkcyjny `GhostEvent::NewPoolDetected` używa `register_runtime_pool_detection(...)`, nie `register_new_pool(...)`;
- `register_runtime_pool_detection(...)` nie populates `OracleRuntime.pools`;
- `lookup_base_mint_for_pool(...)`, `lookup_shadow_metadata_pool(...)`, `build_runtime_state_pool_snapshot(...)` i `refresh_bonding_curve_state(...)` nie polegają już na compat pool map jako produkcyjnym źródle prawdy;
- `mark_pool_committed(...)` i `remove_pool(...)` działają dla registry/session-backed runtime pools bez wymagania compat entry;
- launcher startup wymusza canonical `AccountUpdate` ingest, gdy `account_state_core.enable=true`;
- produkcyjny buy/reject path nie zależy od deprecated `GatekeeperBuffer.on_transaction()`.

W tym sensie:

- `SessionManager` jest realnym aktywnym ownerem runtime sessions per pool,
- `AccountStateCore` pozostaje primary truth dla canonical market state,
- compat state nie jest już wymaganym elementem produkcyjnego flow.

### 2. Werdykt względem literalnego brzmienia `PLANS/REFACTOR.md`

`PLANS/REFACTOR.md` nie jest spełnione literalnie w najszerszym sensie cleanupu, ponieważ w repo nadal istnieją:

- definicja `PerPoolOracleState`,
- pole `OracleRuntime.pools`,
- `register_new_pool(...)`,
- compat/test-support helpery operujące na legacy mapie,
- degraded/test-only ścieżka `account_updates_enabled=false` w Seer internals.

Szczególnie istotne jest to, że plan używa mocnych sformułowań typu:

- „Usunięcie wszystkich deprecated legacy paths”
- „PerPoolOracleState — usunięcie definicji, `OracleRuntime.pools` → `SessionManager`”

Te warunki nie są jeszcze spełnione **symbolicznie/literalnie**, bo legacy artefakty nadal fizycznie istnieją w kodzie.

### 3. Ostateczna granica akceptacji

Dlatego przyjmuje się rozróżnienie SSOT:

- **PR8 = zamknięty jako production-runtime cutover**
- **PR8 ≠ w pełni zakończony jako literalne wycięcie wszystkich legacy symboli i shimów z repo**

Nie wolno używać sformułowania „PR8 spełnia wszystkie kryteria z obu dokumentów” bez tego zastrzeżenia.

Poprawne sformułowanie brzmi:

> PR8 domknął production path i spełnia operacyjną checklistę `ADR-0054`, ale nie spełnia jeszcze literalnie najszerszego cleanup/deletion wording z `PLANS/REFACTOR.md`.

## Architectural Impact

To ADR porządkuje język architektoniczny:

- produkcyjny runtime nie jest już hybrydą wymagającą compat pool map do działania,
- ale repo nadal zawiera legacy ballast przeznaczony dla testów, supportu i compatibility helpers.

Od teraz należy odróżniać dwa stany:

1. **runtime closed** — produkcja działa bez legacy ownership,
2. **repository cleaned** — legacy symbols zostały fizycznie usunięte.

PR8 osiągnął stan (1), ale nie stan (2).

## Risk Assessment

**Risk:** Medium

### Główne ryzyko, jeśli ktoś nazwie to po prostu „pełne zamknięcie PR8”

- powstanie fałszywe przekonanie, że repo nie zawiera już legacy runtime ballast,
- przyszłe audyty będą mieszać brak produkcyjnej zależności z pełnym symbolicznym cleanupem,
- ktoś może uznać, że pozostawione compat helpers są już „zaakceptowanym końcem”, mimo że plan literalnie mówił o usunięciu.

### Główne ryzyko, jeśli ktoś nazwie to „PR8 nadal otwarty” bez doprecyzowania

- cofnie się prawdziwy credit za realny production cutover,
- utraci się rozróżnienie między hot-path closure a repo hygiene,
- operatorzy i reviewerzy mogą wrócić do zamkniętych już sporów o production ownership.

## Consequences

### Co staje się łatwiejsze

- uczciwe raportowanie statusu bez nadpisywania rzeczywistości,
- rozdzielenie „repo cleanup” od „runtime closure”,
- uniknięcie ponownego otwierania zamkniętego już production cutoveru.

### Co staje się trudniejsze

- status PR8 wymaga teraz precyzyjnego języka,
- nie można już skrótem mówić „pełne done” bez dopisku o literalnym cleanup boundary.

## Alternatives Considered

### 1. Uznać PR8 za w pełni zamknięty bez zastrzeżeń

Rejected, ponieważ pozostawione `PerPoolOracleState`, `OracleRuntime.pools` i `register_new_pool(...)` nadal istnieją w repo, więc literalne cleanup wording planu nie zostało spełnione.

### 2. Uznać PR8 za nadal otwarty w całości

Rejected, ponieważ obecny production flow nie zależy już od compat pool map ani od legacy scoring path, więc byłoby to zafałszowanie realnego stanu runtime.

### 3. Rozdzielić „operational closure” od „symbolic deletion closure”

Accepted, ponieważ tylko to podejście jednocześnie zachowuje prawdę o produkcyjnym hot-path oraz prawdę o pozostawionym repo ballast.

## Validation Steps

1. Zweryfikowano w `ghost-launcher/src/oracle_runtime.rs`, że:
   - `GhostEvent::NewPoolDetected` używa `register_runtime_pool_detection(...)`,
   - `register_runtime_pool_detection(...)` nie zapisuje do `self.pools`,
   - `register_new_pool(...)` pozostaje legacy compat path,
   - `build_runtime_state_pool_snapshot(...)`, `lookup_shadow_metadata_pool(...)`, `lookup_base_mint_for_pool(...)` i `refresh_bonding_curve_state(...)` nie opierają się na compat lookup jako produkcyjnym primary path.
2. Zweryfikowano w `ghost-launcher/src/main.rs`, że startup wymusza `oracle_account_updates_enabled = true` przy `account_state_core.enable=true`.
3. Zweryfikowano w `off-chain/components/seer/src/lib.rs`, że degraded tx/bootstrap-only path nadal istnieje, ale pozostaje ścieżką test/support, nie produkcyjnym kontraktem przy core-enabled startup.
4. Uruchomiono celowane testy dowodowe:
   - `cargo test -q -p ghost-launcher --lib pr8 -- --nocapture` → `4 passed; 0 failed`
   - `cargo test -q -p ghost-launcher --bin ghost-launcher test_pr8_startup_forces_account_updates_when_account_state_core_enabled -- --nocapture` → `1 passed; 0 failed`
   - `cargo test -q -p ghost-launcher --lib pr7_invariant -- --nocapture` → `1 passed; 0 failed`
5. Porównano wynik z:
   - operacyjną checklistą `ADR-0054`,
   - oraz literalnym wording cleanup/deletion w `PLANS/REFACTOR.md`.
