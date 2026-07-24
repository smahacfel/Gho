# ADR-8D: Izolowany benchmark latency NLN gRPC/RPC dla Pump.fun — 2026-07-22

## Status

Accepted — diagnostic-only benchmark completed.

## D1. Problem

Przed tą zmianą checkout nie miał krótkiego, powtarzalnego i bezpiecznego testu, który rozdziela:

- transportową gotowość NLN Yellowstone gRPC;
- odbiór aktualizacji transakcji Pump.fun;
- czas pełnego odczytu HTTP JSON-RPC potrzebnego przed transakcją;
- świeżość `processed` slotu gRPC względem RPC;
- rzeczywiste ograniczenia rate-limit;
- metryki odczytu od niemierzonego latency Jito/inclusion.

Uruchomienie pełnego Ghosta nie jest właściwym narzędziem do tej odpowiedzi: mieszałoby gRPC z parserem, kolejkami, Event Busem, sesjami, Gatekeeperem oraz potencjalną ścieżką execution.

## D2. Decyzja

Dodano niezależny przykład Rust:

`off-chain/components/seer/examples/nln_latency_benchmark.rs`

Narzędzie:

- korzysta z tych samych zależności `tonic` i `yellowstone-grpc-*`, co Seer;
- wymaga sekretu wyłącznie przez `NLN_BENCHMARK_API_KEY`;
- używa bieżącego kontraktu NLN `x-api-key` dla `grpc.nln.clr3.org:443` i `rpc.nln.clr3.org`;
- subskrybuje wyłącznie `processed` transakcje z filtrem programu Pump.fun;
- mierzy monotonicznym `Instant` cold/warm transport, pierwszą wiadomość streamu oraz RPC reads;
- nie uruchamia Ghosta, nie modyfikuje configu, nie tworzy transakcji i nie wysyła bundle.

Wynik jednego zaakceptowanego przebiegu zapisano w:

`PLANS/AUDYT/RAPORT_BENCHMARK_NLN_GRPC_RPC_LATENCY_20260722.md`

## D3. Kontekst

Raw Yellowstone proto `1.14` w tym checkoutie ma dla aktualizacji transakcji slot i payload, ale nie provider-side transmit/ingest timestamp. Nie można z niego skonstruować uczciwej bezwzględnej miary `on-chain → local receiver` bez dodatkowego zsynchronizowanego markera.

NLN dokumentuje endpoint gRPC i `x-api-key`; aktualny config checkoutu również deklaruje `grpc_auth_header = "x-api-key"`. Benchmark używa jawnego interceptora nagłówka, analogicznego do aktywnego buildera Seera. Nie zmienia istniejącego klienta Seera ani probe runtime.

## D4. Dowody

Zaakceptowany przebieg z `2026-07-22T13:17:14.602Z` potwierdził:

- gRPC `Ping` warm: p50 `7.46 ms`, p95 `10.25 ms` (`n=7`);
- gRPC Pump.fun stream: first message `17.25 ms` po wysłaniu `Subscribe`, target `8/8` komunikatów osiągnięty;
- RPC `getSlot(processed)` warm: p50 `10.13 ms`, p95 `25.05 ms` (`n=6`);
- RPC `getLatestBlockhash(processed)` warm: p50 `10.08 ms`, p95 `13.92 ms` (`n=6`);
- `GetSlot(processed)` gRPC i RPC zwracały ten sam slot w `5/5` próbkach;
- gęsty, odrzucony preflight RPC zakończył się `-32005 Rate limited`, więc finalny przebieg był sekwencyjny i paced.

Walidacja kodu:

- `cargo fmt --all -- --check`;
- `cargo check -p seer --example nln_latency_benchmark`;
- `cargo build -p seer --example nln_latency_benchmark`;
- `git diff --check`.

## D5. Odrzucone alternatywy

### Pełny shadow/burnin Ghost

Odrzucono, ponieważ nie izoluje transportu i mógłby modyfikować artefakty runtime lub mieszać wynik z opóźnieniem parsera/Gatekeepera.

### Wyprowadzenie on-chain-to-client latency z block time

Odrzucono. Block time jest zbyt gruboziarnisty i ma inną semantykę niż emission Geysera; dałby pozornie dokładną, lecz niewiarygodną liczbę.

### Wysłanie testowej transakcji lub Jito bundle

Odrzucono, ponieważ użytkownik nie udzielił odrębnej zgody na zewnętrzny submit, a RPC response time nie jest dowodem landing/inclusion. Taki test wymaga osobnego kontraktu i niezależnego landed proof.

### Burst o dużej liczbie próbek

Odrzucono po obserwacji `-32005`. Benchmark latency nie może generować własnego throttlingu i uznawać go za provider latency.

## D6. Konsekwencje

Po tej zmianie operator ma krótki test diagnostyczny dla tej samej rodziny klienta i protokołu co Seer. Może on określić aktualny local-to-NLN transport/read RTT i wykryć rate-limit.

Nie dostarcza:

- SLO dla całej doby/regionu;
- end-to-end latency Ghosta;
- dowodu latency od wykonania transakcji przez walidator;
- Jito acknowledgment, forwarding, inclusion ani confirmation latency;
- zgody na live execution.

## D7. Inwarianty

Zachowane:

- brak zmian `MaterializedFeatureSet`, Gatekeepera, BUY/REJECT i selector score;
- brak zmian source-of-truth, event routing oraz aktywnego Seer runtime;
- brak zmian progów i configów;
- brak zmian TX/Jito/live/shadow execution;
- brak nowych sekretów w tracked files;
- brak surowych event payloadów, podpisów transakcji lub runtime artefaktów w raporcie;
- brak uruchomienia bota.

## D8. Bramka akceptacyjna i follow-up

Benchmark jest zaakceptowany jako krótka diagnoza transportu, gdy:

1. endpoint i wersja Geysera zostały zweryfikowane autoryzowanym wywołaniem;
2. cel streamu Pump.fun został osiągnięty bez timeoutu;
3. tabelaryczne wyniki pochodzą wyłącznie z przejścia bez rate-limit;
4. compile/format/diff checks przechodzą;
5. raport jasno oddziela read latency od Jito/inclusion latency.

Osobny benchmark Jito może zostać wykonany wyłącznie po jednoznacznej zgodzie na kontrolowany submit, wyborze endpointu Jito i zdefiniowaniu landed-slot evidence. Nie jest on kontynuacją automatyczną tego ADR.
