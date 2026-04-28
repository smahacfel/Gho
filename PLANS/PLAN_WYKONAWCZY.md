# KOMPLEKSOWY PLAN NAPRAWCZY KRYTYCZNYCH PROBLEMÓW:

## Plan został podzielony na 3 osobne ETAPY, które realizują różne miejsca i element pipeline'u produkcyjnego. 


# WSTĘP I ZASADY OGÓLNE

## 0. Cel planu
Ten dokument nie jest listą luźnych sugestii. To wykonawczy plan odzyskania kontraktów systemu i uporządkowania wejścia do runtime. Celem nie jest poprawienie pojedynczych metryk ani kosmetyczne wyciszenie objawów. Celem jest przywrócenie spójnego modelu prawdy od Seer do ShadowLedgera i przygotowanie gruntu pod usunięcie drugiego, architektonicznie szkodliwego Gatekeepera z `ghost-core`.

Problem systemowy, który ten plan adresuje, brzmi następująco: różne warstwy repo nie zgadzają się dziś, czym jest pojedyncze zdarzenie, kiedy zaszło, kiedy jest prawdziwe, kiedy jest zatwierdzone i kto jest właścicielem commit orchestration. Dlatego Faza I ma odzyskać correctness wejścia i ingestu zanim ruszy jakakolwiek większa migracja architektury.

### 1. Docelowy model architektury:

#### 1.1. Gatekeeper runtime ma być tylko jeden
Docelowo jedynym prawdziwym Gatekeeperem runtime ma być `ghost-launcher/src/components/gatekeeper.rs`. To ten komponent ma być SSOT dla:
- observation window,
- policy decyzyjnej,
- stanu `tracked / approved / committed`,
- buforowania pre-commit,
- triggerowania commitów,
- routingu tx między pre-commit i post-commit,
- współpracy z `OracleRuntime`.

#### 1.2. `ghost-core` nie może być drugim Gatekeeperem
`ghost-core/src/shadow_ledger/gatekeeper.rs` jest wyłącznie warstwą przejściową. W tym pliku wolno robić tylko stabilizujące bugfixy i izolację neutralnych typów. Nie wolno dopisywać tam nowej logiki biznesowej, runtime policy ani przenosić tam kolejnych odpowiedzialności launchera.

#### 1.3. `ghost-core` ma być czystą warstwą domain/storage
Po zakończeniu programu napraw w `ghost-core/shadow_ledger/*` mają zostać wyłącznie neutralne typy i zachowania storage/domain, w szczególności:
- `ShadowLedger`,
- `LivePipeline`,
- `TxKey`,
- `TradeSnapshot`,
- append/merge rules,
- domain math,
- neutralne struktury wynikowe, snapshotowe i replay helpers.

W `ghost-core` nie ma zostać runtime policy, observation semantics, approval state machine ani commit orchestration behavior.

### 2. Nadrzędne kontrakty, które wszystkie fazy muszą przywrócić:

#### 2.1. Event identity contract
- `1 signature != 1 trade`.
- Pojedynczy signature może legalnie zawierać wiele semantic trade events.
- Canonical identity eventu musi uwzględniać coś więcej niż signature, tj. `tx_index` lub `event_ordinal` oraz stabilny `TxKey`.

#### 2.2. Timestamp contract
- `timestamp_ms` = canonical event-time.
- `arrival_ts_ms` = telemetry / latency / fallback only.
- Arrival time nie może być primary ordering axis dla canonical path.

#### 2.3. Status contract
- `tracked != approved != committed`.
- `approved` nie jest canonical truth.
- `committed` oznacza wyłącznie realnie zapisaną canonical history potwierdzoną wynikiem operacji.

#### 2.4. Commit truthfulness contract
- System nie może zachowywać się jak po sukcesie, jeśli commit nie został zapisany.
- `pending_live` nie może żyć na fałszywym commicie.
- Bootstrap `LivePipeline` nie może zależeć od kruchego readbacku w ścieżce post-commit.

#### 2.5. Pre-commit ledger boundary contract
- `SnapshotEngine` pre-commit może utrzymywać wyłącznie lokalne / soft-truth state.
- Przed realnym canonical commitem nie wolno traktować write attempts `SnapshotEngine -> ShadowLedger` jako normalnej ścieżki działania systemu.
- Internal bootstrap/enrichment reads wymagane dla correctness nie mogą być blokowane przez approval-gated public snapshot access.

#### 2.6. Single authoritative ingest contract
- `SnapshotEngine` nie może mieć dwóch konkurencyjnych authoritative ingressów dla tego samego tx.
- Enrichment jest dozwolony.
- Konkurencyjny drugi writer path jest zabroniony.

#### 2.7. Mapping integrity contract
- `mint=111111...` nie może wyjść poza unresolved/staging path.
- Unresolved trade może być buforowany.
- Unresolved trade nie może być emitowany jako poprawna downstream trade identity.

### 3. Zasady prowadzenia prac:

#### 3.1. Kolejność prac jest nienegocjowalna
Kolejność zawsze musi być następująca:
1. correctness repair,
2. migration / extraction,
3. deletion / transport / performance.

Nie wolno odwracać tej kolejności.

#### 3.2. Nie wolno maskować problemów correctness transportem lub logami
Transport, reconnecty, gap recovery i log pressure są ważne, ale nie mogą przykrywać kontraktowych błędów wejścia, mappingu, dedupu i commitu. Dlatego w tej fazie nie wolno rozpraszać zakresu na tuning operacyjny.

#### 3.3. PR-y muszą mieć twarde granice semantyczne
Każdy PR w tej fazie ma mieć osobny, jednoznaczny sens. Nie wolno mieszać napraw parsera, mappingu, listenera i SnapshotEngine w jeden szeroki pakiet zmian.

#### 3.4. Extraction nie może utrwalać złej architektury
Neutralne typy to np. `TxKey`, `CommitResult`, `BufferedTx` bez runtime policy, `ReconstructedState`, typy snapshotowe i helpery replay. Nieneutralne typy to approval logic, observation windows, tracking policy, ownership `pending_live`, trigger commit rules, pre/post-commit routing i gatekeeper state machine. Te drugie nie mogą być utrwalane jako niby-neutralne.

---

## Stan bazowy repozytorium, na którym ten plan jest oparty:

1. `off-chain/components/seer/src/binary_parser.rs` zawiera własny blok `#[cfg(test)]` i już dziś ma testy związane z dedupem oraz unresolved cases.
2. `off-chain/components/seer/src/lib.rs` posiada aktywną logikę `pending_curve_updates`, `queue_curve_mint_resolve(...)` i testy w module `tests`.
3. `off-chain/components/seer/src/grpc_connection.rs` posiada zaimplementowaną strukturę `DelayedAccountQueue`, ale komentarze i rozmieszczenie kodu pokazują, że path recovery jest nadal tylko częściowo zintegrowany.
4. `ghost-launcher/src/components/snapshot_listener.rs` ma już tryb `SnapshotListenerForwardMode::TrackedBuffered` i warunki `known_pool` / `unknown_pool`, więc coverage loss da się naprawić lokalnie w listenerze.
5. `ghost-brain/src/oracle/snapshot_engine.rs` nadal używa dedupu opartego m.in. o signature (`seen_signature_ts`, `seen_signature_fifo`), mimo że plik zna już `TxKey`.
6. `ghost-launcher/tests/` zawiera testy integracyjne, które należy wykorzystać jako miejsce docelowej walidacji po zakończeniu zmian w Fazie I, szczególnie:
   - `snapshot_engine_integration.rs`,
   - `seer_shadow_ledger_bridge_tests.rs`,
   - `gatekeeper_v2_pipeline_integration.rs`.
