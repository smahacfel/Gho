# ADR-8D: Shadow Lifecycle Human Renderer

Status: IMPLEMENTED / SMOKE_PENDING_AT_CREATION
Typ: ADR-8D / operator tooling / lifecycle audit readability
Data: 2026-06-21
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: prosty renderer JSONL shadow/probe lifecycle do czytelnych struktur per token/pozycja
Poziom ryzyka: LOW

Dotkniete moduly/pliki:
- `scripts/render_shadow_lifecycle_human.py`
- `docs/ADR/ADR_8D_SHADOW_LIFECYCLE_HUMAN_RENDERER_20260621.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Ulatwic operatorowi czytanie `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl`, ktore sa maszynowymi logami append-only, a nie ergonomicznym raportem per token.

Wymaganie:
- grupowanie rekordow po pozycji/tokenie,
- pokazanie entry timeline,
- pokazanie kolejnych `time_stop_v2_window`,
- pokazanie finalnego close reason/PnL,
- podsumowanie relacji TimeStop V2 candidate vs terminal close.

## 2. Opis problemu - 3W2H

What:
Raw JSONL ma wiele linii na jeden token: dispatch, kolejne okna TimeStop V2, exit fill i terminal position closed. To jest poprawne dla audytu maszynowego, ale slabo czytelne dla czlowieka.

Where:
- `logs/shadow_run/*/shadow_lifecycle.jsonl`
- `logs/shadow_run/*/probe_shadow_lifecycle.jsonl`

Why:
Analiza TimeStop V2 wymaga szybkiego rozpoznania, kiedy V2 wystawil kandydata, czy potem wystapilo `alive`, oraz jak zakonczyla sie pozycja.

How:
Dodano skrypt `scripts/render_shadow_lifecycle_human.py`, ktory:
- czyta jeden lub wiele plikow JSONL,
- grupuje rekordy po `position_id`,
- dolacza `shadow_dispatch` po `candidate_id`,
- sortuje okna `time_stop_v2_window`,
- wylicza summary `terminal_minus_v2_candidate_ms`,
- wypisuje Markdown albo JSON.

How many:
Zmiana dodaje tylko narzedzie offline. Nie zmienia runtime, configu, Gatekeeper policy ani lifecycle schema.

## 3. Przyczyna zrodlowa

Root cause:
Lifecycle JSONL jest zoptymalizowany pod trwały dowod i replay, nie pod bezposrednie czytanie przez operatora.

## 4. Strategia naprawy

Przyjeta strategia:
- Nie zmieniac formatu runtime logow.
- Dodac offline renderer, aby zachowac kompatybilnosc i nie dotykac hot path.
- Domyslnie emitowac Markdown dla czlowieka.
- Udostepnic `--json` dla dalszej analizy.

## 5. Przeprowadzone akcje naprawcze

Dodano:
- `scripts/render_shadow_lifecycle_human.py`

Funkcje:
- `--mint` filter,
- `--position-id` filter,
- `--limit`,
- `--output`,
- `--json`.

## 6. Walidacja

Wymagana walidacja:
- `python3 -m py_compile scripts/render_shadow_lifecycle_human.py`
- smoke na R45 `shadow_lifecycle.jsonl`
- `git diff --check` dla skryptu i ADR

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: bledna interpretacja `time_stop_v2_price_delta_pct_from_entry` jako pelnego PnL.
- Zabezpieczenie: renderer nazywa pole jawnie jako price delta, a terminalny PnL bierze tylko z `position_closed`/`exit_filled`.

Ryzyko 2: podwojne liczenie `exit_filled` i `position_closed`.
- Zabezpieczenie: summary preferuje `position_closed` jako terminal canonical row.

Ryzyko 3: zmiana runtime.
- Zabezpieczenie: brak zmian runtime; skrypt jest offline.

## 8. Decyzja

Dodac offline renderer do operator-friendly przegladu TimeStop V2 i shadow lifecycle. Runtime JSONL pozostaje maszynowym SSOT, a skrypt jest warstwa czytelnosci.
