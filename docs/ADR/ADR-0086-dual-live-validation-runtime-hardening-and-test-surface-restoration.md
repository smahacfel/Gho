# ADR-0086: Dual live validation, runtime hardening, and test-surface restoration

**Date:** 2026-04-08
**Status:** Accepted
**Author:** Ghost Father

## Context

Sesja zaczęła się od zadania operacyjnego: przeprowadzić kontrolowany dual live run po kolejnych fixach związanych z budową, wysyłką i akceptacją Jito bundle, a następnie obserwować w czasie rzeczywistym:

- czy doszło do BUY
- czy BUY został faktycznie zaakceptowany przez Jito
- czy po poprawnym BUY uruchamia się SELL
- jakie logi, metryki, bundle UUID, sygnatury i statusy towarzyszą każdej próbie

W praktyce live run szybko ujawnił, że problem nie jest pojedynczym bugiem w wysyłce bundle, tylko nakładającym się zbiorem niespójności architektonicznych:

1. jedno pole konfiguracyjne było używane jednocześnie jako auth do Jito gRPC i jako UUID do Jito REST status polling
2. po restarcie runtime nie umiał odtworzyć już otwartych pozycji z chaina, więc SELL lifecycle mógł nigdy nie wystartować mimo realnego stanu portfela
3. bootstrap nie miał twardej bariery hydration-before-trading
4. identyfikacja pozycji była zależna od runtime heurystyk zamiast deterministycznej funkcji
5. duża część testów, przykładów i rollout configów utrwalała już nieaktualny kontrakt

Efektem nie był jeden izolowany failure mode, ale cały łańcuch objawów:

- Jito gRPC mogło oddać ACK, podczas gdy REST status poll działał błędnie
- BUY mógł wyglądać na „submitted”, ale nie mieć wiarygodnego potwierdzenia landed
- SELL nie startował, bo system nie miał pewności, że BUY naprawdę istnieje
- bulkhead i recovery mogły rozjechać się semantycznie
- rollout profiles oraz testy mogły przechodzić mimo niezgodności z rzeczywistym kontraktem runtime

## Session chronology

### 1. Ustalenie właściwego lane dla dual live i przygotowanie configu

Na początku sesji zostało zweryfikowane, który config odpowiada za dual run. Następnie do `configs/rollout/dual-micro-live.toml` zostały przeniesione z lokalnego `config.toml` rzeczy potrzebne do realnego uruchomienia kontrolowanego lane:

- endpoint gRPC
- token x-token
- RPC endpoint Chainstack

Celem tego kroku nie było „uszlachetnienie” rollout profilu, tylko upewnienie się, że live run faktycznie testuje właściwą ścieżkę dual/live.

### 2. Kontrolowany dual live run i pierwsze twarde dowody runtime

Launcher został uruchomiony po preflight i obserwowany w czasie rzeczywistym. Zebrane dowody z tej części sesji były kluczowe, bo odróżniły problemy transportowe od problemów potwierdzania i lifecycle runtime.

Najważniejsze fakty:

- runtime wstał poprawnie: `LiveSellHandle`, `PostBuyRuntime`, `TriggerComponent` i `Oracle Runtime` uruchomiły się, watchdog przeszedł do stanu connected
- pojawiła się realna próba BUY
- dla tej próby zarejestrowano:
  - `mint=9wDPTtVNLNhE4JFtrCAAzMJptK9hshqx7ey7andJpump`
  - `amount=100000` lamports
  - `tip=5000000` lamports
  - `bundle_uuid=5566dd80432a5c371735354a5bcfbd97b33521cfe476f3d3750247c9645af286`
  - `signature=2uPVvVPky5H2ouSj3zPfhkPj3wq78kJwBAJz6MR6MN9M7iHYYy3qziWB5rZJ5jhNHDCLCasduub7VBxzCztzLTYj`

Jednocześnie zebrane logi pokazały, że:

- status polling po ACK nie działał poprawnie
- host Frankfurt zwracał plaintext/non-JSON na ścieżkach statusowych SDK
- dokładny probe `POST /api/v1/getInflightBundleStatuses?uuid=...` zwracał `400` przy niepoprawnym UUID
- `.env` zawierał `GHOST_TRIGGER_JITO_UUID`, które nie było poprawnym UUID v4, ale przechodziło starą, zbyt miękką walidację

To rozdzieliło dwa światy:

- gRPC submit mógł technicznie dojść do ACK
- status REST poll był logicznie źle skonfigurowany

On-chain truth dla tej próby był bezlitosny:

- `getSignatureStatuses(searchTransactionHistory=true)` zwróciło `[null, null]`
- BUY nie wylądował
- SELL nie miał prawa się uruchomić

Istotny pozytywny sygnał był jeden: po `UncertainLanding` runtime zachował slot bulkhead fail-closed, a kolejna próba BUY została odrzucona z `active_positions=1 max_concurrent_positions=1`. To oznaczało, że safety layer był bliżej prawidłowej semantyki niż confirmation path.

### 3. Przejście z diagnozy operacyjnej do przebudowy kontraktu runtime

Po zebraniu runtime evidence zadanie przestało być „naprawą jednego błędu”, a stało się krytycznym hardeningiem semantyki:

- split auth/status dla Jito
- fail-closed status UUID
- fail-closed probe
- hydration-before-trading
- deterministyczna tożsamość pozycji
- zgodność BUY i recovery

Od tego momentu implementacja była prowadzona już nie jako punktowy patch, ale jako świadoma migracja warstwy runtime i test-surface.

## Decision

W tej sesji przyjęto i wdrożono następujące decyzje architektoniczne.

### 1. Rozdzielenie Jito gRPC auth od Jito REST status UUID

Stary model, w którym `jito_uuid` pełnił dwie role naraz, został uznany za logicznie błędny.

Nowy kontrakt:

- `jito_grpc_auth` = opaque token/header dla gRPC submit
- `jito_status_uuid` = obowiązkowy UUID v4 dla REST status polling

Zasady:

- brak fallbacku ze starego `jito_uuid` do `jito_status_uuid`
- legacy `GHOST_TRIGGER_JITO_UUID` może być użyte tylko jako migracyjny fallback dla `jito_grpc_auth`
- status UUID jest fail-closed

### 2. Status UUID jest obowiązkowy i musi być UUID v4

Dodano twardą walidację:

- `jito_status_uuid` nie może być pusty dla live/live_and_shadow
- musi przejść parse jako UUID
- musi być wersji 4

To zlikwidowało sytuację, w której runtime akceptował wartość semantycznie błędną tylko dlatego, że nie wyglądała jak typowy placeholder.

### 3. Startup probe dla Jito status API stał się fail-closed

Probe przestał akceptować „coś przyszło z endpointu” jako sukces.

Nowy kontrakt probe:

- HTTP status musi być `200`
- odpowiedź musi być JSON
- plaintext, `400`, `500` lub odpowiedź spoza kontraktu powodują błąd startup/preflight

To usuwa klasę awarii, gdzie status API było błędnie skonfigurowane, a runtime przechodził dalej z wiecznym `pending` lub mylącą diagnozą.

### 4. BUY confirmation zostało odpięte od RPC jako źródła prawdy

Przyjęto semantykę:

- primary success: Jito `Accepted/Landed` na właściwej ścieżce statusowej
- fallback success: balance delta na portfelu
- RPC confirmation nie jest już krytyczną bramką sukcesu, tylko telemetry/debug signal

To ograniczyło wpływ RPC latency, historii i niestabilności obserwacji na decyzję BUY→SELL.

### 5. Startup hydration musi zakończyć się przed startem tradingu

Przyjęto twardą sekwencję bootu:

1. init config
2. validate/probe Jito
3. init event bus
4. start runtime subscriberów
5. wallet scan
6. synthetic recovery events
7. wait for runtime sync
8. dopiero potem trading / Oracle / Seer live path

Nie ma już akceptacji dla równoległego startu „seer already trading while hydration still catches up”.

### 6. Tożsamość pozycji została przeniesiona do deterministycznej funkcji safety layer

`PositionSlotId` nie może już powstawać z runtime heurystyk, losowości, timestampów albo lokalnych generatorów.

Nowy SSOT:

- `PositionSlotId::derive(owner, mint)`

Ta sama funkcja obowiązuje:

- w BUY flow
- w hydration/recovery
- w bulkhead safety

To zamyka klasę błędów recovery mismatch i duplikacji pozycji.

### 7. Recovery nie omija bulkhead semantics

Hydration i recovery zostały spięte z runtime tak, aby:

- nie obchodziły safety layer bocznym kanałem
- nie generowały alternatywnych slot IDs
- odtwarzały pozycje przez event contract i runtime registration

### 8. Repo-level testy i rollout profiles muszą odzwierciedlać runtime contract

Nie wystarczyło naprawić kod produkcyjny. Naprawiono również:

- testy
- doctesty
- examples
- rollout configi
- testy shipped profiles

Repo ma teraz bardziej spójny kontrakt między:

- runtime
- config loaderem
- preflight
- shipped rollout configami
- harnessami testowymi

## Detailed implementation record

## A. Live validation and forensic findings

W pierwszej części sesji wykonano realny runbook operacyjny:

- zweryfikowano aktywny dual config
- zsynchronizowano rollout config z lokalnym źródłem endpointów
- uruchomiono launcher po preflight
- śledzono:
  - system log
  - eventy runtime
  - status Jito
  - on-chain truth
  - stan portfela

Efekt tej części nie był „BUY succeeded”, tylko bardzo ważny zestaw dowodów:

- ACK z Jito nie jest równoważne landed
- SELL nie startuje bez wiarygodnego BUY confirmation
- bulkhead zachowuje fail-closed slot po uncertain landing
- invalid status UUID był realnym źródłem awarii status poll

To przesunęło środek ciężkości z obserwacji runtime na przebudowę kontraktu.

## B. Config and environment contract changes

W `ghost-launcher/src/config.rs` i powiązanych ścieżkach:

- rozdzielono `jito_grpc_auth` i `jito_status_uuid`
- zachowano alias `jito_uuid` tylko jako migracyjną kompatybilność do `jito_grpc_auth`
- dodano twardą walidację UUID v4
- zaktualizowano env mapping:
  - `GHOST_JITO_GRPC_AUTH`
  - `GHOST_JITO_STATUS_UUID`
- utrzymano tylko ograniczony fallback:
  - `GHOST_TRIGGER_JITO_UUID` -> `jito_grpc_auth`
- usunięto semantyczną możliwość użycia starego pola jako status UUID

To była najważniejsza zmiana kontraktowa całej sesji, bo naprawiała root cause, a nie wyłącznie objaw.

## C. Trigger/Jito transport hardening

W warstwie trigger/Jito:

- wzmocniono startup preflight i live transport validation
- status poll przestał traktować błędy API jako zwykły stan pending
- potwierdzono i utrwalono semantykę, w której REST status path jest częścią fail-closed confirmation path

Dodatkowo wcześniej i w toku tej sesji domknięto powiązane zmiany regionalne:

- status polling został powiązany z hostem, który oddał ACK, aby nie mieszać regionów block-engine podczas obserwacji bundle status

Razem te zmiany uczyniły status confirmation bardziej deterministycznym.

## D. Runtime hydration and recovery reconstruction

Dodano i zintegrowano startup hydration pipeline:

- `ghost-launcher/src/components/wallet_scanner.rs`
- `ghost-launcher/src/components/live_position_registry.rs`

Model działania:

1. scan wallet ownera
2. filtr non-zero token accounts
3. dopasowanie do live position registry
4. emisja synthetic `PostBuySubmitted { source: Recovery }`
5. runtime odtwarza aktywne pozycje i SELL lifecycle

To było krytyczne, bo wcześniejszy runtime żył niemal wyłącznie eventami live i po restarcie tracił zdolność odtworzenia prawdy on-chain.

## E. Deterministic position identity and bulkhead alignment

W safety layer wprowadzono nowy SSOT tożsamości pozycji:

- `PositionSlotId::derive(owner, mint)`

Najważniejsze konsekwencje:

- BUY i hydration liczą ten sam slot ID
- `ActivePositionLease` przestał być generatorem tożsamości
- bulkhead działa na tym samym kluczu, na którym działa recovery
- zniknęła dual-semantyka „BUY ma swój slot, recovery ma inny slot”

To był jeden z najważniejszych ruchów w całej sesji, bo przenosił identity model z runtime do czystej funkcji matematycznej.

## F. BUY→SELL confirmation semantics

W runtime BUY/SELL potwierdzanie zostało przestawione na:

- primary: Jito landed/accepted
- fallback: balance delta
- RPC: observability only

Była to korekta zarówno architektoniczna, jak i operacyjna. System przestał polegać na mniej deterministycznym RPC confirmation jako na jedynej bramce sukcesu.

## G. Real bug fixed in live handoff retention