7. **Znany problem bazowy walidacji:** bazowe uruchomienie `cargo test --workspace --quiet` w aktualnym repozytorium nie przechodzi z przyczyn niezwiązanych z tą dokumentacją: test `ghost-launcher/tests/event_bus_subscription_order.rs` nie inicjalizuje pola `detected_wall_ts_ms` w `DetectedPool`. To jest stan wejściowy, który wykonawcy muszą brać pod uwagę przy walidacji.

---

# REBORN — FAZA I — PLAN WYKONAWCZY

## Cel Fazy I:

Faza I ma odzyskać poprawność wejścia i ingestu zanim runtime Gatekeeper i commit path dostaną kolejne dane. Po zakończeniu tej fazy system ma przestać robić najgorsze rzeczy na wejściu:
- parser ma przestać kłamać o side/time/volume,
- unresolved mapping nie ma już wyciekać jako garbage identity,
- listener nie ma hard-dropować unknown pooli,
- `SnapshotEngine` nie ma mieć dwóch konkurencyjnych ingressów,
- recoverable tx nie mogą ginąć na engine-side gate `pool_not_active`,
- `SnapshotEngine` nie może już próbować pisać pre-commit snapshotów do `ShadowLedgera` jako normalnej ścieżki,
- signature-only collapse ma zostać wyeliminowany w krytycznych miejscach intake.

Kolejność PR-ów w Fazie I jest obowiązkowa:
1. PR-1,
2. PR-2,
3. PR-3a,
4. PR-3b.

---

## PR-1 — `seer-parser-contract`

### Twardy cel PR
Naprawić kontrakt producenta danych tak, aby `binary_parser` i ścieżka producer → launcher emitowały kanoniczny trade payload zgodny z rzeczywistą semantyką zdarzenia, a nie z heurystyką domyślną.

