# ADR-8D: Budget guard for analiza_porownawcza Section 6 DTW

Status: IMPLEMENTED / TARGETED_SMOKE_VERIFIED
Typ: ADR-8D / offline analysis script runtime guard
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: zabezpieczenie Sekcji 6 DTW w `scripts/analiza_porownawcza.py` i root-level `analiza_porownawcza.py` przed nieograniczonym kosztem na nowych logach z pelnymi decision vectors
Poziom ryzyka: LOW

Dotkniete moduly/pliki:
- `scripts/analiza_porownawcza.py`
- `analiza_porownawcza.py`
- `docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_DTW_SECTION6_BUDGET_GUARD_20260619.md`

Powiazane ADR:
- `docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_NEW_LOG_VECTORS_DELTAS_20260619.md`
- `docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_SCRIPTS_TRANSFER_20260619.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w istniejacych ADR-ach PR1-PR6 i offline analyzer ADR-ach.

## 1. Przygotowanie i dzialania wstepne

Problem:
Uzytkownik zglosil, ze `analiza_porownawcza.py` crashuje sie albo zawiesza, gdy zaczyna liczyc Sekcje 6.

Kontekst:
Po dodaniu obslugi nowych logow skrypt zaczal widziec pelne decision vectors, w tym `vectors_prices`, `vectors_d_price` i `vectors_interval_ms`. Sekcja 6 uzywala Dynamic Time Warping dla porownania ksztaltu serii, ale nie miala limitu liczby par wewnatrz A/B.

Akcje wstepne:
- Przejrzano aktywny `scripts/analiza_porownawcza.py`.
- Przejrzano root-level `analiza_porownawcza.py`, bo w repo istnieja dwie wersje skryptu.
- Potwierdzono, ze Sekcja 6 liczyla DTW na wielu parach serii bez budzetu czasu, limitu serii i limitu dlugosci wektora.

## 2. Routing i skills

Uzyte skills:
- `large-data-analytics`: problem dotyczy kosztu offline analizy event-stream/vector features i bezpiecznego ograniczenia eksploracyjnej metryki DTW.

Nie ladowano dokumentow specjalistycznych:
- `gatekeeper-policy-auditor`: brak zmian Gatekeeper policy, verdictow lub reason codes.
- `oracle-session-runtime-engineer`: brak zmian sesji runtime, deadline, event routing lub lifecycle.
- `decision-logging-replay-analyst`: skrypt czyta logi offline, ale nie zmienia DecisionLoggera ani schemy JSONL.
- `config-rollout-safety-reviewer`: zmiana nie dodaje runtime config Ghosta.

## 3. Opis problemu - 3W2H

What:
Sekcja 6 DTW mogla wykonywac bardzo duzo kosztownych porownan sekwencji. Przy nowych logach z pelnymi wektorami tickow koszt rosl praktycznie kwadratowo wzgledem liczby serii.

Where:
- `scripts/analiza_porownawcza.py`, Sekcja 6.
- `analiza_porownawcza.py`, Sekcja 6.

Why it matters:
Skrypt jest narzedziem offline do porownywania zbiorow A/B. Nie moze blokowac pracy tylko dlatego, ze logi sa bogatsze i zawieraja pelne serie decyzji. Sekcja 6 ma byc diagnostyczna, nie powinna zatrzymywac calego raportu.

How observed:
Kod wykonywal `_mean_dtw(series_a)` oraz `_mean_dtw(series_b)` po wszystkich parach wewnatrz zbioru, bez limitu. Dla wielu rekordow i dlugich `decision_time_series` moglo to oznaczac tysiace wywolan `fastdtw` na dlugich wektorach.

How many / scale:
Problem dotyczy kazdego uruchomienia skryptu na nowych, pelnych JSONL z decision vectors, szczegolnie gdy `fastdtw/scipy` sa zainstalowane.

## 4. Przyczyna zrodlowa

Root cause:
Sekcja 6 byla napisana pod krotsze, starsze wektory. Po przejsciu na nowe logi z pelna seria tickow zaczela dostawac wiecej i dluzszych sekwencji, ale algorytm wciaz liczyl pary bez guardow:
- brak `max_series_per_set`,
- brak `max_pairs`,
- brak `max_vector_len`,
- brak `time_budget`,
- brak jawnego `AB_ENABLE_DTW=0` jako awaryjnego wylacznika.

To nie byl problem runtime Ghosta ani DecisionLoggera. To byl problem kosztu offline analityki.

## 5. Strategia naprawy

Przyjeta strategia:
- Zostawic DTW jako diagnostyke ksztaltu czasu.
- Nie usuwac Sekcji 6.
- Dodac deterministyczne limity pracy:
  - limit liczby serii na zbior,
  - limit liczby par DTW per koszyk A/A, B/B i A/B,
  - limit dlugosci serii przez evenly-spaced downsampling,
  - budzet czasu calej sekcji,
  - jawny env kill switch.
- Nie imputowac `null` w `vectors_prices` jako `0`.
- Zachowac raw coverage cen w Sekcji 0B; dla DTW pomijac tylko wartosci nienumeryczne, bo DTW wymaga liczb.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: parametry sterujace DTW
Dodano env vars:
- `AB_ENABLE_DTW` - domyslnie `1`; `0/false/no/off` pomija Sekcje 6.
- `AB_DTW_MAX_SERIES_PER_SET` - domyslnie `80`.
- `AB_DTW_MAX_PAIRS` - domyslnie `40`.
- `AB_DTW_MAX_VECTOR_LEN` - domyslnie `256`.
- `AB_DTW_TIME_BUDGET_SEC` - domyslnie `30`.

Zmiana 2: deterministyczne ograniczanie danych
- Dodano evenly-spaced downsampling wektorow.
- Dodano evenly-spaced selection serii.
- Dodano bounded pair iterators dla porownan inside A, inside B i cross A/B.

Zmiana 3: budzet czasu
- Dodano `_DtwBudgetExceeded`.
- Kazde wywolanie DTW sprawdza deadline.
- Po przekroczeniu budzetu Sekcja 6 konczy sie ostrzezeniem, zamiast blokowac caly raport.

Zmiana 4: status w output
- Sekcja 6 wypisuje aktywne limity.
- Wyniki DTW pokazuja liczbe realnie policzonych par.
- `AB_ENABLE_DTW=0` daje jawny komunikat o pominieciu sekcji.

## 7. Walidacja

Wykonane komendy:
- `python3 -m py_compile scripts/analiza_porownawcza.py analiza_porownawcza.py`
- `git diff --check -- scripts/analiza_porownawcza.py analiza_porownawcza.py`
- `AB_ENABLE_DTW=0 AB_SEGMENT_LAB=0 timeout 60s python3 scripts/analiza_porownawcza.py /tmp/gho_analiza_sample/a.jsonl /tmp/gho_analiza_sample/b.jsonl > /tmp/gho_analiza_porownawcza_smoke.out`
- `AB_SEGMENT_LAB=0 timeout 60s python3 scripts/analiza_porownawcza.py /tmp/gho_analiza_sample/a.jsonl /tmp/gho_analiza_sample/b.jsonl > /tmp/gho_analiza_porownawcza_smoke_default.out`
- kontrolowany import testowy ze stubem `fastdtw`, wymuszajacy wejscie w bounded DTW path mimo braku lokalnych zaleznosci `numpy/fastdtw`.

Wynik:
- Oba skrypty kompiluja sie poprawnie.
- `git diff --check` jest czysty.
- Smoke z `AB_ENABLE_DTW=0` zakonczyl sie exit code 0 w okolo 5.5 s i pokazal komunikat: `SEKCJA 6 DTW wylaczona przez AB_ENABLE_DTW=0`.
- Smoke domyslny w tym srodowisku zakonczyl sie exit code 0 w okolo 5.5 s i pokazal komunikat: `Brak bibliotek fastdtw/scipy - pomin SEKCJA 6`.
- Stub test na 120 rekordach A, 120 rekordach B i wektorach po 900 punktow potwierdzil:
  - serie ograniczone do `AB_DTW_MAX_SERIES_PER_SET=6`,
  - wektory ograniczone do `AB_DTW_MAX_VECTOR_LEN=32`,
  - liczba wywolan DTW ograniczona do 36 przy trzech polach i `AB_DTW_MAX_PAIRS=4`.

Uwaga walidacyjna:
Lokalne srodowisko nie ma `fastdtw` ani `numpy`, wiec pelny realny DTW z zaleznosciami nie zostal uruchomiony. Zostal natomiast przetestowany kod limitow przez kontrolowany stub i oba realne tryby bez zaleznosci.

## 8. Ryzyka i zabezpieczenia

Ryzyko 1: DTW stanie sie probkowaniem, nie pelnym exhaustivem.
Mitigacja:
- Sekcja wypisuje limity i liczbe policzonych par.
- To jest offline diagnostyka ksztaltu, nie runtime policy.
- Limity sa konfigurowalne env vars.

Ryzyko 2: Utrata sygnalu przez zbyt agresywny downsampling.
Mitigacja:
- Domyslny limit `AB_DTW_MAX_VECTOR_LEN=256` zachowuje gesto wystarczajaca do porownania ksztaltu w typowych oknach decyzji.
- Dla deep-dive mozna podniesc limit w env.

Ryzyko 3: Ciche ukrycie problemu missing prices.
Mitigacja:
- `vectors_prices` w raw extractorze nadal zachowuje `None`.
- DTW filtruje nienumeryczne wartosci tylko dlatego, ze matematycznie wymaga liczb.
- Coverage cen pozostaje raportowane w Sekcji 0B.

Ryzyko 4: Dwie wersje skryptu beda zachowywac sie inaczej.
Mitigacja:
- Ten sam guard dodano do `scripts/analiza_porownawcza.py` i root-level `analiza_porownawcza.py`.

## 9. Status koncowy

Status: implemented.

Sekcja 6 nie powinna juz zawieszac skryptu na nowych logach z pelnymi wektorami. Domyslnie nadal dziala, jezeli zaleznosci DTW sa zainstalowane, ale dziala w jawnych limitach kosztu. W razie potrzeby mozna ja calkowicie pominac przez `AB_ENABLE_DTW=0`.

Brak zmian runtime Ghosta:
- brak zmian Gatekeeper policy,
- brak zmian DecisionLoggera,
- brak zmian selector dataset export,
- brak zmian shadow/live behavior.
