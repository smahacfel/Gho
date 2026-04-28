# Plan Naprawczy — Seer / gRPC / Gatekeeper / Shadow-Run

Ten plan obejmuje wyłącznie 4 wskazane kierunki prac:
1. naprawa `success/error` propagation z gRPC do `TradeEvent`,
2. usunięcie reliktu MPCF z runtime i typów wejściowych,
3. wprowadzenie kanonicznego timestampu z gRPC do `GeyserEvent`,
4. naprawa race `POOL_TASK_BUY_NO_METADATA`, który blokuje wejście do `shadow_only`.

Nie dotyka:
- PR-7 compare-only commit path,
- execution lane semantics `live/paper/dual`,
- transportu poza niezbędnym zakresem timestamp/error propagation,
- log pressure poza niezbędnym doprecyzowaniem błędów.

## Cel globalny

Po wdrożeniu planu:
- failed tx z gRPC nie są już traktowane jak landed tx,
- parser nie niesie ani nie udaje MPCF payloadu,
- downstream dostaje stabilny, kanoniczny czas zdarzenia z gRPC,
- BUY verdict z Gatekeepera nie przepada z powodu braku `NewPool` metadata,
- `shadow_only` ma realną szansę wejść do `simulateTransaction`, zamiast umierać przed Triggerem.

---

## Krok 1 — Naprawić parser `success/error` propagation

### Problem

W `parse_trades()` parser tworzy `TradeEvent` z twardo wbitym:
- `success: true`,
- `error_code: None`,

ignorując realny status z `GeyserEvent::Transaction`.

To powoduje, że:
- failed tx wyglądają jak landed tx,
- SnapshotEngine, Gatekeeper i dalsze routowanie dostają fałszywą semantykę sukcesu,
- retry / reject / filtered behavior downstream jest podejmowane na zatrutych danych.