### Pliki obowiązkowe:
- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/types.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`

### Aktualny stan repo wymagający naprawy:
- W `binary_parser.rs` istnieją już testy na candidate dedup i unresolved cases, ale nie ma twardego kontraktu wymuszającego multi-event identity per signature.
- W `ghost-launcher/src/events.rs` struktury eventowe są już rozbudowane i nadają się do rozszerzenia o dodatkowy identyfikator eventu w obrębie transakcji.
- W `ghost-launcher/src/components/seer.rs` istnieje ścieżka emisji `DetectedPool` i forwardingu eventów z Seer do launchera; to jest właściwe miejsce na dopięcie spójnego payloadu transportowego.

### Dokładny zakres implementacyjny
#### 1. Named CPI side ma być autorytatywne
1. W `binary_parser.rs` znaleźć wszystkie ścieżki budujące `TradeEvent` / odpowiednik dla `CpiSwapBuy` i `CpiSwapSell`.
2. Usunąć możliwość, aby inference nadpisywało named semantic side zdarzenia.
3. Heurystyki inference mogą jedynie:
   - uzupełniać metadata,
   - pomagać w resolve kontekstu,
   - służyć jako fallback dla brakujących danych niezmieniających semantyki side.
4. Każde miejsce, które dziś może zamienić BUY na SELL albo odwrotnie, ma zostać usunięte albo zabezpieczone testem regresyjnym.

#### 2. `sol_amount` ma pochodzić z kanonicznego źródła:
1. Przejrzeć budowę canonical output w `binary_parser.rs`.
2. Każdy finalny trade output musi mieć `volume_sol` pochodzące z eventu kanonicznego albo z jawnie uzasadnionego runtime contextu.
3. Zabronić emisji finalnego canonical output z `volume_sol=0`, jeżeli parser rozpoznał realny swap.
4. Jeżeli danych nie da się ustalić bezpiecznie, event ma trafić do ścieżki unresolved/staging, a nie do canonical payloadu.

#### 3. `timestamp_ms` nie może być `arrival_ts`
1. W `binary_parser.rs`, `types.rs` i wszelkich strukturach pośrednich wyznaczyć jedno pole będące canonical event-time.
2. `arrival_ts_ms` ma pozostać wyłącznie telemetrią albo fallbackiem ostatniej szansy.
3. W `ghost-launcher/src/components/seer.rs` upewnić się, że event forwardowany do launchera nie gubi rozróżnienia `event-time` vs `arrival-time`.
4. W `ghost-launcher/src/events.rs` pola transportowe mają to rozróżnienie zachować wprost, a nie przez komentarz.

#### 4. Wprowadzić identyfikator eventu wewnątrz jednej transakcji:
1. Dodać do parser output i event payloadu `tx_index`, `event_ordinal` albo semantycznie równoważne pole.
2. Pole to ma być wymagane wszędzie tam, gdzie event posiada signature i może współistnieć z innymi eventami tej samej transakcji.
3. `signature` ma przestać być jedyną osią dedupu po stronie producer path.
4. Tożsamość eventu musi być możliwa do złożenia w downstream w postaci stabilnego `TxKey`.

#### 5. `mint=111111...` nie może wyjść jako prawidłowa tożsamość trade:
1. W `binary_parser.rs` zidentyfikować wszystkie ścieżki, które przypisują default/system mint jako zamiennik unresolved mappingu.
2. Zastąpić te ścieżki jawnie oznaczoną klasą unresolved / staging.
3. `ghost-launcher/src/components/seer.rs` nie może forwardować takiego eventu jako poprawnego trade identity.
4. `ghost-launcher/src/events.rs` musi rozróżniać canonical identity od unresolved identity.

### Czego nie wolno zrobić w tym PR:

- Nie ruszać `SnapshotListener`.
- Nie ruszać `SnapshotEngine`.
- Nie ruszać commit path.
- Nie wprowadzać jeszcze finalnego single-ingress contract.
- Nie łatać coverage loss downstream zmianami w transporcie.

### Testy obowiązkowe:

#### Gdzie dopisać testy
- Jednostkowe/regresyjne: `off-chain/components/seer/src/binary_parser.rs` (`mod tests`).
- Jeżeli wymagane przez nowe pola transportowe: lokalne testy w `ghost-launcher/src/events.rs` albo `ghost-launcher/src/components/seer.rs`.

#### Minimalny zestaw testów:
- `same_signature_multi_trade_survives_parser`
- `cpi_swap_contract_preserves_side_sol_time`
- `default_mint_never_forwarded`
- regresja na:
  - side flip,
  - zero-volume output,
  - phantom output,
  - `arrival_ts` używane jako canonical time.

### Walidacja techniczna po PR:

1. Uruchomić testy jednostkowe parsera.
2. Zweryfikować, że payload forwardowany do launchera niesie nowy identyfikator eventu i rozróżnienie czasu.
3. Przejrzeć wszystkie miejsca dedupu po signature w ścieżce producerowej i potwierdzić, że nie kasują legalnych drugich eventów.

### Merge gate:

Nie wolno merge’ować, jeżeli po PR:
- named CPI side da się dalej odwrócić inference,
- parser dalej emituje `arrival_ts` jako canonical time,
- downstream payload nie niesie `tx_index/event_ordinal` albo równoważnego pola,
- legalne multi-trade under same signature wciąż zapadają się w jedno,
- `mint=111111...` może opuścić producer path jako zwykła trade identity.

---

## PR-2 — `seer-mapping-rc6-replay`

### Twardy cel PR:

Domknąć kontrakt unresolved/mapping/replay po stronie Seer tak, aby poprawnie sparsowany trade nie był już później tracony lub zatruwany przez półmartwy replay path.

### Pliki obowiązkowe:
- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- opcjonalnie pomocnicze, małe helpery config/replay tylko wtedy, gdy wynikają bezpośrednio z powyższych zmian

### Aktualny stan repo wymagający naprawy:
- `lib.rs` posiada `pending_curve_updates`, `queue_curve_mint_resolve(...)` i replay helpers, więc podstawy mechanizmu istnieją, ale trzeba je domknąć kontraktowo.
- `grpc_connection.rs` posiada `DelayedAccountQueue`, a komentarze wskazują, że integracja hot path nadal nie jest pełna.
- Repo już dziś ma testy dla `DelayedAccountQueue`, co oznacza, że należy rozbudować istniejące testy zamiast budować alternatywną infrastrukturę.

### Dokładny zakres implementacyjny:

#### 1. `mint=111111...` zostaje wyłącznie w unresolved path:
1. W `lib.rs` zidentyfikować wszystkie ścieżki przyjmujące trade z unresolved mint/pool identity.
2. Każdy taki event ma zostać oznaczony jako unresolved i zapisany do bufora/replay path.
3. W żadnym miejscu `ipc` / downstream forwarding nie wolno emitować go jako zwykłego trade payloadu.
4. Wszelkie konwersje unresolved → resolved muszą być jawne i deterministyczne.

#### 2. Replay po mappingu ma być pojedynczy i deterministyczny:
1. `pending_curve_updates` ma zostać użyte jako jedyny prawdziwy stan replay dla tego rodzaju race condition, albo zastąpione jednym spójnym mechanizmem.
2. Po pojawieniu się mappingu replay ma wykonać się dokładnie raz.
3. Replay ma zachować pierwotną tożsamość eventu, w tym `tx_index/event_ordinal` albo pole równoważne.
4. Duplicate emission po overlapping arrivals ma zostać zablokowane testem.

#### 3. Delayed account recovery ma być realnym hot path:
1. Każdy account update przychodzący przed mappingiem/create ma być zachowany.
2. `DelayedAccountQueue` ma być wpięty w realny flow dispatchu, a nie tylko wystawiony przez API.
3. Po zamknięciu zależności account update ma zostać skorelowany z odpowiednim unresolved eventem i zreplayowany.
4. Jeżeli w obecnym kodzie są dwa konkurencyjne miejsca składowania takiego recovery, jedno z nich należy usunąć.

#### 4. `pending_curve_updates` nie może być półmartwym bokiem systemu:
1. Albo mechanizm jest jedynym ownerem replay dla curve→mint recovery,
2. albo należy go zastąpić jednym nowym stanem i przenieść wszystkie wywołania pod tę nową ścieżkę,
3. ale nie wolno zostawić architektury, w której kod wygląda na odporny na race, a realny hot path go omija.

### Czego nie wolno zrobić w tym PR:

- Nie ruszać jeszcze `SnapshotListener`.
- Nie usuwać double-ingress do `SnapshotEngine`.
- Nie dotykać runtime Gatekeepera.
- Nie dotykać commit loop.
- Nie rozwiązywać transportu i log pressure.

### Testy obowiązkowe:

#### Gdzie dopisać testy
- `off-chain/components/seer/src/lib.rs` (`mod tests`)
- `off-chain/components/seer/src/grpc_connection.rs` (`mod tests`)

#### Minimalny zestaw testów:
- `trade_before_create_replays_once`
- `account_update_before_mapping_replays`
- `mint_111_never_reaches_ipc`
- test na replay bez duplikacji przy overlapping arrivals
- test na zachowanie `tx_index/event_ordinal` po replay

### Walidacja techniczna po PR:

1. Potwierdzić, że unresolved trade nie opuszcza Seer jako resolved payload bez mappingu.
2. Potwierdzić, że account update arriving-before-create jest odtwarzany po zamknięciu mappingu.
3. Potwierdzić, że replay wykonuje się raz i bez duplikatów IPC.

### Merge gate:

Nie wolno merge’ować, jeżeli po PR:
- `mint=111111...` może nadal wyjść poza unresolved path,
- replay po mappingu dalej może produkować duble,
- account update before mapping dalej ginie,
- zachowanie event identity znika podczas replay.

---

## PR-3a — `snapshot-listener-buffering-and-mapping`

### Twardy cel PR:
Zatrzymać confirmed coverage loss przed `SnapshotEngine` przez usunięcie hard dropów dla `unknown_pool` i przekierowanie eventów bez tożsamości do odzyskiwalnej ścieżki stagingowej.

### Pliki obowiązkowe:
- `ghost-launcher/src/components/snapshot_listener.rs`

### Aktualny stan repo wymagający naprawy:
- Plik ma już udokumentowany tryb `TrackedBuffered`.
- Logika rozróżnia `known_pool` vs `unknown_pool`.
- Warunki filtrowania są skupione lokalnie, więc zmiana powinna pozostać chirurgiczna i ograniczona do listenera oraz minimalnych helperów zależnych bezpośrednio od tej ścieżki.

### Dokładny zakres implementacyjny:

#### 1. `unknown_pool` nie może być hard dropowany:
1. Znaleźć wszystkie miejsca, w których `known_pool == false` powoduje natychmiastowe odrzucenie eventu.
2. Zastąpić hard drop zachowaniem recoverable:
   - buffer,
   - staging,
   - unresolved queue,
   - albo inne lokalne miejsce odzyskiwania już istniejące w launcherze.
3. Event ma pozostać dostępny do późniejszego mappingu lub ponownej oceny.

#### 2. Brak identity nie oznacza braku wartości tx:
1. Listener ma rozróżniać stan „nie znamy jeszcze identity” od stanu „event jest bezwartościowy”.
2. Odciąć wszelkie warunki, które utożsamiają missing identity z permanent reject.
3. Jeżeli event nie spełnia wymogów finalnego downstream payloadu, ma zostać zbuforowany, a nie skasowany.

#### 3. Listener ma forwardować do ścieżki odzyskiwalnej
1. Zaimplementować jeden jawny recoverable path dla tego typu eventów.
2. Recoverable path musi zachowywać event identity i timestamp.
3. Listener nie może generować garbage identity po to, żeby „przepchnąć” event dalej.

#### 4. Default mint / unresolved identity nie mogą przejść jako finalne trade identity
1. W listenerze dodać twardy guard na identity unresolved/default.
2. Taki event ma trafić tylko do staging/recovery.
3. Żaden tryb forwardingu nie może przepuścić tego jako zwykłego finalnego trade.

### Czego nie wolno zrobić w tym PR
- Nie rozwiązywać jeszcze double-ingress.
- Nie przebudowywać `SnapshotEngine`.
- Nie mieszać coverage fixu z runtime Gatekeeperem.
- Nie dodawać tutaj tuningów transportowych.

### Testy obowiązkowe:

#### Gdzie dopisać testy
- `ghost-launcher/src/components/snapshot_listener.rs` (`mod tests`)
- Ewentualnie uzupełniająco integracja w `ghost-launcher/tests/snapshot_engine_integration.rs`, jeżeli potrzeba potwierdzić, że event dociera do dalszej ścieżki zamiast zniknąć.

#### Minimalny zestaw testów:
- `tracked_buffered_unknown_pool_is_buffered`
- `default_mint_never_forwarded`
- `tx_before_pool_identity_is_staged_not_dropped`

### Walidacja techniczna po PR:

1. Potwierdzić, że `TrackedBuffered` przy `unknown_pool` kończy się stagingiem, a nie dropem.
2. Potwierdzić, że event unresolved nie znika i zachowuje możliwość replayu/ponownej oceny.
3. Potwierdzić, że default mint nie zostaje przepuszczony do finalnego path.

### Merge gate:

Nie wolno merge’ować, jeżeli po PR:
- listener nadal hard dropuje `unknown_pool`,
- unresolved tx może zniknąć zamiast trafić do recoverable path,
- default mint może przejść listener jako normalna trade identity.

---

## PR-3b — `snapshot-engine-single-ingress-and-dedup-contract`

### Twardy cel PR:

Doprowadzić `SnapshotEngine` do jednego authoritative tx ingress i wyeliminować signature-only dedup, który kasuje legalne drugie zdarzenia tej samej transakcji.

### Pliki obowiązkowe:
- `ghost-brain/src/oracle/snapshot_engine.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- tylko minimalnie, jeśli konieczne do domknięcia authoritativeness: `ghost-launcher/src/components/snapshot_listener.rs`

