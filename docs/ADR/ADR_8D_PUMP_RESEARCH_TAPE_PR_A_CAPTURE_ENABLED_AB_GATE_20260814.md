# ADR-8D: Pump Research Evidence Tape V1.1 — lokalna bramka capture-enabled A/B PR-A

**Data:** 2026-08-14

**Status:** IMPLEMENTED / LOCAL GATE PASSED / RESEARCH-ONLY / PROSPECTIVE CAPTURE NOT STARTED / PR-B PENDING

**Task:** `PUMP_RESEARCH_EVIDENCE_TAPE_V1_1_PR_A_CAPTURE_ENABLED_AB_GATE`

## D0. Decyzja

Po poprawce failure paths PR-A uruchamiamy wymaganą przez plan lokalną bramkę
przed dopuszczeniem operator-approved prospective raw capture:

```text
capture-enabled local A/B
→ operator-approved observe-only prospective raw capture
→ inspection immutable raw output
→ PR-B
```

Brama wykonuje rzeczywisty `PumpResearchCaptureIngressV1`, bounded writer,
deterministyczny raw codec V1 i publikację segmentu w tymczasowym katalogu.
Nie łączy się z Yellowstone ani RPC, nie używa credentiali i nie tworzy
datasetu. Nie uruchamia certyfikacji, materializera, eksportera ani strategii.

Standalone capture nie uruchamia parser workerów: po zainstalowaniu research
source sinka źródło jest przeznaczone wyłącznie dla capture, a nie dla
aktywnej ścieżki Seera. Z tego powodu capture-disabled no-sink arm nie wykonuje
pracy równoważnej bounded hand-offowi capture. Nie wolno zatem ogłaszać ratio
względem no-op source arm jako throughput/p99 SLA ani syntetycznie uruchamiać
parser+writer równolegle, aby taki ratio wymusić. Oba warianty nie odpowiadają
rzeczywistemu standalone source path.

Zamiast tego bramka mierzy absolutną p99 rzeczywistego `try_capture` przy
działającym writerze oraz wymaga pełnej zgodności source/admission/writer
outcomes. Frozen parser parity pozostaje oddzielnym, obowiązkowym dowodem dla
capture-disabled aktywnej ścieżki; nie jest udawanym parser-worker pomiarem
standalone capture.

Nie zmieniono:

```text
active connect_geyser / active Seer runtime
Gatekeeper / MaterializedFeatureSet / AccountStateCore
Event Bus / canonical permit / execution
frozen V1 raw binary storage, config ani source profile
PR-B parser inventory, certifier, exporter i qualification audit
```

## D1. Kontrakt bramki

Harness jest jawnie ignorowanym testem release-mode, aby zwykłe testy jednostkowe
nie udawały pomiaru wydajności ani nie wykonywały filesystem I/O. Poprawne
wywołanie jest następujące:

```bash
cargo test --release -p seer --lib \
  research_tape::tests::pr_a_capture_enabled_local_ab_harness \
  -- --ignored --nocapture --test-threads=1
```

Test przekazuje 8 192 prawidłowych decoded Pump Buy `SubscribeUpdate` przy
`queue_capacity = 16 384`. W obu arms powstaje ten sam typ decoded source
payloadu, ale ich semantyka jest jawnie różna:

```text
disabled = owned source payload osiąga pre-capture no-sink control
enabled  = actual bounded ingress + writer + deterministic raw V1 segment
```

Disabled arm jest telemetrycznym punktem odniesienia, nie odpowiednikiem pracy
capture. Nie ma więc normatywnego ratio między arms.

Brama failuje, jeśli wystąpi choć jedno z poniższych:

- source/admission/accepted record counts nie są równe 8 192;
- powstanie dropped update, typed ingress gap lub writer error;
- writer nie zamknie się czysto albo nie opublikuje dokładnie jednego segmentu;
- source abort zostanie nieoczekiwanie anulowany;
- enabled source-side `try_capture` p99 przekracza `100 µs`.

Brama nie zapisuje fikcyjnego pola `parser_worker_blocking_waits = 0`.
W standalone capture taki worker nie istnieje. Zamiast tego świadomie rozdziela:

- frozen capture-disabled parser parity;
- structuralną granicę source receive: `try_send`/atomiki bez disk I/O;
- rzeczywisty A/B timing ingressu i writera;
- odrębny istniejący PR1B harness aktywnej ścieżki Seera.

Limit `100 µs` jest bezpośrednią bramką przeciw blockingowi receive hand-offu,
a nie poluzowaniem dawnego ratio. Zaobserwowane p99 są setki razy niższe; limit
zostawia miejsce na zwykłą wariancję schedulera, ale failuje przy locku, I/O
lub innych milisekundowych waitach na source path.

## D2. Fatal reason podczas wolnego I/O

