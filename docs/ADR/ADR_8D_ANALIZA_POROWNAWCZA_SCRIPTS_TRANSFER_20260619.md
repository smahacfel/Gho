# ADR-8D: Transfer new-log support into scripts/analiza_porownawcza.py

Status: IMPLEMENTED / TARGETED_SMOKE_VERIFIED
Typ: ADR-8D / offline analysis script restoration and compatibility transfer
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: przeniesienie rozszerzen new-log/vector/delta/evidence z root-level `analiza_porownawcza.py` do przywroconego `scripts/analiza_porownawcza.py`, bez usuwania istniejacej sekcji v5.1 Segment Lab
Poziom ryzyka: LOW-MEDIUM

Dotkniete moduly/pliki:
- `scripts/analiza_porownawcza.py`
- `docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_SCRIPTS_TRANSFER_20260619.md`

Powiazany ADR:
- `docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_NEW_LOG_VECTORS_DELTAS_20260619.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w istniejacych ADR-ach PR1-PR6 i runtime repair ADR-ach.

## 1. Przygotowanie i dzialania wstepne

Problem:
Po pierwotnym rozszerzeniu root-level `analiza_porownawcza.py` uzytkownik przywrocil najnowsza wersje skryptu w lokalizacji docelowej `scripts/analiza_porownawcza.py`. Ta wersja zawierala dodatkowa sekcje v5.1 Segment Lab i nie mogla zostac bezpiecznie nadpisana rootowym plikiem.

Dzialanie wstepne:
- Porownano root-level `analiza_porownawcza.py` z `scripts/analiza_porownawcza.py`.
- Potwierdzono, ze `scripts/analiza_porownawcza.py` ma wersje v5.1 Segment Lab, ktorej root-level plik nie mial.
- Przyjeto merge sekcji zamiast pelnego overwrite.

## 2. Routing i skills

Uzyte skills:
- `ghost-execution`: ochrona DecisionLogger/replay/SSOT boundary i brak zmian runtime.
- `large-data-analytics`: poprawne czytanie event-stream/window features bez silent imputation.

Nie ladowano dokumentow specjalistycznych:
- `gatekeeper-policy-auditor`: brak zmian verdictow, hard gates lub reason policy.
- `oracle-session-runtime-engineer`: brak zmian sesji, deadline lub routing eventow.
- `solana-execution-path-engineer`: brak zmian TX buildera, sendera lub execution path.

## 3. Opis problemu - 3W2H

What:
Docelowy skrypt w `scripts/analiza_porownawcza.py` nie mial rozszerzen dla nowych logow:
- embedded-aware extractors,
- nullable `vectors_prices`,
- `decision_time_series`,
- AB tx fields,
- temporal deltas/rates,
- Jito/flipper/CPV deltas,
- sekcja 0B evidence coverage.

Where:
- `scripts/analiza_porownawcza.py`, wersja v5.1 Segment Lab.

Why it matters:
Uzytkownik pracuje na skrypcie z katalogu `/scripts`. Gdyby rozszerzenia zostaly tylko w root-level kopii, faktyczny offline analyzer dalej mielilby nowe logi niekompletnie.

How observed:
`scripts/analiza_porownawcza.py` mial stary `get_val`, `get_vector` i bezposrednie `r.get(...)` w wielu sekcjach. Jednoczesnie mial dodatkowa, wartosciowa sekcje v5.1 Segment Lab, wiec pelne zastapienie pliku byloby regresja funkcjonalna.

How many / scale:
Zmiana dotyczy offline analizy JSONL. Nie dotyka aktywnego runtime, Gatekeeper policy, DecisionLogger runtime mappera ani shadow/live behavior.

## 4. Przyczyna zrodlowa

Root cause:
Rozszerzenia new-log zostaly pierwotnie dodane do root-level `analiza_porownawcza.py`, poniewaz `scripts/analiza_porownawcza.py` nie byl wtedy obecny w checkoutcie. Po przywroceniu skryptu w `/scripts` trzeba bylo przeniesc rozszerzenia do faktycznej lokalizacji roboczej.

Dodatkowo wersja `/scripts` byla nowsza logicznie, bo zawierala Segment Lab v5.1. Dlatego mechaniczne skopiowanie root-level pliku do `/scripts` usuneloby nowszy kod.

## 5. Strategia naprawy

Przyjeta strategia:
- Zachowac v5.1 Segment Lab.
- Przeniesc warstwe new-log support z root-level wersji:
  - embedded-aware extractors,
  - raw nullable vector handling,
  - `TEMPORAL_DELTA_FIELDS` i `TEMPORAL_RATE_FIELDS`,
  - source/evidence/decision series fields,
  - sekcje DTW/MI/Hill/Sybil z nowymi polami,
  - sekcje 0B,
  - main-call do sekcji 0B.
- Dodatkowo podpiac Segment Lab pod `get_val`, aby mogl widziec embedded/top-level fields przez ten sam extractor.

Granice:
- Brak runtime changes.
- Brak silent `null -> 0`.
- Brak usuwania Segment Lab.
- Brak zmian reguly decyzyjnej lub rekomendacji live.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: extractory i embedded SSOT fallback
- `get_val`, `get_bool`, `get_str`, `get_vector_raw`, `get_vector` czytaja top-level, aliasy i embedded `v3_materialized_feature_snapshot`.
- `vectors_prices` zachowuje `None` w raw vector.

Zmiana 2: nowe metryki
- Dodano temporal deltas/rates oraz fields dla price source, decision time series i evidence policy context.
- Dodano alias `delta_burstratio_*` <-> `delta_burst_ratio_*`.

Zmiana 3: sekcja 0B
- Dodano i wywolano `section_new_log_integrity(rec_a, rec_b)` po filtrze A/B i przed profilowaniem.
- Sekcja raportuje coverage `decision_time_series`, AB tx fields, deltas/rates, alpha/sybil metrics i evidence status counters.

Zmiana 4: analityka wektorowa
- DTW uwzglednia `vectors_prices`.
- MI dodaje `vf_price_return`, `vf_price_range_pct`, `vf_price_coverage_ratio`.
- Hill uwzglednia `vectors_prices`.

Zmiana 5: Sybil i Segment Lab
- Sybil coverage uzywa `get_val`, nie tylko bezposredniego top-level `r.get`.
- Segment Lab `_segment_value` najpierw uzywa `get_val`, zeby embedded deltas/CPV mogly wejsc do discovery scan bez imputation.

## 7. Walidacja

Wykonane komendy:
- `python3 -m py_compile scripts/analiza_porownawcza.py`
- `git diff --check -- scripts/analiza_porownawcza.py`
- `python3 scripts/analiza_porownawcza.py /tmp/gho_analiza_sample/a.jsonl /tmp/gho_analiza_sample/b.jsonl > /tmp/gho_analiza_sample/scripts_run_after_call_fix.txt`

Wynik:
- Skrypt kompiluje sie poprawnie.
- `git diff --check` nie wykazal problemow whitespace.
- Smoke run zakonczyl sie exit code 0.
- Wygenerowano raport:
  - `/tmp/gho_analiza_sample/analiza_20260619_203919.html`

Dowody z outputu:
- `SEKCJA 0B` faktycznie sie wykonala.
- `decision_time_series present`: 35/35 dla A i 35/35 dla B.
- `negative interval records`: 0 dla A i 0 dla B.
- `delta_jito_tip_intensity_1s_to_2s`: 47/70 present.
- `delta_flipper_presence_ratio_1s_to_2s`: 47/70 present.
- `signer_cross_pool_velocity`: 36/70 present.
- `cpv_other_pool_activity`: 36/70 present.
- `vectors_prices` zachowuje nullable shape: B mial `null=1/35`.
- `SEKCJA 19: Segment Lab` dalej sie wykonala.

## 8. Ryzyka i zabezpieczenia

Ryzyko 1: Nadpisanie nowszej wersji v5.1 rootowa wersja.
Mitigacja:
- Nie wykonano pelnego overwrite.
- Zachowano Segment Lab i dopieto do niego embedded-aware extractor.

Ryzyko 2: Ukryta imputacja missing.
Mitigacja:
- `get_vector_raw` zachowuje `None`.
- `get_val` zwraca `None`, jezeli wartosc nie istnieje lub nie jest numeryczna.
- Sekcja 0B pokazuje missing/finites oddzielnie.

Ryzyko 3: Sekcja 0B obecna, ale niewywolana.
Mitigacja:
- Dodano jawne wywolanie w `main()`.
- Smoke run potwierdzil header `SEKCJA 0B` i konkretne liczniki coverage.

Ryzyko 4: Segment Lab zaczyna traktowac embedded missing jako zero.
Mitigacja:
- `_segment_value` uzywa `get_val`; brak wartosci pozostaje `None`.
- Segment Lab nadal jest discovery-only, no deploy without OOS validation.

## 9. Status koncowy

Status: implemented.

Faktyczny skrypt roboczy `scripts/analiza_porownawcza.py` ma teraz:
- support dla nowych logow z decision vectors i AB tx,
- pelna liste nowych delt/rates,
- embedded SSOT fallback bez silent imputation,
- sekcje 0B,
- zachowany Segment Lab v5.1.

Pozostale ograniczenia:
- Plik `scripts/analiza_porownawcza.py` jest obecnie untracked w git wedlug `git status --short`.
- Root-level `analiza_porownawcza.py` pozostaje zmodyfikowany z poprzedniego kroku; nie byl revertowany, bo nie bylo dyspozycji cofania zmian.
