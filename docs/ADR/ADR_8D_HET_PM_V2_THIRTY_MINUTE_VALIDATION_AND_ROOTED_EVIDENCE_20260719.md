# ADR-8D: HET-PM V2 — 30-minutowe validation cohorts i rooted primary evidence

Status: `BINDING PROJECT-OWNER DECISION / IMPLEMENTED PRE-RUN CORRECTION`

Typ: ADR-8D / shadow post-buy / validation operations / promotion evidence

Data: `2026-07-19`

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-reproducible-validation`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, §19.9a.

Poziom ryzyka: `MEDIUM` dla wielkości prospective próby, `LOW` dla runtime
authority. Dokument zastępuje wyłącznie horyzont operacyjny decyzji z
`ADR_8D_HET_PM_V2_OWNER_BOUND_ONE_HOUR_VALIDATION_HORIZON_20260718.md`.
Nie zmienia policy HET/V1/TimeStop, progu Gate 1--5, shadow/live mode ani
V1-only proposal/apply/terminal/capacity ownership.

## D0. Decyzja właściciela

Właściciel projektu polecił skrócić świeży `validation-v1a` i przyszły
`validation-v1b` z jednej godziny do trzydziestu minut. Wiążący, native
horyzont launchera dla obu cohortów wynosi od teraz `1800` sekund i musi być
przekazany jawnie jako:

```text
--runtime-timeout-seconds 1800
```

Nie jest to zgoda na obniżenie criteria, minimów próby albo interpretację
krótszego runu jako automatycznej promocji.

## D1. Zaobserwowane fakty

Po zakończonym `validation-v1a-1h` audit wykazał, że runtime faktycznie
emituje durable primary `ExecutionEvent::PositionOpened` do:

```text
datasets/events/<scope>/exec_*.jsonl
```

Każdy taki wiersz posiada `kind.type = PositionOpened`, shadow lane,
`shadow-entry-*` order oraz `(position_id, position_epoch)`. Problemem nie był
brak emitera Rust i nie wymagał on zmiany post-buy authority.

Błędna była operacyjna selekcja artefaktu: do `position_events` wskazano
terminalny sidecar
`position_manager_terminal_truth_v2/shadow_position_event_v2.jsonl`, który
nie jest strumieniem `ExecutionEvent` i nie zawiera PositionOpened. Ponadto
launcher report/output znajdował się poza detached runtime rootem, czego nie
można poprawnie połączyć w jednym source-verifiable manifeście.

## D2. Przyczyna źródłowa

Launcher zezwalał validation runowi na dowolny absolutny `--output-dir`.
Promotion manifest ma twardy kontrakt jednego `repo_root` dla wszystkich
zhaszowanych inputów i proofu launchera. Dwa osobne rooty są więc
strukturalnie niekompatybilne, choć poszczególne pliki są poprawne.

Druga przyczyna była proceduralna: class `position_events` wymaga primary
ExecutionEvent stream, a nie terminal telemetry. Parser promotion gate jest
już restrykcyjny wobec schema PositionOpened; źle wybrany plik kończy się
brakiem primary denominatora.

## D3. Zmiana

1. `start_selector_lifecycle_run.py` dla `run_role=validation` odrzuca output
   directory poza `--root` i utrwala `output_dir_contract =
   inside_runtime_root` w launcher report.
2. Promotion manifest proof odrzuca report bez tej deklaracji lub ze ścieżką
   outputu poza wspólnym runtime rootem.
3. Promotion evaluation odrzuca manifest bez co najmniej jednego durable
   shadow PositionOpened zamiast rozpoczynać analizę z pustym primary
   denominatorem.
4. Oba run-configi używają nowych, niekolidujących identyfikatorów:
   `validation-v1a-30m` oraz `validation-v1b-30m`.

Nie ma zmiany Rust runtime, ponieważ primary emitter istnieje i jest
sprawdzony na rzeczywistym runtime artifact.

## D4. Operacyjny kontrakt kolejnych runów

Każdy prospective run jest uruchamiany z clean detached runtime worktree,
fresh release build oraz native timeoutem `1800`. Launcher report pozostaje w
domyślnym katalogu `reports/selector/...` wewnątrz tego worktree. Nie wolno
nadpisać go absolutną ścieżką innego checkoutu.

Przy budowie manifestu class `position_events` musi globować wyłącznie:

```text
datasets/events/<scope>/exec_*.jsonl
```

Class `lifecycle` i terminal correlation nadal korzystają z terminal lifecycle
artefaktów. Nie miesza to ownershipu: ExecutionEvent jest primary open
denominatorem, a terminal sidecar pozostaje źródłem terminal truth.

## D5. Historyczne artefakty

- historyczny trzygodzinny `validation-v1a` pozostaje invalid;
- `validation-v1a-1h` nie jest prospective validation evidence i pozostaje
  wyłącznie diagnostyczny;
- nie wolno kopiować, hardlinkować, przepisywać ani relabelować ich plików,
  aby udawały rooted 30-minute manifest;
- oba nowe runy potrzebują nowego source/binary/config locku przed startem.

## D6. Weryfikacja

Przed `validation-v1a-30m` należy potwierdzić:

1. parser launchera odrzuca validation `--output-dir` poza runtime rootem;
2. report otrzymuje `output_dir_contract = inside_runtime_root`;
3. `position_events` wskazuje `exec_*.jsonl`, a parser znajduje durable
   PositionOpened;
4. run config zawiera nowy `run_id`, namespace i 30-minute path;
5. locked criteria zawierają exact hash obu nowych configów, nowy commit i
   nową release binary;
6. exact launcher invocation zawiera `--runtime-timeout-seconds 1800`.

Po runie source-recomputed manifest/evaluation nadal fail-closed sprawdza
coverage, writer health, admission, lifecycle/replay, economic gates oraz
stabilność. Krótszy horyzont nie łagodzi żadnego z tych warunków.

## D7. Rollback

Powrót do innego horyzontu wymaga kolejnej jawnej decyzji właściciela oraz
nowego ADR, nowych namespace'ów i ponownego locku provenance. Nie wolno
milcząco zmienić `1800` w command line ani w zewnętrznym schedulerze.

## D8. Zamknięcie

Ta korekta zamyka wyłącznie granicę operacyjnego runu i rooted primary
evidence. Nie jest PR B cutoverem i nie daje prawa do authority promotion.
Nadal wymagane są dwa świeże prospective runy, source-recomputed promotion
artifact i `promotion_gate_passed = true`.
