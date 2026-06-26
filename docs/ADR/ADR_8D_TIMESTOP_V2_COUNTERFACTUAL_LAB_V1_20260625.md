# ADR-8D: TimeStop V2 Counterfactual Lab V1

Status: IMPLEMENTED / TARGETED_TESTS_PASSED / R48_R2_NEGATIVE_SMOKE_PASSED
Typ: ADR-8D / offline research tooling / TimeStop V2 counterfactual economics
Data: 2026-06-25
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: offline-only lab laczacy `shadow_exit_replay_v1.jsonl` z `time_stop_v2_window`
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Dotkniete moduly/pliki:
- `scripts/time_stop_v2_counterfactual_lab.py`
- `scripts/test_time_stop_v2_counterfactual_lab.py`
- `docs/ADR/ADR_8D_TIMESTOP_V2_COUNTERFACTUAL_LAB_V1_20260625.md`

Powiazane runy/logi/raporty:
- `logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_exit_replay_v1.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/probe_shadow_lifecycle.jsonl`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/time_stop_v2_counterfactual_exit_v1.jsonl`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/time_stop_v2_counterfactual_report_v1.json`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/TIME_STOP_V2_COUNTERFACTUAL_REPORT.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zbudowac offline-only research lab, ktory odpowiada na pytanie: czy hipotetyczne wyjscie przy pierwszym TimeStop V2 candidate poprawiloby wynik ekonomiczny, czy ucieloby pozniejsze TARGET-y.

Rzeczywisty przebieg:
- Zweryfikowano R48/R2: scope ma `shadow_exit_replay_v1.jsonl`, ale nie ma rekordow `record_type=time_stop_v2_window`.
- Dodano nowy skrypt offline do `scripts/`.
- Dodano testy fixture-based.
- Uruchomiono negatywny smoke na R48/R2 z oczekiwanym wynikiem `TIMESTOP_V2_NO_WINDOWS`.

Odchylenia od planu:
- W planie roboczym pojawila sie nazwa ADR z data `20260626`; uzyto faktycznej daty repo/srodowiska `20260625`.
- Nie uzyto `actual close_reason` jako biznesowego TARGET/STOP labela. Wynik ekonomiczny jest liczony z barrier simulation na podstawie `shadow_exit_replay_v1`.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powod uzycia: ochrona shadow/live boundary, Gatekeeper/BUY/REJECT i replay contracts.
Zakres uzycia: klasyfikacja zmiany jako offline-only i weryfikacja, ze runtime semantics nie sa dotykane.
Wynik: zmiana pozostala poza runtime.
Ograniczenia: skill nie rozstrzyga statystycznej wartosci sygnalu.

Nazwa: `statistical-research-engine`
Powod uzycia: no-lookahead, separacja evidence od outcome, konserwatywna rekomendacja.
Zakres uzycia: definicja counterfactual classification, matrix i recommendation logic.
Wynik: R48/R2 konczy sie konserwatywnie `TIMESTOP_V2_NO_WINDOWS`.
Ograniczenia: brak okien TimeStop V2 w R48/R2 nie pozwala ocenic ekonomicznej wartosci V2 na tym scope.

Nazwa: `decision-logging-replay-analyst`
Powod uzycia: zadanie dotyczy offline replay i laczenia durable lifecycle evidence.
Zakres uzycia: join quality, parser lifecycle, shadow/probe visibility, brak mutowania logow.
Wynik: skrypt raportuje exact/fallback/unmatched/ambiguous join quality.
Ograniczenia: jakosc analizy zalezy od coverage exit replay i TimeStop V2 windows w danym scope.

## 3. Opis problemu - 3W2H

What:
TimeStop V2 emituje observe-only vitality windows, ale brakowalo narzedzia, ktore liczy ekonomiczny counterfactual: co by bylo, gdyby pozycja wyszla w momencie pierwszego kandydata V2.

