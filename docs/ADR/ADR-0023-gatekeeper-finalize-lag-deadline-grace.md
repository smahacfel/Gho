# ADR-0023: Gatekeeper finalize lag caused by hardcoded post-deadline grace

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

W runtime Gatekeepera obserwowano stały dodatkowy lag po decyzji, widoczny w dwóch miejscach:

- `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` jako `finalize_lag_ms`,
- logach shadow-run jako `decision_to_sim_start_ms`.

Cel diagnostyczny był jednoznaczny: wyjaśnić, skąd bierze się powtarzalne ~100 ms opóźnienia po decyzji Gatekeepera i czy jest to koszt nieunikniony, czy sztucznie dołożony przez kod.

Inspekcja kodu wykazała, że w `ghost-launcher/src/components/gatekeeper.rs` metryka `finalize_lag_ms` nie mierzy pełnego okna obserwacji, tylko **overshoot ponad skonfigurowane `max_wait_time_ms`**:

- `finalize_lag_ms = now_wall_ms - registered_wall_ts_ms - config.max_wait_time_ms`

To oznacza, że wartości ~100 ms nie są normalnym kosztem samej obserwacji; są dodatkowym opóźnieniem po przekroczeniu deadline'u.

Następnie w `ghost-launcher/src/oracle_runtime.rs` wykryto bezpośrednią przyczynę:

- per-pool task ustawia deadline przez `tokio::time::sleep(max_wait_time_ms + 100)`,
- komentarz przy tym kodzie jawnie opisuje `+100ms grace to avoid races with event-time based deadline in buffer`.

To jest twardo zakodowane, sztuczne opóźnienie dodawane po każdym deadline Gatekeepera.

Dodatkowo analiza ścieżki BUY wykazała, że shadow-run ma drugi, odrębny problem obserwowalności:

- `decision_to_sim_start_ms` liczone jest od `decision_ts_ms`,
- ale `decision_ts_ms` jest dziś stemplowane dopiero po zakończeniu `prepare_buy_request(...)`,
- a `prepare_buy_request(...)` wykonuje kilka sekwencyjnych RPC przed startem symulacji.

W efekcie obecna metryka shadow-run **nie mierzy prawdziwego Gatekeeper verdict -> simulation start**, tylko późniejszy, zawężony fragment ścieżki.

## Decision

Przyjęto następującą diagnozę architektoniczną:

1. Główną przyczyną `finalize_lag_ms ≈ 100 ms` jest **hardcoded `+100 ms` deadline grace** w `ghost-launcher/src/oracle_runtime.rs`.
2. `finalize_lag_ms` jest poprawną metryką ujawniającą ten overshoot; sama definicja metryki nie jest błędna.
3. `decision_to_sim_start_ms` częściowo koreluje z tym samym zjawiskiem, ale obecnie jest metryką zaniżoną względem prawdziwego verdict-to-sim latency, bo start pomiaru następuje za późno.
4. Samo usunięcie `+100 ms` powinno zbić lag rejestrowany jako overshoot niemal do zera lub do pojedynczych milisekund scheduler/runtime jitter.
5. Jeśli celem operacyjnym jest **<5 ms od prawdziwego werdyktu Gatekeepera do startu shadow sim**, to poza usunięciem grace trzeba także:
   - przenieść timestamp decyzji do momentu faktycznego werdyktu BUY,
   - oddzielić obserwowalność od kosztu `prepare_buy_request(...)`,
   - rozważyć prefetch/caching danych potrzebnych do shadow simulation jeszcze w trakcie okna obserwacji.

## Architectural Impact

Ta decyzja rozdziela trzy różne pojęcia, które wcześniej były mieszane:

1. **okno obserwacji Gatekeepera** — zamierzony koszt analityczny,
2. **post-deadline overshoot** — niezamierzony koszt wprowadzony przez `+100 ms grace`,
3. **rzeczywisty verdict-to-sim latency** — koszt przejścia z decyzji do shadow simulation, dziś tylko częściowo widoczny w telemetryce.