### Pliki obowiązkowe

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/components/snapshot_listener.rs`
- opcjonalnie testy w `off-chain/components/seer/tests/*`

### Zakres implementacyjny

#### 1. Ustalić SSOT dla statusu transakcji

Źródłem prawdy ma być `GeyserEvent::Transaction { success, error_code, ... }`.

Parser nie może:
- nadpisywać sukcesu na `true`,
- zerować `error_code`,
- zgadywać statusu po rodzaju eventu.

#### 2. Dodać helper do wyciągania statusu runtime

W `binary_parser.rs` dodać mały helper w stylu:
- `extract_runtime_trade_status(event) -> (bool, Option<String>)`

który czyta tylko z `GeyserEvent::Transaction`.

Wymagania:
- jeżeli event nie jest `Transaction`, zachować bezpieczny fallback,
- nie używać `unwrap`,
- zachować brak regresji dla źródeł nie-gRPC.

#### 3. Przepiąć wszystkie konstrukcje `TradeEvent`

W każdej gałęzi `ParsedEventKind::*` w `parse_trades()`:
- podmienić `success: true` na realny status,
- podmienić `error_code: None` na realny kod błędu.

Minimalnie dotyczy:
- `ParsedEventKind::Trade`
- `ParsedEventKind::CpiTrade`
- `ParsedEventKind::SwapTrade`
- `ParsedEventKind::CpiSwapBuy`
- oraz pozostałych generatorów `TradeEvent` w tym module

#### 4. Nie zepsuć ścieżek syntetycznych i fallbackowych

Jeżeli gdzieś tworzony jest sztuczny `GeyserEvent::Transaction` do testów / bootstrapu:
- trzeba jawnie ustawić `success` i `error_code`,
- nie wolno zostawić cichego defaultu, który przypadkiem maskuje regresję.

#### 5. Utrzymać downstream bez zmiany kontraktu typów

Na tym kroku nie trzeba zmieniać samego shape `TradeEvent`.
Wystarczy przywrócić prawdziwe wartości istniejących pól.

### Testy obowiązkowe

- `parse_trades_preserves_success_and_error_code_from_geyser_transaction`
- `parse_trades_marks_failed_cpi_trade_as_failed`
- `snapshot_listener_keeps_failed_trade_status_from_trade_event`

### Merge gate

Nie wolno merge’ować, jeżeli:
- w `parse_trades()` pozostaje choć jedna gałąź z `success: true` wbitym na sztywno,
- `error_code` dalej ginie przy mapowaniu do `TradeEvent`,
- failed gRPC tx dalej mogą wejść downstream jako landed.

---

## Krok 2 — Wypierdolić MPCF relikt

### Problem

MPCF payload jest dziś:
- częściowo transportowany,
- częściowo udawany jako „ProviderDoesNotSupport”,
- praktycznie nieużyteczny dla obecnego celu,
- architektonicznym reliktem, który zaciemnia telemetry i rozlewa puste dane po typach.

Wymaganie jest jednoznaczne:
- nie naprawiać przekazywania `mpcf_payload_bytes`,
- tylko usunąć ten koncept z aktywnej ścieżki runtime.

### Zakres decyzji architektonicznej

To jest zmiana cross-crate.
Najbezpieczniejszy wariant to usunięcie w 2 fazach, ale w jednym PR:

1. najpierw odciąć użycie runtime,
2. potem usunąć pola i telemetry, które już nie mają konsumentów.

### Pliki obowiązkowe

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/ipc.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/components/snapshot_listener.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-brain/src/oracle/engine.rs`

### Zakres implementacyjny

#### 1. Wyłączyć produkcję MPCF payloadu w gRPC

W `grpc_connection.rs`:
- przestać wkładać raw bytes do `mpcf_payload_bytes`,
- nie oznaczać „NotMissing”,
- zostawić jawny brak danych albo całkowicie usunąć pole z eventu.

#### 2. Wyciąć MPCF z parsera trade’ów

W `binary_parser.rs`:
- usunąć ustawianie `mpcf_payload`,
- usunąć `mpcf_payload_missing_reason`,
- nie emitować fałszywych powodów typu `ProviderDoesNotSupport`.

#### 3. Wyciąć MPCF z `TradeEvent` i `PoolTransaction`

W `seer::types` i eventach launcherowych:
- usunąć pola payloadu,
- usunąć pola reason,
- poprawić serde/test fixtures,
- poprawić IPC encoding/decoding.

#### 4. Wyciąć MPCF z SnapshotListener i OracleRuntime

W launcherze:
- usunąć przepinanie `pool_tx.mpcf_payload`,
- usunąć `register_pool_tx(..., mpcf_payload, ...)`,
- usunąć miejsca, które tylko tasują puste bajty.

#### 5. Wyciąć telemetry/metryki MPCF

W `ghost-brain`:
- usunąć metryki typu `mpcf_payload_present_total`,
- usunąć reason telemetry dla missing payload,
- nie zostawiać martwych liczników po feature, którego już nie ma.

### Testy obowiązkowe

- `trade_event_no_longer_contains_mpcf_payload`
- `snapshot_listener_builds_tx_event_without_mpcf_fields`
- `grpc_transaction_event_no_longer_exposes_mpcf_payload_bytes`
- `oracle_runtime_register_pool_tx_no_longer_requires_mpcf_payload`

### Merge gate

Nie wolno merge’ować, jeżeli:
- jakiekolwiek aktywne ścieżki runtime dalej niosą `mpcf_payload` jako pusty `Vec`,
- istnieją telemetry reason o „braku MPCF”, mimo że feature został usunięty,
- parser dalej produkuje pola MPCF bez prawdziwego konsumenta.

---

## Krok 3 — Dodać kanoniczny timestamp z gRPC do `GeyserEvent`

### Problem

Live gRPC nie propaguje żadnego kanonicznego czasu do `GeyserEvent::Transaction`.
W rezultacie parser używa `arrival_time_ms()` obliczanego w chwili parsowania, a nie w chwili przyjęcia eventu.

Przy współbieżnym przetwarzaniu daje to:
- niestabilny ordering,
- sztuczny jitter,
- rozjazd `event time` vs `worker processing time`.

### Pliki obowiązkowe

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `ghost-launcher/src/components/seer.rs`
- opcjonalnie testy `seer`

### Zakres implementacyjny

#### 1. Ustalić semantykę czasu dla gRPC

Priorytet źródła czasu:
1. `block_time` -> `event_ts_ms` jeżeli dostępny,
2. w przeciwnym razie timestamp ingressu gRPC pobrany przy odbiorze eventu,
3. dopiero brak obu = jawny fallback.

Parser nie może samodzielnie tworzyć czasu „teraz”, jeżeli ingress już miał lepszą wartość.

#### 2. Dodać timestamp ingressu w `grpc_connection.rs`

Przy tworzeniu `GeyserEvent::Transaction`:
- pobrać czas odbioru raz,
- zapisać go jako `event_ts_ms`, jeśli brak `block_time`.

Nie wolno:
- robić tego później w parserze,
- liczyć czasu kilka razy w różnych workerach.

#### 3. Uprościć parser

W `binary_parser.rs`:
- użyć `event.event_ts_ms` jako SSOT dla `timestamp_ms`,
- `arrival_time_ms()` zachować tylko jako ostateczny fallback diagnostyczny,
- nie traktować chwili parsowania jako normalnego źródła czasu dla gRPC.

#### 4. Zachować zgodność źródeł innych niż gRPC

PumpPortal / IPC / fallback paths:
- mają dalej działać,
- ale semantyka powinna być jawna: skąd timestamp pochodzi.

### Testy obowiązkowe

- `grpc_transaction_sets_event_ts_ms_from_ingress_when_block_time_missing`
- `parse_trades_prefers_event_ts_ms_over_arrival_time`
- `event_order_does_not_depend_on_parser_worker_arrival`

### Merge gate

Nie wolno merge’ować, jeżeli:
- parser dalej używa chwili parsowania jako domyślnego czasu gRPC,
- `event_ts_ms` pozostaje `None` dla live gRPC mimo posiadania ingress timestamp,
- ordering może zależeć od worker scheduling zamiast czasu eventu.

---

## Krok 4 — Naprawić race `POOL_TASK_BUY_NO_METADATA`

### Problem

Gatekeeper potrafi dać `BUY`, ale runtime odcina execution, bo:
- `pool_data` jest `None`,
- czeka tylko 500 ms na `NewPool`,
- po timeout robi `BUY skipped`,
- nie dochodzi ani do Triggera, ani do `shadow_only`.

To jest realny blocker wejścia trade shadow-run.

### Pliki obowiązkowe

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/events.rs`
- opcjonalnie mały registry/helper wewnątrz runtime

### Zakres implementacyjny

#### 1. Przestać polegać wyłącznie na lokalnym `rx.recv()` przy BUY time

Obecny model:
- task poola czeka lokalnie na `PoolObservationMsg::NewPool`,
- jeśli przegra wyścig, BUY path nie ma metadata.

To trzeba zastąpić runtime-level źródłem prawdy.

#### 2. Dodać runtime cache / registry pełnego `DetectedPool`

W `OracleRuntime` wprowadzić mapę:
- `pool_id -> DetectedPool`

Zasady:
- wpis jest ustawiany natychmiast przy `NewPoolDetected`,
- BUY path może go odczytać synchronicznie,
- cache ma mieć cleanup razem z lifecycle poola.

#### 3. BUY path ma najpierw próbować hydracji z registry

Nowa kolejność:
1. jeżeli `pool_data` lokalne istnieje -> użyj,
2. jeżeli nie istnieje -> spróbuj odczytać z runtime registry,
3. dopiero potem krótki wait na late msg jako fallback,
4. dopiero po wyczerpaniu obu ścieżek loguj `BUY skipped`.

#### 4. Nie gubić minimalnych danych potrzebnych do Triggera

Do wejścia shadow-run potrzebne są co najmniej:
- `base_mint`,
- `quote_mint`,
- `creator/dev`,
- `initial_liquidity_sol`,
- `slot`,
- `timestamp_ms`.

Jeżeli registry ma komplet tych danych, BUY path nie może skipować tylko dlatego, że lokalna kopia taska jest pusta.

#### 5. Zachować obecne semantyki okna i logowania

Nie zmieniać:
- samego verdictu Gatekeepera,
- commit staging,
- event writer ordering,
- log pressure poza doprecyzowaniem reason.

### Testy obowiązkowe

- `buy_path_hydrates_pool_metadata_from_runtime_registry`
- `buy_path_does_not_skip_shadow_only_when_new_pool_arrived_earlier`
- `late_new_pool_still_rescues_buy_path`
- `buy_path_logs_skip_only_after_registry_and_wait_fallback_fail`

### Merge gate

Nie wolno merge’ować, jeżeli:
- verdict `BUY` nadal może zostać pominięty wyłącznie przez brak lokalnego `pool_data`,
- `shadow_only` nadal nie dostaje szansy uruchomienia mimo obecnego `DetectedPool` w runtime,
- `POOL_TASK_BUY_NO_METADATA` pozostaje częstym race path bez registry fallback.

---

## Proponowana kolejność wykonania

Najmniejsza sensowna kolejność:

1. **Krok 1** — success/error propagation
2. **Krok 3** — kanoniczny timestamp
3. **Krok 4** — naprawa race `POOL_TASK_BUY_NO_METADATA`
4. **Krok 2** — pełne usunięcie MPCF reliktu

Powód:
- kroki 1/3/4 naprawiają realną semantykę live ingress i execution path,
- krok 2 jest szerokim cleanupem cross-crate i najlepiej robić go, gdy wejście już niesie poprawne dane.

---

## Kryterium „gotowe do dalszej pracy nad shadow-run”

Ten plan uznaje się za wykonany dopiero wtedy, gdy jednocześnie:

1. `TradeEvent.success/error_code` odzwierciedlają prawdę z gRPC,
2. gRPC event wnosi kanoniczny czas i parser nie używa już chwili parsowania jako normalnej osi czasu,
3. BUY verdict nie przepada z powodu `pool_data missing`, jeśli runtime ma już `DetectedPool`,
4. MPCF relikt nie istnieje w aktywnej ścieżce runtime,
5. `shadow_only` może realnie dojść do `simulateTransaction`, zamiast umierać przed Triggerem.
