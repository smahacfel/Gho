## Plan: Atomiczny bootstrap create+dev-buy

Naprawa powinna zlikwidować semantyczne rozszczepienie pojedynczej sygnatury `Create + dev-buy` na niezależne bootstrapy i replaye, ale bez zmiany SSOT, bez zmiany transportowych kontraktów `SeerEvent`/`GhostEvent`, i bez rozwalania osi czasu dla zwykłych pooli bez dev-buy. Rekomendowany kierunek: wprowadzić atomowy kontekst genesis dla wspólnej sygnatury, zunifikować writerów `InitPoolEvent`, zachować `ShadowLedger` jako SSOT curve state, a `SnapshotEngine` jako konsumenta z jednoznacznym bootstrapem i bez podwójnego liczenia.

**Steps**
1. Faza 0 — Zamrożenie kontraktów i inwentaryzacja blast radius.
   1.1. Spisać obecne kontrakty wejścia/wyjścia dla `TradeEvent`, `CandidatePool`, `PoolTransaction`, `InitPoolEvent`, `TxEvent`, `MarketSnapshot`, `BondingCurve`, `GhostEvent`, `SeerEvent`.
   1.2. Oznaczyć jako niezmienne granice SSOT: raw on-chain tx = Yellowstone/gRPC; curve→mint mapping = Seer; bonding curve state = `ShadowLedger`; market snapshots = `SnapshotEngine`; decyzja buy/reject = Gatekeeper V2.
   1.3. Oznaczyć jako niezmienne granice czasowe: nie wolno zmienić kolejności transportowej gRPC→parser→IPC, ale wolno zmienić semantykę bootstrapu dla wspólnej sygnatury create+buy.
   1.4. Zdefiniować twarde invarianty wdrożeniowe: (a) `Create` bez `Buy` zachowuje dzisiejszy flow, (b) `Buy` w innej sygnaturze pozostaje zwykłym trade, (c) `Create+Buy` w tej samej sygnaturze bootstrapuje stan post-buy, (d) ten sam dev buy nie może być policzony dwa razy ani w curve state, ani w snapshots, ani w metrykach Gatekeepera.

2. Faza 1 — Ustanowienie kanonicznego modelu genesis per signature. *Blokuje dalsze kroki.*
   2.1. Dodać nowy model pomocniczy w warstwie Seera, np. `GenesisBootstrapContext` / `GenesisBundle`, ale jako model wewnętrzny na początek, bez natychmiastowej zmiany publicznego transportu IPC.
   2.2. Model ma zawierać minimum: `signature`, `pool_amm_id`, `base_mint`, `bonding_curve`, `create_event_ts_ms`, `create_slot`, `dev_buy_lamports_total`, `dev_buy_event_ordinals`, `post_genesis_reserve_quote`, `post_genesis_reserve_base`, `initial_price_quote`, `initial_liquidity_sol`, `bootstrap_mode` (`CreateOnly` / `CreateAndDevBuySameSignature`).
   2.3. Uzgodnić źródła pól: dane identyfikacyjne pochodzą z `CandidatePool`; semantyka dev-buy z `TradeEvent.is_dev_buy`; ilość dev-buy z `TradeEvent.max_sol_cost`; rozróżnienie wieloeventowe z `event_ordinal`; rezerwy początkowe nie mogą być liczone z przypadkowego fallbacku genesis, jeśli znany jest kontekst `CreateAndDevBuySameSignature`.
   2.4. Nie zmieniać jeszcze zewnętrznego `SeerEvent`; najpierw użyć modelu jako wewnętrznego agregatu, żeby ograniczyć blast radius testów i serde-kontraktów.