Wpływ na system:

- usunięcie `+100 ms` nie zmienia logiki selekcji Gatekeepera ani analitycznych progów,
- zmienia tylko moment terminalizacji po deadline,
- wymaga ostrożnego potwierdzenia, że istniejące event-time checks w buforze wystarczą bez dodatkowego sztucznego opóźnienia,
- może ujawnić prawdziwy koszt `prepare_buy_request(...)`, który dziś jest częściowo ukryty przez późne timestampowanie.

## Risk Assessment

**Rate:** Medium

### Główne ryzyka

1. **Race na granicy deadline'u**
   - usunięcie `+100 ms` może odsłonić wcześniej maskowany wyścig między timerem runtime a event-time deadline w buforze.

2. **Zmiana semantyki metryk operacyjnych**
   - po usunięciu grace `finalize_lag_ms` powinien spaść gwałtownie; to jest oczekiwane, ale trzeba uważać, by nie zinterpretować tego jako „magicznego przyspieszenia całej ścieżki BUY”.

3. **Odkrycie ukrytego kosztu shadow prep**
   - po poprawnym timestampowaniu `decision_to_sim_start_ms` może chwilowo wzrosnąć, bo zacznie mierzyć prawdziwy koszt verdict-to-sim, a nie zawężony fragment po RPC prep.

## Consequences

### Positive

- znika sztuczne ~100 ms opóźnienie po deadline Gatekeepera,
- `finalize_lag_ms` wraca do roli metryki schedulerowego overshootu zamiast odzwierciedlać świadomie doklejony grace,
- łatwiej odróżnić prawdziwy koszt shadow preparation od kosztu finalizacji Gatekeepera.

### Negative / Trade-offs

- jeśli grace rzeczywiście maskował rzadkie boundary races, trzeba je teraz obsłużyć poprawnie na poziomie deadline contract, a nie przez sztywny sleep,
- korekta telemetryki może chwilowo „pogorszyć” raportowane shadow latency, ale tylko dlatego, że metryka zacznie mówić prawdę.

## Alternatives Considered

### 1. Zostawić `+100 ms` i tylko ukryć/zmienić metrykę

Odrzucone, bo problem jest rzeczywisty i wprowadzony przez runtime, a nie przez samą obserwowalność.

### 2. Zastąpić `+100 ms` mniejszą stałą, np. `+10 ms`

Odrzucone jako półśrodek. To nadal byłby sztuczny overshoot zamiast poprawnego kontraktu deadline.

### 3. Usunąć `+100 ms`, ale nie ruszać telemetryki shadow-run

Odrzucone jako rozwiązanie niepełne, jeśli celem operacyjnym ma być prawdziwe `<5 ms` od werdyktu do startu symulacji.

### 4. Rozdzielić naprawę na dwa etapy

Przyjęte jako właściwe podejście:

- **Etap A:** usunąć hardcoded `+100 ms` grace i zweryfikować spadek `finalize_lag_ms`,
- **Etap B:** poprawić timestamping i ewentualnie zredukować/precache koszt `prepare_buy_request(...)` dla realnego verdict-to-sim SLA.

## Validation Steps

1. Usunąć w `ghost-launcher/src/oracle_runtime.rs` hardcoded `saturating_add(100)` z per-pool deadline sleep.
2. Uruchomić shadow-run i potwierdzić, że `finalize_lag_ms` spada z ~100 ms do wartości bliskich 0–5 ms.
3. Sprawdzić, czy nie pojawiają się nowe boundary regressions: brak terminalizacji, podwójna terminalizacja, spóźnione deadline verdicts.
4. Przenieść `decision_ts_ms` do chwili faktycznego werdyktu BUY w Gatekeeper BUY path.
5. Ponownie zmierzyć `decision_to_sim_start_ms` po korekcie timestampingu.
6. Jeśli wynik nadal przekracza docelowe `<5 ms`, zidentyfikować i zredukować/precache RPC-dependent część `prepare_buy_request(...)`.