Podczas migracji testów ujawniono realny bug w `ghost-launcher/src/oracle_runtime.rs`:

- kod logował, że przy potwierdzonym BUY i błędzie handoff slot ma zostać zachowany fail-closed
- w praktyce lease był zwalniany

Naprawa:

- dodano rzeczywiste `lease.retain()` w ścieżce błędu

To był nie tylko test-fix, ale realna poprawka semantyki safety w produkcyjnym runtime.

## H. Audit-driven follow-up fixes

Po implementacji głównych zmian przepracowano dodatkowy audyt i naprawiono wykryte problemy:

1. `wallet_scanner.rs`
   - poprawiono rozróżnienie `UiAccountEncoding::Base64` vs `LegacyBinary`
   - legacy payload jest dekodowany przez `bs58`
   - usunięto dead code i mylący failure mode dekodowania

2. hydration wait loop
   - warunek zmieniono z `==` na `>=`
   - usunięto race, w którym nowa pozycja mogła przeskoczyć licznik i wywołać timeout startu

3. recovery registration / runtime sync
   - poprawiono ścieżki tak, by recovery nie porzucał pozycji w sposób cichy

4. durability / startup consistency
   - domknięto luki ujawnione podczas audytu i re-runów testowych

## I. Large-scale test harness migration

Po zmianie kontraktu runtime bardzo duża część harnessu była już nieaktualna. W tej sesji wykonano szeroką migrację testów i przykładów.

### Zaktualizowane obszary

- `ghost-launcher/tests/post_buy_runtime_integration.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/examples/oracle_validation_comprehensive.rs`
- `ghost-launcher/examples/oracle_pipeline_diagnostic.rs`
- `ghost-launcher/examples/capital_preservation_demo.rs`
- `ghost-launcher/tests/oracle_logging_demo.rs`
- `ghost-launcher/tests/oracle_transaction_gathering.rs`
- `ghost-launcher/tests/refactor_invariants_tests.rs`
- `ghost-launcher/src/lib.rs`

### Zakres zmian w harnessie

- nowe pola konfiguracyjne `jito_grpc_auth` i `jito_status_uuid`
- typed `PositionSlotId`
- nowe sygnatury funkcji live exit / startup runtime
- nowy contract `PostBuySource`
- `lease.slot_id` i `lease.retain()` zamiast starego `into_slot_id()`
- preflight tests dostosowane do mandatory `jito_status_uuid`
- mock Jito server dostosowany do realnej ścieżki probe:
  - `POST /api/v1/getInflightBundleStatuses?uuid=...`
- example binaries dostosowane do nowej sygnatury `start_oracle_runtime_task(...)`
- demo po usunięciu `max_tip_ratio_percent`
- string-based invariant tests dostosowane do nowych nazw funkcji
- doctest `DetectedPool` dostosowany do wymaganego pola `semantic`

### Specjalny przypadek: legacy tests

Trzy testy w `oracle_transaction_gathering.rs`, które sprawdzały dawny observable legacy gather path, zostały oznaczone jako `#[ignore]` z jawnym uzasadnieniem, ponieważ aktualny runtime nie gwarantuje już tego kontraktu.

To nie było „zamiatanie pod dywan”, tylko formalne przyznanie, że testował stary model, którego system już nie implementuje.

## J. Rollout profile hardening

Shipped live rollout profiles przeszły dwie iteracje:

### Iteracja 1

Najpierw profile zostały doprowadzone do schema-valid stanu przez jawne użycie:

- `jito_grpc_auth`
- `jito_status_uuid`

oraz usunięcie reliance na `jito_uuid`.

### Iteracja 2

Następnie zostały celowo zaostrzone:

- `jito_grpc_auth = "replace-me"`
- `jito_status_uuid = "replace-me"`

plus komentarze operator-facing, że:

- bez `env/.env` te profile mają paść fail-closed
- live use wymaga sekretów środowiskowych

Równolegle zaostrzono test `ghost-launcher/src/main.rs`, aby sprawdzał:

- że rollout live config nie używa legacy `jito_uuid`
- że bez env override ładowanie pada
- że z env override ładowanie przechodzi

To usuwa wcześniejszy antywzorzec, gdzie test sam „naprawiał” config do temp file i przez to maskował realny drift repo.

## K. Local operator environment update

Pod sam koniec sesji:

- wygenerowano nowy UUID v4
- wpisano go do lokalnego, ignorowanego przez git `/root/Gho/.env` jako `GHOST_JITO_STATUS_UUID`
- potwierdzono, że wartość jest poprawnym UUID v4

Ważne:

- ta zmiana nie została wpisana do trackowanych rollout profile
- rzeczywista wartość nie powinna być traktowana jako repo-level default
- to jest lokalna konfiguracja runtime dla następnych preflight/live runów

## Files materially affected

Najważniejsze pliki zmodyfikowane lub dodane w tej sesji:

- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/wallet_scanner.rs`
- `ghost-launcher/src/components/live_position_registry.rs`
- `ghost-launcher/tests/post_buy_runtime_integration.rs`
- `ghost-launcher/tests/oracle_logging_demo.rs`
- `ghost-launcher/tests/oracle_transaction_gathering.rs`
- `ghost-launcher/tests/refactor_invariants_tests.rs`
- `ghost-launcher/examples/oracle_validation_comprehensive.rs`
- `ghost-launcher/examples/oracle_pipeline_diagnostic.rs`
- `ghost-launcher/examples/capital_preservation_demo.rs`
- `ghost-launcher/src/lib.rs`
- `configs/rollout/dual-micro-live.toml`
- `configs/rollout/future-live.toml`
- lokalny nieśledzony `/root/Gho/.env`

## Architectural impact

Ta sesja nie skończyła się na naprawie jednego błędu. Zmieniła kilka fundamentalnych własności systemu:

1. **Kontrakt Jito został rozdzielony semantycznie**
   - auth i status nie współdzielą już jednego pola

2. **Startup stał się bardziej fail-closed**
   - invalid/missing status UUID
   - non-JSON status API
   - rollout live config bez env override

3. **Recovery zostało zbliżone do prawdy on-chain**
   - wallet scan + registry + synthetic event pipeline

4. **Tożsamość pozycji została zdeterministyczniona**
   - BUY, hydration i bulkhead używają tej samej funkcji

5. **Test-surface znowu opisuje aktualny runtime**
   - mniej rozjazdu między kodem, configiem i testami

## Risk assessment

**Risk:** Medium

Ryzyko zostało istotnie obniżone względem stanu początkowego, ale nie spadło do zera.

### Ryzyka zredukowane

- użycie nieprawidłowego Jito status UUID
- ACK vs wrong-region/wrong-contract status polling confusion
- recovery mismatch dla pozycji po restarcie
- unlock bulkhead przez dual-semantykę slot ID
- testy przechodzące mimo repo-level drift configów

### Ryzyka nadal istniejące

- operacyjna poprawność prawdziwego `GHOST_JITO_GRPC_AUTH`
- rzeczywista dostępność i jakość odpowiedzi z Jito status API w środowisku live
- obecność legacy `GHOST_TRIGGER_JITO_UUID` w lokalnych środowiskach operatorów
- możliwe dalsze driftujące testy/examples w mniej uczęszczanych ścieżkach workspace

## Consequences

### Positive

- runtime ma znacznie czytelniejszy kontrakt Jito
- BUY→SELL lifecycle jest mniej zależny od niedeterministycznego RPC
- recovery po restarcie jest architektonicznie bardziej poprawne
- bulkhead zachowuje spójność z recovery
- shipped live rollout configi nie udają już „gotowe do live” bez sekretów
- `ghost-launcher` odzyskał spójność między runtime, testami, preflightem i rolloutami

### Negative

- wzrosła liczba explicit checks i contract boundaries, więc konfiguracja live jest bardziej rygorystyczna
- kilka harnessów testowych musiało zostać przepisanych zamiast lekko dopasowanych
- lokalne środowiska operatorów wymagają świadomej migracji z legacy `GHOST_TRIGGER_JITO_UUID` na nowy model

## Alternatives considered

### 1. Pozostawić stary model `jito_uuid` i tylko „lepiej go walidować`

Odrzucono, bo auth gRPC i status UUID są semantycznie różnymi bytami. Lepsza walidacja nie naprawiłaby błędnego modelu danych.

### 2. Dodać miękki fallback `status_uuid <- old jito_uuid`

Odrzucono, bo utrwalałoby to bombę migracyjną. System nadal mógłby ruszać na złej semantyce, tylko z mniejszą widocznością.

### 3. Naprawić wyłącznie live confirmation bez hydration/recovery redesign

