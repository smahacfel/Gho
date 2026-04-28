# ADR-0030: Gatekeeper dev-buy must remain observed-only

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Po ponownej analizie logów i hot path runtime potwierdzono, że wcześniejsze założenie było
błędne: `initial_liquidity_sol` nie jest wiarygodnym odpowiednikiem `dev_buy_total_sol`.

W tym systemie `initial_liquidity_sol` może pochodzić z bootstrap / reserve contextu bonding
curve, a w praktyce dla pump.fun bywa skorelowane z wirtualnymi rezerwami SOL na starcie
krzywej. Typowym przypadkiem jest wartość około `30.0`, która opisuje curve bootstrap state,
a nie to, ile SOL rzeczywiście kupił creator/dev.

Skutek wcześniejszej zmiany był poważny semantycznie:

- `dev_buy_total_sol` przestawało oznaczać observed creator buy,
- logi Phase-5 mogły raportować sztuczne `30.0` nawet bez żadnego BUY-a creatora,
- analiza dev exposure była zanieczyszczana wartością pochodzącą z modelu krzywej, nie z
  transakcji.

## Decision

`GatekeeperBuffer` nie może używać `initial_liquidity_sol` jako fallbacku dla
`dev_buy_total_sol`.

Od teraz obowiązuje twarda semantyka:

1. `dev_buy_total_sol` = observed canonical creator buy amount,
2. jeśli creator ma tylko SELL-e i brak observed BUY-a, wtedy `dev_buy_total_sol = 0.0`,
3. `initial_liquidity_sol` pozostaje oddzielną metryką runtime / curve bootstrap i nie może
   być reinterpretowane jako creator buy exposure.

Wdrożona zmiana usuwa fallback:

- `primary_buy_tx == None`
- brak observed creator BUY
- nawet przy dodatnim `initial_liquidity_sol`

nie wolno już ustawiać `dev_buy_total_sol` z `pool_initial_liquidity_sol`.

## Architectural Impact

Zmiana przywraca jednoznaczną granicę między:

- obserwowanym zachowaniem creatora w streamie transakcji,
- metadata / reserve contextem krzywej bonding curve.

Nie zmienia to:

- trackingu `dev_sell_total_sol`,
- trackingu `dev_buy_volume_total_sol`,
- `dev_volume_ratio`,
- runtime metadata hydration dla innych komponentów.

Zmienia wyłącznie semantykę i źródło prawdy dla `dev_buy_total_sol`: source-of-truth to tylko
obserwowane transakcje BUY creatora.

## Risk Assessment

**Rate:** Medium

Ryzyka:

1. część logów wróci do `dev_buy_total_sol = 0.0`,
2. istniejące analizy oparte na wcześniejszym błędnym fallbacku mogą zauważyć zmianę,
3. runtime sync `initial_liquidity_sol` pozostaje w kodzie, więc trzeba uważać, aby nie
   wykorzystać tej wartości ponownie do Phase-5 exposure.

To jednak jest ryzyko akceptowalne, bo alternatywą byłoby dalsze raportowanie fałszywego
creator buy exposure.

## Consequences

Po zmianie:

- `dev_buy_total_sol` znów oznacza dokładnie to, co sugeruje nazwa,
- creator-only-sell przypadki nie będą już sztucznie wyglądały jak buy `30 SOL`,
- analiza logów stanie się bardziej brutalna, ale prawdziwa.

Trade-off:

- tracimy heurystyczne „wypełnianie dziury” dla creator-only-sell cases,
- ale unikamy pomieszania ekonomiki krzywej z rzeczywistym zachowaniem dev wallet.

## Alternatives Considered

### 1. Utrzymać fallback i tylko przemianować pole

Odrzucone. Pole `dev_buy_total_sol` jest używane i interpretowane jako creator buy exposure;
zmiana nazwy nie rozwiązuje błędnych danych historycznych ani aktualnej logiki.

### 2. Dodać osobne pole typu `creator_bootstrap_liquidity_sol`

Nie wdrożono w tej zmianie. To mogłoby być poprawne architektonicznie, ale byłoby nowym
zakresem pracy. Aktualny fix ma tylko usunąć błędne mapowanie semantyczne.

### 3. Pozostawić zero i nic więcej nie robić

To właśnie jest wdrożona semantyka dla creator-only-sell bez observed BUY. Zero jest tutaj
poprawniejsze niż fałszywe `30.0`.

## Validation Steps

1. Test jednostkowy creator-primary-buy nadal wybiera observed canonical BUY.
2. Test jednostkowy fallback do earliest creator BUY nadal działa, gdy create signature nie jest
   obecny.
3. Test jednostkowy creator-only-sell potwierdza, że `dev_buy_total_sol` pozostaje `0.0` nawet
   przy dodatnim `initial_liquidity_sol`.
4. Uruchomić wybrane testy `ghost-launcher` dla `canonical_creator_dev_buy*`.
5. Zweryfikować na świeżych logach, że sztuczne `30.0` nie pojawia się już jako substytut
   observed creator buy.