Where:
- `shadow_exit_replay_v1.jsonl`
- `shadow_lifecycle.jsonl`
- `probe_shadow_lifecycle.jsonl`
- raporty pod `reports/selector/<scope>/`

Why it matters:
Bez tego narzedzia candidate TimeStop V2 moze wygladac intuicyjnie dobrze, ale nadal moze ucinac pozniejsze TARGET-y. Potrzebna jest falsyfikacja ekonomiczna przed jakimkolwiek shadow-close lub live-exit design.

How observed:
R48/R2 posiada exit replay records, ale zero `time_stop_v2_window`, wiec obecny scope jest negatywnym testem coverage, nie testem wartosci V2.

How many / scale:
Smoke R48/R2:
- `positions_with_exit_replay = 2678`
- `positions_with_tsv2_windows = 0`
- `candidate_positions = 0`

Evidence:
Raport R48/R2 wygenerowal `recommendation = TIMESTOP_V2_NO_WINDOWS`.

## 4. Przyczyna zrodlowa

Root cause:
Dotychczasowe narzedzia analizowaly exit replay albo okna TimeStop V2 oddzielnie. Nie bylo offline joinu i matrixa ekonomicznego laczacego candidate time z replayed target/stop/max-hold outcome.

Mechanizm bledu:
Bez counterfactual lab mozna pomylic "weak/dead-flow candidate" z realnie korzystnym exit signalem.

Miejsce:
Warstwa offline research scripts.

Skutek:
Brak jednoznacznej odpowiedzi, czy TimeStop V2 saved STOP/TIMEOUT, czy cut TARGET.

Dowod:
Istnial `scripts/shadow_exit_replay_eval.py`, ale nie obslugiwal lifecycle join, TimeStop V2 windows, stale safety, resurrection checks ani per-position evidence.

Odrzucone hipotezy:
- Nie trzeba zmieniac runtime engine, bo wszystkie wymagane dane sa w logach.
- Nie trzeba dopisywac rekordow do `shadow_lifecycle.jsonl`, bo PR ma byc offline-only.

## 5. Strategia naprawy

Przyjeta strategia:
Dodac nowy samodzielny skrypt offline z fixture-based tests. Skrypt czyta wejscia, buduje jawny join, liczy baseline barrier result z exit replay, a nastepnie porownuje go z hipotetycznym wyjsciem na TimeStop V2 candidate.

Zakres ingerencji:
- Nowy skrypt `scripts/time_stop_v2_counterfactual_lab.py`.
- Nowe testy `scripts/test_time_stop_v2_counterfactual_lab.py`.
- Nowy ADR-8D.

Czego nie zmieniano:
- Gatekeeper policy.
- BUY/REJECT/TIMEOUT runtime.
- TimeStop V2 engine.
- shadow/probe close behavior.
- live execution.
- sender / tx builder / Jito path.
- runtime logs.

Ryzyka:
- Bledny join replay/lifecycle.
- Ukryty lookahead z `path_bps`.
- Pomieszanie lifecycle `close_reason` z biznesowym barrier label.
- Brak coverage TimeStop V2 windows w danym scope.

Odrzucone alternatywy:
- Rozszerzenie runtime TimeStop V2 o active close: odrzucone jako przedwczesne.
- Dopisywanie counterfactual rekordow do `shadow_lifecycle.jsonl`: odrzucone, bo lamaloby offline-only boundary.
- Rozbudowa `shadow_exit_replay_eval.py`: odrzucona, bo nowy lab ma szerszy lifecycle join/report contract.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `scripts/time_stop_v2_counterfactual_lab.py`
- Co zmieniono: dodano offline lab z CLI, parserami, join quality, per-position output, matrix summary, JSON/Markdown report.
- Dlaczego: potrzebne narzedzie laczace TimeStop V2 windows z exit replay economics.
- Efekt: R48/R2 generuje raport `TIMESTOP_V2_NO_WINDOWS` bez dotykania runtime logow.

