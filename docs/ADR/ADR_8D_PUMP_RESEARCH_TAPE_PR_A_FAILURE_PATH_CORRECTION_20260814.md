# ADR-8D: Pump Research Evidence Tape V1.1 — korekta failure paths PR-A

**Data:** 2026-08-14

**Status:** IMPLEMENTED / RESEARCH-ONLY / PROSPECTIVE CAPTURE NOT STARTED / PR-B PENDING

**Task:** `PUMP_RESEARCH_EVIDENCE_TAPE_V1_1_PR_A_FAILURE_PATH_CORRECTION`

## D0. Decyzja

Korygujemy wyłącznie failure paths standalone PR-A, które mogły błędnie
oznaczyć capture jako `Complete`, zgubić informację o lokalnej utracie albo
rozłączyć manifest od rzeczywistego requestu Yellowstone.

Granica pozostaje taka sama:

```text
decoded Yellowstone SubscribeUpdate
→ bounded source ingress
→ immutable raw V1 writer
→ run receipts
```

Nie jest to PR-B i nie rozpoczyna prospective capture. Nie zmieniono parsera,
trajectory certifiera, Gatekeepera, `MaterializedFeatureSet`, AccountStateCore,
Event Busa, canonical permitu, execution ani `connect_geyser()` aktywnego
runtime'u.

Korekta nie zmienia frozen binary storage V1:

```text
PumpResearchRawRecordV1
PumpRawSegmentHeaderV1
PumpRawSegmentClosedV1
codec framing / bincode options / record limit
```

Golden raw fixture i descriptor fixture pozostały niezmienione. Dodane pola
lifecycle znajdują się wyłącznie w JSON `run_completion_receipt`; mają
`#[serde(default)]`, więc starszy receipt nadal daje się odczytać. Nie są one
polem framed binary recordu ani zmianą jego layoutu.

## D1. Complete wymaga rzeczywistego source liveness

`YellowstoneConnector::run()` w trybie research nie ignoruje już wyniku
`JoinHandle`. Nadzór fail-closed odrzuca:

- `JoinError`, w tym panikę workera;
- `Err` workera;
- clean exit workera przed jawnym shutdownem.

Zachowanie istniejącego, aktywnego connectora pozostaje best-effort: rygor
dotyczy wyłącznie profilu z zainstalowanym research sinkiem.

`PumpResearchSourceConnectionV1` zapisuje trwały stan source lifecycle:

```text
stream_established
received_source_update_count
admitted_source_update_count
source_workers_cleanly_stopped
source_worker_error
capture_failure
```

`Complete` wymaga teraz jednocześnie:

1. sukcesu source i writera;
2. przynajmniej jednego faktycznie zestawionego streamu;
3. przynajmniej jednego odebranego i admitted source update;
4. przynajmniej jednego zapisanego primary raw recordu oraz segmentu;
5. czystego zakończenia wszystkich source workerów;
6. zgodnej liczby dropped source updates i missing events zapisanych w typed
   ingress gaps;
7. completion ProgramData receipt oraz braku ProgramData boundary;
8. zwykłych warunków `required_for_run` dotyczących gapów.

Dlatego lifecycle bez zestawionego streamu, pusty stream, panic workera albo
brak zapisanego source material nie mogą już dać `Complete`. Test lokalny
sprawdza ten predykat completion na lifecycle bez streamu/rekordów; nie jest
symulacją sygnału OS `Ctrl-C` ani pełnego transport shutdownu.

## D2. Błąd writera i shutdown source

Writer działa w pojedynczym wątku z `catch_unwind`. Jego błąd lub panika:

1. zapisuje błąd w summary;
2. ustawia atomowy fail-closed reason;
3. na własnej control-plane ścieżce anuluje `CancellationToken` source;
4. zatrzymuje `PumpResearchSourceConnectionV1` przed drainem i receiptami.