### Aktualny stan repo wymagający naprawy:
- `snapshot_engine.rs` utrzymuje struktury `seen_signature_ts` oraz `seen_signature_fifo`, więc signature nadal jest osią krytycznego dedupu.
- Plik zna już `TxKey`, co oznacza, że naprawa powinna polegać na domknięciu istniejącego kierunku, a nie na projektowaniu od zera.
- `ghost-launcher/tests/snapshot_engine_integration.rs` istnieje i powinien zostać rozbudowany o walidację end-to-end dla multi-event tx.

### Dokładny zakres implementacyjny

#### 1. Wskazać jeden authoritative tx ingress do `SnapshotEngine`
1. W `oracle_runtime.rs` spisać wszystkie miejsca, które zapisują albo mogą zapisywać tx do engine.
2. Jedną ścieżkę oznaczyć jako authoritative writer.
3. Każdą pozostałą ścieżkę ograniczyć do enrichmentu metadata/reserve contextu bez prawa do konkurencyjnego write.
4. Jeżeli dziś dwie ścieżki robią ten sam write pod innymi warunkami, jedna musi zostać usunięta albo zdegradowana do enrichera.

#### 2. Usunąć signature-only dedup
1. Dedup w engine i inactive buffer ma zostać przełączony na event identity kompatybilne z `TxKey`.
2. `signature` może pozostać jako pomocniczy klucz telemetrii lub korelacji, ale nie jako jedyny klucz legalności tx.
3. Dwa różne eventy tej samej transakcji muszą współistnieć, jeżeli różni je `tx_index`, `event_ordinal` albo inny element canonical identity.

#### 3. Enriched path nie może przegrywać z poorer duplicate path
1. Jeżeli biedniejszy event przyjdzie pierwszy, nie może zablokować przyjęcia późniejszego, lepiej wzbogaconego odpowiednika tej samej tożsamości.
2. Dedup i enrichment muszą rozróżnić przypadek „ten sam event z lepszym kontekstem” od „prawdziwy dubel”.
3. Review ma wymagać jawnej tabeli przypadków: prawdziwy dubel, legalny drugi event, ten sam event wzbogacony później.

#### 4. `SnapshotEngine` nie może wykonywać normal-path pre-commit write attempts do `ShadowLedgera`
1. Przed realnym canonical commitem snapshoty `SnapshotEngine` mają pozostać lokalne / soft-truth i nie mogą być traktowane jako materiał do zapisu w `ShadowLedger`.
2. Należy usunąć albo odciąć wszystkie ścieżki, które w normalnym toku działania próbują pisać pre-commit snapshoty do `ShadowLedgera` dla niecommitted mintów.
3. Po commitcie `SnapshotEngine` może pozostać konsumentem dla sygnałów/in-memory state, ale nie może odzyskiwać prawa do konkurencyjnego write pathu do ledgera.
4. Review ma wymagać jawnego opisu, gdzie od tej pory żyje pre-commit state: lokalnie w engine albo w osobnym, wyraźnie niekanonicznym store.

#### 5. Gate `pool_not_active` ma być recoverable, a nie stratny
1. Wszystkie ścieżki `track_pool(...)`, auto-track i `buffer_inactive_tx(..., \"pool_not_active\")` mają zostać przejrzane jako jeden kontrakt.
2. Recoverable tx trafiający w `pool_not_active` może zostać zbuforowany, ale nie może zostać permanentnie utracony z powodu kolejności aktywacji/replay.
3. Replay po aktywacji musi zachować pełną event identity i ordering zgodne z `TxKey`.
4. Jeżeli aktywny tx powinien dać się samoleczyć przez track/replay, nie wolno zostawiać go w stanie chronicznego bufferingu lub starvation.

### Czego nie wolno zrobić w tym PR:

- Nie wracać do parsera.
- Nie ruszać RC6/replay w Seer.
- Nie dotykać commit semantics.
- Nie mieszać tego z migracją Gatekeepera.

### Testy obowiązkowe:

#### Gdzie dopisać testy
- `ghost-brain/src/oracle/snapshot_engine.rs` (`mod tests`)
- `ghost-launcher/tests/snapshot_engine_integration.rs`

#### Minimalny zestaw testów
- `snapshot_engine_does_not_drop_second_trade_same_signature`
- `single_ingress_wins`
- `enriched_tx_path_does_not_lose_to_poorer_duplicate_path`
- `snapshot_engine_precommit_does_not_write_shadow_ledger`
- `pool_not_active_buffers_and_replays_recoverably`

### Walidacja techniczna po PR:

1. Potwierdzić, że engine nie ma już dwóch konkurencyjnych writer pathów.
2. Potwierdzić, że dwa legalne eventy z tym samym signature przechodzą oba.
3. Potwierdzić, że enrichment nie przegrywa z first-arrival poverty path.
4. Potwierdzić, że `SnapshotEngine` nie wykonuje już normal-path pre-commit write attempts do `ShadowLedgera`.
5. Potwierdzić, że recoverable tx nie ginie na gate `pool_not_active`.

### Merge gate:

Nie wolno merge’ować, jeżeli po PR:
- engine ma dwa konkurencyjne writer pathy,
- legalny drugi trade z tym samym signature nadal może zostać skasowany,
- enriched path nadal może przegrywać z gorszym first-arrival eventem,
- `SnapshotEngine` nadal próbuje pisać pre-commit snapshoty do `ShadowLedgera` w normalnym toku działania,
- recoverable tx nadal może zginąć na `pool_not_active`.

---

## Warunki uznania Fazy I za zakończoną:

Faza I jest zakończona wyłącznie wtedy, gdy jednocześnie prawdziwe są wszystkie poniższe warunki:
1. Parser contract jest naprawiony:
   - named CPI side nie jest nadpisywane inference,
   - `timestamp_ms` nie jest `arrival_ts`,
   - zero-volume/phantom outputs znikają z canonical producer path.
2. `1 signature != 1 trade` jest realnie obsłużone:
   - parser niesie `tx_index/event_ordinal` albo równoważne pole,
   - intake i engine nie redukują legalnych multi-event tx do jednej jednostki.
3. `mint=111111...` nie wychodzi poza unresolved/staging path.
4. `SnapshotListener` nie hard-dropuje `unknown_pool`.
5. `SnapshotEngine` ma jeden authoritative tx ingress.
6. Recoverable tx nie ginie na engine-side gate `pool_not_active`.
7. `SnapshotEngine` nie wykonuje już normal-path pre-commit write attempts do `ShadowLedgera`.
8. Coverage counters jakościowo poprawiają się na ścieżce:
   - `seer_rx`,
   - `listener_forwarded`,
   - `snapshot_engine_accepted`.


