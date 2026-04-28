# ADR-0028: Gatekeeper dev-buy initial-liquidity fallback

**Date:** 2026-03-22  
**Status:** Superseded  
**Author:** Ghost Father  

> Superseded by `ADR-0030-gatekeeper-dev-buy-observed-only.md` after confirming that
> `initial_liquidity_sol` may represent pump.fun bootstrap / virtual reserve state and must
> not be reinterpreted as observed creator buy exposure.

## Context

Po wdrożeniu ADR-0025 `dev_buy_total_sol` było nadal poprawnie zakotwiczone do canonical primary creator BUY, ale świeże logi Phase-5 nadal pokazywały liczne przypadki `dev_buy_total_sol = 0.0` przy jednoczesnym:

- `dev_wallet_known = true`,
- niezerowym `dev_volume_ratio`,
- `dev_has_sold = true`.

Analiza runtime i logów dla reprezentatywnych pooli wykazała, że creator pojawia się w obserwowanym streamie wyłącznie jako sprzedający, bez żadnego obserwowanego BUY creatora w oknie Gatekeepera. Równocześnie runtime i `DetectedPool` niosą `initial_liquidity_sol`, które reprezentuje create-time economic exposure puli.

Problem nie wynikał więc z kolejności, pruningu czy buforowania transakcji, tylko z rozjazdu semantycznego:

- Gatekeeper liczył `dev_buy_total_sol` wyłącznie z BUY-ów w streamie transakcji,
- część creator exposure istnieje jako create-time liquidity / bootstrap allocation i nigdy nie pojawia się jako zwykły BUY event.

## Decision

W `GatekeeperBuffer` dodano jawny fallback dla `dev_buy_total_sol` oparty o `DetectedPool.initial_liquidity_sol`.

Fallback jest **ściśle ograniczony** i aktywuje się wyłącznie wtedy, gdy jednocześnie spełnione są wszystkie warunki:

1. creator jest znany,
2. nie istnieje canonical primary creator BUY,
3. creator nie ma żadnego obserwowanego BUY w buforze,
4. creator ma co najmniej jeden **udany SELL** w buforze,
5. `initial_liquidity_sol` jest dodatnie i skończone.

W takim przypadku:

- `dev_buy_total_sol = initial_liquidity_sol`,
- `dev_initial_buy_tokens` pozostaje `None`, bo runtime nie ma wiarygodnego create-time token amount dla tego fallbacku,
- `dev_buy_volume_total_sol` pozostaje sumą wyłącznie obserwowanych BUY-ów creatora,
- `dev_volume_ratio` pozostaje bez zmiany i nadal opiera się wyłącznie o obserwowane transakcje creatora.

Runtime przekazuje teraz `initial_liquidity_sol` do `GatekeeperBuffer` przy:

- starcie per-pool taska,
- późnym upgrade metadata przez `NewPool`.

## Architectural Impact

Zmiana pozostaje lokalna dla launcherowego runtime i Gatekeepera.

Nie zmienia:

- parser contract dla `PoolTransaction`,
- semantyki `dev_buy_volume_total_sol`,
- semantyki `dev_volume_ratio`,
- SSOT Shadow Ledgera,
- flow SnapshotEngine / LivePipeline.

Zmianie ulega wyłącznie interpretacja publicznej metryki `dev_buy_total_sol` w jednym, jawnie zawężonym przypadku: creator bootstrap exposure bez obserwowalnego BUY eventu.

## Risk Assessment

**Rate:** Medium

Główne ryzyka:

- potraktowanie `initial_liquidity_sol` jako proxy `dev_buy_total_sol` może być zbyt szerokie, jeśli upstream dostarczy niekanoniczne bootstrap liquidity,
- rozszerzenie semantyki `dev_buy_total_sol` mogłoby zanieczyścić inne metryki Phase-5, gdyby fallback był użyty zbyt agresywnie.

Ryzyko zostało ograniczone przez twarde bariery:

- fallback nie działa, jeśli istnieje choć jeden obserwowany BUY creatora,
- fallback nie działa bez obserwowanego udanego SELL-a creatora,
- fallback nie dotyka `dev_buy_volume_total_sol` ani `dev_volume_ratio`,
- observed primary BUY nadal ma absolutny priorytet.

## Consequences

Po zmianie:

- `dev_buy_total_sol` przestaje pozostawać zerowe w przypadkach creator-only-sell wynikających z bootstrap liquidity,
- Phase-5 logi lepiej odzwierciedlają realny creator exposure,
- próg `dev_buy_total_sol` nadal pozostaje stabilny dla pooli, które mają normalny creator BUY w streamie.

Trade-off:

- `dev_buy_total_sol` nie jest już wyłącznie „observed creator buy amount”; jest teraz „canonical creator entry exposure”, z wyraźnym bootstrap fallbackiem.
- `dev_buy_total_sol` i `dev_buy_volume_total_sol` mają od teraz jeszcze wyraźniej rozdzielone role.

## Alternatives Considered

### 1. Nic nie zmieniać i zaakceptować `0.0`

Odrzucone, bo logi i downstream interpretują `dev_buy_total_sol` jako miarę creator entry exposure; `0.0` w creator-only-sell cases jest mylące i operacyjnie błędne.

### 2. Dodać bootstrap liquidity do `dev_buy_volume_total_sol` i `dev_volume_ratio`

Odrzucone jako scope creep i ryzyko zmiany bramek Gatekeepera. Problem dotyczył konkretnej publicznej metryki, nie całej Phase-5 księgowości wolumenu.

### 3. Przywrócić lub rozszerzyć buforowanie unknown-pool trades

Odrzucone. Poprzednia hipoteza została już sfalsyfikowana i zrollbackowana; root cause nie leżał w utracie transakcji, tylko w braku odpowiedniego fallbacku semantycznego.

## Validation Steps

1. Test jednostkowy potwierdzający, że observed primary BUY nadal wygrywa z `initial_liquidity_sol`.
2. Test jednostkowy potwierdzający fallback do najwcześniejszego creator BUY, gdy create-signature BUY nie istnieje.
3. Test jednostkowy potwierdzający fallback do `initial_liquidity_sol`, gdy creator ma tylko SELL-e i brak BUY-ów.
4. Kompilacja/testy pakietu `ghost-launcher` dla ścieżki `canonical_creator_dev_buy*`.
5. Walidacja runtime na świeżych JSONL-ach: przypadki `dev_wallet_known=true && dev_has_sold=true && no observed creator buy` nie powinny już kończyć z `dev_buy_total_sol=0.0`, o ile metadata zawiera poprawne `initial_liquidity_sol`.
