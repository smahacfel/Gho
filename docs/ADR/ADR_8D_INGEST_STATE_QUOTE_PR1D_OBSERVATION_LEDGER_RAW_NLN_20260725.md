# ADR-8D: Ingest State Quote PR1D — Observation Ledger i raw/NLN reconciliation

Status: `IMPLEMENTED / DRAFT PR #85 REVIEW REMEDIATION VALIDATED / READY FOR RE-REVIEW / OWNER-APPROVED PERFORMANCE WAIVER`

Typ: ADR-8D / PR1D / aktywna granica integralności ingestu

Data: `2026-07-25`

Repo: `/root/Gho_ingest`

Gałąź: `agent/ingest-observation-ledger-pr1d-20260725`

Base: `a7a7bf194033331a2a59cad89f6ce255b82c7635` (merge PR #84 / PR1C)

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Zakres: `PR 1 / Commit 1D` — bounded Observation Ledger, raw/NLN
reconciliation, `CandidateIntegrity` oraz pełna macierz konfliktów lifecycle.

## D0. Cel i granice niepodlegające reinterpretacji

PR1D dodaje jedną izolowaną ścieżkę obserwacyjną równolegle do zachowanej
aktywnej ścieżki parenta:

```text
primary raw Yellowstone Pump observation
  -> Observation Ledger
  -> dokładnie jedna shadow structural canonical mutation
  -> CandidateIntegrity shadow evidence
  -> would_block / would_abort / would_cancel / would_reconcile /
     would_quarantine metrics

primary raw Yellowstone Pump event
  -> istniejąca ścieżka parenta
  -> MFS / Gatekeeper / submit bez PR1D CandidateIntegrity enforcement

secondary raw / parsed NLN
  -> Observation Ledger
  -> witness, agreement, conflict albo brak korelacji
  -> zero samodzielnych structural canonical mutations
```

Następujące reguły są zamrożone:

1. `PrimaryAuthority` z raw Yellowstone jest jedynym structural canonical
   authority dla Pump mutation.
2. `SecondaryWitness` oraz parsed NLN są witness-only.
3. Raw primary nigdy nie czeka na NLN.
4. NLN-first i NLN-only tworzą zero `NewPoolDetected` i zero
   `PoolTransaction`.
5. Signature, claims, provider timestamp, receive time i receive sequence nie
   są samodzielnym kluczem identity ani dedupe.
6. Jedna signature może nieść wiele prawidłowych mutacji, w tym create i
   initial buy.
7. Technical integrity failure nie jest strategicznym `REJECT`, `TIMEOUT`,
   reason code ani policy feature.
8. PR1D nie zmienia quote math, fee math, route authorization, sizing,
   Gatekeeper policy ani post-buy protective-exit policy.
9. PR1D nie dodaje legacy fallbacku i nie przekazuje NLN production authority.
10. Provider-role branching istnieje wyłącznie na granicy integralności; nie
    powstaje konkurencyjna ścieżka Gatekeepera ani MFS.
11. Seer nie może wykonać role-blind mapping/watch/bootstrap/replay side
    effect przed klasyfikacją providera. Parser-local evidence jest dozwolone,
    ale tylko kompletna obserwacja primary raw może otworzyć istniejące
    primary-only structural helpers.
12. `CandidateIntegrity` nie korzysta z best-effortowego broadcastu jako
    źródła prawdy. Ledger i account arbiter zapisują sygnały synchronicznie do
    tej samej współdzielonej instancji registry; broadcast może pozostać
    wyłącznie nieautorytatywnym kanałem kompatybilnościowym/audytowym.
13. Wynik ledgera, registry, receiptu albo apply fence nie tłumi eventu primary
    raw, który parent `a7a7bf1` wyemitowałby po swoich istniejących filtrach.
14. `CandidateIntegrity` w PR1D nie blokuje MFS, ewaluacji, submitu, capacity
    ani lifecycle pozycji. Macierz lifecycle wylicza wyłącznie shadow actions.

## D1. Jeden właściciel architektury

Root agent jest jedynym właścicielem granic:

- canonical authority;
- identity i correlation;
- lifecycle conflict matrix;
- `CandidateIntegrity`;
- observe-only/no-quote-math boundary.

Podagenci wykonują wyłącznie read-only inventory, izolowane testy albo
mechaniczne zmiany po zamrożeniu tego ADR. Nie mogą tworzyć alternatywnego
ledgera, alternatywnej macierzy lifecycle ani osobnej interpretacji authority.

## D2. Model kontraktów Observation Ledger

Kontrakty wspólne należą do `ghost-core::ingest_integrity`.

### D2.1. Obserwacja

`ObservedPumpMutationV1` zachowuje oddzielnie:

- family mutacji (`InitializePool` albo `Trade`);
- signature;
- opcjonalny pełny source-neutral locator;
- opcjonalny canonical order dostępny wyłącznie z raw;
- opcjonalny transaction-local raw mutation count;
- semantic claims, gdzie `None` oznacza Unknown;
- provider role;
- immutable provenance z captured-payload BLAKE3.

Observation identity nie zawiera claims ani czasu odbioru. Exact replay
oznacza tę samą immutable provider observation identity. Dwie obserwacje o tej
samej signature, ale innym locatorze, pozostają odrębne.

### D2.2. Structural canonical mutation

Tylko kompletna obserwacja:

```text
source_family = RawYellowstone
provider_role = PrimaryAuthority
pełny RawPumpMutationLocatorV1
pełny CanonicalPumpOrderKeyV1
provider provenance obecne
```

może utworzyć `StructuralCanonicalPumpMutationV1`.

Początkowy status ekonomiczny pozostaje `PendingAnchor`. PR1D nie certyfikuje
economics i nie używa NLN do quote math.

Brak locatora, order, provider ID/role albo captured-payload provenance w
obserwacji deklarującej primary jest typed `PrimaryRawCoverageIncomplete` dla
shadow proof. Nie wolno syntetyzować brakujących pól z receive order,
timestampu, claims ani historycznego resolvera. Niekompletność shadow proof
nie tłumi jednak istniejącej emisji primary raw na aktywnej ścieżce parenta.

### D2.3. Bounded stores bez witness veto

Ledger ma rozdzielone pasy:

1. primary canonical locator/watermark lane;
2. pending/noncanonical witness lane;
3. correlated witness evidence per canonical mutation;
4. provider/source conflict evidence;
5. immutable first-overflow evidence i liczniki completeness.

NLN ani secondary raw nie zużywa capacity zarezerwowanej dla primary
canonical lane. Saturacja witness/conflict lane:

```text
typed overflow
+ evidence_complete = false
+ first rejected observation retained
+ późniejszy eligible primary nadal canonical-applies
```

Saturacja primary lane jest typed `PrimaryRawCoverageIncomplete`; event nie
jest po cichu tracony i nie jest zastępowany receive-order eviction.

Żaden lock ledgera nie jest utrzymywany przez `.await`.

## D3. Identity i korelacja

### D3.1. ExactStructuralMatch

Dozwolone tylko, gdy witness niesie kompletny `RawPumpMutationLocatorV1`,
który mapuje się na dokładnie jedną raw mutation.

Sam `instruction_index`, stack height, mint, curve, side albo amount nie
wystarcza. Aktualny NLN `instruction_index` pozostaje partial hint, dopóki
payload nie dostarczy wszystkich pól pełnego locatora.

### D3.2. UniqueSignatureSingletonMatch

Dozwolone wyłącznie po deterministycznym zakończeniu bounded correlation
window, gdy:

- signature jest identyczna;
- deklarowany i zaobserwowany raw transaction inventory zawiera dokładnie
  jedną Pump mutation;
- ledger zachował dokładnie jeden uncorrelated parsed NLN Pump witness dla
  signature.

Raw nie czeka na to rozstrzygnięcie. Primary canonical mutation powstaje
natychmiast, a singleton correlation jest późniejszą decyzją witness-only.

### D3.3. Ambiguous

Wiele raw mutations przy locatorless/partial NLN daje `Ambiguous`. Claims nie
służą do wyboru jednej mutacji. Witness pozostaje nieprzypięty.

### D3.4. Unmatchable

NLN bez jednoznacznego raw match po bounded retention daje `Unmatchable`.
Nie powstaje canonical event NLN-only.

### D3.5. Symetria arrival order

`raw -> NLN` i `NLN -> raw` muszą zbiegać do tego samego końcowego:

- correlation outcome;
- agreement/conflict;
- evidence snapshot;
- `CandidateIntegrity`.

Jedyną celową różnicą czasową jest natychmiastowa emisja primary raw.

Jeżeli po provisional/retained witness pojawia się drugi locatorless NLN dla
tej samej signature, ledger nie może utrzymać fałszywego singleton match.
Finalizacja singleton odbywa się dopiero w bounded expiry/finalize boundary.

## D4. Parser i transport source-neutral locatora

Yellowstone adapter zawsze oblicza provenance hash z prost-encoded
`SubscribeUpdateTransaction` przekazanego do normalizacji. Nie jest to hash
oryginalnej ramki gRPC.

`InstructionProvenance` zostaje addytywnie rozszerzone o
`inner_instruction_path`. Dla top-level path jest pusty. Dla inner CPI path
jest deterministyczną pozycją w execution tree; brak wymaganej informacji nie
jest zastępowany stack height.

`BinaryParser::parse_transaction_bundle()` jest jedynym miejscem ustalającym
transaction-local raw Pump mutation inventory. Ten sam count i provenance
przechodzą przez:

```text
GeyserEvent
  -> InitializePoolEvent / TradeEvent
  -> CandidatePool / Seer IPC
  -> launcher Observation Ledger
  -> DetectedPool / PoolTransaction
```

Create nie może tracić `tx_index`, ordinala ani provenance. `Some(0)` pozostaje
`Some(0)`.

Brak jawnego `raw_transaction_mutation_count` jest
`PrimaryRawCoverageIncomplete`. `None` nie oznacza singletona ani kompletnego
inventory.

Wszystkie siblings jednego parsed bundle są przekazywane do bounded IPC FIFO
sekwencyjnie w canonical parser order. Równoległe `join_all` nie może
odwrócić ordinali przez mapping/replay await.

## D5. Miejsce ledgera i odcięcie NLN

Jedna współdzielona instancja `PumpObservationLedgerV1` należy do launcherowego
wrappera Seera i jest dostępna dla:

- IPC `PoolDetected`;
- IPC `Trade`;
- NLN Pump.fun create topic;
- NLN Pump.fun trade topic;
- okresowego deterministic expiry.

Ledger widzi NLN przed dotychczasowym semantic resolver/dedupe. Dotychczasowy
`NlnTradePoolIdentityResolver` nie może suppressować observation evidence.
Może zostać usunięty z aktywnej ścieżki lub pozostawiony wyłącznie jako
historyczny/testowy compatibility helper bez canonical emission.

Oba bezpośrednie calle:

```text
NLN trade -> to_trade_event -> emit_pool_transaction_to_event_bus
NLN replay -> to_trade_event -> emit_pool_transaction_to_event_bus
```

zostają usunięte z aktywnego runtime.

Poprawny raw primary przechodzi do istniejącego adaptera zgodnie z filtrami
parenta niezależnie od wyniku shadow ledgera. Secondary, NLN i providerless
observation nie wywołują adaptera canonical. Exact duplicate, ambiguous i
unmatchable pozostają klasyfikacjami ledgera; nie otrzymują nowej authority.

Launcher porównuje zaakceptowaną structural canonical mutation z towarzyszącym
IPC payloadem przed zapisaniem mutacji w ledgerze. Niezgodność signature,
locator/order, provider provenance, curve/mint, side, success albo amount jest
typed `PrimaryRawCoverageIncomplete`, nie może pozostawić w ledgerze fałszywej
shadow canonical mutation, ale nie zmienia emisji, którą wykonałby parent.

## D6. CandidateIntegrity i lifecycle registry

Typ `CandidateIntegrityOutcomeV1` obejmuje:

- `Ready`;
- `PrimaryRawCoverageIncomplete`;
- `AccountProviderConflict`;
- `SourceReconciliationConflict`;
- `AnchorMissing`;
- `EconomicsNonEvaluable`.

PR1D aktywnie produkuje pierwsze cztery. `AnchorMissing` i
`EconomicsNonEvaluable` pozostają typami dla PR2 i nie są w PR1D wyprowadzane
z nowej quote logiki.

Jeden bounded `CandidateIntegrityRegistry` należy do `OracleRuntime`, nie do
`PoolObservationSession`. Rekord:

- przeżywa usunięcie sesji;
- ma monotonic generation;
- zachowuje bieżący integrity outcome;
- zachowuje lifecycle phase;
- przechowuje bounded immutable evidence/audit markers;
- udostępnia synchroniczne snapshot/CAS;
- nigdy nie recomputuje MFS ani Gatekeeper policy.

Ta sama instancja registry jest przekazywana do launcherowego wrappera Seera.
Zapis sygnału następuje synchronicznie przed `NewPoolDetected` albo
`PoolTransaction`, gdy registry ma capacity. Niepowodzenie registry zapisuje
metrykę/evidence niekompletności, lecz nie blokuje canonical runtime emission
primary raw. Oracle broadcast lag nie może zostać zinterpretowany jako active
authority gate.

Pool i mint są aliasami tego samego rekordu po udowodnionej canonical pool
identity. Claims NLN nie mogą samodzielnie utworzyć aliasu authority.

## D7. Pełna macierz konfliktów i punkty linearizacji

| Faza shadow modelu | Punkt obserwacji | Wyliczona akcja (bez enforcementu) |
|---|---|---|
| `PRE_MFS` | przed `session.begin_evaluation()` | `would_block_pre_mfs` |
| `MFS_MATERIALIZED` | po materializacji | `would_abort_evaluation` |
| `EVALUATION_RUNNING` | po policy call | `would_abort_evaluation` |
| `TERMINAL_REJECT` | po historycznym terminalu | immutable verdict + audit marker |
| `TERMINAL_TIMEOUT` | po historycznym terminalu | immutable verdict + audit marker |
| `TERMINAL_BUY_NOT_SUBMITTED` | po historycznym BUY | `would_cancel_pre_submit` |
| `SUBMIT_STARTED` | po zarejestrowanym shadow transition | `would_require_reconciliation` |
| `CONFIRMED_OPEN_POSITION` | po zarejestrowanym shadow confirmation | `would_quarantine_position` |

### D7.1. Pre-MFS i evaluation fence

Active runtime może odczytać shadow `CandidateIntegrityGuard` przed MFS, aby
policzyć `would_block`/`would_abort`, ale brak `Ready`, conflict albo zmiana
generation nie zatrzymują `begin_evaluation`, materializacji, policy call ani
publikacji terminalnego wyniku. GatekeeperBuffer, verdict i reason chain
pozostają sterowane wyłącznie kontraktem parenta.

### D7.2. Terminal verdict

Terminal transition jest wykonywany przed pierwszym asynchronicznym
DecisionLogger/WAL side effect. Późny konflikt po `REJECT`/`TIMEOUT` nie zmienia
verdictu ani reason chain.

Technical failure nie jest zapisywany jako strategiczny rejected pool ani
Gatekeeper verdict. Jest wyłącznie shadow/audit markerem i nie blokuje
ponownej kandydatury w aktywnym runtime PR1D.

### D7.3. Pre-submit, submit i confirmation

`PreparedBuyRequest` nie niesie w PR1D integrity submit guarda. Active sender
zachowuje dokładnie parentowe preconditions, retry, klasyfikację błędów,
retention slotu i confirmation lifecycle. Registry może lokalnie zasymulować
`try_begin_submit`/`mark_confirmed` w testach shadow macierzy, ale
`TriggerComponent` ani `LiveTxSender` nie wywołują tych przejść i brak guarda
nie blokuje sender call.

Późny conflict po confirmation nie usuwa AccountStateCore, position slot ani
PostBuyRuntime, nie przerywa protective exit i nie wywołuje automatycznego
SELL. Powstaje wyłącznie shadow `would_quarantine_position`.

## D8. Account provider conflict

PR1C `AccountObservationDecisionV1` nie może być spłaszczony bez sygnału dla
CandidateIntegrity. `SameVersionDifferentHashConflict`, identity conflict oraz
typed evidence incompleteness są zachowywane przed konwersją do legacy
`AccountUpdateResult`.

`AccountProviderConflict` aktualizuje ten sam shadow lifecycle registry według
zaobserwowanej fazy. Canonical apply PR1C pozostaje bez zmian i nie jest
blokowany przez wynik CandidateIntegrity.

Po strategicznym terminalu AccountUpdate nadal przechodzi przez zachowany
bounded account arbiter w trybie evidence-only. Taka obserwacja może utworzyć
provider agreement/conflict i immutable lifecycle audit marker, ale nie może
mutować `AccountStateCore`, odtworzyć sesji ani uruchomić MFS. Cleanup usuwa
canonical runtime state, lecz nie usuwa arbitra/evidence przed końcem procesu.

## D9. No-wait boundary

NLN artifact writer nie używa `send(...).await` w receiver tasku. Enqueue jest
`try_send` do bounded lane.

Saturacja:

- nie blokuje receivera;
- daje typed/metryczny incomplete artifact segment;
- zachowuje first overflow metadata;
- nie degraduje raw Yellowstone coverage;
- nie przyznaje NLN authority.

Observation Ledger nie wykonuje I/O ani await. Durable/audit export jest
oddzielnym bounded consumerem.

## D10. Autorytatywna allowlista produkcyjnych callsite’ów

Zmiany produkcyjne PR1D są dozwolone wyłącznie w poniższych granicach.
Rozszerzenie listy wymaga aktualizacji tego ADR z uzasadnieniem przed edycją.

### Shared contracts i ledger

- `ghost-core/src/ingest_integrity.rs`
- nowy `ghost-core/src/pump_observation_ledger.rs`
- `ghost-core/src/lib.rs`
- `ghost-core/src/account_state_core/reducer.rs` wyłącznie evidence-only
  klasyfikacja po terminalnym cleanupie i zachowanie bounded arbitra

### Yellowstone/parser/IPC propagation

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/ipc.rs`
- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/nln_program_streams.rs`
- `off-chain/components/seer/src/pumpportal_connection.rs` wyłącznie mechaniczne
  uzupełnienie addytywnego pola provenance w konstruktorach compatibility
- `off-chain/components/seer/src/enhanced_builder.rs` wyłącznie mechaniczne
  uzupełnienie addytywnego pola provenance w konstruktorach compatibility
- `off-chain/components/seer/src/helius_websocket_adapter.rs` wyłącznie
  mechaniczne `observation_provenance = None` dla nie-Yellowstone adaptera
- `off-chain/components/seer/src/websocket_connection.rs` wyłącznie
  mechaniczne `observation_provenance = None` dla compatibility WebSocket
- `off-chain/components/seer/src/hot_path_harness.rs` wyłącznie mechaniczne
  uzupełnienie monotonic provenance timestamp oraz prawdziwa asercja jednego
  provenance prost-encode niezależnie od wyłączonego MPCF capture
- `off-chain/components/seer/tests/time_contract.rs` wyłącznie mechaniczne
  `observation_provenance = None` w konstruktorze testowym
- `off-chain/components/seer/tests/source_router.rs` wyłącznie mechaniczne
  `observation_provenance = None` w dwóch konstruktorach testowych

### Launcher ingest boundary

- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`

### CandidateIntegrity/lifecycle

- nowy `ghost-launcher/src/candidate_integrity.rs`
- `ghost-launcher/src/lib.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs` wyłącznie mechaniczne
  zachowanie `submit_guard = None` w niezwiązanych konstruktorach
- `ghost-launcher/src/main.rs` wyłącznie przekazanie jednej współdzielonej
  instancji `CandidateIntegrityRegistry` do Seera

`gatekeeper.rs`, quote modules, strategy, MFS schema i post-buy policy nie są
na allowliście produkcyjnej. Post-buy może otrzymać wyłącznie test dowodzący,
że protective exit pozostaje aktywny; nie zmieniamy jego policy.

### Wąska korekta bazowego builda

Clean base `a7a7bf1` ma niezależny błąd E0425:

```text
ghost-launcher/src/components/post_buy_runtime.rs:6772
cannot find value `bonding_curve` in this scope
```

Jeżeli `origin/main` nie otrzyma osobnej poprawki przed finalną walidacją,
PR1D może objąć wyłącznie minimalną, formalnie sklasyfikowaną korektę tego
jednego callsite’u, bez zmiany zachowania post-buy. Nie wolno mieszać innych
baseline fixture repairs.

## D11. Hard-gate differential corpus

Przed pierwszą zmianą kodu produkcyjnego zostaje zamrożony:

```text
ghost-core/tests/fixtures/pump_observation_ledger_v1/
  README.md
  pump_observation_differential_corpus_v1.jsonl
ghost-core/tests/pump_observation_ledger_corpus_tests.rs
```

Corpus obejmuje co najmniej:

- raw-only immediate;
- exact raw/NLN w obu arrival orders;
- singleton w obu arrival orders;
- NLN-only expiry/unmatchable;
- wiele raw mutations jednej signature;
- locatorless ambiguity;
- exact locator pośród wielu;
- każdy materialny claim conflict;
- Unknown kontra concrete;
- różne cross-source payload hashes bez semantic conflict;
- reconnect duplicate;
- secondary raw same/different;
- `tx_index=0`;
- missing primary order;
- arrival permutations;
- witness saturation bez primary veto;
- create + initial buy jednej signature;
- account provider conflict handoff;
- wszystkie fazy lifecycle.

README zamraża BLAKE3 dokładnych bajtów fixture. Test corpus w pierwszym kroku
waliduje tylko schemat, inventory oraz digest; executable replay zostaje
dołączony po implementacji ledgera bez zmiany fixture.

## D12. Weryfikacja i merge gates

Wymagane testy:

1. unit i frozen-corpus replay ledgera;
2. raw/NLN obie kolejności;
3. multi-mutation same signature przez parser -> IPC -> launcher -> ledger;
4. exact/singleton/ambiguous/unmatchable;
5. material conflict matrix i Unknown;
6. secondary/NLN saturation bez authority veto;
7. no direct NLN canonical callsite;
8. no-wait artifact sink;
9. CandidateIntegrity pre-MFS/evaluation shadow `would_*` matrix;
10. terminal Reject/Timeout immutability;
11. pre-submit conflict kontra submit CAS w lokalnym shadow modelu;
12. submit-started shadow reconciliation evidence;
13. confirmed-position shadow quarantine marker bez wpływu na protective exit;
14. account provider conflict handoff;
15. raw-only parity i brak quote-math diff.

Wymagane bramki końcowe:

```text
cargo fmt --all --check
git diff --check
cargo test -p ghost-core --lib pump_observation_ledger
cargo test -p ghost-core --test pump_observation_ledger_corpus_tests
cargo test -p ghost-core --test account_observation_arbiter_corpus_tests
cargo test -p seer [targeted parser/sibling/no-wait/parity]
cargo test -p ghost-launcher --lib pr1d
cargo check -p ghost-brain --tests
timeout 900s cargo build --release --workspace
```

Pełne package/workspace suites są porównywane z podpisanymi baseline failure
signatures. Zielony release build jest bezwarunkowy, ponieważ baseline release
był zielony.

## D13. Rollback

Rollbackiem jest revert pojedynczego finalnego commita PR1D. Typy i serde są
addytywne. Nie ma migracji danych, production authority cutover ani quote
schema cutover. Po rewercie wraca poprzednia ścieżka, dlatego merge wymaga
pełnego dowodu, że NLN canonical bypass został usunięty i raw-only parity
zachowane.

## D14. Remediacja wyników końcowego audytu konfrontacyjnego

Przed finalnymi bramkami obowiązuje następująca zamrożona mapa korekt:

1. usunąć globalny `seen_sigs` z transportowego fan-in; signature nigdy nie
   jest transportowym dedupe key;
2. preserve secondary-first i primary-first jako dwie obserwacje, a jedną
   signature z wieloma mutacjami jako wiele locatorów;
3. dopuścić mapping/watch/bootstrap/replay wyłącznie z kompletnego primary raw;
   secondary i EntryAnchor CPI scan pozostają parser/evidence-only;
4. sprawdzać IPC semantic payload przeciw observation przed `ledger.observe`;
5. próbować zapisać `CandidateIntegrity` bezpośrednio do shared registry przed
   canonical event-bus emission, ale nigdy nie tłumić parentowej emisji primary
   przez błąd ledgera, registry, receipt staging albo inventory sealing;
6. alias conflict atomowo fail-closuje rekordy wskazane przez oba aliasy;
7. bounded lifecycle audit zachowuje licznik i pierwszy odrzucony immutable
   marker;
8. stale/terminal AccountUpdate przechodzi evidence-only arbitraż bez state
   apply;
9. usunąć CandidateIntegrity submit guard z aktywnego TriggerComponent i
   zachować parentowe dispatch preconditions oraz klasyfikację sender errors;
10. zapisywać pre-submit/submit/confirmed conflict wyłącznie jako shadow
    `would_cancel`/`would_reconcile`/`would_quarantine`, bez sterowania senderem,
    capacity ani protective exits;
11. sibling Pump mutations są enqueueowane w canonical order.

Te korekty nie rozszerzają PR1D o NLN authority, quote math, Gatekeeper policy,
model features, proof-based local-gap recovery ani legacy fallback.

## D15. OWNER-APPROVED PERFORMANCE WAIVER

Formalne względne progi wydajności PR1D nie zostały spełnione:

- throughput geometric mean ratio: `0.983083101`;
- throughput lower one-sided 95% CI: `0.973791127 < 0.98`;
- receive-to-normalize p99 geometric mean ratio: `1.158026220`;
- receive-to-normalize p99 upper one-sided 95% CI:
  `1.214104079 > 1.05`;
- missing events: `0` we wszystkich `50` formalnych przebiegach.

Jest to jawny wyjątek zatwierdzony przez właściciela projektu, a nie `PASS`
bramki i nie deklaracja braku regresji. Pełne surowe logi, snapshoty i hashe
pomiarów pozostają zachowane; nie wolno ich usuwać, przepisywać ani
przedstawiać jako wyniku zielonego.

Właściciel akceptuje mikrosekundową regresję dla PR1D, ponieważ wymagane
naprawy integralności, exactly-once i downstream-apply ordering są
obowiązkowe, testy celowane przechodzą, parity V1/V2 jest zachowane, release
workspace build przechodzi i nie utracono żadnego eventu.

Waiver obejmuje wyłącznie dwa względne progi performance. Nie obejmuje:

- poprawności semantycznej i canonical parity;
- zero missing events, no receiver blocking ani bounded memory;
- `CandidateIntegrity` i downstream-apply happens-before;
- rozdzielenia primary raw authority od secondary/NLN witness;
- poprawności testów, shadow/observe-only ani przyszłego live-readiness
  review.

PR1D pozostaje shadow/observe-only. Waiver nie autoryzuje live promotion,
production authority cutover, zmian quote math, strategii ani Gatekeeper
policy.

## D16. Remediacja niezależnego review Draft PR #85

Review wykazało, że pierwotny Draft przedwcześnie podłączył lokalny model
`CandidateIntegrity` do aktywnego runtime. Korekta przywraca kontrakt planu:

1. błędy observation metadata, boundary validation, ledger/registry capacity,
   receipt staging i inventory sealing zapisują shadow evidence oraz metryki,
   ale nie suppressują parentowej emisji poprawnego primary raw;
   w Seerze aktywne mapowanie/emisja jest dlatego oparte na roli
   `PrimaryAuthority`, a nie na kompletności shadow locatora/inventory;
2. secondary raw, parsed NLN i providerless observation pozostają witness-only
   i tworzą zero canonical `NewPoolDetected`/`PoolTransaction`;
3. MFS, Gatekeeper verdict/reason chain, capacity, TriggerComponent,
   LiveTxSender, confirmation i protective exits nie są sterowane przez
   CandidateIntegrity;
4. jeden prywatny bounded apply fence nadal wylicza shadow downstream-apply
   proof, lecz jego failure nie steruje aktywnym flow;
5. typed apply outcome opisuje rzeczywistą mutację właściciela stanu:
   `Accepted`, `Unkeyable` i dust/not-counted są `AppliedNewMutation`, ponieważ
   parentowy `TxIntelligence` przyjmuje nową obserwację; exact replay jest
   `Duplicate`, terminal session jest `Terminal`, a samo
   `BufferedHistory`/`PendingLive` jest `Ignored`;
6. istniejąca rejestracja InitializePool jest `Duplicate`; brak bootstrapu
   albo błąd instalacji jest `Failed`;
7. nierozwiązany `SecondaryRaw` po istniejącym `correlation_window_ns` dostaje
   typed `SecondaryWitnessExpired`, nie tworzy canonical mutation ani `Ready`,
   zwalnia pending capacity i nie uzyskuje recovery authority. Późniejszy
   primary nadal jest jedynym structural authority.

Testy macierzy lifecycle opisują od tej korekty wyłącznie wyliczone shadow
actions. Nazwy `BlockBeforeMfs`, `InterruptEvaluation`,
`CancelExecutionBeforeSubmit`, `ReconciliationRequired` i
`ConfirmedPositionQuarantined` pozostają lokalną taksonomią registry; nie są
aktywnymi akcjami runtime w PR1D.

## D17. Receipt remediacji Draft PR #85

Walidacja zamrożonego diffu po korekcie potwierdziła:

- wszystkie testy celowane PR1D ledgera, korelacji, CandidateIntegrity,
  observe-only parity, typed apply, downstream-apply ordering oraz expiry
  `SecondaryRaw` przechodzą;
- zamrożone digests pozostają bez zmian:
  - V1:
    `549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202`;
  - V2:
    `507b13704d5b90c3f724a395acbf0d0cc55fdc37a83fcb95cf67cceb6247569f`;
- obowiązkowy isolated global-counter contract przechodzi;
- `cargo fmt --all --check` i `git diff --check` przechodzą;
- `cargo build --release --workspace` przechodzi bez waivera.

Pełne package/workspace suites pozostają czerwone wyłącznie w zweryfikowanych,
odziedziczonych klasach:

- `ghost-core`: `InvalidTagEncoding(104)` w
  `foundational_types_serialize_and_deserialize_roundtrip`;
- Seer: identyczne względem czystego parenta `a7a7bf1` historyczne failures
  PumpPortal/synthetic-WAL/mapping oraz `source_router`;
- Trigger: dwa braki `status_uuid` oraz
  `presigned.size_bytes < 700`;
- Ghost Launcher/workspace: testowe `PoolTransaction` `E0063` z identycznym
  zestawem brakujących pól; zweryfikowane literały istniały już na
  `88aa1b7`, a PR1D ich nie modyfikuje.

Pełna bramka Seera ujawniła pięć testów, które nadal modelowały aktywną
primary mapping/emission przez providerless fixture. Fixture'y zostały
mechanicznie przeniesione na jawny `PrimaryAuthority`; pełne nazwy modułowe
każdego testu przechodzą, a ponowiona bramka Seera zawiera już wyłącznie
parentowe failure signatures. Produkcja nie została przy tej korekcie
zmieniona.
