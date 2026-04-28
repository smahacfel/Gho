/*
SELL LOGIC - CURRENT HEAD AUDIT
As of: 2026-04-04

UWAGA
- Ten plik jest dokumentem technicznym zapisanym jako .rs na wyrazne polecenie operatora.
- Nie jest to modul produkcyjny. Ma sluzyc jako jedno miejsce, w ktorym zebrano
  aktualny obraz sell-path, jego integracji, runtime proof i wszystkich istotnych
  early-exit branches w biezacym HEAD.
- Zakres dotyczy przede wszystkim live lane po potwierdzonym BUY oraz wszystkich
  miejsc, w ktorych system moze zakonczyc przeplyw bez realnej proby SELL.
- Zakres linii podane nizej sa "as of current HEAD" i moga sie przesuwac przy
  kolejnych commitach.


==============================================================================
0. EXECUTIVE SUMMARY
==============================================================================

Stan na teraz:

1. Repo-side refactor / Phase 6 cleanup jest domkniety.
2. Paper-side sell verification jest potwierdzona.
3. Live sign-off NIE jest domkniety.

Co jest juz udowodnione:

- W aktualnym HEAD naprawiono regresje paper lane:
  successful ShadowSimulated -> PostBuySubmitted dla lane="paper".
- Bezpieczny, izolowany paper-burnin proof przeszedl:
  session: launcher-1775302576977
  shadow_success=4
  paper_admitted=1
  paper_completed=1
  paper_closed=1
  safe_to_stop_now=true

Potwierdzone log markers z proofu:

- "GATEKEEPER V2: KUPUJ!"
- "PostBuyRuntime: received PostBuySubmitted"
- "PaperLifecycle: entry submitted"
- "PaperBroker: position opened"
- "PaperBroker: position closed"
- "PaperLifecycle: position lifecycle complete"

Potwierdzone event counts z proofu:

- Candidate=56
- EntrySubmitted=1
- EntryFilled=1
- PositionOpened=1
- ExitSubmitted=1
- ExitFilled=1
- PositionClosed=1
- AemTick=239
- ManagementDecision=239
- ManagementOutcome=1

Najwazniejszy wniosek:

- paper sell path jest potwierdzony,
- live sell path nadal ma galezie, w ktorych po realnym BUY system moze:
  - stracic ownership slotu,
  - zgubic handoff do post-buy runtime,
  - przerwac arming przed pierwsza proba SELL,
  - albo zakonczyc lifecycle bez gwarancji, ze jakikolwiek SELL faktycznie polecial.

Najwazniejsze obecne blokery live:

1. ActivePositionLease auto-release w handoffie oracle -> post-buy.
2. Potwierdzony BUY moze nie odpalic PostBuySubmitted i nadal zostac uznany za bought=true.
3. run_live_sell_lifecycle() ma kilka fail-closed / no-sell exits:
   - ATA miss/zero,
   - load_magazine failure,
   - zero bullets,
   - invalid / empty bullets,
   - terminal time-stop drop remaining bullets.

Wniosek operatorski:

- NIE odpalac kolejnego live micro, dopoki te galezie nie zostana uszczelnione
  i dopoki nie bedzie swiezego, realnego proofu live BUY -> live SELL landed.


==============================================================================
1. CURRENT HEAD SSOT CHAIN FOR SELL LOGIC
==============================================================================

1. BUY dispatch + slot reservation
   File:
   - ghost-launcher/src/components/trigger/component.rs
   Najwazniejszy zakres:
   - ok. 1634-1665

   Co robi:
   - sprawdza live Jito transport,
   - rezerwuje active position slot,
   - sklada prepared BUY przez Jito,
   - zwraca TriggerDispatchReceipt zawierajacy:
     - primary_outcome,
     - shadow_task,
     - active_position_lease,
     - failed_request / failed_context.

2. BUY outcome -> post-buy handoff
   File:
   - ghost-launcher/src/oracle_runtime.rs
   Najwazniejsze zakresy:
   - apply_trigger_buy_outcome(): ok. 5925-6014
   - apply_trigger_dispatch_receipt(): ok. 6018-6077

   Co robi:
   - wysyla TransactionSent / PostBuySubmitted dla live confirmed buy,
   - dla paper shadow proof wysyla ShadowBuySimulated + PostBuySubmitted (paper lane),
   - przenosi position_slot_id z dispatch receipt do post-buy runtime.

3. Live post-buy routing + lifecycle owner
   File:
   - ghost-launcher/src/components/post_buy_runtime.rs
   Najwazniejsze zakresy:
   - event consumer / routing: ok. 586-609 oraz 645-676
   - ATA balance lookup: ok. 737-842
   - live arming / magazine loading: ok. 857-905
   - price loop + time-stop: ok. 915-1002
   - bullet fire / transport submit: ok. 1036-1124

   Co robi:
   - odbiera GhostEvent::PostBuySubmitted,
   - dla lane="live" uruchamia run_live_sell_lifecycle(),
   - dla lane paper uruchamia PaperPositionLifecycle,
   - odpowiada za release_slot(...) przy wyjsciu z live lifecycle.

4. Magazine arming
   Files:
   - off-chain/components/trigger/src/revolver_worker.rs
   - off-chain/components/trigger/src/revolver_sell_builder.rs
   Najwazniejsze zakresy:
   - revolver_worker.rs: ok. 389-465
   - revolver_sell_builder.rs: ok. 129-228

   Co robi:
   - pobiera fresh blockhash,
   - liczy min_output,
   - buduje i podpisuje SELL tx,
   - tworzy Bullet,
   - laduje bullets do Revolvera.

5. Live transport / confirmation
   File:
   - off-chain/components/trigger/src/jito_client.rs
   Najwazniejszy zakres:
   - ok. 1915-1999

   Co robi:
   - submit_single_transaction_and_confirm(),
   - status poll Accepted / Rejected / Expired / Pending,
   - on-chain reconciliation,
   - zwraca confirmed bundle albo error.

6. Startup/config fail-closed contract
   Files:
   - ghost-launcher/src/main.rs: 704-765
   - ghost-launcher/src/config.rs: 801-829

   Co robi:
   - buduje LiveSellHandle tylko dla execution_mode Live / Dual,
   - wymaga keypair_path,
   - wymaga niepustego jito_endpoint,
   - wymaga use_jito=true dla live execution.


==============================================================================
2. ARCHITECTURAL INTENT / ADR CONTEXT
==============================================================================

Kluczowy dokument:
- docs/ADR/ADR-0050-live-sell-ssot-launcher-no-monitoring-engine.md

Aktualny kontrakt architektoniczny:

- launcher jest SSOT dla live exit,
- MonitoringEngine / Guardian / PaperPositionLifecycle nie sa live exit path,
- live positions NIE maja byc delegowane do ghost-brain paper machinery,
- live transport ma byc fail-closed na Jito gRPC bundles.

To jest istotne, bo po historycznych "buy without sell" architektura zostala
przepieta tak, aby live sell nie szedl przez paper runtime. Dzisiaj problem nie
polega juz na "zlym ownership path" do paper runtime, tylko na tym, ze aktualny
launcher live path ma jeszcze kilka exits bez gwarancji realnej proby SELL.


==============================================================================
3. EXHAUSTIVE INVENTORY OF CURRENT EARLY-EXIT / NO-SELL BRANCHES
==============================================================================

--------------------------------------------------------------------------
3.1 [CRITICAL] ActivePositionLease auto-release during oracle handoff
--------------------------------------------------------------------------

Files:
- ghost-launcher/src/oracle_runtime.rs: ok. 6018-6077
- ghost-launcher/src/components/trigger/safety.rs: ok. 196-219

Functions:
- apply_trigger_dispatch_receipt()
- ActivePositionLease::Drop
- ActivePositionLease::into_slot_id()

Condition:
- TriggerDispatchReceipt zawiera active_position_lease,
- apply_trigger_dispatch_receipt() bierze tylko lease.slot_id(),
- lease nie jest konsumowany przez into_slot_id(),
- po wyjsciu ze scope Drop zwalnia slot automatycznie.

Phase:
- przed pierwsza proba SELL,
- natychmiast po buy outcome handling.

State impact:
- slot moze zostac zwolniony zanim run_live_sell_lifecycle sie zakonczy,
- bulkhead przestaje byc wiarygodny dla realnie kupionej pozycji.

Operator-visible logs / metrics:
- brak dedykowanego warninga w tym miejscu,
- efekt uboczny widoczny glownie w trigger_active_positions / tracker state.

Integration dependencies:
- TriggerComponent reservation,
- TriggerDispatchReceipt handoff,
- PositionLimitTracker / ActivePositionLease Drop semantics.

Current tests:
- brak testu pokrywajacego ten konkretny bug,
- aktualne testy integracyjne obchodza ten problem recznie, bo w setupie robia:
  lease.into_slot_id()
  zamiast przejsc przez pelny oracle handoff.

Wniosek:
- to jest jeden z najtwardszych blockerow live sign-off.


--------------------------------------------------------------------------
3.2 [CRITICAL] PostBuySubmitted send failure still returns bought=true
--------------------------------------------------------------------------

File:
- ghost-launcher/src/oracle_runtime.rs: ok. 5925-5948

Function:
- apply_trigger_buy_outcome()

Condition:
- event_tx.send(GhostEvent::post_buy_submitted(...)) zwraca Err

Phase:
- przed pierwsza proba SELL

State impact:
- function nadal zwraca:
  bought=true
  close_reason=PoolBoughtEarly
- realny buy zostaje uznany za dokonany,
- ale live post-buy lifecycle moze w ogole nie wystartowac.

Operator-visible logs / metrics:
- warning:
  "Failed to send PostBuySubmitted event: ..."
- brak dedykowanego licznika "confirmed buy without post-buy handoff".

Integration dependencies:
- EventBusSender health,
- broadcast subscribers,
- post_buy_runtime subscriber availability.

Current tests:
- brak testu dla send-failure path.

Wniosek:
- confirmed buy bez post-buy lifecycle to bezposrednia droga do "buy but no sell".


--------------------------------------------------------------------------
3.3 [HIGH] PostBuyRuntime can lag and lose live handoff events
--------------------------------------------------------------------------

File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 645-676

Function:
- run()

Condition:
- broadcast::error::RecvError::Lagged(n)

Phase:
- przed pierwsza proba SELL

State impact:
- PostBuySubmitted moze zostac pominiety,
- runtime tylko loguje lag i idzie dalej,
- zgubione eventy nie sa replayowane.

Operator-visible logs / metrics:
- warning:
  "PostBuyRuntime: lagged by {} events"
- metric:
  record_event_bus_lag("post_buy_runtime", ...)

Integration dependencies:
- event bus capacity,
- consumer throughput,
- lifecycle load przy duzym ruchu.

Current tests:
- brak testu na zgubiony live handoff przez lag.


--------------------------------------------------------------------------
3.4 [MEDIUM] PostBuyRuntime can stop consuming on Closed / shutdown drain
--------------------------------------------------------------------------

File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 658-676

Function:
- run()

Condition:
- RecvError::Closed
- shutdown drain elapsed

Phase:
- przed pierwsza proba SELL dla eventow, ktore przyjda po zakonczeniu subscriber loop

State impact:
- pozniejsze PostBuySubmitted nie maja konsumenta,
- live lifecycle nie startuje.

Operator-visible logs / metrics:
- info:
  "PostBuyRuntime: event bus closed"
- info:
  "PostBuyRuntime shutdown drain elapsed; stopping subscriber"

Integration dependencies:
- shutdown ordering,
- lifecycle event bus,
- bounded drain window.

Current tests:
- paper drain semantics sa czesciowo pokryte,
- brak live-specific coverage dla utraty handoffu przy Closed / drain.


--------------------------------------------------------------------------
3.5 [MEDIUM] lane=live with no LiveSellHandle fail-closes immediately
--------------------------------------------------------------------------

Files:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 586-609
- ghost-launcher/src/main.rs: 704-765
- ghost-launcher/src/config.rs: 801-829

Function:
- run()

Condition:
- lane == "live"
- config.live_sell.is_none()

Phase:
- przed pierwsza proba SELL

State impact:
- explicit release_slot(...)
- brak paper fallback
- brak live exit lifecycle

Operator-visible logs / metrics:
- warning:
  "PostBuyRuntime: live lane but no LiveSellHandle configured - refusing paper fallback and releasing slot"

Integration dependencies:
- startup build_live_sell_handle(),
- config validation,
- execution_mode / trigger.use_jito / jito_endpoint / keypair_path.

Current tests:
- TAK
- ghost-launcher/tests/post_buy_runtime_integration.rs: ok. 458-529

Ocena:
- to jest sensowny pre-runtime fail-closed branch,
- nie jest glownym blockerem pod warunkiem poprawnej live konfiguracji.


--------------------------------------------------------------------------
3.6 [CRITICAL] ATA balance miss/zero aborts live sell before arming
--------------------------------------------------------------------------

File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 737-842

Functions:
- query_actual_ata_balance()
- run_live_sell_lifecycle()

Condition:
- obie proby ATA lookup (Token-2022 i legacy SPL) koncza sie:
  - rpc error
  - albo amount == 0

Phase:
- przed pierwsza proba SELL

State impact:
- release_slot(...)
- return
- brak SELL attempt

Operator-visible logs / metrics:
- info przy sukcesie:
  "LiveSell: resolved actual ATA balance"
- debug przy per-program fail:
  "LiveSell: ATA balance query failed for token program"
- warning przy terminalnym miss:
  "LiveSell: ATA balance query returned zero or failed - releasing slot"
- metrics:
  record_live_sell_rpc_latency("get_token_account_balance", ...)
  record_live_sell_rpc_latency("query_actual_ata_balance", ...)

Integration dependencies:
- payer pubkey,
- mint pubkey,
- ATA derivation for Token-2022 and legacy SPL,
- RPC balance read,
- canonical post-buy account state visibility.

Current tests:
- czesciowo TAK jako fail-closed routing,
- ghost-launcher/tests/post_buy_runtime_integration.rs: ok. 341-456
- brak testu pokazujacego bezpieczne zachowanie dla realnego live arming retry strategy.

Wniosek:
- to jest realny blocker live sign-off.


--------------------------------------------------------------------------
3.7 [CRITICAL] load_magazine_from_direct_buy() failure aborts lifecycle
--------------------------------------------------------------------------

Files:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 857-894
- off-chain/components/trigger/src/revolver_worker.rs: ok. 389-465
- off-chain/components/trigger/src/revolver_sell_builder.rs: ok. 129-228

Functions:
- run_live_sell_lifecycle()
- load_magazine_from_direct_buy()
- SellTxBuilder::build_signed_sell_tx()
- SellTxBuilder::calculate_min_output()

Condition:
- magazine load zwraca Err na dowolnym etapie:
  - get_latest_blockhash() fail
  - calculate_min_output() fail
  - build_signed_sell_tx() fail
  - Bullet::new(...) fail

Phase:
- przed pierwsza proba SELL

State impact:
- release_slot(...)
- return
- brak aktywnego live exit lifecycle

Operator-visible logs / metrics:
- info przy sukcesie:
  "LiveSell: magazine loaded"
- warning przy fail:
  "LiveSell: failed to load magazine - releasing slot"
- metrics:
  record_live_sell_rpc_latency("load_magazine_from_direct_buy", ...)

Integration dependencies:
- RPC latest blockhash,
- sell builder math,
- serialization,
- bullet creation contract,
- Revolver state.

Current tests:
- brak bezposredniego coverage dla fail path,
- obecne sell tests sa glownie happy-path math/build tests.

Wniosek:
- to jest realny blocker live sign-off.


--------------------------------------------------------------------------
3.8 [HIGH] bullet_count == 0 ends flow without sell attempt
--------------------------------------------------------------------------

Files:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 897-905
- off-chain/components/trigger/src/revolver_worker.rs: ok. 408-420

Functions:
- run_live_sell_lifecycle()
- load_magazine_from_direct_buy()

Condition:
- wszystkie TP levels daja token_amount == 0
- wszystkie bullets zostaja pominiete

Phase:
- przed pierwsza proba SELL

State impact:
- warning
- release_slot(...)
- return
- brak SELL attempt

Operator-visible logs / metrics:
- warning:
  "LiveSell: magazine loaded 0 bullets - releasing slot"
- warning z worker:
  "Skipping TP level {}: calculated token amount is 0"

Integration dependencies:
- amount_tokens po BUY,
- TP fractions,
- rounding,
- strategy.tp_levels.

Current tests:
- brak coverage.

Wniosek:
- branch jest szczegolnie grozny dla malych pozycji / rounding edge cases.


--------------------------------------------------------------------------
3.9 [CRITICAL] Empty bullet bytes are dropped without submission
--------------------------------------------------------------------------

File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 1036-1044

Function:
- fire_triggered_bullets()

Condition:
- bullet.tx_bytes.is_empty()

Phase:
- po armingu, ale przed submission dla konkretnego bulleta

State impact:
- bullet jest pomijany przez continue,
- NIE jest reinsertowany,
- jesli to dotyczy wszystkich remaining bullets, runtime moze dojsc do remaining == 0
  bez realnego SELL.

Operator-visible logs / metrics:
- warning:
  "LiveSell: bullet has empty tx_bytes - skipping"

Integration dependencies:
- bullet construction integrity,
- Revolver loaded state.

Current tests:
- brak coverage.


--------------------------------------------------------------------------
3.10 [CRITICAL] Deserialization failure drops bullet without submission
--------------------------------------------------------------------------

File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 1064-1075

Function:
- fire_triggered_bullets()

Condition:
- bincode::deserialize(&bullet.tx_bytes) fail

Phase:
- po armingu, przed submission dla konkretnego bulleta

State impact:
- bullet jest tracony,
- nie ma reinsertu,
- mozna dojsc do remaining == 0 bez ani jednej skutecznej proby SELL.

Operator-visible logs / metrics:
- warning:
  "LiveSell: failed to deserialize bullet tx: ..."

Integration dependencies:
- serialized VersionedTransaction bytes,
- bullet integrity,
- bincode contract.

Current tests:
- brak coverage.


--------------------------------------------------------------------------
3.11 [CRITICAL] Time-stop can drop unsold bullets and end management
--------------------------------------------------------------------------

Files:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 915-1002
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 1078-1124
- off-chain/components/trigger/src/jito_client.rs: ok. 1915-1999

Functions:
- run_live_sell_lifecycle()
- fire_triggered_bullets()
- submit_single_transaction_and_confirm()

Condition:
- elapsed >= LIVE_SELL_MAX_DURATION_SECS
- po force-fire nadal zostaja remaining bullets > 0

Phase:
- po armingu / po partial progress

State impact:
- runtime robi:
  guard.tokens.remove(&mint)
  break
  release_slot(...)
- position lifecycle jest zakonczony mimo pozostalych niesprzedanych bullets

Operator-visible logs / metrics:
- warning:
  "LiveSell: time-stop reached - dropping remaining bullets and releasing slot"
- wczesniej moga wystapic:
  "LiveSell: failed to submit SELL bullet via Jito gRPC bundle: ..."
  "LiveSell: firing stale SELL bullet; launcher path has no active refresh worker in this loop"
  "LiveSell: no canonical or point-query price available - skipping poll cycle"
- metrics:
  transport latency
  stale bullet age
  price-source telemetry

Integration dependencies:
- canonical / point-query price availability,
- Jito Accepted / Rejected / Expired / Pending semantics,
- on-chain reconciliation,
- stale bullet handling,
- runtime clock / max_duration.

Current tests:
- brak coverage dla tego terminalnego branch.

Wniosek:
- to jest najgrozniejszy terminalny branch po czesciowym postepie.


==============================================================================
4. IMPORTANT NON-EXIT BUT RISK-AMPLIFYING BRANCHES
==============================================================================

4.1 Brak live price nie konczy lifecycle od razu
File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 917-963

Behavior:
- jesli brak canonical lub point-query price, runtime tylko:
  "LiveSell: no canonical or point-query price available - skipping poll cycle"
- sam ten branch nie jest terminalny,
- ale jezeli problem trwa do time-stop, to przechodzi w 3.11.

4.2 Jito submit failure reinserts bullet
File:
- ghost-launcher/src/components/post_buy_runtime.rs: ok. 1078-1124

Behavior:
- failed submit nie konczy od razu lifecycle,
- bullet jest reinsertowany,
- ale seria takich failow moze na koncu doprowadzic do 3.11 (time-stop drop).

Wniosek:
- Jito error sam w sobie nie jest jeszcze "buy without sell",
- staje sie nim dopiero w polaczeniu z time-stop i brakiem skutecznego final confirmation.


==============================================================================
5. DISTINCTIONS: LIVE-ONLY VS SHARED VS ACCEPTABLE FAIL-CLOSED
==============================================================================

5.1 Live-only dangerous branches

- brak LiveSellHandle -> immediate release
- ATA balance miss/zero
- load_magazine failure
- zero bullets
- empty tx_bytes bullet drop
- deserialize failure bullet drop
- time-stop dropping remaining bullets

5.2 Shared branches, malo grozne w paper, niebezpieczne w live

- PostBuySubmitted send failure
- event bus lag / closed / shutdown drain
- ActivePositionLease auto-drop

W paper:
- zwykle skutkuja utrata telemetry / lifecycle startu symulacji

W live:
- moga zostawic realnie kupiona pozycje bez aktywnego, autorytatywnego exit path.

5.3 Acceptable fail-closed branches (pre-buy / startup)

Files:
- ghost-launcher/src/config.rs: 801-829
- ghost-launcher/src/main.rs: 704-765
- ghost-launcher/src/components/trigger/component.rs: ok. 1634-1658

Examples:
- use_jito=false dla live config -> config reject
- empty jito_endpoint -> config reject
- missing keypair_path -> startup fail
- ensure_live_jito_transport() fail przed BUY -> dispatch error bez kupna

Ocena:
- te branche sa poprawne, bo zatrzymuja system PRZED buy side effect.


==============================================================================
6. TEST COVERAGE: WHAT IS COVERED VS NOT COVERED
==============================================================================

6.1 Covered today

1. Live lane does not fall back to paper
   File:
   - ghost-launcher/tests/post_buy_runtime_integration.rs: ok. 341-456

   What it proves:
   - runtime idzie w live lane,
   - zly RPC powoduje query_actual_ata_balance fail,
   - slot jest zwalniany,
   - paper lifecycle events nie powstaja.

2. Live lane without handle fails closed
   File:
   - ghost-launcher/tests/post_buy_runtime_integration.rs: ok. 458-529

   What it proves:
   - brak paper fallback,
   - slot jest zwalniany,
   - brak paper events.

3. Startup/config transport guard
   Files:
   - ghost-launcher/src/config.rs
   - ghost-launcher/src/main.rs
   - trigger live transport tests

   What it proves:
   - live/dual wymaga Jito transport,
   - brak endpointu / zly config jest fail-closed przed buy.

4. SELL tx building / math
   Files:
   - off-chain/components/trigger tests

   What it proves:
   - builder i min_output logic sa poprawne w happy path.

6.2 NOT covered today

- ActivePositionLease handoff bug
- PostBuySubmitted send failure
- event bus lag/closed causing live handoff loss
- load_magazine failure path
- zero-bullet path
- empty bullet bytes path
- deserialize-fail bullet path
- terminal time-stop drop path
- full real live BUY -> live SELL landed proof

6.3 Important testing nuance

Aktualne live integration tests czesto robia:
- lease.into_slot_id()

To oznacza:
- testy omijaja realny bug z auto-release lease w oracle handoffie,
- i dlatego nie daja proofu, ze pelny current HEAD handoff jest bezpieczny.


==============================================================================
7. PAPER PROOF STATUS (LOCAL, SAFE, CONFIRMED)
==============================================================================

Isolated paper proof:
- profile: shadow_only + paper
- odseparowane porty i artefakty
- session: launcher-1775302576977

Final guard:
- SAFE_TO_STOP

Final report:
- shadow_success=4
- paper_seen=56
- paper_admitted=1
- paper_completed=1
- paper_closed=1
- paper_inflight=0
- trace_correlation passed
- no_live_side_effects passed

Final event counts:
- AemTick=239
- Candidate=56
- EntryFilled=1
- EntrySubmitted=1
- ExitFilled=1
- ExitSubmitted=1
- ManagementDecision=239
- ManagementOutcome=1
- PositionClosed=1
- PositionOpened=1

Important nuance:
- final shadow_run_report verdict zostal utrzymany jako NO-GO nie dlatego, ze sell
  lifecycle sie nie domknal, tylko dlatego, ze recovery_contract na swiezo pustych
  snapshot/WAL directories raportowal:
  "ShadowLedger restore failed"
- to NIE obala paper sell proof.

Wniosek:
- paper sell mechanism jest potwierdzony.


==============================================================================
8. LIVE SIGN-OFF STATUS (NOT DONE)
==============================================================================

Co nadal NIE jest udowodnione:

- realny launcher-driven live BUY -> live SELL landed
- stabilny ownership slot od dispatch do final sell exit
- brak confirmed-buy-without-post-buy-handoff
- brak arming exits bez sell attempt
- brak terminalnego porzucenia remaining bullets po time-stop

Co retained / code audit juz potwierdzilo:

- historyczne "buy without sell" byly realnym problemem,
- current HEAD jest bezpieczniejszy od starego,
- ale wciaz ma galezie, ktore moga zostawic live position bez gwarancji sell attempt.

Wniosek:
- live micro nadal zablokowany.


==============================================================================
9. PRIMARY SSOT FILES FOR THE FUTURE FIX
==============================================================================

Jesli zacznie sie implementacja fixu, glownymi plikami SSOT beda:

1. ghost-launcher/src/oracle_runtime.rs
   - handoff BUY -> PostBuySubmitted
   - przenoszenie slot ownership
   - semantics bought=true przy send failure

2. ghost-launcher/src/components/trigger/safety.rs
   - ActivePositionLease
   - PositionLimitTracker
   - Drop / into_slot_id contract

3. ghost-launcher/src/components/post_buy_runtime.rs
   - live routing
   - ATA lookup
   - arming
   - bullet fire loop
   - time-stop terminal policy
   - release_slot timing

4. off-chain/components/trigger/src/revolver_worker.rs
   - blockhash fetch
   - bullet generation
   - zero-amount / skipped-level behavior

5. off-chain/components/trigger/src/revolver_sell_builder.rs
   - min_output
   - signed SELL tx build
   - serialization contract

6. off-chain/components/trigger/src/jito_client.rs
   - live submit / confirm / reconciliation semantics


==============================================================================
10. PRIORITIZED LIST OF REAL BLOCKERS
==============================================================================

P0 / hard blockers:

1. ActivePositionLease auto-release in oracle handoff
2. PostBuySubmitted send failure still treated as bought=true
3. ATA miss/zero -> release slot, no sell attempt
4. load_magazine failure -> release slot, no sell attempt
5. time-stop dropping remaining bullets and ending management

P1 / serious but secondary:

6. event bus lag / closed / drain losing live handoff
7. zero-bullet arming path
8. empty / invalid bullet silent drop

P2 / expected fail-closed, not the main blocker:

9. lane=live with no LiveSellHandle
10. startup/config live Jito enforcement before buy


==============================================================================
11. SAFE OPERATOR CONCLUSION
==============================================================================

Na bazie current HEAD code audit oraz lokalnego proofu:

- paper lane mozna uznac za zweryfikowana,
- live lane NIE jest jeszcze gotowa do bezwarunkowego rolloutu,
- kolejny live micro powinien pozostac zablokowany,
  dopoki nie zostana uszczelnione P0 branches i nie pojawi sie swiezy proof:

  realny BUY landed
  -> live sell armed
  -> live sell submitted
  -> live sell confirmed

Bez tego nadal istnieje ryzyko powtorki scenariusza:
"kupilismy tokeny nie sprzedajac ich".
*/
