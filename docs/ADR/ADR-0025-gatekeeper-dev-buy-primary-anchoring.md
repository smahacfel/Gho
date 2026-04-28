# ADR-0025: Gatekeeper dev-buy primary anchoring

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

W trybie `source_mode = "grpc"` Gatekeeper Phase 5 nie dostawał wiarygodnego `is_dev_buy`, więc po późniejszym poznaniu canonical creatora mógł co najwyżej odtworzyć aktywność deva heurystycznie z bufora transakcji.

Sama wiedza o creatorze nie wystarcza jednak do poprawnego ustalenia metryki `dev_buy_total_sol`. W produkcyjnym flow pump.fun pierwotny dev buy, jeśli występuje, jest semantycznie związany z transakcją `CreatePool` i dzieli z nią tę samą sygnaturę. Bez jawnej preferencji dla tej sygnatury Gatekeeper mógłby zakotwiczyć `dev_buy_total_sol` na późniejszym buyu creatora, co fałszuje metrykę genesis i progi `max_dev_buy_sol` / `min_dev_buy_sol`.

## Decision

W `GatekeeperBuffer` dodano canonical pool identity pochodzącą z `DetectedPool`:

- `pool_creator`
- `pool_create_signature`

Dodano też creator-aware retrofit Phase 5:

1. Po ustawieniu identity Gatekeeper przeszukuje `buffered_txs`.
2. Primary dev buy jest wybierany według następującej kolejności:
	- **preferowany**: najwcześniejszy BUY creatora z tą samą sygnaturą co `DetectedPool.signature`
	- **fallback**: najwcześniejszy BUY creatora w buforze, gdy create-signature BUY nie istnieje
3. Publiczna metryka `dev_buy_total_sol` jest zakotwiczona do tego primary BUY.
4. Całkowity buy volume creatora w oknie obserwacji jest liczony osobno (`dev_buy_volume_total_sol`) i nadal zasila `dev_volume_ratio`.
5. `oracle_runtime` przekazuje `creator` oraz `signature` do `GatekeeperBuffer` zarówno przy starcie taska poola, jak i przy późnym `NewPool` upgrade.

## Architectural Impact

Zmiana pozostaje w granicy launcherowego runtime/Gatekeeper i nie modyfikuje:

- parser semantics w `off-chain/components/seer`
- kontraktu `PoolTransaction`
- kontraktu `GhostEvent`
- SSOT Shadow Ledgera

Gatekeeper uzyskuje minimalną, ale kanoniczną wiedzę o tym, który BUY jest genesis-dev-buy, bez przesuwania tej odpowiedzialności do parsera czy IPC bridge.

## Risk Assessment

**Rate:** Medium

Ryzyka regresji:

- błędne przypisanie primary BUY, jeśli `DetectedPool.signature` byłoby niekanoniczne,
- rozjazd z wcześniejszą interpretacją `dev_buy_total_sol` jako sumy wszystkich buyów deva,
- potencjalna niezgodność z testami lub analizami offline, które zakładały starą semantykę.

Ryzyko zostało ograniczone przez:

- preferencję dla create signature zamiast gołego `creator == signer`,
- fallback tylko wtedy, gdy genesis BUY nie występuje,
- pozostawienie `dev_volume_ratio` opartego o łączny wolumen deva.

## Consequences

Po zmianie:

- `dev_buy_total_sol` reprezentuje canonical primary dev buy, a nie przypadkowy późniejszy BUY creatora,
- progi Gatekeepera dla dev buy lepiej odzwierciedlają intencję genesis-phase risk checks,
- późniejsze buy’e deva nadal są widoczne w `dev_tx_count` i `dev_volume_ratio`.

Trade-off:

- wewnętrzna semantyka Phase 5 staje się bardziej złożona, bo rozdziela primary dev buy od łącznego buy volume deva.

## Alternatives Considered

### 1. Pozostawić `dev_buy_total_sol` jako sumę wszystkich BUY-ów deva

Odrzucone, ponieważ nie gwarantuje zakotwiczenia metryki do genesis BUY z create signature i może fałszować ocenę `max_dev_buy_sol`.

### 2. Naprawić semantykę wyłącznie w parserze/gRPC path

Odrzucone w tym kroku jako zbyt inwazyjne względem obecnych kontraktów i granic SSOT.

### 3. Używać tylko `creator == signer` bez preferencji dla create signature

Odrzucone, bo nie rozwiązuje przypadku, w którym późniejszy BUY creatora pojawi się przed poprawnym rozpoznaniem genesis BUY.

## Validation Steps

1. Testy jednostkowe Gatekeepera:
	- preferencja `create signature` dla primary creator BUY,
	- fallback do najwcześniejszego BUY creatora, gdy create-signature BUY nie istnieje.
2. Walidacja runtime:
	- `oracle_runtime` przekazuje `creator + signature` przy starcie i przy late metadata upgrade.
3. Walidacja produkcyjna/logowa:
	- dla pooli z create+dev-buy w jednej sygnaturze `dev_buy_total_sol` ma odpowiadać właśnie temu BUY,
	- późniejsze buy’e creatora nie mogą nadpisywać primary genesis wartości.
