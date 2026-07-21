# ADR-8D: HET-PM V2 validation — serializacja rekordów lifecycle JSONL

Status: `IMPLEMENTED / VALIDATION-v1a REJECTED / RERUN REQUIRED`

Typ: ADR-8D / shadow post-buy / lifecycle evidence integrity / prospective
promotion validation

Data: 2026-07-18

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-reproducible-validation`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, PR B prerequisite
promotion-evidence contract.

Powiązane ADR:

- `ADR_8D_HET_PM_V2_PR_A_OBSERVE_ONLY_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_BOUNDED_SIDECAR_TIMING_ISOLATION_20260716.md`.

Poziom ryzyka: `HIGH` dla wiarygodności evidence, `LOW` dla authority. Zmiana
nie modyfikuje polityki V1/V2, konfiguracji progów, proposal/apply/terminal
ownership ani live execution.

## 1. Ustalony problem

Pierwszy prospective run `validation-v1a` zakończył się kontrolowanym
shutdownem, lecz podczas materializacji manifestu promotion evidence jego
`shadow_lifecycle.jsonl` nie przeszedł fail-closed parse:

```text
invalid JSONL .../shadow_lifecycle.jsonl:3439
Expecting ',' delimiter
```

Pełny skan artefaktu wykazał 12 wadliwych wierszy w sześciu incydentach.
Fragmenty zawierają bajty dwóch różnych rekordów JSON przeplecione w tym samym
wierszu, a następujący po nim pusty wiersz. To nie jest pojedynczy uszkodzony
ogon po przerwanym procesie ani błąd analyzera.

Przyczyną jest poprzedni zapis:

```text
OpenOptions::append
-> serde_json::to_writer(file, record)
-> write_all(newline)
-> flush
```

Równoległe guardian tasks mogły otworzyć ten sam plik i przeplatać własne
payloady przed osobnymi zapisami newline. `O_APPEND` wyznacza pozycję każdego
wywołania write, ale nie ustanawia własności całego wielowywołaniowego rekordu.

## 2. Decyzja

`MonitoringEngine` posiada teraz jeden jawny,
`Arc<parking_lot::Mutex<()>>`, należący do jego lifecycle sinku. Każde
`shadow_lifecycle.jsonl` append jest wykonywane pod tą samą blokadą:

```text
acquire lifecycle sink lock
-> create parent / open append
-> serialize complete record
-> append newline
-> flush
-> release lifecycle sink lock
```

Blokada jest współdzielona przez wszystkie taski korzystające z tej samej
instancji `MonitoringEngine`. Primary i probe monitor mają osobne lifecycle
paths oraz osobne instancje, więc nie tworzą niepotrzebnej globalnej blokady.

## 3. Granice i inwarianty

Zmiana obejmuje wyłącznie operacyjny lifecycle JSONL:

- HET comparison sidecar nadal używa własnego bounded workera;
- writer health, admission evidence i replay sidecar nie zmieniają schematu;
- V1 pozostaje jedynym proposal/apply/terminal/capacity ownerem;
- V2 pozostaje observe-only i nie otrzymuje nowej ścieżki authority;
- canonical terminal source of truth nie jest zastępowany lifecycle JSONL;
- blokada nie jest trzymana przez `.await` i nie obejmuje policy evaluation.

Istniejący synchroniczny append lifecycle nie zostaje rozszerzony na nową
ścieżkę hot-path. Korekta tylko serializuje równoczesne wywołania istniejącego
sinku, aby zachować granicę jednego rekordu JSONL.

## 4. Konsekwencja dla evidence

Historyczny `validation-v1a` jest trwałym artefaktem diagnostycznym, ale nie
jest poprawnym source inputem promotion:

```text
clean runtime shutdown != valid promotion evidence
invalid lifecycle JSONL -> manifest/evaluate fail closed
```

Nie wolno usuwać wadliwych wierszy, scalać fragmentów ani rekonstruować
lifecycle logu z comparison/replay. Taka operacja zmieniłaby zhashowany input i
ukryłaby źródłową awarię emitera.

Po tej zmianie wymagane są:

1. nowy commit runtime i deterministyczny release binary;
2. ponowne canonical lock criteria dla nowego commita/binarki;
3. nowy prospective run `validation-v1a`;
4. pełny manifest i source-recomputing evaluation;
5. dopiero potem niezależny `validation-v1b`.

Stary run nie może spełnić żadnego minimum Gate 1–5 i nie jest liczony jako
jeden z dwóch validation runs.

## 5. Test kontraktowy

Dodano test:

```text
concurrent_lifecycle_jsonl_appends_preserve_record_boundaries
```

Test uruchamia 16 równoległych writerów, z których każdy zapisuje 32 rekordy
o payloadzie 8 KiB przez dokładnie ten sam lifecycle sink lock. Następnie
parsuje cały plik jako JSONL i wymaga:

- dokładnie 512 poprawnych rekordów;
- jednego unikalnego `(writer_id, row_id)` dla każdego appendu;
- braku pustych, zlepionych albo częściowych rekordów.

Istniejący terminal test `het_sidecar_writer_failure_cannot_retain_v1_terminal_or_capacity`
pozostaje dowodem, że error HET sidecara nie zatrzymuje V1 terminal/capacity
flow.

## 6. Walidacja lokalna

Po zmianie uruchomiono:

```text
cargo fmt --all --check
cargo test -p ghost-brain concurrent_lifecycle_jsonl_appends_preserve_record_boundaries --lib
cargo test -p ghost-brain het_sidecar_writer_failure_cannot_retain_v1_terminal_or_capacity --lib
```

Ostrzeżenia z istniejących, niezmienionych modułów nie są częścią tej zmiany.
Przed rozpoczęciem kolejnego runu wymagane są jeszcze clean release rebuild,
criteria relock oraz standardowy lifecycle preflight.

## 7. Rollback

Rollback kodu jest technicznie prosty (usunąć lock), ale nie jest bezpiecznym
rollbackiem kontraktu evidence: powróciłaby możliwość zlepienia JSONL. Jeżeli
nowa ścieżka persistence zawiedzie, poprawną degradacją jest odrzucenie runu
przez manifest/analyzer, a nie użycie niepełnego lifecycle artefaktu.
