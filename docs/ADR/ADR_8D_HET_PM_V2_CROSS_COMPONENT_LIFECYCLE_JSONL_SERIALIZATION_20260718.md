# ADR-8D: HET-PM V2 — serializacja cross-component `shadow_lifecycle.jsonl`

Status: `IMPLEMENTED / FOCUSED VERIFICATION PASS`

Typ: ADR-8D / shadow post-buy / lifecycle evidence / evidence integrity

Data: `2026-07-18`

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-reproducible-validation`

Poziom ryzyka: `HIGH` dla ważności evidence, `LOW` dla lifecycle authority.

Ten ADR uzupełnia i zastępuje w zakresie przyczyny źródłowej wcześniejszą
diagnozę w
`ADR_8D_HET_PM_V2_VALIDATION_LIFECYCLE_JSONL_SERIALIZATION_20260718.md`:
sam mutex `MonitoringEngine` serializował wyłącznie writerów guardiana, a nie
niezależny writer launchera do tego samego pliku.

## D0. Decyzja

Wszyscy współpracujący writerzy primary `shadow_lifecycle.jsonl` muszą używać
jednego helpera, który serializuje dokładnie jeden już zbudowany wiersz JSONL
pod advisory exclusive file lockiem obejmującym `append + flush`.

Nie naprawiamy historycznego pliku evidence. Run zawierający uszkodzony JSONL
pozostaje invalid promotion evidence.

## D1. Zaobserwowany fakt

W historycznym `validation-v1a` parser fail-closed odrzucił
`shadow_lifecycle.jsonl` na linii `6763`: jeden fizyczny wiersz zawierał dwa
sklejone obiekty JSON, najpierw obserwację CrashGuarda guardiana, a następnie
`shadow_dispatch` launchera. Następna linia była pusta.

To nie jest brak metryki ani błąd analyzera. Nie istnieje wiarygodny sposób
odtworzenia granicy rekordów po fakcie bez zmiany evidence.

## D2. Przyczyna źródłowa

Guardian posiadał mutex tylko we własnym `MonitoringEngine`. Launcherowy
shadow dispatcher otwierał ten sam plik niezależnie i wykonywał osobne
`write_all(json)`, `write_all(newline)` oraz `flush`. Dwa niezależne runtime
obiekty nie dzieliły lokalnego mutexa, więc ich sekwencje zapisu mogły się
przeplatać.

## D3. Zmiana

Dodano `guardian::post_buy::lifecycle_jsonl`:

1. serializuje rekord poza sekcją krytyczną;
2. waliduje, że prepared payload jest dokładnie jednym niepustym wierszem
   JSONL (jedno końcowe `newline`, bez wewnętrznego raw `newline`);
3. otwiera plik append-only;
4. zakłada Unix `flock(LOCK_EX)`;
5. wykonuje `write_all` i `flush` pod tym samym lockiem;
6. zwalnia lock przez RAII.

Guardian i launcherowy dispatcher korzystają z tego samego helpera.
Dispatcher wykonuje blokujący helper w `spawn_blocking`, więc nie dodaje
filesystem I/O do asynchronicznego tasku runtime.

## D4. Zachowane granice

Zmiana nie dotyka:

- V1-only proposal/apply/terminal/capacity authority;
- V2 observe-only policy ani hierarchy gate'ów;
- canonical terminal truth i retry;
- HET sidecara, writer-health ani promotion analyzer;
- shadow/live trybu wykonania.

Jeżeli append lifecycle zawiedzie, zachowuje istniejącą semantykę błędu danego
wywołującego; helper nie zamienia błędu w sukces ani nie wykonuje retry.

## D5. Weryfikacja

Obowiązkowe testy:

1. 16 niezależnych writerów zapisuje po 32 duże rekordy; każdy wynikowy wiersz
   musi dać się sparsować jako dokładnie jeden JSON i wszystkie identity muszą
   być unikalne.
2. Istniejący test guardiana wykonuje tę samą współbieżną granicę przez shared
   helper.
3. Test launcherowy współbieżnie uruchamia rzeczywisty `shadow_dispatch`
   writer i guardian writer; wynik ma dokładnie 64 poprawne, unikalne rekordy.
4. Po nowym prospective runie manifest validation musi sparsować całe
   `shadow_lifecycle.jsonl` bez recovery lub ręcznej edycji.

Punkty 1--3 przeszły lokalnie. Punkt 4 jest operacyjną bramką rerunu; nie
dotyczy historycznego, już uszkodzonego pliku.

## D6. Operacyjna konsekwencja

Ponowiony `validation-v1a` może wystartować wyłącznie z nowej binarki oraz po
ponownym locku provenance/criteria. Musi użyć osobnego namespace'u outputu i
native `--runtime-timeout-seconds 3600`. Historyczny trzygodzinny run nie jest
częścią nowej pary validation.

## D7. Rollback

Rollback polega wyłącznie na cofnięciu tej zmiany przed nowym runem i wymaga
nowego ADR oraz ponownego frozen contractu. Nie wolno mieszać artefaktów
produced before/after rollback ani uznawać historycznego pliku za poprawny.

## D8. Zamknięcie

Ten ADR naprawia wyłącznie durability granic rekordów lifecycle evidence.
Nie daje zgody na PR B authority cutover; nadal wymagane są dwa nowe,
source-recomputed prospective validation runy i `promotion_gate_passed=true`.