3. Faza 2 — Seer core: złożyć wspólną sygnaturę w atomowy kontekst bootstrapu. *Zależy od 2. Może być wdrażane równolegle z fazą 3 tylko po ustaleniu modelu.*
   3.1. W `/root/Gho/off-chain/components/seer/src/lib.rs` zinwentaryzować dokładnie flow `handle_trade_event`, `register_curve_mapping`, `emit_trade_only`, emisji `CandidatePool` i miejsca, gdzie parser już ma równocześnie `signature + event_ordinal`.
   3.2. Dodać lekki assembler per `signature` dla krótkiego okna jednej transakcji, który scala `Create` i następujące po nim `TradeEvent` z `is_dev_buy=true` w jeden kontekst genesis.
   3.3. Nie opierać asamblera na czasie ściennym ani kolejności tasków; klucz ma być transakcyjny: `signature` plus uporządkowanie po `event_ordinal`.
   3.4. Utrzymać obecny parser-level dedupe oraz pending-trades buffer dla mappingu curve→mint; nowy assembler nie zastępuje mapping SSOT, tylko dobudowuje semantykę bootstrapu.
   3.5. W przypadku `CreateOnly` asambler emituje dokładnie taki sam bootstrap jak dziś.
   3.6. W przypadku `CreateAndDevBuySameSignature` asambler oznacza powiązane `TradeEvent` jako `consumed_by_genesis_bootstrap=true` w modelu wewnętrznym albo przygotowuje ich listę do późniejszego kontrolowanego replayu.
   3.7. Nie usuwać samego `TradeEvent` z pipeline analitycznego; zachować go dla feature path, ale od tego momentu musi istnieć jednoznaczna polityka, czy dla curve bootstrapu jest folded into genesis, a dla heurystyk jest nadal widoczny jako genesis-dev-buy.
   3.8. Dodać testy jednostkowe Seera na przypadki: `create only`, `create+dev_buy same signature`, `create+2 dev buys same signature`, `trade before create in session but same signature`, `trade in different signature`.

4. Faza 3 — Bridge Seer→EventBus: zmienić semantykę session gate z replay-first na bootstrap-aware. *Zależy od 2. Może iść równolegle z 3.8 i częścią fazy 4.*
   4.1. W `/root/Gho/ghost-launcher/src/components/seer.rs` utrzymać istniejący `SessionPoolTradeBridge`, bo zabezpiecza race `Trade-before-PoolDetected`, ale rozszerzyć go o wiedzę, że nie każdy buffered trade ma być odtworzony jako zwykły pierwszy live trade.
   4.2. Dodać wewnętrzny mechanizm sprawdzania, czy buffered trade należy do tej samej sygnatury genesis co świeżo wykryty pool.
   4.3. Zmienić kolejność semantyczną `process_pool_detected_event_for_session_gate`: zamiast „emit `NewPoolDetected` → register pool → replay buffered trades” użyć „ustal bootstrap context → zarejestruj pool → oznacz buffered genesis trades jako folded/consumed → wyemituj bootstrap → replay tylko niegenesisowe trades”.
   4.4. Zachować dokładnie-once dla buffered replay: kluczem pozostaje `(pool, signature, event_ordinal)`; nie wolno wracać do dedupe tylko po samej sygnaturze.
   4.5. Jeśli `TradeEvent.is_dev_buy` i należy do tej samej sygnatury co create, bridge nie może już emitować tego trade’a jako zwykłego pierwszego trade’a krzywej. Zamiast tego ma przekazać jego wolumen do bootstrap context albo oznaczyć event jako `genesis_folded=true` dla downstream.
   4.6. Pozostawić zwykłe zachowanie dla trade’ów z innych sygnatur, nawet jeśli są `is_dev_buy=true`.
   4.7. Usunąć lub wycofać bezpośredni bootstrap `SnapshotEngine` z tego pliku jako źródło prawdy dla initu. Ten writer jest obecnie bocznym kanałem i stanowi ryzyko regresji. W planie wdrożenia należy go wygasić dopiero po uruchomieniu kanonicznego writer path w fazie 5.
   4.8. Rozszerzyć testy bridge o przypadki: `trade buffered then pool detected with same signature => no normal replay of genesis dev buy`, `pool detected with unrelated buffered trades => replay remains`, `multiple ordinals under same signature => only designated genesis ordinals folded`.