Receive ingress nie czeka na writer. Zwykła ścieżka capture wykorzystuje tylko
atomiki i `try_send`; nie wykonuje mutexa, BLAKE3, `prost`, bincode ani I/O.
Również fatalny stan na receive path jest małym atomowym kodem, a nie
alokowanym komunikatem chronionym muteksem. W szczególności receive path nie
wywołuje `CancellationToken::cancel()`: token może wewnętrznie przejść
synchroniczne mutexy i obudzić waiterów. Writer sprawdza atomowy fatal reason
na swoim bounded pollu i wykonuje anulowanie dokładnie raz poza Yellowstone
receive taskiem. Błąd/panika writera nadal może anulować source bezpośrednio,
ponieważ dzieje się na writer/control-plane threadzie.

Dodano admission guard do shutdownu. Tylko shutdown czeka, aż wywołanie
`try_capture`, które już uzyskało rezerwację, wyemituje data outcome lub
ordered drop marker. Dopiero potem zamrażany jest `final_capture_sequence`.
Receive task nie wykonuje waitu. Eliminuje to wyścig, w którym terminalny writer
mógłby zamknąć segment przed markerem rezerwowanym równolegle z shutdownem.

## D3. Typed gaps bez cichego overflow

Gap construction przeniesiono całkowicie na writer. Receive ingress przy pełnej
data lane przekazuje wyłącznie mały, uporządkowany `DroppedSource` marker do
drugiej bounded lane. Writer scala outcomes po `capture_sequence`, buduje
`LocalCoverageGapV1` i utrwala jego frozen adapter `PumpRawCoverageGapV1`.

Control lane ma tę samą ograniczoną pojemność co data lane. Jeśli marker nie
może zostać przekazany, receive path zapisuje fatal reason, a writer wykonuje
anulowanie source na najbliższym bounded pollu. Run jest wtedy `Incomplete`,
także przy `required_for_run = false`. Finalizacja dodatkowo sprawdza równość:

```text
dropped_source_update_count
== persisted_ingress_gap_missing_event_count
```

Writer opróżnia completed gaps po każdym ordered source outcome. Mimo że
ogólny `LocalGapTracker` ma ograniczony bufor ukończonych gapów dla swoich
innych użytkowników, research writer nie akumuluje tych wpisów. Jawnie
fail-closed sprawdza też `completed_overflowed()`; przyszła regresja nie może
zmienić overflowu w cichą utratę.

Reason dla tej lane jest `IngressQueueSaturated`, ponieważ dotyczy hand-offu
bezpośrednio po source tapie, a nie downstream evidence sinka.

## D4. Manifest requestu i transport-control

Fingerprint manifestu nie jest już ręcznie utrzymywanym opisem części pól.
Stream i manifest używają tego samego konstruktora
`pump_research_subscribe_request_v1()`.

`subscribe_request_fingerprint_blake3_v1()` tworzy stabilną reprezentację
pełnego, konkretnego `SubscribeRequest`: wszystkie mapy są sortowane po
rzeczywistym kluczu i prost-encoded value, a wszystkie pozostałe pola (w tym
puste mapy, `accounts_data_slice`, `commitment` i `ping`) są objęte
fingerprintem. To nadal jest fingerprint logicznego, frozen-schema requestu;
nie jest deklaracją wire-byte identity gRPC.

Server-side `UpdateOneof::Ping` i `Pong` są jawnie traktowane jako
transport-control. Nie trafiają do raw writer, który zgodnie z zamrożonym enum
nie ma dla nich market-record wariantu.

## D5. Segment receipt i durability

Hash SHA-256 i BLAKE3 całego opublikowanego segmentu są aktualizowane
inkrementalnie przy headerze, każdym framed recordzie i footerze. Writer nie
wykonuje już `fs::read()` całego segmentu po publikacji, więc nie alokuje
jednorazowo `segment_max_bytes` RAM.

Po `rename(.partial → .bin)` receipt segmentu jest rejestrowany natychmiast,
przed następnym fallible `.partial` open i przed directory sync. Jeśli
późniejszy krok durability zawiedzie, run pozostaje `Incomplete`, ale widoczny
`.bin` nie znika z completion receipt. Dotyczy to zarówno rotacji, jak i
końcowego zamknięcia.