Odrzucono, bo nawet poprawny BUY confirmation nie rozwiązuje problemu restartu runtime i martwych SELL lifecycle po cold start.

### 4. Zostawić rollout configi schema-valid i polegać na komentarzach

Odrzucono, bo operator-facing config bez fail-closed placeholdera tworzy fałszywe wrażenie gotowości do live.

### 5. Naprawić tylko kod produkcyjny i zostawić test drift na później

Odrzucono, bo tak duża zmiana kontraktu bez migracji harnessu oznaczałaby stały szum i brak wiarygodnego feedback loop.

## Validation performed during the session

### Runtime / live evidence

- wykonano kontrolowany dual live rerun
- zebrano bundle UUID, signature, logi runtime i on-chain status
- potwierdzono, że ACK bez landed nie wystarcza do SELL
- potwierdzono fail-closed bulkhead retention na uncertain landing

### Build / compile / tests

W toku sesji przechodziły kolejno:

- `cargo check -p ghost-launcher -p trigger`
- `cargo test -p ghost-launcher --lib`
- `cargo test -p ghost-launcher --bin ghost-launcher`
- `cargo test -p ghost-launcher --doc`
- finalnie pełne `cargo test -p ghost-launcher`

Po ostatnich zmianach dodatkowo potwierdzono:

- `cargo test -p ghost-launcher --lib --bin ghost-launcher`
- `cargo test -p ghost-launcher --bin ghost-launcher`

Końcowy stan dla `ghost-launcher` był green; w workspace pozostały jedynie niezwiązane warningi.

## Current state

Na koniec tej sesji stan systemu jest następujący:

1. **Runtime contract**
   - `jito_grpc_auth` i `jito_status_uuid` są rozdzielone
   - `jito_status_uuid` jest mandatory i walidowany jako UUID v4
   - probe Jito status API jest fail-closed
   - BUY confirmation używa Jito landed jako primary, balance delta jako fallback

2. **Recovery / startup**
   - działa wallet hydration przed tradingiem
   - recovery używa runtime/event contract
   - `PositionSlotId::derive(owner, mint)` jest wspólnym SSOT dla BUY i recovery

3. **Repo / harness**
   - testy, examples i doctesty zostały doprowadzone do zgodności z nowym kontraktem
   - rollout live profiles są jawnie fail-closed bez env override

4. **Local operator env**
   - lokalny `/root/Gho/.env` zawiera `GHOST_JITO_STATUS_UUID` z poprawnym UUID v4
   - rzeczywisty `GHOST_JITO_GRPC_AUTH` nadal powinien być traktowany jako operacyjnie wymagający świadomego ustawienia
   - legacy `GHOST_TRIGGER_JITO_UUID` pozostaje w lokalnym `.env`; powinien zostać usunięty lub zastąpiony przez `GHOST_JITO_GRPC_AUTH`, żeby nie utrwalać starej semantyki

## Proposed next steps

1. **Dokończyć migrację operator environment**
   - usunąć legacy `GHOST_TRIGGER_JITO_UUID` z lokalnych envów
   - ustawić jawne `GHOST_JITO_GRPC_AUTH`
   - zachować `GHOST_JITO_STATUS_UUID` jako oddzielny sekret

2. **Wykonać świeży preflight na docelowym dual profile**
   - potwierdzić:
     - poprawny Jito status probe
     - brak placeholderów
     - poprawny keypair path
     - poprawny endpoint binding

3. **Uruchomić kolejny kontrolowany dual live run**
   - obserwować BUY
   - potwierdzić Jito landed/balance delta
   - sprawdzić, czy SELL lifecycle rusza już bez rozjazdu

4. **Zebrać nową paczkę dowodów operacyjnych**
   - log systemowy
   - log oracle
   - event dataset
   - metryki
   - bundle UUID / signatures / landed slot

5. **Domknąć cleanup legacy config/test semantics**
   - usuwać pozostałe odwołania do `jito_uuid` tam, gdzie są już tylko compatibility-only
   - dalej redukować drift między runtime a mniej uczęszczanymi harnessami workspace

6. **Rozważyć kolejny ADR tylko dla operator migration**
   - jeśli środowiska live nadal utrzymują mieszany model `GHOST_TRIGGER_JITO_UUID` + nowe pola, warto osobno udokumentować finalny cutover plan dla env/secret management