Ta sama bramka wykonuje osobny test control-plane. Test-only probe,
kompilowany wyłącznie przy `cfg(test)`, wstrzymuje writer na 50 ms bezpośrednio
przed `flush/sync` podczas rotacji segmentu. W tym oknie receive-side zapisuje
atomowy `DropControlLaneSaturated` przez `record_fatal_capture_error()`.

Test mierzy:

```text
fatal_reason_recorded → source_cancel_dispatched
```

oraz wymaga, aby writer/control plane ostatecznie anulował source i aby
syntetyczny run zakończył się `Incomplete`. Nie umieszcza mutexa, sleepa ani
dodatkowej gałęzi w production buildzie: probe i hook nie istnieją poza
`cfg(test)`.

`WRITER_IDLE_POLL_V1 = 5 ms` pozostaje jedynie ograniczeniem pollingu, gdy
writer jest bezczynny. Nie jest SLA maksymalnego czasu anulowania w trakcie
blokującego flush/sync/rotacji; wynik testu wolnego I/O ma właśnie tę granicę
udokumentować, a nie ją ukrywać.

## D3. Receipt lokalnego wykonania

Pierwszy wariant harnessu próbował mierzyć ratio przez syntetyczne równoległe
uruchomienie parsera i writera. Powtarzalne wyniki throughput `0.9603`,
`0.9778` i `0.9664` poprawnie go odrzuciły: mierzył konkurencję CPU ścieżki,
której standalone capture nigdy nie uruchamia. Nie został zapisany jako
zaliczona bramka i nie obniżono progu.

Skorygowana, source-path bramka przeszła cztery niezależne release executions
z następującym raportem:

```text
events                                        = 8,192
accepted/admitted/received                    = 8,192 / 8,192 / 8,192
dropped updates / persisted ingress gaps      = 0 / 0
writer error / gap count / segment count      = none / 0 / 1
writer clean shutdown                         = true
enabled try_capture p99                        = 260 / 211 / 230 / 330 ns
source-ingress p99 limit                       = 100,000 ns
fatal_reason → source_cancel                  = 53,361,194 / 62,069,322 / 53,754,519 / 53,138,762 ns
injected slow rotation/sync delay             = 50,000,000 ns
```

Wartość fatal-to-cancel jest zgodna z celowo wymuszoną 50-ms pauzą i nie jest
porównywana z 5-ms idle pollem. Jest to lokalny receipt serii czterech
przebiegów; nie zastępuje qualification ani pomiaru u providera.

## D4. Weryfikacja i dalszy krok

Po implementacji bramki wykonano:

```text
cargo fmt --all -- --check
cargo check -p seer --lib
cargo check -p seer --bin pump-research-tape
cargo test -p seer research_tape --lib --no-fail-fast
cargo test --release -p seer --lib research_tape::tests::pr_a_capture_enabled_local_ab_harness -- --ignored --nocapture --test-threads=1
cargo test -p seer grpc_connection::tests --lib --no-fail-fast
cargo test -p seer pr1d_v1_v2_parser_digests_remain_frozen --lib -- --nocapture
cargo test -p ghost-core pump_research_tape --lib --no-fail-fast
cargo test -p seer --test pump_research_tape_cs0 -- --nocapture
cargo test -p seer --bin pump-research-tape -- --nocapture
```

Wyniki:

```text
format / seer lib+CLI checks                    PASS
research_tape unit tests                       PASS (19 passed, 1 ignored)
capture-enabled local A/B release harness      PASS (4 independent executions)
grpc_connection tests                          PASS (95 passed)
frozen parser digest                           PASS (1 passed)
ghost-core raw V1 contract tests               PASS (10 passed)
CS0 integration tests                          PASS (2 passed)
standalone CLI tests                           PASS (1 passed)
```

Następny krok nie jest automatycznym uruchomieniem sieciowym. Wymaga osobnej
decyzji operatora oraz jawnie przekazanych endpointów i credentiali:

```text
operator-approved observe-only prospective raw capture
→ inspect manifest / segments / completion receipt
→ dopiero PR-B
```

Nie wolno z lokalnego A/B wyprowadzać `PUMP_RESEARCH_TAPE_V1_READY`, source
completeness, canonicality, exact trajectory coverage ani dowodu realnego
provider filter coverage.

## D5. Rollback

Rollback tej bramki polega wyłącznie na niewywoływaniu ignorowanego harnessu
lub usunięciu test-only helpera w kolejnym, świadomym patchu. Nie ma wpływu na
aktywny runtime ani na format raw V1. W razie regresji należy zatrzymać
prospective capture przed pierwszym admitted recordem; nie wolno obniżać
absolutnego source-ingress p99, maskować gapów ani zamieniać pomiaru wolnego
I/O w deklarację twardego 5-ms deadline'u.