# FAZA II — PLAN WYKONAWCZY

## WSTĘP I ZASADY OGÓLNE

### 0. Cel:
Ta część dokumentu definiuje wykonanie Fazy II jako etapu ustanawiającego poprawną semantykę runtime i prawdomówność commit path. To nie jest faza migracji architektury ani sprzątania reliktów. To faza, w której system ma przestać mieszać statusy, czas i wynik commitu.

Celem jest ustanowienie spójnego modelu prawdy między launcherowym Gatekeeperem, `OracleRuntime`, commit loop i warstwą `ghost-core`, tak aby dalsza ekstrakcja i usunięcie core gatekeepera były bezpieczne.

### 1. Docelowy model architektury

# REBORN — FAZA III — PLAN WYKONAWCZY

## WSTĘP I ZASADY OGÓLNE

### 0. Cel
Ta sekcja opisuje **zaktualizowaną** Fazę III po ukończeniu Fazy 4 z `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`, tj. po domknięciu durability: strict WAL ordering, restore i replay delta.

Faza III nie jest już miejscem na budowę fundamentów recovery. Te fundamenty są w aktualnym repo wdrożone i muszą być traktowane jako stan zastany. Celem Fazy III jest teraz:
- dokończenie architektonicznego odcięcia legacy/core gatekeepera od bieżącego runtime,
- domknięcie rolloutowego compare-only dla commit path,
- dopiero potem usunięcie `ghost-core/src/shadow_ledger/gatekeeper.rs`,
- a na samym końcu prace transportowe i log-pressure.

### 1. Docelowy model architektury

#### 1.1. Gatekeeper runtime ma być tylko jeden
Docelowym ownerem runtime semantics, commit orchestration i post-commit routingu pozostaje `ghost-launcher/src/components/gatekeeper.rs` wraz z `OracleRuntime` i `GatekeeperCommitLoop`.

#### 1.2. `ghost-core` nie może pozostać drugim Gatekeeperem
`ghost-core/src/shadow_ledger/gatekeeper.rs` ma zniknąć z produkcyjnej architektury, ale **nie jest dziś martwym plikiem**. Nadal:
- eksportuje `GatekeeperRegistry`, `GatekeeperMintBuffer`, `GatekeeperConfig` i `GatekeeperError`,
- jest re-exportowany przez `ghost-core/src/shadow_ledger/mod.rs`,
- dostarcza typy używane jeszcze przez config compatibility i testy legacy/equivalence.

Wniosek wykonawczy: pliku nie wolno traktować jako nieaktywnego śmiecia. Jest to **przejściowy relikt aktywny technicznie, ale nie będący już canonical runtime writerem**.

#### 1.3. `ghost-core` ma być czystą warstwą domain/storage
Po zakończeniu programu w `ghost-core` mają zostać wyłącznie:
- `ShadowLedger`,
- `LivePipeline`,
- neutralne typy historii/snapshotów/replay,
- storage arbitration i domain math,
- helpers recovery/domain bez runtime policy ownership.

### 2. Kontrakty, które w Fazie III muszą zostać domknięte

#### 2.1. Commit ownership contract
- Launcher jest ownerem commit triggera, commit window semantics i interpretacji wyniku commitu.
- Core nie może zachować runtime ownership pod postacią helpera, adaptera ani config compatibility pathu.

#### 2.2. Rollout truthfulness contract
- Test equivalence nie wystarcza jako rollout closure.
- PR-7 nie jest domknięty, dopóki repo nie ma operatorskiego compare-only runtime dla commit path oraz raportu divergence.

#### 2.3. Deletion safety contract
- `ghost-core/src/shadow_ledger/gatekeeper.rs` wolno usunąć dopiero po wykazaniu, że żadna ścieżka runtime nie zależy od jego zachowania.
- Usunięcie nie może naruszyć neutralnych typów ani testowalności domain/storage.

#### 2.4. Scope discipline contract
- Faza III nie wraca do Fazy 4.
- Faza III nie dokłada nowych writerów, nowych feature'ów ani nowej semantyki execution.
- PR-9 pozostaje ostatni.

### 3. Zasady prowadzenia prac

#### 3.1. Najpierw rollout closure, potem deletion
Po ukończonej Fazie 4 kolejność w Fazie III jest teraz następująca:
1. domknąć resztki extraction/compat cleanup,
2. dowieźć compare-only rollout dla commit path,
3. przełączyć ownership dopiero po czystym raporcie,
4. usunąć legacy/core gatekeeper,
5. dopiero wtedy ruszyć transport i log pressure.

#### 3.2. Nie wolno ponownie otwierać zamkniętego zakresu durability
Do Fazy III nie należą już:
- `ReplayOrderKey`,
- restore snapshotu,
- replay WAL od watermarka,
- odbudowa staged/pending state,
- recovery telemetry.

Te elementy są już częścią aktualnego repo i nie mogą wracać jako „zadania do zrobienia” w PR-7.

#### 3.3. PR-y muszą mieć twarde granice semantyczne
- PR-6 = domknięcie residual extraction / transitional cleanup,
- PR-7 = rollout closure launcher commit coordinatora,
- PR-8 = deletion and cleanup only,
- PR-9 = transport i log pressure po zamknięciu migracji.

#### 3.4. Nie wolno mylić dwóch shadow-runów
- execution-layer shadow-run BUY (`shadow_only`, `live_and_shadow`) jest osobnym, już istniejącym torem,
- compare-only commit path dla PR-7 to osobny rollout mechanizm i nadal brakujący element operacyjny.

---

## Stan bazowy repozytorium, na którym ten plan jest oparty

1. `ghost-launcher/src/components/gatekeeper.rs` zawiera `LauncherCommitCoordinator`, launcher-owned `pending_live` semantics oraz wynik `LauncherCommitOutcome` oparty o `CommitResult`.
2. `ghost-launcher/src/components/gatekeeper_commit_loop.rs` wykonuje `process_ready_commits(&shadow_ledger)`, inicjalizuje `LivePipeline` po wyniku commitu i forwarduje `pending_live` do `LivePipeline`.
3. `ghost-launcher/src/oracle_runtime.rs` posiada aktywne ścieżki `stage_history(...)`, `add_approved_tx(...)`, `RouteToLive` i routing committed tx bez pośrednictwa legacy core gatekeepera jako runtime ownera.
4. `ghost-launcher/src/main.rs` wykonuje restore snapshotu `ShadowLedger`, replay WAL i emituje recovery telemetry (`shadow_ledger_restore_duration_ms`, `wal_replay_duration_ms`, `runtime_recovery_mode`).
5. `ghost-launcher/src/wal_recovery.rs` odtwarza staged commit, committed history, pending live delta i rollback seeds, co oznacza, że durability/recovery foundation jest już wdrożone.
6. `ghost-core/src/wal.rs` definiuje `ReplayOrderKey`, więc ordering recovery-critical rekordów nie jest już planem przyszłym, tylko elementem bieżącego stanu kodu.
7. `ghost-core/src/shadow_ledger/gatekeeper.rs` **nie pełni dziś roli canonical runtime commit writer path**, ale nadal pełni aktywną funkcję przejściową przez:
   - re-exporty w `ghost-core/src/shadow_ledger/mod.rs`,
   - typy używane jeszcze w `ghost-launcher/src/config.rs` (`GatekeeperConfig` compatibility),
   - testy equivalence i legacy core tests używające `GatekeeperRegistry`.
