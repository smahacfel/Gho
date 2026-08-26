# ADR-8D: ACE Core V3 — puste pliki EventWritera nie są uciętym JSONL

Status: `IMPLEMENTED / FOCUSED_VALIDATION_PENDING / QUALIFYING_SMOKE_PENDING /
DAY1_NOT_STARTED / OBSERVE_ONLY / PR2_STILL_BLOCKED`

Typ: ADR-8D / capture-health false-positive remediation

Data: `2026-07-29`

Repo: `smahacfel/Gho`

Baseline naukowy: `origin/main = 43057b296663129ca9b4f572e793474830a5452c`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_ACE_CORE_ONE_DAY_KILL_TEST_V3_POST_PR86.md`

Poprzedni ADR integrity:
`docs/ADR/ADR_8D_ACE_CORE_V3_INGRESS_CUTOFF_AND_QUALIFYING_SMOKE_20260729.md`

Uwaga o szablonie: ścieżka wskazana w instrukcji globalnej,
`/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument
zachowuje lokalny format ADR-8D stosowany w `docs/ADR/`.

## D0. Decyzja

`EventWriter::new()` może legalnie utworzyć pierwszy `exec_*.jsonl` podczas
rotacji, zanim zapisze jakikolwiek event. Pusty plik nie ma zatem
niedokończonego rekordu JSONL.

`validate_event_files()` pomija teraz plik o zerowym rozmiarze przed bramką
final-newline. Dla każdego niepustego pliku zachowana zostaje niezmieniona,
fail-closed zasada: ostatni bajt musi być `0x0a`.

Nie zmieniono `EventWriter`, OracleRuntime, EventEmittera, CandidateIntegrity,
offline ACE probe'a, quote authority, cutoffu, capacity bounds, Gatekeepera
ani ścieżki shadow/live.

## D1. Powód

Kwalifikujący smoke
`ace-core-one-day-probe-r1-qualifying-smoke-20260729t123700z` zawierał:

```text
0 B        exec_launcher-1785328662376_20260729_123742_0000.jsonl
2 096 326 B exec_ace-core-one-day-probe-r1-qualifying-smoke-20260729t123700z_20260729_123742_0000.jsonl
```

Niepusty, właściwy ACE tape kończył się `0x0a`. Wcześniejszy helper uznawał
pusty plik za `final newline missing` tylko dlatego, że `b"".endswith(b"\n")`
jest fałszywe. Capture pozostał prawidłowo nieważny, ponieważ nie powstał
receipt; nie jest rehabilitowany wstecznie.

## D2. Kontrakt po korekcie

```text
pusty exec_*.jsonl                         -> pominięty jako brak rekordu
niepusty exec_*.jsonl z finalnym 0x0a       -> walidowany dalej
niepusty exec_*.jsonl bez finalnego 0x0a    -> finalize = 2, brak receiptu
wszystkie pliki puste                      -> brak birth/trade evidence, finalize = 2
```

Pominięcie pustego pliku nie osłabia minimum evidence: helper nadal wymaga co
najmniej jednego `NewPoolDetected`, jednego `PoolTransaction` i jednego
successful, non-synthetic trade z balances, kompletnym order key oraz pełnymi
reserves.

## D3. Weryfikacja

Dwa focused testy chronią wyłącznie poprawiony kontrakt:

1. pusty `exec_*.jsonl` obok poprawnego tape'u daje `finalize = 0` i receipt;
2. niepusty tape bez finalnego newline daje `finalize = 2` i brak receiptu.

Przed `GO` Dnia 1 konieczny jest nowy, niezależny qualifying smoke z nowym
run ID i nowymi ścieżkami. Wymaga on: trzy counters równe zero, czas 120–300 s,
`finalize = 0`, istniejący receipt, `capture_status = VALID_CAPTURE`, pustą
listę invalid reasons oraz `verify-probe = 0`.