5. Faza 4 — Kanoniczny kontrakt bootstrapu: rozszerzyć `InitPoolEvent`, ale bez zmiany SSOT. *Zależy od 2 i 4.*
   5.1. W `/root/Gho/ghost-brain/src/oracle/snapshot_engine.rs` rozszerzyć `InitPoolEvent` o jawne pola bootstrapu genesis, minimum: `genesis_dev_buy_lamports: u64`, `genesis_dev_buy_count: u32` lub równoważnik, opcjonalnie `bootstrap_origin` / `bootstrap_mode`.
   5.2. Nie przeciążać `initial_liquidity_sol` semantyką dev-buy. To pole dziś opisuje create/bootstrap i nie powinno ukrywać dodatkowego buy volume.
   5.3. Zachować kompatybilność wsteczną konstruktorów testowych i helperów: wszędzie, gdzie budowany jest `InitPoolEvent`, nowe pola muszą dostać jawne wartości defaultowe dla `CreateOnly`.
   5.4. Zmienić wszystkie miejsca tworzenia `InitPoolEvent` w repo, ale nadać im status: jeden writer kanoniczny, reszta tymczasowo fallback/test-only.
   5.5. Zdefiniować nowy kontrakt bootstrapu: `InitPoolEvent` opisuje stan ekonomiczny poola po zakończeniu transakcji genesis, a nie teoretyczny protokołowy punkt zero.

6. Faza 5 — SnapshotEngine: bootstrapować z post-genesis state zamiast z pre-dev-buy fikcji. *Zależy od 5; blokuje bezpieczne wygaszenie starych writerów.*
   6.1. W `/root/Gho/ghost-brain/src/oracle/snapshot_engine.rs` zmodyfikować `handle_initialize_pool_event` tak, by g0/g1/g2 były budowane z `initial_reserve_base` / `initial_reserve_quote` reprezentujących post-genesis state.
   6.2. Ustalić semantykę wolumenu bootstrapu: jeśli dev buy został folded into genesis, `cum_volume_sol` w g0/g1/g2 nie może już bezrefleksyjnie wynosić `0.0`, o ile downstream metryki interpretują baseline jako „stan po genesis”.
   6.3. Jeśli architektura wymaga zachowania „baseline-safe zero volume” dla bootstrap snapshotów, trzeba dodać oddzielne pola auditowe (`genesis_dev_buy_lamports`, `genesis_volume_sol`) zamiast wciskać prawdę w `cum_volume_sol=0`. Decyzja ta musi być jawna i spójna z consumerami metryk.
   6.4. Ustalić i opisać politykę dla `dev_buy_lamports` w g0/g1/g2: rekomendacja — odzwierciedlać genesis dev buy już w bootstrap snapshot metadata, nawet jeśli liczniki transakcyjne startują od zera.
   6.5. Nie zmieniać istniejącego dedupe `TxKey(signature, event_ordinal)` dla zwykłych `TxEvent`; zamiast tego dopilnować, by genesis-folded trade nie był po bootstrapie drugi raz wysłany jako normalny `TxEvent` do silnika.
   6.6. Rozszerzyć testy `snapshot_engine_data_reliability.rs` i integracyjne scenariusze o: bootstrap bez dev buy, bootstrap z folded dev buy, dwa eventy pod jedną sygnaturą z różnymi `event_ordinal`, brak podwójnego naliczenia cumulative counters.