8. `ghost-launcher/src/components/trigger/component.rs`, `ghost-launcher/src/components/trigger/shadow_run.rs` i `ghost-launcher/src/config.rs` mają już execution-layer shadow-run BUY (`shadow_only`, `live_and_shadow`, structured report, JSONL/event/metrics path). To nie jest już backlog Fazy III.
9. Repo ma test equivalence `shadow_run_equivalence_matches_legacy_gatekeeper`, ale **nie ma jeszcze operatorskiego compare-only runtime mode dla commit path** ani switch gate z raportem divergence.
10. Historyczna notatka o failure `event_bus_subscription_order.rs` z brakującym `detected_wall_ts_ms` nie jest już aktualnym faktem repo i nie może być dalej traktowana jako stan wejściowy tego planu.

---

## Cel Fazy III

Po Fazie I, II i ukończonej Fazie 4 z planu closure-mode system ma już:
- poprawniejszy ingest i identity contract,
- rozdzielone statusy runtime,
- prawdomówny commit result,
- deterministic startup restore i replay.

Faza III ma teraz domknąć **wyłącznie to, co pozostało otwarte**:
1. usunąć resztki aktywnej zależności od core gatekeepera,
2. dowieźć operatorski compare-only rollout dla commit path,
3. usunąć `ghost-core/src/shadow_ledger/gatekeeper.rs` bez naruszenia domain/storage,
4. dopiero potem zająć się PR-9.

Kolejność PR-ów w Fazie III jest obowiązkowa:
1. PR-6,
2. PR-7,
3. PR-8,
4. PR-9.

---

## PR-6 — `extract-neutral-core-types`

### Twardy cel PR
Domknąć residual extraction i compatibility cleanup po wcześniejszych przenosinach neutralnych typów, tak aby `ghost-core/src/shadow_ledger/gatekeeper.rs` został zredukowany do minimalnego, jawnie przejściowego reliktu.

### Stan wejściowy
Ten PR nie jest już greenfield extraction. Część neutralnych typów została już wydzielona do:
- `commit_types.rs`,
- `history_types.rs`,
- `trade_types.rs`,
- pozostałych modułów `shadow_ledger/*`.

Pozostały zakres PR-6 dotyczy więc **niedomkniętych zależności i eksportów**, nie ponownego projektowania tej warstwy.

### Pliki obowiązkowe
- `ghost-core/src/shadow_ledger/gatekeeper.rs`
- `ghost-core/src/shadow_ledger/mod.rs`
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- ewentualnie inne pliki `shadow_ledger/*` wyłącznie wtedy, gdy zamykają residual re-export/import graph

### Dokładny zakres implementacyjny

#### 1. Zidentyfikować, co jeszcze niepotrzebnie żyje pod `gatekeeper.rs`
Do weryfikacji kwalifikują się wyłącznie elementy przejściowe nadal trzymane przez stary plik, np.:
- `GatekeeperConfig` używany jeszcze przez compatibility wiring,
- legacy registry/buffer types potrzebne już tylko dla testów,
- komentarze i eksporty sugerujące runtime owner semantics.

#### 2. Nie reaktywować runtime policy w core
Zakazane jest utrwalanie w core pod nowymi nazwami:
- ownership `pending_live`,
- observation window semantics,
- runtime routing,
- launcher state machine `tracked/approved/committed`,
- compare-only rollout logic.

#### 3. Zmniejszyć public surface legacy gatekeepera
1. Po PR API `ghost-core/src/shadow_ledger/gatekeeper.rs` ma być mniejsze lub wyraźniej oznaczone jako transitional.
2. Re-exporty w `mod.rs` mają odzwierciedlać stan przejściowy, a nie sugerować docelowe znaczenie runtime.
3. `ghost-launcher/src/config.rs` nie może utrwalać nowej zależności na core gatekeeperze pod pretekstem compatibility.

### Czego nie wolno zrobić w tym PR
- Nie przełączać jeszcze canonical writer ownership.
- Nie usuwać jeszcze starego pliku.
- Nie wracać do tematów Fazy 4 recovery.
- Nie rozszerzać execution-layer shadow-run BUY.

### Testy i walidacja
1. Potwierdzić cleanup import graph dla launcher/core.
2. Dodać walidację architektoniczną: brak nowych produkcyjnych użyć legacy gatekeepera.
3. Utrzymać testy i behavior existing launcher commit coordinatora bez zmiany semantyki.

### Merge gatePR-7 — `launcher-commit-coordinator`
Nie wolno merge’ować, jeżeli po PR:
- rośnie public surface `ghost-core/src/shadow_ledger/gatekeeper.rs`,
- `ghost-launcher` dostaje nowe produkcyjne zależności od `GatekeeperRegistry`,
- extraction zmienia behavior runtime szerzej niż cleanup transitional layer.

---

## 

### Twardy cel PR
Domknąć rollout launcherowego commit coordinatora: od istniejącej implementacji i testów equivalence przejść do operatorskiego compare-only runtime, raportu zgodności i kontrolowanego switch gate przed ostatecznym odcięciem legacy pathu.

### Stan wejściowy
PR-7 jest **częściowo już dowieziony kodowo**, ale nie rolloutowo.

W repo już istnieją:
- launcher-owned `pending_live`,
- `LauncherCommitCoordinator`,
- `GatekeeperCommitLoop` pracujący na wyniku launchera,
- post-commit bootstrap `LivePipeline` na podstawie wyniku commitu,
- test equivalence z legacy gatekeeperem,
- failure recovery testy commit window.

Nie istnieją jeszcze:
- operatorski compare-only mode dla commit path,
- structured divergence report będący merge/switch gate,
- formalne odseparowanie compare-only od canonical writer path w runtime configu/rolloucie.