Zmiana 2:
- Plik/modul: `scripts/test_time_stop_v2_counterfactual_lab.py`
- Co zmieniono: dodano 9 testow fixture-based dla saved STOP, cut TARGET, stale exclusion, no candidate, candidate after terminal, duplicate fallback, tie STOP wins, max hold TIMEOUT i no windows recommendation.
- Dlaczego: glowne ryzyka sa analityczne, nie runtime; testy falsyfikuja najlatwiejsze regresje.
- Efekt: targeted unit tests PASS.

Zmiana 3:
- Plik/modul: `docs/ADR/ADR_8D_TIMESTOP_V2_COUNTERFACTUAL_LAB_V1_20260625.md`
- Co zmieniono: dodano raport decyzyjny ADR-8D.
- Dlaczego: zmiana dodaje repo tooling i wymaga udokumentowania kontraktu offline-only.
- Efekt: zakres i ograniczenia sa jawne.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Py compile | `python3 -m py_compile scripts/time_stop_v2_counterfactual_lab.py scripts/test_time_stop_v2_counterfactual_lab.py` | no errors | PASS | lokalny run |
| Unit | `python3 scripts/test_time_stop_v2_counterfactual_lab.py -v` | 9 tests OK | PASS | lokalny run |
| CLI help | `python3 scripts/time_stop_v2_counterfactual_lab.py --help` | usage rendered | PASS | lokalny run |
| R48/R2 smoke | `python3 scripts/time_stop_v2_counterfactual_lab.py --scope shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2 --target-bps 6000 --stop-bps -6000 --max-hold-ms 120000 --resurrection-windows-ms 4000,8000` | `TIMESTOP_V2_NO_WINDOWS` | PASS | generated report |

Wniosek walidacyjny:
Tooling dziala na fixture tests i na realnym R48/R2 jako negatywny coverage smoke. R48/R2 nie ma okien TimeStop V2, wiec nie dostarcza oceny ekonomicznej V2.

Ograniczenia walidacji:
- Nie przetestowano jeszcze realnego scope z `time_stop_v2_window` + `shadow_exit_replay_v1` jednoczesnie.
- Skrypt jest offline-only i nie potwierdza zadnej gotowosci active exit.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: test stale candidate
- Co zabezpiecza: `stale_data_no_action` nie moze stac sie aktywnym `TSV2_EXIT`.
- Kiedy sie aktywuje: gdy candidate opiera sie o stale/missing/invalid market sample.
- Jak przetestowano: `test_stale_candidate_is_excluded`.
- Co pozostaje poza zakresem: runtime active close, ktorego nie implementowano.

Guardrail 2:
- Typ: no-lookahead / candidate PnL source
- Co zabezpiecza: candidate PnL nie moze uzywac punktu `path_bps` po candidate age.
- Kiedy sie aktywuje: fallback `candidate_pnl_bps_from_path_prev`.
- Jak przetestowano: saved/cut fixture cases.
- Co pozostaje poza zakresem: jakosc samego runtime price path.

Guardrail 3:
- Typ: join ambiguity accounting
- Co zabezpiecza: fallback duplicate key nie jest laczony po cichu.
- Kiedy sie aktywuje: brak exact join i wiele lifecycle positions dla `run_id/pool/base_mint`.
- Jak przetestowano: `test_duplicate_fallback_key_is_not_joined_silently`.
- Co pozostaje poza zakresem: naprawa upstream identity fields.

## Otwarte ryzyka / follow-up

- Uruchomic lab na scope, ktory ma jednoczesnie `shadow_exit_replay_v1` i `time_stop_v2_window`.
- Jesli realne scope pokaza duzo target cuts, nie promowac TimeStop V2 do shadow-close.
- Jesli realne scope pokaza dodatni PnL delta, niski target cut rate i stabilne saved STOP/TIMEOUT, przygotowac osobny plan `shadow_close_only`.
- Zachowac rozdzial `actual_lifecycle_close_reason` vs `baseline_barrier_result`; nie uzywac lifecycle close reason jako labela biznesowego.
