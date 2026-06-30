# ADR-8D: porownaj_zbiory.py anti-leakage hardening

Status: IMPLEMENTED / NOT VALIDATED
Typ: ADR-8D / operator tooling / offline selector analytics
Data: 2026-06-23
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: poprawka skryptu `scripts/porownaj_zbiory.py` porownujacego zbiory A/B
Poziom ryzyka: LOW-MEDIUM

Dotkniete moduly/pliki:
- `scripts/porownaj_zbiory.py`
- `docs/ADR/ADR_8D_POROWNAJ_ZBIORY_ANTI_LEAKAGE_20260623.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Usunac wycieki danych ze skryptu A/B `porownaj_zbiory.py`, ktory trenuje model XGBoost i generuje raport SHAP na rekordach laczacych decyzje Gatekeepera z lifecycle/outcome.

Zakres:
- lokalna zmiana narzedzia offline,
- brak zmian runtime Ghost,
- brak zmian Gatekeeper, DecisionLogger, execution path, configow lub schematow produkcyjnych.

Wstepna obserwacja rekordu:
Przykladowy rekord `scripts/zbior_A.jsonl` zawiera jednoczesnie:
- top-level metryki obserwacyjne,
- identyfikatory, timestampy, run/config/hash fields,
- payloady decyzji i materialized snapshots,
- shadow/v25/v3 statusy i verdicts,
- lifecycle/entry/exit/truth fields,
- outcome: `final_pnl`, `final_pnl_pct`, entry/exit price/value.

## 2. Opis problemu - 3W2H

What:
Poprzedni skrypt opieral sie glownie na denylist patternow nazw kolumn. To nie wystarcza dla rekordow, ktore zawieraja zagniezdzone payloady, listy, statusy shadow, lifecycle outcome oraz pola ewaluacyjne.

Where:
`scripts/porownaj_zbiory.py`, szczegolnie:
- `LEAKAGE_PATTERNS` / `LEAKAGE_FIELDS`,
- `build_features`,
- cross-validation threshold selection,
- temporal validation threshold selection.

Why:
Model mogl uczyc sie z pol niedostepnych w czasie decyzji albo z pol pochodzacych z outcome/lifecycle. Dodatkowo prog F1 byl dobierany na foldzie/test window, co jest wyciekiem ewaluacyjnym.

## 3. Przyczyna zrodlowa

Root cause:
Kontrakt wejscia do modelu nie byl wystarczajaco fail-closed. Skrypt zakladal, ze pojedyncza lista nazw leakow odfiltruje wszystkie niebezpieczne pola, mimo ze rekordy sa mieszanka decision evidence, payloadow audytowych i lifecycle truth.

Dodatkowy root cause:
Metryki klasyfikacyjne uzywaly progu zoptymalizowanego na danych testowych, przez co raport klasyfikacyjny nie byl czystym OOS pomiarem.

## 4. Strategia naprawy

Przyjeta strategia:
- Zmienic filtracje na konserwatywny kontrakt: do modelu trafiaja tylko top-level numeryczne skalary.
- Odrzucac dict/list/tuple/set, bo payloady i szeregi czasowe nie sa bezpiecznymi cechami tabularnymi.
- Odrzucac bool flags, poniewaz w tych rekordach czesto koduja pass/status/availability/decision state.
- Rozszerzyc blokade nazw o outcome, lifecycle, entry/exit, truth, shadow, legacy, decision, verdict, reason, config, hash, id, timestamp, slot, payload, snapshot i status families.
- Dodac raport `4_odrzucone_pola_leakage.csv`, zeby operator widzial, ktore pola zostaly zablokowane i dlaczego.
- Liczyc dobieranie cech per fold/temporal split tylko na treningowej czesci.
- Liczyc prog F1 tylko na treningowej czesci, a nie na test fold / temporal test window.

## 5. Przeprowadzone akcje naprawcze

Zmiany w kodzie:
- `scripts/porownaj_zbiory.py`
  - dodano `leakage_reason`, ktore zwraca jawny powod odrzucenia pola,
  - dodano blokade zagniezdzonych wartosci i bool flags,
  - rozszerzono `LEAKAGE_PATTERNS`, `LEAKAGE_FIELDS`, `BLOCKED_PREFIXES`, `BLOCKED_SUFFIXES`,
  - dodano `SAFE_NAME_ALLOWLIST` dla jawnie obserwowanych `max_*_observed`,
  - dodano `write_leakage_report` zapisujacy `4_odrzucone_pola_leakage.csv`,
  - zmieniono CV tak, aby fold dobieral cechy, mediany i prog F1 tylko z treningu,
  - zmieniono temporal validation tak, aby cechy, mediany i prog F1 byly liczone tylko z pierwszych 80%,
  - zabezpieczono przypadek pustego zbioru B.

## 6. Walidacja

Walidacja nie zostala uruchomiona w tej turze zgodnie z aktywnym trybem pracy: nie wykonywac testow ani dodatkowej weryfikacji bez jawnej prosby uzytkownika.

Rekomendowane waskie sprawdzenia:
- `python3 -m py_compile scripts/porownaj_zbiory.py`
- `cd scripts && python3 porownaj_zbiory.py`
- po runie sprawdzic `4_odrzucone_pola_leakage.csv`, szczegolnie czy nie zostaly wybrane pola `entry_*`, `exit_*`, `final_*`, `truth_*`, `decision_*`, `shadow_*`, `v25_*`, `v3_*`, `*_payload`, `*_snapshot`, `*_ts_ms`, `*_slot`, `*_id`.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: filtr jest konserwatywny i moze odrzucic czesc cech, ktore sa faktycznie decision-time-safe.
- zabezpieczenie: odrzucone pola sa jawnie raportowane; przywracanie powinno odbywac sie przez explicit allowlist po sprawdzeniu dostepnosci w czasie decyzji.

Ryzyko 2: liczba cech moze spasc mocno wzgledem poprzedniego skryptu, a metryki modelu moga byc nizsze.
- zabezpieczenie: to oczekiwane przy usuwaniu leakage; nizszy wynik jest bardziej wiarygodny niz wysoki wynik oparty o outcome/status fields.

Ryzyko 3: finalny model nadal jest trenowany na pelnym zbiorze dla artefaktu `ghost_selector_xgb.json` i SHAP.
- zabezpieczenie: walidacja OOS pozostaje czysta; artefakt full-fit nalezy traktowac jako model eksploracyjny, nie jako dowod skutecznosci.

## 8. Decyzja

Przyjac fail-closed filtracje anty-leakage jako domyslna dla `porownaj_zbiory.py`. Kazde przyszle przywrocenie pola do modelu powinno byc jawne, udokumentowane i oparte o dowod, ze pole jest top-level, numeryczne, deterministyczne, replay-safe i dostepne w czasie decyzji.