### Pliki obowiązkowe
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/config.rs`
- wyłącznie pomocniczo: dokumentacja rolloutu / metrics helpers

### Dokładny zakres implementacyjny

#### 1. Nie wracać do już wykonanej migracji ownership
W tym PR nie należy ponownie implementować od zera:
- launcher ownership `pending_live`,
- commit loop orchestration,
- post-commit `LivePipeline` init,
- restore/replay staged commit state.

To są fakty aktualnego repo. PR-7 ma domknąć to, czego jeszcze brakuje do bezpiecznego przełączenia.

#### 2. Dodać operatorski compare-only runtime dla commit path
1. Legacy/core path pozostaje canonical writerem w trakcie compare-only.
2. Launcher coordinator liczy wynik równolegle na tym samym wejściu.
3. Compare-only nie może wykonywać drugiego canonical write do prawdziwej historii runtime.
4. Compare-only ma być jawnie nazwanym trybem rolloutowym, a nie wyłącznie testem jednostkowym.

#### 3. Raportować divergence w formie operatorskiej
Minimalny zestaw porównań:
- `snapshot_hash`,
- `tx_count`,
- `last_tx_key`,
- `persisted_success`,
- `committed_count`,
- `merged_pending_count`,
- success/failure classification.

Raport ma istnieć jako:
- structured log / JSONL,
- czytelny summary operatorski,
- merge/switch gate przed pełnym przełączeniem ownership writer path.

#### 4. Domknąć odcięcie runtime od legacy zależności
1. Produkcyjny runtime launchera nie może opierać się na `GatekeeperRegistry` jako aktywnym ownerze commitu.
2. Legacy core path może pozostać tylko jako compare-only/canonical counterpart do czasu końca rolloutu.
3. Po PR ma być jednoznaczne, które elementy są jeszcze tylko przejściowe i czekają na PR-8.

### Czego nie wolno zrobić w tym PR
- Nie usuwać jeszcze `ghost-core/src/shadow_ledger/gatekeeper.rs`.
- Nie ruszać transportu ani log pressure.
- Nie mieszać compare-only commit path z execution-layer shadow-run BUY.
- Nie wracać do prac z Fazy 4.

### Testy obowiązkowe
- `runtime_gatekeeper_commits_without_core_gatekeeper`
- `pending_live_survives_commit_window`
- istniejący test equivalence launcher vs legacy
- `legacy_canonical_launcher_compare_reports_match`
- `legacy_canonical_launcher_compare_reports_divergence`
- `compare_only_never_becomes_canonical_writer`
- `switch_gate_blocks_on_divergence`
- `switch_gate_allows_transition_only_after_clean_report`

### Merge gate
Nie wolno merge’ować z przełączeniem writer path, jeżeli:
- compare-only runtime nie istnieje poza testami equivalence,
- divergence nie jest raportowany strukturalnie,
- switch nie jest blokowany przy divergence,
- launcher i legacy rozjeżdżają się w `snapshot_hash`, `tx_count` lub `last_tx_key`.

---

## PR-8 — `delete-core-gatekeeper`

### Twardy cel PR
Usunąć `ghost-core/src/shadow_ledger/gatekeeper.rs` oraz wszystkie pozostałe produkcyjne zależności od tej warstwy po formalnym zamknięciu PR-7.

### Twarde doprecyzowanie
PR-8 nie jest jeszcze „gotowy teraz”. Jest aktualnym kolejnym krokiem **dopiero po**:
- istnieniu compare-only runtime dla commit path,
- czystym raporcie zgodności,
- formalnym przejęciu canonical writer ownership przez launcher.

### Pliki obowiązkowe
- usunięcie `ghost-core/src/shadow_ledger/gatekeeper.rs`
- cleanup w:
  - `ghost-core/src/shadow_ledger/mod.rs`
  - `ghost-launcher/src/config.rs`
  - `ghost-launcher/src/oracle_runtime.rs`
  - `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
  - dokumentacji, testach i compatibility wiring

### Dokładny zakres implementacyjny
1. Usunąć produkcyjne importy do `GatekeeperRegistry`, `GatekeeperMintBuffer`, `GatekeeperConfig` i `shadow_ledger::gatekeeper`.
2. Oczyścić re-exporty w `mod.rs`.
3. Rozdzielić to, co ma zostać jako neutralne typy/domain helpers, od tego, co ma zniknąć razem z legacy gatekeeperem.
4. Potwierdzić, że `ShadowLedger` i `LivePipeline` pozostają czystą warstwą domain/storage.
5. Pozostawić wyłącznie te testy i helpery, które nie reaktywują runtime ownership przez stary plik.

### Czego nie wolno zrobić w tym PR
- Nie dodawać nowej semantyki runtime.
- Nie naprawiać tutaj problemów z PR-7 compare-only rolloutem.
- Nie ruszać PR-9.
- Nie zostawiać „tymczasowego” hotfixowego mostu z powrotem do legacy gatekeepera.

### Testy i walidacja
- full workspace compile,
- zero-reference verification dla produkcyjnych importów legacy gatekeepera,
- smoke/e2e na launcher-owned commit path,
- test post-commit routing na nowej architekturze,
- weryfikacja, że config compatibility nie zależy już od `ghost_core::shadow_ledger::GatekeeperConfig`.

### Merge gate
Nie wolno merge’ować, jeżeli:
- istnieją jeszcze produkcyjne importy do starego gatekeepera,
- launcher nadal potrzebuje legacy gatekeepera do commit ownership,
- usunięcie wymaga natychmiastowych przywróceń starej warstwy.

---

## PR-9 — `transport-and-log-pressure`

### Twardy cel PR
Po zamknięciu correctness, recovery i migracji Gatekeepera ograniczyć realne problemy transportowe, reconnect/gap recovery oraz log pressure bez utraty obserwowalności.

### Stan wejściowy
PR-9 pozostaje ostatni. Tego porządku nie wolno zmieniać.

### Pliki obowiązkowe
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/config.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `config.toml`
- ewentualnie tracing/logging wyłącznie wtedy, gdy dotyczy redukcji hot-path spam bez utraty diagnostyki

### Dokładny zakres implementacyjny

#### 1. Reconnect/gap/backfill mają być mierzalnym recovery path
1. Model odzyskiwania po zerwaniu streamu ma być deterministyczny i zmierzony.
2. Recovery nie może być best effort bez telemetrii.
3. Każdy reconnect ma mieć jawny model: wykrycie luki, backfill, wznowienie.

#### 2. Ograniczyć residual coverage loss bez rozmiękczania guardów correctness
1. Zmierzyć residual `tracked_dropped`, churn watchlist i skuteczność recovery po zamknięciu wcześniejszych faz.
2. Każda zmiana ma mieć before/after na tych samych scenariuszach.

#### 3. Radykalnie ograniczyć log pressure bez utraty obserwowalności
Do redukcji kwalifikują się noisy hot-path logs (`TRADE_DEDUP`, websocket/base64 spam, niskowartościowe debug bursts), ale nie wolno usuwać informacji potrzebnej do diagnozy correctness lub rollout regressions.

### Czego nie wolno zrobić w tym PR
- Nie wracać do PR-7 i PR-8.
- Nie poprawiać parser correctness przez obejścia transportowe.
- Nie maskować braków recovery albo rolloutu cięciem logów.

### Testy i walidacja
- reconnect scenario,
- artificial gap recovery,
- watch churn / burst tests,
- runtime smoke z porównaniem wolumenu logów,
- pomiar before/after dla recovery success, duplicate suppression i log rate.

### Merge gate
Nie wolno merge’ować, jeżeli:
- zmiany transportowe ukrywają correctness regressions,
- logging jest ograniczony kosztem utraty diagnostyki,
- nie ma before/after dla recovery behavior i log pressure.

---

## Twarde warunki usunięcia `ghost-core` gatekeepera

Usunięcie starego gatekeepera jest dozwolone wyłącznie wtedy, gdy jednocześnie prawdziwe są wszystkie warunki:
1. `ghost-launcher` nie importuje już produkcyjnie:
   - `GatekeeperRegistry`,
   - `GatekeeperMintBuffer`,
   - `GatekeeperConfig` z legacy path,
   - żadnego runtime behavior z `ghost-core/src/shadow_ledger/gatekeeper.rs`.
2. `pending_live`, commit window i post-commit routing są własnością launchera nie tylko kodowo, ale też rolloutowo.
3. PR-7 posiada zatwierdzony report compare-only bez krytycznych divergence.
4. `ShadowLedger.commit_history()` i `LivePipeline` są wywoływane bez pośrednictwa legacy core gatekeepera jako ownera logiki runtime.
5. Nie ma już produkcyjnej potrzeby utrzymywania compatibility wiring do tego pliku.
6. Testy i smoke'y przechodzą na nowym path bez awaryjnego fallbacku do legacy gatekeepera.

---

## Rollout i shadow-run — wymagania obowiązkowe

1. Compare-only commit path dla PR-7 jest obowiązkowy przed switch'em.
2. Execution-layer shadow-run BUY nie jest substytutem compare-only commit path.
3. PR-8 nie może wejść przed zatwierdzonym raportem PR-7 compare-only.
4. PR-9 nie może zacząć się przed zamknięciem PR-8.

---

## ANEKS — AKTUALNY STATUS SHADOW-RUN I ROLLOUTU

