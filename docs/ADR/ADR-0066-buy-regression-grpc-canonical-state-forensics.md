# ADR-0066: BUY regression gRPC canonical-state forensics

**Date:** 2026-03-31
**Status:** Accepted
**Author:** Ghost Father

## Context

Po zmianach opisanych w `PLANS/REFACTOR.md` runtime gRPC przestał generować nowe wpisy w `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`, mimo bardzo niskich progów i wielogodzinnego czasu działania. Jednocześnie standardowe logi wskazują dominację decyzji `TIMEOUT_PHASE1` i `TIMEOUT_NO_DATA` z `ghost_launcher::oracle_runtime`.

Dodatkowo w runtime nadal istnieje RPC fallback związany z odświeżaniem krzywej bondingowej, w tym ostrzeżenie:

`RPC LAG BYPASS: Symuluje swieza krzywa ...`

Wymagane było ustalenie:

1. pełnej ścieżki danych prowadzącej do `gatekeeper_v2_buys.jsonl`,
2. miejsc, w których refaktor mógł przerwać dopływ eventów / update’ów tx / signerów / curve state,
3. dokładnej lokalizacji RPC fallbacku i bezpiecznej sekwencji jego usunięcia.

## Decision

Przyjmuje się następującą diagnozę techniczną jako aktualny SSOT śledztwa:

1. `gatekeeper_v2_buys.jsonl` nie jest osobnym źródłem decyzji. To wtórny routing z `DecisionLogger`, aktywny wyłącznie dla rekordów z `decision_verdict_buy == Some(true)`.
2. Dominacja `TIMEOUT_PHASE1` / `TIMEOUT_NO_DATA` oznacza, że runtime nie dochodzi do terminalnego `GatekeeperVerdict::Buy`, a nie że logger BUY-only jest uszkodzony.
3. Najbardziej prawdopodobny regres po refaktorze znajduje się na styku:
   - `GhostEvent::AccountUpdate`
   - `OracleRuntime::build_account_state_update`
   - `AccountStateCore`
   - `PoolObservationSession::current_curve_readiness`

   Konkretnie: update’y account-state mogą być gubione, jeśli przyjdą przed pełną rejestracją identity/base-mint w runtime.
4. RPC fallback w `refresh_bonding_curve_state()` nie zasila nowego feature-driven policy path, bo zapisuje tylko do `session.candidate_snapshot`, a nie do `account_state_core`. W nowej architekturze jest to behavior mylący i częściowo martwy.
5. Bezpieczne usunięcie fallbacku RPC musi nastąpić dopiero po potwierdzeniu, że canonical account path (`Seer AccountUpdate -> GhostEvent::AccountUpdate -> AccountStateCore.apply_account_update`) jest stabilny i kompletny.

## Architectural Impact

Wynik śledztwa wzmacnia architekturę docelową z `PLANS/REFACTOR.md`:

- `AccountStateCore` pozostaje jedynym live source-of-truth dla canonical curve state.
- `ShadowLedger` pozostaje fallbackiem bootstrap/degraded, ale nie może odzyskiwać odpowiedzialności za live truth.
- `DecisionLogger` jest tylko warstwą routingu/retencji i nie jest źródłem regresji BUY.
- `refresh_bonding_curve_state()` w obecnym kształcie narusza czytelność architektury, bo symuluje świeżą krzywą poza canonical state path.

Dotknięte komponenty:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/components/seer.rs`

## Risk Assessment

**Rate: High**

Ryzyka:

1. Jeśli account updates są gubione przed rejestracją pool identity, cały gatekeeper może systemowo wpadać w `PendingCurve` i timeouty mimo prawidłowego tx feedu.
2. Jeśli zespół usunie RPC fallback przed naprawą canonical ingest, może chwilowo stracić jedyną degradacyjną ścieżkę rezerwy diagnostycznej.
3. Jeśli fallback zostanie pozostawiony bez zmian, operatorzy będą dalej dostawać mylący sygnał, że curve state jest „odświeżany”, choć nowy policy path tego nie widzi.

## Consequences

Po przyjęciu tej diagnozy:

- debugowanie powinno koncentrować się na ingressie `AccountUpdate` i rejestracji identity, nie na loggerze BUY-only,
- usunięcie RPC fallbacku wymaga najpierw potwierdzenia zdrowia canonical path,
- metryki i logi powinny zostać rozszerzone o jawne sygnały dropów dla account updates przy braku identity.

Łatwiejsze:

- wskazanie właściwej warstwy regresji,
- odróżnienie problemu routingu logów od problemu verdict path.

Trudniejsze:

- utrzymanie graceful degradation bez jawnego RPC bypassu, jeśli ingress account path nadal ma race condition.

## Alternatives Considered

### 1. Uznanie, że problem leży w `DecisionLogger`

Odrzucono, ponieważ `write_gatekeeper_buy_log()` poprawnie zapisuje każdy rekord do `gatekeeper_v2_decisions.jsonl` i tylko BUY-y do `gatekeeper_v2_buys.jsonl`. Brak nowych wpisów BUY-only jest skutkiem braku BUY verdictów, nie awarii loggera.

### 2. Uznanie, że winne są wyłącznie zaostrzone progi Gatekeepera

Odrzucono, ponieważ użytkownik wskazał niemal zerowe progi, a obserwowane tagi to timeouty `TIMEOUT_PHASE1` / `TIMEOUT_NO_DATA`, co wskazuje na problem data-path lub readiness, nie stricte policy thresholds.

### 3. Pozostawienie `refresh_bonding_curve_state()` jako trwałego obejścia

Odrzucono, ponieważ nowy feature-driven path odczytuje canonical curve readiness z `AccountStateCore`, a nie z `candidate_snapshot`, więc fallback nie rozwiązuje przyczyny i zaciemnia obraz systemu.

## Validation Steps

1. Potwierdzić, że `SeerEvent::AccountUpdate` są emitowane i mostkowane do `GhostEvent::AccountUpdate`.
2. Zmierzyć, ile wywołań `build_account_state_update()` kończy się `None` przez brak `pool_identities.get_by_base_mint(base_mint)`.
3. Potwierdzić wzrost liczników/metryk związanych z promocją bootstrap → canonical w `AccountStateCore`.
4. Dla timeoutujących pooli sprawdzić, czy `session.account_features.update_count == 0` i czy `current_curve_readiness().is_ready` pozostaje `false`.
5. Dopiero po potwierdzeniu zdrowego canonical ingest:
   - usunąć branch RPC simulation / bypass w `refresh_bonding_curve_state()`,
   - usunąć jego call-site’y,
   - zostawić ewentualnie wyłącznie reconciliation RPC cycle jako jawny, obserwowalny degraded path.
