# ADR-8D: Selector lifecycle launcher process group guard

Status: `IMPLEMENTED / SHADOW ONLY`

Typ: ADR-8D / runtime launcher / validation harness

Data: `2026-07-19`

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Uwaga o szablonie: wskazany w globalnych instrukcjach plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje. Dokument uzywa lokalnego
ukladu D1--D8 stosowanego przez pozostale ADR-8D tego repozytorium.

## D1. Problem

Kontrolowany shadow run wykazal dwa osobne fakty:

1. Runtime dzialal i zakonczyl sie czysto po zakladanym czasie.
2. Launcher lifecycle guard zapisal `FAIL_EVENT_CANARY` i probowal ubic tmux,
   ale faktyczny proces `ghost-launcher` pozostal osierocony i kontynuowal run.

Powod byl praktyczny: `tmux kill-session` nie jest wystarczajacym dowodem
zatrzymania procesu uruchomionego w panelu. Shell tmux moze zniknac, a child
`timeout -> ghost-launcher` moze przezyc pod pid 1.

## D2. Decyzja

Launcher tworzy teraz osobna grupe procesu runtime przez `setsid`, zapisuje jej
PID/PGID do pidfile w katalogu raportu i przy awaryjnym zatrzymaniu wysyla
sygnal do calej grupy procesu.

Kolejnosc awaryjnego stopu:

1. odczyt pidfile;
2. `SIGINT` do runtime process group;
3. bounded wait;
4. `SIGKILL`, jezeli grupa nadal zyje;
5. `tmux kill-session` jako sprzatanie sesji;
6. zapis wyniku stopu w `RUN_LIFECYCLE_LAUNCHER_REPORT`.

## D3. Zakres

Zmieniono tylko wrapper startowy:

- `scripts/start_selector_lifecycle_run.py`;
- `scripts/test_selector_lifecycle_run_guard.py`;
- niniejszy ADR-8D.

Nie zmieniono logiki Gatekeepera, managera pozycji, symulacji shadow, decyzji
sprzedazy ani konfiguracji live.

## D4. Reguly zachowania

- Po failu event canary albo lifecycle proof timeout raport nie moze zakladac,
  ze samo zamkniecie tmux zatrzymalo runtime.
- Jezeli pidfile istnieje, zatrzymywany jest faktyczny process group runtime.
- Jezeli pidfile nie istnieje, skrypt nadal sprzata tmux, ale raport zachowuje
  `pidfile_found=false`.
- Runtime timeout pozostaje kontrolowanym `SIGINT` z hard backstopem.

## D5. Slad audytowy

Raport launchera zapisuje teraz:

- `artifacts.runtime_pidfile`;
- `runtime_termination.<reason>.runtime_pgid`;
- wyslane sygnaly;
- czy runtime process group zyje po stopie;
- czy tmux session istnieje po stopie.

Markdown report pokazuje te same pola w sekcji `Runtime Termination`.

## D6. Testy

Dodano testy jednostkowe:

- start tmux zawiera `setsid sh -c`, pidfile i `timeout --foreground`;
- awaryjny stop wysyla `SIGINT` do runtime process group;
- jezeli grupa przezyje `SIGINT`, launcher eskaluje do `SIGKILL`.

## D7. Run weryfikacyjny

Nastepny kontrolowany shadow run powinien zostac uruchomiony z event canary
dluzszym niz realne okno obserwacji Gatekeepera. Dla profilu z
`max_wait_ms=66000`, event canary nie powinien byc krotszy niz 90 sekund.

## D8. Rollback

Rollback polega na przywroceniu poprzedniego startu tmux bez pidfile/setsid.
Nie jest zalecany, bo odtwarza cichy blad z retry4: raport moze twierdzic, ze
run zostal ubity po failu canary, podczas gdy runtime nadal pracuje.