Ten aneks rozdziela dwa różne mechanizmy, które w repo mają dziś **różny status dojrzałości**.

### 1. Execution-layer shadow-run BUY — status bieżący

W aktualnym repo execution-layer shadow-run BUY jest już wdrożony w istotnym zakresie i **nie stanowi już backlogu Fazy III**.

Za fakty wdrożone należy uznać co najmniej:
- jawne tryby `shadow_only` i `live_and_shadow`,
- wspólny prepared request path dla live/shadow,
- realne `simulateTransaction` przez shadow RPC,
- structured report dla symulacji,
- rozdzielenie `shadow_only` od realnego `TransactionSent` / `PostBuySubmitted`.

Wniosek wykonawczy: dalsze prace nad execution-layer shadow-run BUY nie należą do bieżącego krytycznego toru Fazy III, chyba że wynikną z osobnej decyzji architektonicznej poza tym planem.

### 2. Compare-only commit path dla PR-7 — status bieżący

Tu stan jest inny:
- istnieją testy equivalence,
- nie istnieje jeszcze operatorski compare-only runtime mode,
- nie istnieje jeszcze formalny switch gate oparty o raport divergence.

To oznacza, że **prawdziwym brakującym shadow-runem w Fazie III jest tylko commit-path compare-only dla PR-7**.

### 3. Backlog implementacyjny — commit-path compare-only

Kolejność PR-ów w tym torze jest obowiązkowa:
1. PR-S7a,
2. PR-S7b.

---

## PR-S7a — `launcher-vs-legacy-compare-only`

### Twardy cel PR
Dodać operatorski compare-only mode dla commit path, w którym legacy path nadal zapisuje canonical write, a launcher coordinator liczy wynik równolegle tylko do porównania.

### Pliki obowiązkowe
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/config.rs`

### Dokładny zakres implementacyjny
#### 1. Wprowadzić jawny rollout mode dla PR-7
Minimalna semantyka:
- legacy path = canonical writer,
- launcher coordinator = compare-only,
- divergence = telemetry/report,
- brak ownership switch.

#### 2. Zasilić compare-only tym samym wejściem co canonical path
1. Compare-only launcher coordinator ma liczyć z tego samego `txs_snapshot` / okna commitu.
2. Nie wolno karmić go zsyntetyzowanym ani uproszczonym wejściem.

#### 3. Użyć osobnego in-memory targetu dla compare-only
1. Compare-only może użyć ephemeral `ShadowLedger` lub równoważnego medium.
2. Nie wolno wykonywać drugiego canonical write do prawdziwej historii runtime.

#### 4. Zbierać minimalny zestaw porównań
- `snapshot_hash`,
- `tx_count`,
- `last_tx_key`,
- `persisted_success`,
- `committed_count`,
- `merged_pending_count`.

### Testy obowiązkowe
- `legacy_canonical_launcher_compare_reports_match`
- `legacy_canonical_launcher_compare_reports_divergence`
- `compare_only_never_becomes_canonical_writer`

### Merge gate
Nie wolno merge’ować, jeżeli:
- compare-only może wykonać canonical write do produkcyjnej historii,
- compare-only używa innego wejścia niż canonical path,
- raport nie rozróżnia match od divergence.

---

## PR-S7b — `shadow-run-report-and-switch-gate`

### Twardy cel PR
Sformalizować raport zgodności PR-7 i uczynić go merge/switch gate przed przełączeniem writer path.

### Pliki obowiązkowe
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/main.rs`
- opcjonalnie metrics/report helpers
- dokumentacja rolloutu

### Dokładny zakres implementacyjny
1. Wygenerować raport zgodności zawierający:
   - liczbę commitów porównanych,
   - liczbę divergence,
   - listę divergence krytycznych,
   - `snapshot_hash`,
   - `tx_count`,
   - `last_tx_key`,
   - `persisted_success`,
   - `committed_count`,
   - `merged_pending_count`.
2. Raport ma istnieć jako:
   - structured log / JSONL,
   - summary operatorski.
3. Pełne przełączenie writer path ma być zabronione bez zatwierdzonego raportu.

### Testy obowiązkowe
- `shadow_run_report_serializes`
- `switch_gate_blocks_on_divergence`
- `switch_gate_allows_transition_only_after_clean_report`

### Merge gate
Nie wolno merge’ować, jeżeli:
- report nie jest generowany,
- divergence nie blokuje switcha,
- rollout nadal opiera się na manualnym „wydaje się OK”.

### Kryteria uznania compare-only commit path za „fakt”

Shadow-run PR-7 uznaje się za dowieziony wyłącznie wtedy, gdy jednocześnie prawdziwe są wszystkie warunki:
1. istnieje operatorski compare-only mode,
2. legacy path pozostaje canonical writerem w trakcie compare-only,
3. launcher coordinator liczy wynik równolegle na tym samym wejściu,
4. compare-only nie wykonuje drugiego canonical write,
5. divergence jest raportowany w structured formie,
6. pełne przełączenie writer path jest blokowane przez divergence,
7. istnieje zatwierdzony raport zgodności.

---

# Ogólne warunki akceptacji całego programu:

## Etap I:

- parser contract naprawiony,
- `1 signature != 1 trade` realnie obsłużone,
- `mint=111111...` nie wychodzi poza unresolved path,
- `SnapshotListener` nie hard-dropuje `unknown_pool`,
- `SnapshotEngine` ma jeden authoritative tx ingress,
- recoverable tx nie ginie na `pool_not_active`,
- `SnapshotEngine` nie wykonuje normal-path pre-commit write attempts do `ShadowLedgera`,
- coverage counters jakościowo poprawione.

## Etap II:

- `tracked`, `approved`, `committed` rozdzielone,
- `approved` nie uruchamia HardTruth semantics,
- launcher używa canonical event-time,
- commit path nie kłamie,
- commit result jest prawdziwym nośnikiem sukcesu/porażki,
- internal bootstrap/enrichment reads nie są blokowane przez approval-gated public snapshot access.

## Etap III:

- residual extraction i compatibility cleanup są domknięte,
- launcher ma operatorski compare-only dla commit path i zatwierdzony raport zgodności,
- canonical writer ownership jest przełączony dopiero po czystym raporcie divergence,
- core gatekeeper został usunięty dopiero po odcięciu wszystkich produkcyjnych zależności,
- pre-commit ledger boundary jest zachowana: `SnapshotEngine` pozostaje local/soft-truth ownerem stanu przed commitem,
- transport i log pressure są poprawione dopiero po zamknięciu correctness i migracji,
- architektura końcowa ma jednego Gatekeepera runtime — w launcherze.


# KOŃCOWY DEFINITION OF DONE:

Cały program jest zakończony wyłącznie wtedy, gdy:
1. istnieje tylko jeden prawdziwy Gatekeeper runtime w `ghost-launcher`,
2. `ghost-core` jest czystą warstwą domain/storage,
3. żaden etap nie pozostawia miejsca na dublowanie źródeł prawdy,
4. multi-event tx, event-time, statusy i commit truthfulness są spójne od wejścia do live path,
5. `SnapshotEngine` nie traci recoverable tx na `pool_not_active` i nie próbuje już normal-path pre-commit writes do `ShadowLedgera`,
6. internal bootstrap/enrichment reads nie są blokowane przez publiczny approval gate,
7. compare-only rollout dla commit path został przeprowadzony i zatwierdzony przed usunięciem legacy gatekeepera,
8. transport i logi są uporządkowane bez maskowania correctness regressions.