`run_start_manifest.json` oraz `run_completion_receipt.json` wykonują teraz
`sync_all()` pliku oraz sync katalogu nadrzędnego po `create_new`.

## D6. Weryfikacja

Wykonano po korekcie:

```text
cargo fmt --all -- --check
cargo check -p seer --lib
cargo check -p seer --bin pump-research-tape
cargo test -p seer research_tape --lib --no-fail-fast
cargo test -p seer grpc_connection::tests --lib --no-fail-fast
cargo test -p ghost-core pump_research_tape --lib --no-fail-fast
cargo test -p seer --test pump_research_tape_cs0 -- --nocapture
cargo test -p seer --bin pump-research-tape -- --nocapture
cargo test --release -p seer --lib pr1b_hot_path_harness -- --ignored --nocapture --test-threads=1
git diff --check
git diff --cached --check
```

Ponieważ część plików PR-A pozostaje untracked w istniejącym dirty worktree,
`git diff --no-index --check /dev/null <path>` wykonano osobno dla każdego
nowego pliku objętego PR-A; dwa zwykłe diff-checki nie stanowią dowodu dla
untracked files.

Wyniki:

```text
research_tape unit tests                         PASS (19)
grpc_connection tests                            PASS (95)
frozen ghost-core Pump Research contracts        PASS (10)
CS0 deterministic source tests                   PASS (2)
standalone capture CLI test                      PASS (1)
```

Nowe regresje obejmują co najmniej:

- panic workera i early clean exit przed shutdownem;
- zero-record / unestablished source nie może być `Complete`;
- anulowanie source po błędzie open writera;
- full control lane: receive zapisuje fatal reason bez synchronicznego cancel,
  a writer/control plane anuluje source dokładnie raz;
- writer bounded poll rzeczywiście odbiera pending receive fatal i dopiero
  wtedy anuluje token source;
- >1024 oddzielnych saturation episodes bez cichego overflowu;
- final source sequence nie wyprzedza in-flight admission;
- direct-ingress saturated burst z p99 ingress poniżej 100 ms;
- fingerprint każdego top-level pola rzeczywistego requestu oraz invariance
  względem kolejności `HashMap`;
- filtr server-side Ping;
- rotację połączoną z błędem następnego open;
- inkrementalny SHA-256/BLAKE3 równy bytes opublikowanego segmentu.

Test direct-ingress nie jest pełną bramką capture-enabled A/B z planu
(`throughput ratio`, `p99 latency ratio`, parser-worker blocking waits), ani
proofem rzeczywistego provider qualification. Nie uruchomiono Yellowstone/RPC
capture, nie użyto credentialu i nie powstał dataset. Ostrzeżenia kompilatora
pochodzą z istniejących, niepowiązanych części dirty worktree i nie zostały
wyciszone tą korektą.

## D7. Granica i rollback

Poprawiona ścieżka nadal jest standalone research-only. W razie kolejnego
błędu należy zatrzymać `pump-research-tape capture`, zachować istniejące
immutable `.bin`, `.partial`, manifesty i receipts, a następnie wykonać nową
korektę. Nie wolno nadpisywać artefaktów, wstawiać danych RPC do raw tape,
łączać po curve+slot ani łagodzić statusu `Incomplete`.

Następnym etapem jest najpierw lokalna bramka capture-enabled A/B dla
standalone `PumpResearchCaptureIngressV1` i writera. Musi mierzyć progi z
planu oraz opóźnienie `fatal_reason_recorded → source_cancel_dispatched` przy
wymuszonym wolnym flush/sync/rotacji. `WRITER_IDLE_POLL_V1` ogranicza tylko
bezczynny poll writera; nie jest twardą gwarancją tego opóźnienia podczas
filesystem I/O.

Dopiero po przejściu tej bramki i osobnej decyzji operatora wolno uruchomić
observe-only prospective raw capture na jawnie dostarczonych endpointach i
credentialach. Inspekcja immutable manifestu, segmentów i completion receipt
poprzedza PR-B. PR-B nie jest rozpoczęty.
