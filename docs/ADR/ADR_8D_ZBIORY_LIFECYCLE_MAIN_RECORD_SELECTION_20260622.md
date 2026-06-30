# ADR-8D: zbiory.py lifecycle main-record selection for mint dedupe

Status: IMPLEMENTED / COMPILATION_CHECKED
Typ: ADR-8D / operator tooling / shadow/probe lifecycle analytics
Data: 2026-06-22
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: poprawka skrypta `scripts/zbiory.py` odpowiadającego za łączenie decyzji Gatekeepera z lifecycle
Poziom ryzyka: LOW

Dotkniete moduly/pliki:
- `scripts/zbiory.py`
- `docs/ADR/ADR_8D_ZBIORY_LIFECYCLE_MAIN_RECORD_SELECTION_20260622.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zmienić dobór rekordu lifecycle dla pojedynczego `mint_id` tak, aby zawsze był to jeden, jednoznaczny rekord końcowy pozycji, a nie przypadkowy pierwszy rekord napotkany podczas skanowania.

Wymaganie:
- dla jednego `mint_id` nie powinna powstawać duplikacja wyników wynikająca z wielu rekordów końcowych,
- preferencja rekordu powinna być deterministyczna,
- zachować kompatybilność dotychczasowego formatu wyjściowego (tylko logika wyboru zmieniona).

How:
Modyfikacja ogranicza się do logiki deduplikacji w `load_unique_lifecycle_records`.

## 2. Opis problemu - 3W2H

What:
W danych `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl` dla jednego tokena (`mint_id`) może występować kilka rekordów z `final_pnl_pct`, typowo `exit_filled` oraz `position_closed`. Dotychczasowy skrypt zapamiętywał pierwszy rekord i pomijał kolejne, co powodowało, że „główny rekord” mógł być niestabilny względem kolejności plików i zmian rodzaju rekordu.

Where:
`scripts/zbiory.py` w funkcji `load_unique_lifecycle_records`.

Why:
Niestabilny wybór rekordu powodował mylące statystyki i utrudniał porównanie zestawów A/B/N, mimo że końcowy `final_pnl_pct` był ten sam, ale pochodził z rekordu o niższym priorytecie semantycznym.

## 3. Przyczyna zrodlowa

Root cause:
Algorytm deduplikacji opierał się wyłącznie o pierwsze wystąpienie `mint_id` i nie uwzględniał znaczenia typu rekordu lifecycle.

## 4. Strategia naprawy

Przyjęta strategia:
- Wprowadzenie deterministycznej funkcji porównującej rekordy lifecycle:
  - priorytet typu rekordu: `position_closed` (najwyższy), potem `exit_filled`,
  - przy remisie nowszy `timestamp_ms`,
  - przy dalszym remisie wcześniejszy plik wejściowy (`shadow_lifecycle` przed `probe_shadow_lifecycle`).
- Podmienianie dotychczasowego rekordu tylko gdy nowy ma lepszy klucz.
- Aktualizacja statystyk o liczbę podmienionych rekordów.

## 5. Przeprowadzone akcje naprawcze

Zmiany w kodzie:
- `scripts/zbiory.py`
  - dodano ranking rekordów lifecycle (`position_closed` > `exit_filled`, dalej `timestamp_ms`, potem kolejność pliku),
  - zmieniono `load_unique_lifecycle_records`, aby dla każdego `mint_id` trzymać rekord o najlepszym rankingu zamiast pierwszego napotkanego,
  - dodano licznik `lifecycle_replaced_records` do podsumowania.

## 6. Walidacja

Wymagana walidacja:
- `python3 -m py_compile scripts/zbiory.py`
- szybki smoke-check na runie `shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1` poprzez import funkcji `load_unique_lifecycle_records`:
  - wcześniej liczba zachowanych rekordów terminalnych była 711 przy 711 duplikatach i losowym wyborze,
  - po zmianie nadal 711 zachowanych, ale 711 rekordów podmieniono na wersję o wyższym priorytecie.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: zmiana semantyki przy bardzo nietypowych lifecycle,
- zabezpieczenie: ranking ma jednoznaczną, jawnie opisaną kolejność i działa w pełni deterministic.

Ryzyko 2: potencjalny wpływ na zgodność starych skryptów, które implicitnie zakładały pierwszy rekord,
- zabezpieczenie: zmiana ogranicza się do selektora rekordu `mint_id`; struktura wyjścia i ścieżki plików pozostają bez zmian.

## 8. Decyzja

Przyjąć zmianę w `scripts/zbiory.py` i używać jej jako domyślnego sposobu łączenia lifecycle z decyzjami w analizach offline.