7. Faza 6 — OracleRuntime: uczynić `InitPoolEvent` single-writer i odseparować bootstrap od późniejszej korekty. *Zależy od 5. Krytyczne dla braku regresji osi czasu.*
   7.1. W `/root/Gho/ghost-launcher/src/oracle_runtime.rs` zidentyfikować wszystkie ścieżki emitujące `handle_initialize_pool_event`, w szczególności `maybe_emit_init_pool_event` i inne wywołania znalezione przez grep.
   7.2. Wyznaczyć jedno kanoniczne miejsce emisji `InitPoolEvent` dla produkcyjnego runtime. Rekomendacja: utrzymać to po stronie runtime, ale tylko jeśli wejściowy `DetectedPool`/bootstrap context zawiera już prawdę post-genesis; inaczej przenieść writer responsibility do jednego, jawnego bridge/bootstrap coordinatora.
   7.3. Usunąć semantykę „spróbuj z `resolve_price_context`, jak brak to `tokio::spawn(refresh)` i od razu strzel drugi resolve”. To nie jest deterministyczny bootstrap, tylko wyścig z własnym cieniem.
   7.4. Dla pooli z `CreateAndDevBuySameSignature` `maybe_emit_init_pool_event` nie może wyliczać initu z `ShadowLedger.get_curve()` jeśli ten ledger jeszcze nie zawiera post-buy prawdy; ma użyć preobliczonego bootstrap context.
   7.5. Dla `CreateOnly` zachować dzisiejszy fallback path do `resolve_price_context`, ale opisać go jako `CreateOnlyColdStartFallback`, nie jako ogólną prawdę.
   7.6. `refresh_bonding_curve_state` i reconciliation mają zostać ścieżkami korekcyjnymi dla późniejszego stanu, a nie źródłem prawdy dla pierwszego bootstrapu.
   7.7. Rozszerzyć testy runtime o przypadki, gdzie `NewPoolDetected` i pierwszy tx dzielą sygnaturę, oraz o przypadki, gdzie brak account update na starcie nie psuje bootstrapu.

8. Faza 7 — ShadowLedger: zachować SSOT bez przesuwania granicy odpowiedzialności. *Zależy od 6.*
   8.1. Nie przenosić SSOT snapshotowego do `SnapshotEngine`; `ShadowLedger` pozostaje autorytatywnym cache’em curve state.
   8.2. Jeśli bootstrap context zna post-genesis reserves, pierwsze seedowanie curve w `ShadowLedger` ma od razu używać post-buy state dla przypadku `CreateAndDevBuySameSignature`.
   8.3. Nie zmieniać bez potrzeby kontraktu `AccountUpdate -> ShadowLedger`; to nadal ma być korekta z on-chain, nie inicjalizacja logiki genesis.
   8.4. Upewnić się, że pierwszy zapis stanu do `ShadowLedger` nie zostanie później „naprawiony” przez replay tego samego genesis dev buy, bo to dałoby podwójny wzrost reserve_quote.
   8.5. Dodać test integracyjny: seed curve post-genesis + późniejszy replay tego samego signature/ordinal nie zmienia curve state drugi raz.

9. Faza 8 — Wygaszenie wielopiszących bootstrap writerów i porządki architektoniczne. *Zależy od 5–7.*
   9.1. Usunąć produkcyjne boczne wywołania `snapshot_engine.handle_initialize_pool_event` z `/root/Gho/ghost-launcher/src/components/seer.rs`, jeśli po wdrożeniu nadal istnieją.
   9.2. Zostawić helpery testowe / fixture writers tylko tam, gdzie są jawnie test-only.
   9.3. Zaktualizować ADR i komentarze architektoniczne: `SnapshotListener` nie był jedynym writerem initu; po zmianie ma powstać rzeczywiście pojedynczy kanoniczny writer bootstrapu.
   9.4. Dodać logi i metryki wdrożeniowe: `bootstrap_mode=create_only|create_and_dev_buy_same_signature`, `genesis_dev_buy_folded_total`, `genesis_trade_replay_suppressed_total`, `bootstrap_writer_source_total`.

