# ADR-0029: Gatekeeper pre-assessment initial-liquidity sync

**Date:** 2026-03-22  
**Status:** Superseded  
**Author:** Ghost Father  

> Superseded by `ADR-0030-gatekeeper-dev-buy-observed-only.md`. The timing fix documented
> here supported an invalid semantic mapping (`initial_liquidity_sol` → `dev_buy_total_sol`).
> Runtime hydration may still be useful for other paths, but it is no longer a valid source
> for Phase-5 creator buy exposure.

## Context

Po wdrożeniu ADR-0028 świeże `gatekeeper_v2_buys.jsonl` nadal pokazywały liczne przypadki:

- `dev_wallet_known=true`
- `dev_has_sold=true`
- `dev_buy_total_sol=0.0`

To oznaczało, że sam fallback semantyczny do `initial_liquidity_sol` nie wystarczył w aktywnym hot path runtime.

Analiza kodu wykazała rzeczywistą przyczynę:

1. `GatekeeperBuffer` umiał już ustawić `dev_buy_total_sol = initial_liquidity_sol` w bounded creator-only-sell case.
2. `OracleRuntime` przekazywał `initial_liquidity_sol` do bufora tylko wtedy, gdy wartość była dostępna bardzo wcześnie (`pool_data` przy starcie taska lub późny `NewPool`).
3. W praktyce brakujące `initial_liquidity_sol` było często backfillowane dopiero przez `backfill_initial_liquidity_sol_from_runtime(...)` wewnątrz `execute_gatekeeper_buy_path(...)`.
4. To backfillowanie następowało **po** tym, jak Gatekeeper zdążył już policzyć assessment i zalogować Phase-5 dev metrics.
5. Skutek: runtime znał lub potrafił wyprowadzić bootstrap liquidity, ale Gatekeeper assessment był zamknięty na starszym stanie bez tej wartości.

Problem nie leżał więc już w formule fallbacku, tylko w momencie synchronizacji runtime metadata do `GatekeeperBuffer`.

## Decision

Wprowadzono jawny hot-path sync runtime metadata do `GatekeeperBuffer` **przed assessment/finalizacją**, a nie dopiero w BUY path.

Dodano helper:

- `sync_gatekeeper_pool_identity_from_runtime(...)`

Helper:

1. bierze aktualne `pool_data`,
2. wykonuje `backfill_initial_liquidity_sol_from_runtime(...)` gdy wartość jest brakująca,
3. aktualizuje `GatekeeperBuffer::set_pool_identity_with_liquidity(...)`,
4. zapisuje zaktualizowane `pool_data` z powrotem do task state.

Sync jest teraz wywoływany w krytycznych punktach per-pool observation task:

- bezpośrednio po początkowym ustawieniu identity bufora,
- na początku obsługi każdej `PoolObservationMsg::Transaction(...)`,
- po późnym `NewPool` metadata upgrade,
- bezpośrednio przed `buffer.force_check_deadline(...)` przy zamknięciu kanału,
- bezpośrednio przed deadline-driven finalizacją okna.

W efekcie Gatekeeper assessment widzi najnowsze `initial_liquidity_sol` zanim policzy Phase-5 dev metrics i zanim zapisze decyzję do JSONL.

## Architectural Impact

Zmiana pozostaje lokalna dla:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/gatekeeper.rs`

Nie zmienia:

- parser contract,
- `PoolTransaction`,
- SSOT Shadow Ledgera,
- logiki samego fallbacku z ADR-0028.

Zmienia wyłącznie kontrakt czasowy między runtime metadata hydration a Gatekeeper assessment:

- wcześniej: hydration częściowo następowała po assessment,
- teraz: hydration jest wymuszana przed assessment/finalizacją.

To domyka lukę między runtime truth a Phase-5 telemetry.

## Risk Assessment

**Rate:** Medium

Główne ryzyka:

1. częstsze wywoływanie runtime hydration helpera na hot path per-transaction,
2. możliwość niezamierzonego nadpisania identity metadata, jeśli sync byłby zbyt agresywny,
3. ryzyko regresji w per-pool finalize ordering.

Ryzyko zostało ograniczone przez:

- reuse istniejącego `backfill_initial_liquidity_sol_from_runtime(...)`,
- reuse istniejącego `set_pool_identity_with_liquidity(...)`,
- brak zmian w formule scoringu poza timingiem dostarczenia danych,
- zachowanie bounded update tylko dla aktualnego `pool_data` taska.

## Consequences

### Positive

- `dev_buy_total_sol` może zostać ustawione z bootstrap liquidity zanim assessment trafi do JSONL,
- Phase-5 telemetry przestaje być zależne od tego, czy `initial_liquidity_sol` było znane wcześnie czy dopiero później,
- ADR-0028 staje się skuteczny na realnym hot path, a nie tylko semantycznie poprawny w izolacji.

### Negative / Trade-offs

- observation task wykonuje dodatkowy sync helper na ścieżce transakcyjnej i deadline path,
- logika identity hydration w runtime jest bardziej jawna i przez to bardziej rozproszona po kilku branchach task loop.

## Alternatives Considered

### 1. Nie zmieniać runtime i dalej backfillować tylko w `execute_gatekeeper_buy_path(...)`

Odrzucone, bo to jest za późno względem assessment/logging path Phase-5.

### 2. Rozszerzyć sam fallback Phase-5 o kolejne heurystyki

Odrzucone, bo root cause nie był już w formule metryki, tylko w kolejności dostarczenia danych.

### 3. Cofnąć się do launcher-side trade buffering lub innych wcześniejszych hipotez race-window

Odrzucone, bo ta hipoteza została już sfalsyfikowana i zrollbackowana; aktualny problem wynikał z timing gap runtime hydration.

## Validation Steps

1. Potwierdzić, że `sync_gatekeeper_pool_identity_from_runtime(...)` jest wywoływany:
   - po inicjalizacji bufora,
   - przy `Transaction`,
   - po `NewPool`,
   - przed timeout/finalize.
2. Potwierdzić, że `canonical_creator_dev_buy*` testy nadal przechodzą.
3. Potwierdzić kompilację `ghost-launcher` po zmianie hot path.
4. Zweryfikować na świeżych `gatekeeper_v2_buys.jsonl`, że przypadki creator-only-sell z prawidłowym runtime `initial_liquidity_sol` przestają kończyć z `dev_buy_total_sol=0.0`.
