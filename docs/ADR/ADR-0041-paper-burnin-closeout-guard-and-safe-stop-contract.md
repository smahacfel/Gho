# ADR-0041: Paper burn-in closeout guard and safe-stop contract

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

Po naprawieniu shutdown-drain w `PostBuyRuntime` oraz ekonomiki raportu okazało się, że ostatnim realnym blokerem pełnego `GO` dla całej sesji paper burn-in nie jest już runtime bug, tylko przedwczesne zamknięcie runu przez operatora.

Formalny raport ma poprawny kontrakt: jeśli w chwili zamknięcia istnieje `paper_inflight`, `paper_lifecycle_complete` musi pozostać czerwone. Problemem było to, że operacyjny runbook mówił jedynie „wyślij `SIGINT` i czekaj na shutdown”, zostawiając decyzję *kiedy* zatrzymać proces zbyt dużej interpretacji operatora.

## Decision

Dodano dedykowany guard closeoutu `scripts/paper_burnin_closeout_guard.py` oraz zaostrzono runbook stopu do kontraktu binarnego:

- `SAFE_TO_STOP` — można wykonać graceful shutdown,
- `WAIT_PENDING_PAPER_HANDOFF` — nie wolno jeszcze zatrzymywać procesu, bo istnieje `shadow_success` bez paper eventu,
- `WAIT_PAPER_CLOSEOUT` — nie wolno jeszcze zatrzymywać procesu, bo istnieje admitted/opened paper lifecycle bez `PositionClosed`.

Guard wylicza też `Earliest safe stop ms` na podstawie aktualnego runtime contract:

- `tick_interval_ms = 500`,
- `max_ticks_before_exit = 240`,
- `aem_t_s = 120`,
- `shutdown_drain_ms = 10_000`.

W praktyce operator nie zatrzymuje procesu, dopóki guard nie przejdzie do `SAFE_TO_STOP`.

## Architectural Impact

- Raport formalny `scripts/shadow_run_report.py` pozostaje nieosłabiony; nie ukrywamy `paper_inflight`.
- Operacyjna odpowiedzialność za poprawny moment stopu została przeniesiona do jawnego guardu zamiast nieformalnej heurystyki.
- Runbook paper burn-in uzyskał twardy, powtarzalny kontrakt zamknięcia sesji.

## Risk Assessment

**Rate:** Low

- Ryzyko regresji raportu jest niskie, bo guard nie zmienia kryteriów `GO/NO-GO`; zmienia tylko przygotowanie operatora do shutdownu.
- Główne ryzyko to drift między guardem a runtime contract, jeśli przyszłe wartości `tick_interval_ms`, `max_ticks_before_exit`, `aem_t_s` albo `shutdown_drain_ms` zmienią się bez aktualizacji guardu lub bez jawnego override CLI.

## Consequences

- Łatwiejsze: deterministyczne zamykanie runu bez „czy już chyba można?”.
- Łatwiejsze: rozróżnienie błędu runtime od błędu operacyjnego closeoutu.
- Trudniejsze: operator musi wykonać dodatkowy krok guardu przed shutdownem, ale to celowy koszt bezpieczeństwa.

## Alternatives Considered

1. **Pozostawić sam runbook tekstowy bez helpera**
   - Odrzucone, bo nadal zostawiałoby zbyt dużo interpretacji operatorowi.

2. **Osłabić raport i ignorować inflight przy końcu sesji**
   - Odrzucone, bo łamałoby kontrakt burn-in zamiast naprawiać closeout.

3. **Dodać automatyczny self-stop do launchera**
   - Odrzucone na tym etapie jako szersza zmiana zachowania runtime, wykraczająca poza bieżący zakres domknięcia PR-6.

## Validation Steps

1. Uruchomić `python3 /root/Gho/scripts/paper_burnin_closeout_guard.py --config /root/Gho/configs/rollout/paper-burnin.toml` na aktywnej lub zamkniętej sesji testowej.
2. Potwierdzić, że guard zwraca:
   - `WAIT_PENDING_PAPER_HANDOFF`, gdy `shadow_success` nie ma jeszcze paper eventu,
   - `WAIT_PAPER_CLOSEOUT`, gdy istnieje `PositionOpened` bez `PositionClosed`,
   - `SAFE_TO_STOP`, gdy handoff i lifecycle są domknięte.
3. Wykonać formalny raport po sesji zamkniętej dopiero po `SAFE_TO_STOP` i potwierdzić, że `paper_lifecycle_complete` nie pada z powodów operacyjnych.