10. Faza 9 — Testy regresji i walidacja osi czasu. *Może być częściowo równoległa z 8.3–8.4, ale końcowe uruchomienie blokuje domknięcie planu wykonawczego.*
   10.1. Testy jednostkowe parsera/Seera: poprawne parowanie `Create` + `Trade` po `signature/event_ordinal`.
   10.2. Testy bridge: buffered trade przed pool detected, ale genesis folded bez normalnego replay jako pierwszy live trade.
   10.3. Testy `SnapshotEngine`: brak double counting dla tej samej pary `signature+event_ordinal`; poprawny bootstrap metadata dla dev buy genesis.
   10.4. Testy `OracleRuntime`: `maybe_emit_init_pool_event` korzysta z kanonicznego bootstrap context, nie z przypadkowego fallbacku.
   10.5. Testy integracyjne launcher/event bus: `CreateOnly`, `Create+DevBuySameSignature`, `Create then BuySeparateSignature`, `TradeBeforePoolDetectedButSameSignature`, `NoAccountUpdateColdStart`, `TxOnly mode`.
   10.6. Walidacja niezmienionych kontraktów: `SeerEvent::Trade`, `GhostEvent::PoolTransaction`, `PoolTransaction.event_ordinal`, `TxEvent.signature/event_ordinal`, `GatekeeperBuffer` decision flow, `IWIM veto gate`, commit pipeline, coverage metrics.
   10.7. Manualna walidacja logami na próbce produkcyjnej: ta sama sygnatura powinna dać jeden bootstrap post-buy i zero wtórnego zwiększenia reserve/volume od replayu genesis buy.

**Relevant files**
- `/root/Gho/off-chain/components/seer/src/lib.rs` — parser/runtime orchestration, `handle_trade_event`, `emit_trade_only`, `register_curve_mapping`, pending-trade handling; miejsce na assembler per-signature i bootstrap context.
- `/root/Gho/off-chain/components/seer/src/types.rs` — `TradeEvent`, `CandidatePool`; trzeba utrzymać istniejące kontrakty i ewentualnie dołożyć pomocniczy model bootstrapu bez niepotrzebnego rozwalania serde.
- `/root/Gho/off-chain/components/seer/src/binary_parser.rs` — źródło `signature` i `event_ordinal`; referencja do gwarancji intra-tx ordering.
- `/root/Gho/ghost-launcher/src/components/seer.rs` — `SessionPoolTradeBridge`, `process_trade_event_for_session_gate`, `process_pool_detected_event_for_session_gate`, `trade_event_to_pool_transaction`; zmiana semantyki replay/folded genesis i wygaszenie bocznego writer path do `SnapshotEngine`.
- `/root/Gho/ghost-launcher/src/events.rs` — `PoolTransaction`; kontrakt event bus musi pozostać stabilny, z ewentualnym minimalnym rozszerzeniem tylko jeśli konieczne.
- `/root/Gho/ghost-launcher/src/oracle_runtime.rs` — `maybe_emit_init_pool_event`, `resolve_price_context`, `refresh_bonding_curve_state`; ustanowienie single-writer bootstrapu i odcięcie initu od niedeterministycznego fallbacku.
- `/root/Gho/ghost-brain/src/oracle/snapshot_engine.rs` — `InitPoolEvent`, `handle_initialize_pool_event`, `TxEvent`, dedupe; bootstrap post-genesis oraz brak podwójnego liczenia.
- `/root/Gho/ghost-launcher/src/components/snapshot_listener.rs` — translacja `PoolTransaction -> TxEvent`; trzeba utrzymać spójność `signature + event_ordinal + dev_buy_lamports`.
- `/root/Gho/ghost-core/src/shadow_ledger/types.rs` i powiązane moduły — `MarketSnapshot`, `BondingCurve`, `TxKey`; weryfikacja, że SSOT curve state nie przestaje być w `ShadowLedger`.
- `/root/Gho/ghost-launcher/tests/seer_shadow_ledger_bridge_tests.rs` — istniejące testy zachowania `is_dev_buy`/`dev_buy_lamports`; baza do rozszerzeń.
- `/root/Gho/ghost-launcher/tests/snapshot_engine_integration.rs` — szeroki blast radius dla `InitPoolEvent` i `TxEvent`; główny pakiet regresji.
- `/root/Gho/ghost-launcher/tests/oracle_event_bus_integration.rs` — testy osi czasu event-busowej i dev-buy payloadu.
- `/root/Gho/ghost-brain/tests/snapshot_engine_data_reliability.rs` — testy jakości bootstrapu i dedupe.

