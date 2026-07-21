# ADR-8D: HET-PM V2 — wiążący jednogodzinny horyzont prospective validation

Status: `BINDING PROJECT-OWNER DECISION / HISTORICAL v1a INVALID / FUTURE RUNS REQUIRE NATIVE 3600s`

Typ: ADR-8D / shadow post-buy / validation operations / promotion evidence

Data: `2026-07-18`

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-reproducible-validation`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, §19.9a.

Poziom ryzyka: `MEDIUM` dla ilości zebranej próby, `LOW` dla runtime authority.
Decyzja nie zmienia V1/V2 policy, progów, source of truth, proposal/apply/
terminal/capacity ownership ani shadow/live separation.

## D0. Kontekst i decyzja właściciela

Właściciel projektu polecił ograniczyć obecny prospective run
`validation-v1a` oraz przyszły `validation-v1b` z trzech godzin do jednej
godziny. Decyzja jest wiążąca dla tych dwóch runów i ma ograniczyć czas
oczekiwania na wykrycie błędów kontraktu evidence.

Zakres decyzji jest wyłącznie operacyjny: maksymalny horyzont runtime wynosi
`3600` sekund. Nie jest to zgoda na obniżenie zamrożonych progów Gate 1--5 ani
na dopasowanie criteria po wyniku runu.

## D1. Zespół i odpowiedzialności

- właściciel projektu: wiążąca dyspozycja horyzontu; historyczna próba
  zewnętrznego skrócenia bieżącego runu nie jest uznawana za lifecycle proof;
- launcher lifecycle: jedyny owner uruchomienia, sygnału shutdown i
  hard-backstopu procesu runtime;
- `ghost-launcher`: wykonuje istniejącą graceful shutdown path;
- promotion analyzer: nadal fail-closed ocenia kompletność i minima próby;
- HET-PM V2: pozostaje wyłącznie producentem observe-only evidence.

## D2. Problem

`validation-v1a` został rozpoczęty z argumentem launcherowym
`--runtime-timeout-seconds 10800`. GNU `timeout` ustala ten limit przy starcie
procesu; nie ma bezpiecznej operacji zmiany jego horyzontu in-place.

Po zakończeniu historycznego runu jedyną poprawną drogą jest nowy preflight,
nowy start time oraz kolejna pełna godzina od początku z natywnym limitem
launchera. Historyczne trzygodzinne artefakty nie są przenoszone do nowej
pary validation.

## D3. Wynik containmentu i klasyfikacja historycznego runu

Próba zewnętrznego dostarczenia `SIGINT` przez scheduler/tmux nie dała
trwałego dowodu dispatchu, a bieżący proces wykonał pierwotny timeout
launcherowy `10800` sekund. Runtime log wskazuje start
`2026-07-18T19:31:38.920517Z` oraz sygnał shutdown dopiero
`2026-07-18T22:31:38.911179Z`.

To jest trzykrotnie dłużej niż wiążący horyzont. Niezależnie od tego, czy
końcowy graceful shutdown był poprawny, ten historyczny `validation-v1a` jest
`invalid promotion evidence`. Nie wolno go re-labelować, naprawiać JSONL ani
łączyć z przyszłymi runami jako jeden z prospective validation runów.

## D4. Przyczyna źródłowa

Początkowy launcherowy timeout `10800` sekund został wybrany przed tą
dyspozycją właściciela. Niezmiennego timeoutu już uruchomionego procesu nie
można bezpiecznie skrócić z zewnątrz. Dodatkowy scheduler/tmux nie jest
częścią launcherowego lifecycle contractu, dlatego nie stanowi wystarczającej
kontroli dla prospective evidence.

## D5. Trwała decyzja korygująca

Każdy nowy prospective validation run z tej pary, w szczególności
`validation-v1b`, musi być uruchomiony przez lifecycle launcher z:

```text
--runtime-timeout-seconds 3600
```

Argument jest celowo pozostawiony jawny w invokacji launchera — nie zmieniamy
globalnego domyślnego timeoutu używanego przez inne, niepowiązane rollouty.
`RUN_LIFECYCLE_LAUNCHER_REPORT.json` musi utrwalić
`runtime_timeout_seconds = 3600`, a launcher proof exact invocation musi
zawierać tę samą wartość.

Nie istnieje wyjątek dla historycznego `validation-v1a`. Ponowiony v1a i v1b
mają mieć od startu native timeout `3600`, osobne identyfikatory run/cohort i
osobne namespace'y/ścieżki outputu. Nie wolno użyć zewnętrznego
schedulera/tmux do zmiany horyzontu ani uruchomić v1b z limitem trzygodzinnym.

## D6. Implementacja i granice

Decyzja o horyzoncie zmienia wyłącznie plan kontraktu i ten ADR. Nie zmieniono:

- HET/V1/TimeStop config hash;
- locked policy criteria ani sample minima;
- schema comparison, writer health, admission evidence, replay lub lifecycle;
- effective mode CrashGuarda;
- V1-only lifecycle authority;
- konfiguracji brain/run active runtime.

Kolejny run otrzymuje nowy, jawny launcher horizon bez ukrytej zmiany defaultu
dla innych operacji. Jeżeli przed rerunem zmieni się producer/runtime, należy
ponownie zablokować provenance/criteria dla nowego commita i binarki.

## D7. Weryfikacja i zapobieganie

Przed startem `validation-v1b` operator sprawdza:

1. command line zawiera dokładnie `--runtime-timeout-seconds 3600`;
2. report launchera zapisuje `runtime_timeout_seconds: 3600`;
3. `exact_launcher_invocation` w launcher proof jest zgodne z reportem;
4. run ma nowy `run_id`, nowy `launch_cohort_id` i osobny output namespace,
   bez zmiany locked behavioral/policy contract;
5. po shutdownie manifest i source-recomputing promotion evaluation przechodzą
   zwykłą fail-closed walidację.

Jednogodzinny run, który nie osiągnie wymaganej liczby candidates, matched
positions, cohort coverage lub kompletności evidence, pozostaje `FAIL`.
Minimalne progi nie są redukowane przez ten ADR.

## D8. Zamknięcie i rollback

Decyzja obowiązuje dla ponowionego `validation-v1a` i `validation-v1b`.
Rollback wymaga kolejnej, jawnej decyzji właściciela projektu zapisanej w
następnym ADR; nie jest dozwolone milczące wydłużenie timeoutu w command line.

Niezależnie od wyniku jednego lub obu jednogodzinnych runów ten ADR nie daje
zgody na PR B authority cutover. Cutover nadal wymaga poprawnego, committed,
source-recomputed promotion artifact z `promotion_gate_passed = true`.
