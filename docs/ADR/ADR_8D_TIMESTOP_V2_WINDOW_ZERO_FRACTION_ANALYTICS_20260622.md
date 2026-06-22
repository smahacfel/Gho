# ADR-8D: TimeStop V2 Window Zero-Fraction Analytics

Status: IMPLEMENTED
Typ: ADR-8D / analytics script / TimeStop V2 telemetry
Data: 2026-06-22
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: skrypt do oceny zero-fraction metryk TimeStop V2 window i rekomendacji minimalnego `window_ms`
Poziom ryzyka: LOW

Dotkniete moduly/pliki:
- `scripts/analyze_time_stop_v2_window_zero_fraction.py`
- `docs/ADR/ADR_8D_TIMESTOP_V2_WINDOW_ZERO_FRACTION_ANALYTICS_20260622.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Dostarczyc read-only narzedzie, ktore analizuje `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl`, wyciaga rekordy `time_stop_v2_window`, liczy zero-fraction dla metryk okiennych i pomaga dobrac najmniejsze sensowne `window_ms`.

Zalozenia:
- Skrypt nie zmienia runtime, configow ani artefaktow wejscia.
- Skrypt nie traktuje missing jako zero; raportuje missing osobno.
- Skrypt domyslnie pomija i raportuje uszkodzone linie JSONL, bo lifecycle jest append-only i moze zawierac pojedyncze sklejone/czesciowe rekordy podczas live runa.
- Z wiekszych okien korzysta przez syntetyczne grupowanie kolejnych okien per pozycja.
- Nie da sie wiarygodnie testowac okna mniejszego niz cadence, w ktorym runtime juz zebral dane.

## 2. Opis problemu - 3W2H

What:
Potrzebna jest metryka mowiaca, czy `window_ms` TimeStop V2 jest zbyt waskie, czyli czy zbyt duzo okien ma brak zmiany w kluczowych metrykach.

Where:
- lifecycle JSONL:
  - `shadow_lifecycle.jsonl`
  - `probe_shadow_lifecycle.jsonl`
- rekordy `record_type = "time_stop_v2_window"`

Why:
Jesli wysoki odsetek okien ma delte zero albo missing, sygnal TimeStop V2 moze byc zdominowany przez martwe/za waskie okna zamiast realna dynamike poola.

How:
- Skrypt czyta JSONL.
- Filtruje `time_stop_v2_window`.
- Dla kazdej metryki liczy:
  - `zero_fraction_all`
- `zero_fraction_present`
- `missing_fraction_all`
- `zero_or_missing_fraction_all`
- `invalid_json_lines`
- Testuje kandydackie okna jako wielokrotnosci bazowego cadence: 1x, 2x, 3x itd.
- Rekomenduje pierwsze okno, ktore spelnia progi dla zero/missing.

How many:
Zmiana dodaje jeden skrypt i jeden ADR. Brak zmian w Rust runtime, DecisionLogger, Gatekeeper policy, `MaterializedFeatureSet`, SSOT lub shadow/live behavior.

## 3. Przyczyna zrodlowa

Root cause:
Same rekordy TimeStop V2 mowia, co wydarzylo sie w checkpointach, ale nie daja operatorowi natychmiastowej odpowiedzi, czy aktualne `window_ms` ma wystarczajaca rozdzielczosc sygnalu. Potrzebna jest jawna agregacja zero-fraction.

## 4. Strategia naprawy

Przyjeta strategia:
- Zbudowac osobny read-only script w `scripts/`.
- Bazowac na istniejacych polach `time_stop_v2_*_window`.
- Zachowac rozdzial zero vs missing, aby nie wprowadzac hidden missing-as-zero.
- Zachowac tryb domyslny odporny na pojedyncze uszkodzone live JSONL rows oraz opcjonalny `--strict-json` dla fail-fast audytu.
- Wygenerowac output tekstowy/Markdown oraz JSON.

## 5. Przeprowadzone akcje naprawcze

Dodano `scripts/analyze_time_stop_v2_window_zero_fraction.py`.

Obslugiwane metryki:
- `time_stop_v2_tx_delta_window`
- `time_stop_v2_volume_delta_sol_window`
- `time_stop_v2_price_delta_pct_window`
- `time_stop_v2_mcap_delta_pct_window`
- `time_stop_v2_bonding_delta_pct_window`
- `time_stop_v2_avg_volume_per_tx_sol_window`

Jawnie raportowane jako oczekiwane, ale nieemitowane przez TimeStop V2 windows:
- `total_buyers_delta`
- `unique_buyers_delta`
- `total_unique_buyers_delta`

## 6. Walidacja

Wymagana walidacja:
- `python3 -m py_compile scripts/analyze_time_stop_v2_window_zero_fraction.py`
- uruchomienie skryptu na aktywnych artefaktach R46
- `git diff --check`

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: missing potraktowany jako zero.
- Zabezpieczenie: skrypt raportuje `missing_fraction_all` osobno i uzywa `zero_or_missing_fraction_all` jako surowszej miary szumu.

Ryzyko 2: wniosek o oknie mniejszym niz cadence zbierania danych.
- Zabezpieczenie: raport jasno mowi, ze nie da sie testowac okna mniejszego niz bazowy cadence.

Ryzyko 3: procentowe delty sumowane nieliniowo.
- Zabezpieczenie: syntetyczne wieksze okna traktuja grupe jako zero tylko wtedy, gdy wszystkie skladowe delty sa zero; nie probuja rekalkulowac dokladnej procentowej delty ceny.

## 8. Decyzja

Skrypt zostaje dodany jako narzedzie read-only do oceny, czy `window_ms` TimeStop V2 jest zbyt waskie. Wynik ma sluzyc do doboru runtime cadence dla kolejnych runow, nie do automatycznej zmiany configu.
