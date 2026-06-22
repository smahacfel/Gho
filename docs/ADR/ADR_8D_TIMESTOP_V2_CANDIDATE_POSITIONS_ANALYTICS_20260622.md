# ADR-8D: TimeStop V2 Candidate Positions Analytics

Status: IMPLEMENTED / SMOKE_VALIDATED
Typ: ADR-8D / operator tooling / shadow lifecycle analytics
Data: 2026-06-22
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: rozszerzenie offline renderera shadow/probe lifecycle o tabele `candidate_positions`
Poziom ryzyka: LOW

Dotkniete moduly/pliki:
- `scripts/render_shadow_lifecycle_human.py`
- `docs/ADR/ADR_8D_TIMESTOP_V2_CANDIDATE_POSITIONS_ANALYTICS_20260622.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Rozszerzyc istniejacy renderer lifecycle tak, aby z plaskich agregatow TimeStop V2 przejsc do audytowalnej tabeli per pozycja-kandydat.

Wymaganie:
- wskazac kazda pozycje, ktora dostala `time_stop_v2_candidate=true`,
- pokazac terminalny wynik tej pozycji,
- pokazac, czy po kandydacie wystapilo `alive`,
- pokazac rebound i najlepsza pozniejsza cene wzgledem entry,
- pokazac timing i jakosc danych przy oknie kandydata.

## 2. Opis problemu - 3W2H

What:
Agregaty typu `window_status_counts` i `terminal_candidate_close_reason_counts` nie wystarczaja do oceny jakosci TimeStop V2, bo nie pokazuja, ktore kandydaty byly potencjalnymi false positive.

Where:
- `scripts/render_shadow_lifecycle_human.py`
- `shadow_lifecycle.jsonl`
- `probe_shadow_lifecycle.jsonl`

Why:
Na R45 kandydaci V2 konczyli sie zarowno `Target`, jak i `TimeStop`. Bez tabeli per kandydat nie da sie odroznic pozycji, ktore V2 moglby slusznie zamknac, od pozycji, ktore po kandydacie odbily i trafily target.

How:
Dodano `candidate_positions`, wyliczane z pierwszego okna, w ktorym dana pozycja ma `time_stop_v2_candidate=true`.

How many:
Zmiana dotyka tylko offline skryptu. Nie zmienia runtime, configu, JSONL schema, Gatekeeper policy ani shadow close behavior.

## 3. Przyczyna zrodlowa

Root cause:
Pierwotny renderer pokazywal relacje TimeStop V2 do terminalnego wyniku per pozycja, ale nie mial zwartej tabeli kandydatow, ktora nadawalaby sie do porownania false positive / true positive.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac dotychczasowy format pozycji i JSON.
- Addytywnie dodac top-level `candidate_positions`.
- W Markdown dodac tabele `TimeStop V2 Candidate Positions`.
- Dodac `--candidates-only`, aby przy duzych `probe_shadow_lifecycle.jsonl` wypisywac tylko tabele kandydatow.

## 5. Przeprowadzone akcje naprawcze

Dodane metryki per kandydat:
- `alive_after_candidate`
- `alive_windows_after_candidate`
- `rebound_after_candidate_pct`
- `max_price_after_candidate_pct`
- `candidate_price_delta_pct_from_entry`
- `terminal_reason`
- `terminal_pnl_pct`
- `candidate_to_terminal_ms`
- `sample_age_at_candidate_ms`
- `schedule_lag_at_candidate_ms`
- `decision_to_dispatch_record_ms`
- `stale_streak_before_candidate`
- `weak_heartbeat_streak_before_candidate`
- `statuses_before_candidate`
- `statuses_after_candidate`

Definicje:
- `stale_streak_before_candidate` i `weak_heartbeat_streak_before_candidate` licza kolejne okna konczace sie na pierwszym oknie kandydata.
- `rebound_after_candidate_pct` to roznica miedzy najlepszym pozniejszym `time_stop_v2_price_delta_pct_from_entry` a wartoscia z okna kandydata.
- `max_price_after_candidate_pct` uwzglednia tylko okna po kandydacie, nie samo okno kandydata.

## 6. Walidacja

Wymagana walidacja:
- `python3 -m py_compile scripts/render_shadow_lifecycle_human.py`
- smoke Markdown `--candidates-only` na R45 `shadow_lifecycle.jsonl`
- smoke JSON `--json --candidates-only` na R45 `shadow_lifecycle.jsonl`
- smoke JSON dla combined shadow + probe R45
- `git diff --check` dla dotknietych plikow

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: interpretacja `rebound_after_candidate_pct` jako PnL.
- Zabezpieczenie: pole jest jawnie nazwane jako rebound ceny wzgledem `price_delta_pct_from_entry`; terminalny PnL ma osobne pole `terminal_pnl_pct`.

Ryzyko 2: duze pliki probe generuja zbyt duzy raport.
- Zabezpieczenie: dodano `--candidates-only` oraz istniejacy `--limit`.

Ryzyko 3: zmiana runtime.
- Zabezpieczenie: brak zmian runtime. Skrypt dziala offline na gotowych artefaktach JSONL.

## 8. Decyzja

Rozszerzyc offline renderer o tabele `candidate_positions`, aby dalsza decyzja o TimeStop V2.1 opierala sie na pozycyjnej analizie false positive / true positive, a nie na plaskich agregatach statusow.