**Verification**
1. Uruchomić testy jednostkowe i integracyjne dla `seer`, `ghost-launcher`, `ghost-brain`, z naciskiem na pliki wymienione wyżej oraz scenariusze `CreateOnly` vs `Create+DevBuySameSignature`.
2. Dodać dedykowany test end-to-end: jedna sygnatura zawiera `Create` i `Buy`; oczekiwany rezultat to jeden bootstrap post-buy, brak normalnego replay tego samego genesis buy jako pierwszego live trade’u i brak podwójnego naliczenia `reserve_quote`, `cum_volume_sol`, `dev_buy_lamports`.
3. Dodać test negatywny: oddzielne sygnatury `Create` i późniejszy `Buy` muszą zachować aktualny flow i nie mogą zostać błędnie złożone do jednego bootstrapu.
4. Dodać test invariantu SSOT: `ShadowLedger` pozostaje miejscem prawdy dla curve state; `SnapshotEngine` nie staje się writerem curve state i nie wpływa na decyzje commitowe.
5. Dodać test kontraktowy osi czasu: `Trade-before-PoolDetected` w session bridge nadal działa, ale genesis-folded trade nie pojawia się jako zwykły pierwszy trade poola.
6. Zweryfikować logi/metryki na próbce produkcyjnej: `bootstrap_mode`, suppressed replay genesis, brak `InitPoolEvent reserve_quote=30` dla create+dev-buy same-signature, brak drugiego naliczenia po replay.

**Decisions**
- Zakres obejmuje tylko naprawę bootstrapu create+dev-buy w tej samej sygnaturze oraz wygaszenie wielopiszących init writerów.
- Zakres nie obejmuje reaktywacji martwego `oracle_pipeline.rs`; to pozostaje poza planem.
- Zakres nie obejmuje zmiany SSOT `ShadowLedger` ani przenoszenia decyzji z Gatekeeper V2.
- Zakres nie obejmuje przepisywania całego IPC na nowy event typ od pierwszego commita; najpierw wewnętrzny bootstrap context, potem ewentualne publiczne uproszczenia.
- Rekomendacja: wdrożyć najpierw single-writer bootstrap i folded genesis semantics, dopiero potem porządkować ADR i nazewnictwo.

**Further Considerations**
1. Semantyka bootstrap volume wymaga jawnej decyzji: albo `cum_volume_sol` odzwierciedla stan po genesis buy, albo bootstrap snapshoty zostają „zero-volume”, ale z osobnym polem auditowym `genesis_volume_sol`. Rekomendacja: nie fałszować `cum_volume_sol` bez uzgodnienia consumerów; dodać oddzielne pole auditowe i dopiero potem ewentualnie zmienić semantykę cumulative counters.
2. Jeśli celem jest minimalny hotfix przed pełnym refactorem, najkrótsza ścieżka to: wykrycie buffered genesis trade w `SessionPoolTradeBridge`, zablokowanie jego normalnego replayu, przekazanie `genesis_dev_buy_lamports` do `InitPoolEvent`, oraz wygaszenie bootstrap writerów pobocznych. To jednak nadal wymaga późniejszego uporządkowania single-writer semantics.
3. Dokument `/root/Gho/docs/ADR/20260318_production_pipeline_data_flow.md` po wdrożeniu będzie wymagał korekty sekcji o `SnapshotEngine` writerach, o session gate oraz o osi czasu dla `Create+Buy` w jednej sygnaturze.
