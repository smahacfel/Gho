PLAN WYKONAWCZY NAPRAWY GRANICY INGEST–STATE–QUOTE

Status: "READY FOR IMPLEMENTATION / SHADOW-FIRST / NO LIVE PROMOTION"
Data: "2026-07-24"
Repozytorium: "smahacfel/Gho"
Baseline: "a12ef9cfb7199d44841cde27be2ecd8af13e2f3f"
Zakres: siedem błędów integralności ingestu, stanu i ekonomiki
Struktura realizacji: baseline receipt + maksymalnie dwa średnie PR-y

Aktualny "main" kończy się na commicie wprowadzającym typowany kontrakt Pump V2 i quote parity prerequisite. Ten commit stanowi bazę planowanej realizacji.

---

1. Cel nadrzędny

Celem jest usunięcie siedmiu powiązanych błędów:

1. niejednoznacznej i błędnej semantyki "sol_amount_lamports";
2. błędnego quote math albo roundingu;
3. podwójnego liczenia tej samej mutacji z raw Yellowstone i parsed NLN;
4. nieidempotentnego stosowania "AccountUpdate";
5. blokowania receivera przez pełne kolejki i writer JSONL;
6. braku jednoznacznego arbitrażu między raw Yellowstone i parsed NLN;
7. braku transaction-local post-state anchor.

Docelowy przepływ:

PRIMARY RAW YELLOWSTONE TRANSACTION ─────────┐
PRIMARY RAW YELLOWSTONE ACCOUNT UPDATE ──────┼─→ Observation Ledger
SECONDARY RAW YELLOWSTONE OBSERVATION ───────┤
PARSED NLN WITNESS ──────────────────────────┘
                                                     │
                                                     ▼
                                      deterministic reconciliation
                                                     │
                         ┌───────────────────────────┴─────────────────────────┐
                         ▼                                                     ▼
             Structural Canonical Mutation                          conflict/gap evidence
                         │
                         ▼
           Transaction-Local Certification
                         │
                ┌────────┴────────┐
                ▼                 ▼
             Exact          NonEvaluable
                │
                ▼
      AccountStateCore / MFS / Gatekeeper
      execution valuation / PM / accounting

Najważniejsza zmiana polega na rozdzieleniu:

obserwacji providera
≠
canonical mutation
≠
certyfikowanej ekonomiki
≠
kosztu transakcyjnego

---

2. Twardy kontrakt braku regresji

„Ani jednej regresji” oznacza:

2.1. Przypadki niedotknięte naprawą

Dla rekordów, których nie dotyczy żaden z siedmiu błędów, wymagamy bit-for-bit parity dla:

- liczby i kolejności canonical mutations;
- canonical identities;
- canonical state hashes;
- "MaterializedFeatureSet";
- Gatekeeper verdictów;
- reason chains;
- lifecycle Position Managera;
- terminal outcomes;
- accounting;
- pól istniejącego schematu JSONL;
- replay checksums.

2.2. Przypadki naprawiane

Każda dozwolona różnica musi mieć dokładnie jedną klasyfikację:

AMBIGUOUS_LEGACY_SOL_AMOUNT
QUOTE_MATH_OR_ROUNDING_REPAIR
DUPLICATE_SOURCE_OBSERVATION
ACCOUNT_OBSERVATION_DUPLICATE
ACCOUNT_OBSERVATION_CONFLICT
PRIMARY_RAW_COVERAGE_GAP
SOURCE_RECONCILIATION_CONFLICT
MISSING_TRANSACTION_LOCAL_ANCHOR
TRANSACTION_TRAJECTORY_MISMATCH

Każda różnica musi zawierać:

- canonical locator;
- fixture albo raw replay evidence;
- wartość przed zmianą;
- wartość po zmianie;
- konkretną klasyfikację;
- wskazanie pola naprawionego przez zmianę;
- dowód braku dodatkowych różnic.

Twarde bramki:

unclassified_differences = 0
unexpected_verdict_differences = 0
unexpected_lifecycle_differences = 0
duplicate_canonical_mutations = 0
duplicate_account_state_mutations = 0
silent_primary_raw_gaps = 0
false_exact_trajectories = 0
legacy_math_authority_calls = 0

Nie ma runtime fallbacku pojedynczego eventu do starego zachowania.

Rollback oznacza wyłącznie uruchomienie poprzedniego, zamrożonego binary i configu.

---

3. Granice zakresu

3.1. Zmieniamy

Zmiany obejmują wyłącznie:

- transport i normalizację Yellowstone;
- propagację provenance oraz "txn_signature";
- bounded observation ledger;
- idempotencję "AccountUpdate";
- deduplikację i reconcyliację raw/NLN;
- techniczną integralność kandydata przed MFS;
- transaction-local trajectory certification;
- aktywne authority callsite’y quote/valuation/accounting;
- writer JSONL i gap telemetry;
- replay i differential parity.

3.2. Nie zmieniamy

Nie zmieniamy:

- sygnałów strategii;
- progów strategii;
- kolejności Gatekeepera;
- logiki score;
- "MaterializedFeatureSet" jako SSOT;
- semantyki terminalnych "BUY/REJECT/TIMEOUT";
- Position Manager ownership;
- shadow/live boundary;
- sizingu;
- route authorization;
- progów TP/SL/emergency exit;
- historycznych terminalnych decyzji;
- niespokrewnionych builderów;
- legacy simulatorów, o ile pozostają poza aktywną authority.

W szczególności plan nie wprowadza żadnych wartości typu:

TP +10%
SL -5%

Istniejące skonfigurowane progi pozostają bez zmian. Zmienia się jedynie źródło wartości, na której są oceniane.

---

4. Nienaruszalne inwarianty

1. Primary raw Yellowstone jest jedynym live state i event authority.
2. Secondary raw Yellowstone jest witness/recovery source, nie konkurencyjnym authority.
3. Parsed NLN jest witness/enrichment lane.
4. NLN nigdy samodzielnie nie tworzy canonical market mutation.
5. Runtime nie czeka na NLN przed emisją poprawnego raw eventu.
6. Source ID, timestamp i receive sequence nie należą do canonical identity.
7. "recv_seq" nie jest dowodem nowej mutacji chain state.
8. Identyczna mutacja obserwowana przez dwóch providerów zmienia state dokładnie raz.
9. Brak anchoru oznacza "NonEvaluable", nigdy syntetyczny exact state.
10. Niepełna ekonomika nie może zasilać executable valuation ani PnL.
11. MFS po materializacji pozostaje niemutowalny.
12. Późniejszy witness albo anchor nie może przepisać terminalnej historii.
13. Submit nie oznacza confirmation.
14. Unknown confirmation nie oznacza success.
15. Receiver nigdy nie czeka na writer JSONL.
16. Żaden lock nie jest trzymany przez ".await".
17. Nie powstaje drugi równoległy quote contract.
18. Transaction costs są rozliczane raz na signature albo execution attempt.
19. "tx_index=0" jest poprawną wartością, nie brakiem danych.
20. Żadna ścieżka nie deduplikuje wyłącznie po sygnaturze.

---

5. Jawne role providerów

Aktualny multi-provider fan-in musi otrzymać deterministyczne role.

enum RawProviderRoleV1 {
    PrimaryAuthority,
    SecondaryWitness,
}

Konfiguracja produkcyjna musi zawierać dokładnie jednego:

primary_raw_provider_id

oraz zero lub więcej:

secondary_raw_provider_ids

Reguły:

primary raw:
  może emitować StructuralCanonicalMutation;
  może mutować AccountStateCore po przejściu arbitra;
  jest źródłem canonical order.

secondary raw:
  dopisuje provenance;
  wykrywa provider conflict;
  może zasilać kontrolowany recovery po zadeklarowanej luce;
  nie mutuje live state w hot path.

parsed NLN:
  jest semantic witness;
  nie mutuje AccountStateCore;
  nie emituje canonical PoolTransaction;
  nie odblokowuje execution.

Nie obowiązuje zasada „pierwszy provider wygrywa”.

Kolejność arrival nie może decydować o canonical truth.

Jeżeli primary raw ma lukę:

1. luka zostaje zadeklarowana;
2. kandydaci przecinający lukę nie uzyskują statusu integrity-ready;
3. secondary raw albo raw backfill może odbudować ledger;
4. readiness jest przywracane dopiero po reconcyliacji;
5. recovery nie przepisuje historycznej terminalnej decyzji.

---

6. Kontrakty identity, order, semantics i provenance

6.1. Raw structural locator

Canonical locator identyfikuje miejsce mutacji w drzewie wykonania transakcji.

struct RawPumpMutationLocatorV1 {
    program_id: Pubkey,
    signature: Signature,
    outer_instruction_index: u16,
    inner_instruction_path: Vec<u16>,
    semantic_event_ordinal: u16,
}

Locator nie zawiera:

- "source_id";
- "provider_id";
- "slot";
- "tx_index";
- "curve";
- "mint";
- "semantic_kind";
- side;
- amount;
- timestampa;
- receive sequence.

"curve", "mint", side i semantic kind są danymi, które mogą być sprzeczne między providerami. Nie mogą więc definiować identity używanego do wykrywania konfliktu.

6.2. Canonical order

struct CanonicalPumpOrderKeyV1 {
    slot: u64,
    tx_index: u32,
    outer_instruction_index: u16,
    inner_instruction_path: Vec<u16>,
    semantic_event_ordinal: u16,
}

Zasady:

- order pochodzi wyłącznie z primary raw;
- "tx_index=0" pozostaje "0";
- order nie może być syntetyzowany z czasu;
- order nie może być syntetyzowany z receive sequence;
- rozbieżny slot albo "tx_index" dla tego samego locatora daje typed provider conflict;
- event bez pełnego raw order może pozostać obserwacją, ale nie uzyskuje statusu exact ordered mutation.

6.3. Semantic payload

struct PumpMutationSemanticPayloadV1 {
    curve: Pubkey,
    mint: Pubkey,
    route_variant: PumpRouteVariant,
    side: PumpTradeSideV1,
    success: bool,
    token_amount_units: u64,
    instruction_limit: Option<PumpInstructionLimitV1>,
    reported_curve_quote_lamports: Option<u64>,
    reported_wallet_delta_lamports: Option<u64>,
    reported_fee_breakdown: Option<Vec<ProgramFeeCharge>>,
}

Pola "reported_*" są obserwacją providera. Nie stają się authority bez transaction-local certification.

6.4. Provenance

struct ObservationProvenanceV1 {
    source_kind: SourceKindV1,
    source_id: String,
    provider_id: String,
    schema_id: String,
    payload_hash: [u8; 32],
    received_at_monotonic_ns: u64,
}

"received_at_monotonic_ns":

- jest zachowywane diagnostycznie;
- nie należy do identity;
- nie należy do canonical payload hash;
- nie należy do MFS hash;
- nie należy do state hash;
- nie należy do differential parity checksum.

---

7. Korelacja parsed NLN z raw eventem

NLN może nie dostarczyć pełnego inner instruction path albo semantic ordinal. Dlatego nie wolno udawać, że parsed witness zawsze posiada canonical locator.

enum ParsedWitnessCorrelationOutcomeV1 {
    ExactStructuralMatch,
    UniqueSignatureSingletonMatch,
    Unmatchable,
    Ambiguous,
}

7.1. Exact structural match

Dozwolony, gdy NLN dostarcza wystarczające indeksy, które jednoznacznie mapują się na jeden raw locator.

7.2. Unique signature singleton match

Dozwolony wyłącznie wtedy, gdy:

- signature jest identyczna;
- w raw transaction istnieje dokładnie jedna Pump mutation;
- NLN opisuje dokładnie jeden Pump event.

Dopasowanie nie może używać curve, mint, side ani amount jako klucza, ponieważ właśnie te pola muszą być później porównane pod kątem konfliktu.

7.3. Ambiguous

Jeżeli transakcja zawiera kilka Pump mutations, a NLN nie dostarcza strukturalnego locatora:

AMBIGUOUS_PARSED_WITNESS

Witness pozostaje w ledgerze, ale nie jest przypinany do żadnej mutacji.

7.4. Unmatchable

Jeżeli nie istnieje raw event, do którego witness może zostać jednoznacznie przypisany:

UNMATCHABLE_PARSED_WITNESS

Nie powstaje canonical event NLN-only.

---

8. Observation Ledger i dwufazowa canonicalizacja

Powstaje bounded ledger obserwacji.

struct ObservedPumpMutationV1 {
    locator_hint: Option<RawPumpMutationLocatorV1>,
    semantic_payload: PumpMutationSemanticPayloadV1,
    provenance: ObservationProvenanceV1,
}

8.1. Faza pierwsza — structural canonical

Primary raw po pełnej walidacji tworzy:

struct StructuralCanonicalPumpMutationV1 {
    locator: RawPumpMutationLocatorV1,
    order: CanonicalPumpOrderKeyV1,
    semantic_payload: PumpMutationSemanticPayloadV1,
    primary_raw_provenance: ObservationProvenanceV1,
    economics_status: PumpEconomicCertificationStatusV1,
}

Początkowy status:

PendingAnchor

Structural canonical mutation może:

- zwiększyć canonical buy/sell event count dokładnie raz;
- uczestniczyć w structural flow;
- zostać zapisana w ledgerze;
- otrzymać witness provenance.

Nie może jeszcze:

- zasilać exact SOL volume;
- zasilać executable PnL;
- zasilać TP/SL valuation;
- zasilać terminal accounting;
- zostać użyta jako exact quote evidence;
- odblokować execution wymagającego exact economics.

8.2. Faza druga — economic certification

Po uzyskaniu independent pre-state oraz transaction-local post-state:

enum PumpEconomicCertificationStatusV1 {
    PendingAnchor,
    Exact(AnchoredPumpQuoteV1),
    NonEvaluable(PumpEconomicGapReasonV1),
    Conflict(PumpEconomicConflictReasonV1),
}

Dzięki temu raw transaction nie musi czekać na AccountUpdate, ale nie jest przedwcześnie uznawana za ekonomicznie kompletną.

---

9. Typed semantyka amount

9.1. Instruction fact

struct PumpInstructionFactV1 {
    locator: RawPumpMutationLocatorV1,
    order: CanonicalPumpOrderKeyV1,
    route_variant: PumpRouteVariant,
    token_amount_units: u64,
    instruction_limit: Option<PumpInstructionLimitV1>,
}

enum PumpInstructionLimitV1 {
    MaxWalletDebitLamports(u64),
    MinWalletCreditLamports(u64),
    ExactQuoteInputLamports(u64),
    MinTokenOutputUnits(u64),
}

Instruction limit nigdy nie jest traktowany jako:

- curve quote;
- wallet settlement;
- transaction cost;
- market volume.

9.2. Istniejący quote contract pozostaje SSOT

Repozytorium posiada już:

PumpQuoteV1
PumpRouteVariant
ProgramStateTransition
ProgramSettlement
ProgramFeeSchedule
RuntimeProgramFeeScheduleRegistryV1
TransactionCosts

Kontrakt jawnie oddziela transition krzywej, program settlement i transaction costs oraz nie używa historycznego "FEE_BPS=100" jako runtime authority.

"PumpQuoteV1" zawiera route, fee authority, reserve transition, token amount, curve quote, program settlement i instruction limit checks. Nie tworzymy drugiego modelu tych danych.

Dodajemy wyłącznie wrapper certyfikacji:

struct AnchoredPumpQuoteV1 {
    quote: PumpQuoteV1,
    math_version_id: String,
    pre_anchor_ref: CurveStateAnchorRefV1,
    post_anchor_ref: CurveStateAnchorRefV1,
    transaction_cost_ref: Option<TransactionCostRefV1>,
    certification_hash: [u8; 32],
}

Wrapper nie powiela:

- reserve transition;
- settlement;
- fee breakdown;
- wallet debit/credit;
- limit checks.

9.3. Transaction costs

Istniejący "TransactionCosts" pozostaje typem kosztów transakcyjnych.

Powstaje signature-level ledger:

struct SignatureCostLedgerEntryV1 {
    signature: Signature,
    execution_attempt_id: Option<String>,
    costs: TransactionCosts,
}

Zasady:

- jedna observed transaction ma jeden wpis kosztowy;
- jeden execution attempt ma jeden wpis kosztowy;
- kilka Pump mutations w jednej transakcji nie powoduje kilkukrotnego odjęcia kosztów;
- market-flow volume nie zawiera base fee, tip ani priority fee;
- program settlement nie zawiera kosztów transakcyjnych;
- position accounting odwołuje się do signature/attempt cost entry dokładnie raz.

9.4. Migracja "sol_amount_lamports"

1. Pole pozostaje czytelne w historycznej deserializacji.
2. Historyczny rekord otrzymuje status:

LegacyAmbiguous

3. Legacy rekord nie może odblokować exact valuation ani execution.
4. Nowe canonical eventy nie zapisują cap/floor jako "sol_amount_lamports".
5. Market SOL flow korzysta wyłącznie z:

PumpQuoteV1.curve_quote_amount

6. Entry cost korzysta wyłącznie z:

PumpQuoteV1.program_settlement.wallet_debit_or_credit
+
jednokrotnego transaction cost entry

7. Exit proceeds korzysta z net program credit, a transaction costs są rozliczane osobno.
8. Builder korzysta z typed instruction limit.
9. CI zabrania odczytów "sol_amount_lamports" poza:
   - legacy deserializerem;
   - replay compatibility adapterem;
   - jawnie nieautorytatywnym diagnostic tooling.

---

10. Idempotentne AccountUpdate

Aktualny "MonotonicUpdateGuard" używa:

slot
write_version
recv_seq

i akceptuje ten sam slot oraz "write_version", jeżeli drugi event ma wyższy lokalny "recv_seq".

Reducer po zaakceptowaniu update’u zwiększa liczniki i ponownie wylicza reserve velocity, dlatego ponowne zastosowanie tej samej mutacji realnie zmienia state evidence.

10.1. Rozdzielenie version od payloadu

struct AccountMutationVersionV1 {
    pubkey: Pubkey,
    slot: u64,
    write_version: Option<u64>,
}

struct AccountObservationV1 {
    version: AccountMutationVersionV1,
    data_hash_blake3: [u8; 32],
    txn_signature: Option<Signature>,
    owner: Pubkey,
    data: Vec<u8>,
    provenance: ObservationProvenanceV1,
}

Hash nie jest częścią version key. Dzięki temu ten sam version key z różnym hashem jest wykrywany jako konflikt.

10.2. Wynik arbitrażu

enum AccountObservationOutcomeV1 {
    AppliedNewMutation,
    DuplicateObservation,
    StaleObservation,
    ProviderConflict,
    UnorderableWithoutWriteVersion,
    SecondaryWitnessRecorded,
}

10.3. Reguły

Warunek| Wynik| Mutacja state
ten sam pubkey/slot/write_version/hash| duplicate| nie
ten sam pubkey/slot/write_version, różny hash| provider conflict| nie
starszy slot| stale| nie
ten sam slot, niższy write version| stale| nie
ten sam slot, wyższy write version z primary| applied| tak
nowszy slot z primary| applied| tak
"write_version=None", ten sam slot i hash| duplicate| nie
"write_version=None", ten sam slot i różny hash| unorderable/conflict| nie
identyczna obserwacja z secondary| witness| nie
nowszy event wyłącznie z secondary| witness/pending recovery| nie

"recv_seq" pozostaje wyłącznie transport metadata.

10.4. Liczniki

Tylko "AppliedNewMutation" może:

- zmienić reserves;
- zmienić canonical state;
- zwiększyć "update_count";
- zwiększyć "data_change_count";
- zwiększyć state-facing "observation_count";
- wpłynąć na reserve velocity;
- materializować feature evidence.

Provider observation count, duplicate count i conflict count są osobnymi metrykami Observation Ledger i nie trafiają do strategii.

W tej serii nie zmieniamy samego wzoru reserve velocity ani jego osi czasu.

---

11. Transaction-local post-state anchor

11.1. Propagacja "txn_signature"

Aktualny "GeyserEvent::AccountUpdate" zachowuje slot, write version, pubkey, data i owner, ale nie zachowuje transaction signature.

Upstream Yellowstone "SubscribeUpdateAccountInfo" posiada opcjonalne pole:

optional bytes txn_signature = 8;

Propagujemy to pole:

SubscribeUpdateAccountInfo.txn_signature
→ PumpEvent::AccountUpdate
→ GeyserEvent::AccountUpdate
→ IPC AccountUpdate
→ AccountStateUpdate
→ AccountObservationLedger

"None" pozostaje "None".

Nie stosujemy:

- dopasowania po czasie;
- dopasowania po receive sequence;
- dopasowania po samym slocie;
- dopasowania po tuple reserves;
- dopasowania po „najbliższym” AccountUpdate.

Jeżeli przypięta w repo wersja generated proto nie wystawia pola mimo obecności w upstream schema, dopuszczalna jest wyłącznie minimalna, izolowana aktualizacja pary:

yellowstone-grpc-client
yellowstone-grpc-proto

Aktualizacja musi przejść compile, wire compatibility oraz replay parity przed dalszym wdrożeniem. Nie wolno implementować lokalnego heurystycznego substytutu.

11.2. Independent pre-state anchor

Nie wyprowadzamy pre-state przez cofnięcie finalnego stanu tym samym wzorem, który następnie próbujemy zwalidować.

Pre-state musi pochodzić z niezależnego chain evidence:

1. poprzedniego transaction-local final anchor tej samej curve;
2. canonical AccountUpdate poprzedzającego transakcję w porządku chain;
3. exact route-defined initialization dla transakcji create;
4. exact account snapshot z poprawnym slot/order boundary, jeżeli został pobrany przed ocenianą mutacją.

Speculative bootstrap nie jest independent pre-state anchor.

11.3. Final post-state anchor

struct CurveStateAnchorV1 {
    curve: Pubkey,
    slot: u64,
    write_version: Option<u64>,
    txn_signature: Signature,
    owner: Pubkey,
    data_hash_blake3: [u8; 32],
    decoded_state: PumpReserveState,
    provenance: ObservationProvenanceV1,
}

Final anchor wymaga:

- zgodnej signature;
- zgodnej curve pubkey;
- poprawnego ownera;
- poprawnego layoutu;
- jednoznacznego data hash;
- successful transaction;
- braku provider conflict.

Jeżeli w tej samej transakcji istnieje kilka update’ów curve:

- wybieramy najwyższy jednoznaczny "write_version";
- niższe write versions pozostają intermediate observations;
- dwa różne hashe dla tego samego najwyższego write version oznaczają konflikt.

11.4. Trajectory certification

enum TransactionLocalCurveTrajectoryV1 {
    Exact {
        signature: Signature,
        slot: u64,
        curve: Pubkey,
        pre_anchor: CurveStateAnchorRefV1,
        post_anchor: CurveStateAnchorRefV1,
        ordered_mutations: Vec<AnchoredPumpMutationV1>,
        math_version_id: String,
        certification_hash: [u8; 32],
    },
    NonEvaluable {
        signature: Signature,
        curve: Pubkey,
        reason: TransactionTrajectoryGapReasonV1,
    },
    Conflict {
        signature: Signature,
        curve: Pubkey,
        reason: TransactionTrajectoryConflictReasonV1,
    },
}

Budowa "Exact" wymaga:

1. independent pre-state;
2. pełnego raw inventory wszystkich Pump mutations zapisujących curve;
3. pełnego outer/inner order;
4. successful transaction;
5. finalnego AccountUpdate z tą samą signature;
6. znanej route semantics;
7. znanego fee schedule;
8. znanej math version;
9. forward replay wszystkich mutacji;
10. exact zgodności końcowych modelowanych pól curve z finalnym anchor.

Finalne raw bytes i ich hash są zachowywane jako anchor. Nie twierdzimy, że quote engine odtwarza wszystkie niemodelowane bajty konta. Status "Exact" oznacza exact trajectory dla jawnie zdefiniowanych, modelowanych pól Pump curve.

11.5. Forward replay

Algorytm:

1. Grupuj po "(signature, slot, curve)".
2. Zbierz wszystkie top-level i CPI Pump mutations.
3. Nadaj raw structural locator każdej mutacji.
4. Uporządkuj po canonical order key.
5. Pobierz niezależny pre-anchor.
6. Dla każdej mutacji wywołaj istniejący route-specific "PumpQuoteV1".
7. Post-state jednej mutacji staje się pre-state następnej.
8. Po ostatniej mutacji porównaj wynik z finalnym transaction-local account anchor.
9. Wymagaj exact zgodności wszystkich modelowanych reserve fields.
10. Dopiero wtedy nadaj status "Exact".

Reverse replay może pozostać diagnostyką, ale nie jest źródłem pre-state ani dowodem poprawności math.

11.6. Fail-closed reasons

MISSING_ACCOUNT_TX_SIGNATURE
MISSING_PRE_STATE_ANCHOR
MISSING_FINAL_POST_STATE_ANCHOR
INCOMPLETE_MUTATION_INVENTORY
MISSING_CANONICAL_ORDER
UNKNOWN_ROUTE_VARIANT
UNKNOWN_CURVE_MUTATION
MISSING_FEE_SCHEDULE
ACCOUNT_PROVIDER_CONFLICT
POST_STATE_MISMATCH
AMBIGUOUS_MUTATION_MATCH
ARITHMETIC_OVERFLOW

Dotychczasowy join po:

slot + curve + reserve tuple

pozostaje wyłącznie historical diagnostic i nie może przyznać statusu "Exact".

---

12. Arbitraż raw Yellowstone vs parsed NLN

12.1. Pola materialne

Konflikt może dotyczyć:

- locator/order;
- program ID;
- curve;
- mint;
- route variant;
- side;
- success/error;
- token amount;
- instruction limit;
- curve quote;
- wallet settlement;
- fee breakdown;
- post-state.

Różnica w:

- receive timestamp;
- provider timestamp;
- payload formatting;
- kolejności pól JSON;
- brakującym polu opcjonalnym;

nie jest automatycznie materialnym konfliktem.

Brak pola oznacza "Unknown", a nie niezgodność. Konflikt występuje, gdy oba źródła podają konkretną wartość i wartości są różne albo naruszają typed invariant.

12.2. Macierz

Primary raw| NLN| Zachowanie
raw obecny, NLN brak| raw structural canonical natychmiast| 
raw i NLN zgodne| jeden canonical event, NLN jako witness| 
NLN przychodzi pierwszy| bounded pending witness, zero runtime mutation| 
tylko NLN do końca retention| witness-only, zero canonical event| 
raw/NLN konflikt przed MFS| candidate integrity fail| 
konflikt po MFS, przed terminalnym verdict| przerwanie oceny| 
konflikt po terminalnym REJECT/TIMEOUT| audit marker, historia bez zmian| 
konflikt po terminalnym BUY, przed submit| anulowanie execution intent| 
konflikt po rozpoczęciu submit| typed unknown/reconciliation, bez udawania anulowania| 
konflikt po potwierdzonym BUY| raw authority, NLN quarantine, monitoring i protective exits działają| 
drugi identyczny raw provider| witness provenance, zero drugiej mutacji| 
dwa raw źródła, ten sam locator i różny payload| provider conflict| 

12.3. Gate przed MFS

Technical integrity nie staje się nowym Gatekeeper policy reason.

enum CandidateIntegrityOutcomeV1 {
    Ready,
    PrimaryRawCoverageIncomplete,
    AccountProviderConflict,
    SourceReconciliationConflict,
    AnchorMissing,
    EconomicsNonEvaluable,
}

Tylko:

CandidateIntegrityOutcomeV1::Ready

może trafić do MFS i Gatekeepera.

Gatekeeper nie jest wywoływany dla technicznie niekompletnego kandydata.

Dzięki temu ingest corruption nie jest przedstawiany jako wynik strategii.

12.4. Lifecycle boundary

Konflikt obsługujemy zgodnie z rzeczywistą fazą lifecycle:

PRE_MFS:
  blokada materializacji.

MFS_MATERIALIZED / EVALUATION_RUNNING:
  przerwanie evaluation bez policy recomputation.

TERMINAL_REJECT/TIMEOUT:
  historia immutable, audit marker.

TERMINAL_BUY / NOT_SUBMITTED:
  execution intent cancelled, capacity released.

SUBMIT_STARTED / UNKNOWN:
  reconciliation required;
  nie wolno uznać ani cancel, ani success.

CONFIRMED_OPEN_POSITION:
  raw authority;
  NLN quarantined;
  protective exit działa;
  position nie jest automatycznie zamykana.

---

13. Nieblokujący transport i coverage gaps

Aktualny "DualLaneChannel" po zapełnieniu fast i overflow wykonuje blokujące "overflow.send(ev)".

Aktualny NLN artifact writer wykonuje "mpsc::Sender::send(...).await", więc pełna kolejka może zatrzymać task wywołujący writer.

Oba zachowania zostają usunięte.

13.1. Deterministyczny gap ID

struct CoverageGapIdV1([u8; 32]);

ID jest hashem:

source_role
provider_id
stream_kind
supervisor_epoch
last_complete_slot
first_failed_slot

Nie używamy losowego UUID.

13.2. Gap contract

struct CoverageGapV1 {
    gap_id: CoverageGapIdV1,
    source_role: SourceRoleV1,
    provider_id: String,
    stream_kind: StreamKindV1,
    last_complete_slot: Option<u64>,
    first_failed_slot: Option<u64>,
    first_slot_after_reconnect: Option<u64>,
    last_signature_before_gap: Option<Signature>,
    first_signature_after_gap: Option<Signature>,
    cause: CoverageGapCauseV1,
    recovery_status: GapRecoveryStatusV1,
}

13.3. Network receive

Receiver:

- nie wykonuje "blocking_send";
- nie wykonuje bounded ".send().await";
- nie zapisuje JSONL;
- nie czeka na normalizację;
- nie czeka na NLN;
- przekazuje minimalny envelope przez "try_send".

Przy saturacji:

1. "try_send" zwraca typed "Saturated";
2. event nie jest udawany jako przetworzony;
3. lekki in-memory supervisor otwiera gap;
4. stream zostaje kontrolowanie anulowany;
5. rozpoczyna się reconnect;
6. po reconnect ustalany jest koniec luki;
7. raw backfill rekonstruuje brakujący zakres;
8. readiness primary raw pozostaje obniżone do zakończenia reconcyliacji.

13.4. Skutek luki zależnie od źródła

PRIMARY RAW GAP:
  candidate windows przecinające lukę nie są integrity-ready.

SECONDARY RAW GAP:
  telemetry/recovery degradation;
  brak blokady, jeśli primary jest kompletny.

NLN WITNESS GAP:
  reconciliation coverage degraded;
  sam brak NLN nie blokuje poprawnego raw candidate.

REQUIRED ARTIFACT GAP:
  bieżący research/run segment invalid;
  runtime receiver kontynuuje.

DIAGNOSTIC ARTIFACT GAP:
  jawny drop marker i counter;
  brak wpływu na runtime state.

13.5. Writer

Writer JSONL jest konsumentem za canonical/observation ledgerem.

enum ArtifactDeliveryClassV1 {
    RunEvidenceRequired,
    DiagnosticBestEffort,
}

- "RunEvidenceRequired": saturacja oznacza invalid run segment.
- "DiagnosticBestEffort": saturacja oznacza jawny marker i counter.
- writer nie jest częścią socket receive path;
- writer nie może zatrzymać normalizatora;
- nie powstaje unbounded queue.

Każda bounded queue posiada:

- capacity;
- current depth;
- high-water mark;
- oldest event age;
- drain latency;
- saturation count;
- powiązany "gap_id" albo invalid-segment ID.

---

14. Udowodnienie quote math i roundingu

Nie wprowadzamy arbitralnej poprawki typu „dodaj jeden lamport”.

14.1. Zakres route authority

Nie rozszerzamy obecnego route authorization.

Na baseline:

BuyV2       → parity validated
LegacySell  → parity validated
LegacyBuy   → not execution authorized
BuyExactQuoteInV2 → not execution authorized
SellV2      → typed supported, not execution authorized

Obecny kontrakt jawnie zachowuje blokadę tras bez route-specific settlement fixture. Nie zdejmujemy jej w tej serii.

14.2. Authority evidence

Dla każdej autoryzowanej route wymagamy:

- pinned program/IDL identity;
- raw transaction;
- dokładnych instruction arguments;
- exact account manifest;
- independent pre-state anchor;
- transaction-local final post-state anchor;
- ordered top-level/CPI inventory;
- program fee evidence;
- wallet debit/credit evidence;
- transaction costs osobno;
- forward replay do exact post-state.

14.3. Stratified arithmetic corpus

Nie opieramy dowodu na arbitralnym wymaganiu „1000 fixture’ów”.

Corpus musi pokrywać:

- exact divisibility;
- non-zero division remainder;
- wymagany ceil;
- wymagany floor;
- minimalne poprawne amount;
- typowe amount;
- amount blisko reserve boundary;
- niezerowe wszystkie aktywne fee legs;
- overflow;
- reserve exhaustion;
- multi-event transaction;
- top-level instruction;
- CPI;
- create + initial trade;
- każdy autoryzowany route;
- każdy aktywny fee schedule.

Duży live corpus pozostaje burn-in evidence, ale nie zastępuje arithmetic edge coverage.

14.4. Wybór formuły

Dla każdego route i schedule:

exact post-state parity = 100%
wallet settlement parity = 100%
fee component parity = 100%
instruction limit parity = 100%

Jeżeli:

- żadna formuła nie pasuje — brak promocji;
- kilka formuł pasuje — corpus nie rozróżnia wariantów, brak promocji;
- jedna formuła pasuje — zostaje przypięta przez "math_version_id".

Żadnej tolerancji jednego lamporta w reserve transition ani settlement.

14.5. Arytmetyka

Authority math:

- wyłącznie integer;
- checked "u128" dla iloczynów;
- jawne route-specific ceil/floor;
- brak "f64";
- fail-closed overflow;
- fail-closed unknown route;
- fail-closed unknown fee schedule;
- fail-closed missing anchor;
- program fees oddzielone od transaction costs.

---

15. Migracja aktywnych authority paths

Najpierw powstaje manifest wszystkich callsite’ów:

sol_amount_lamports
simulate_buy
simulate_sell
FEE_BPS
curve_quote_amount
wallet_debit_or_credit
TransactionCosts

Każdy callsite otrzymuje jedną klasyfikację:

ACTIVE_AUTHORITY
DIAGNOSTIC_ONLY
REPLAY_COMPATIBILITY
DEAD_OR_ORPHANED

Migrujemy wyłącznie "ACTIVE_AUTHORITY":

- entry quote;
- execution builder input;
- executable entry valuation;
- executable exit valuation;
- PM net PnL;
- istniejące TP/SL/emergency thresholds;
- material-sell executable comparison;
- terminal accounting;
- decision evidence zależne od rzeczywistej wartości SOL.

Nie migrujemy w tej serii:

- historycznych diagnostics;
- compatibility replay;
- nieużywanych eksperymentów;
- UI projections bez wpływu na decyzję;
- legacy simulatorów, które są odcięte od authority.

CI guard zabrania legacy math w katalogach authority.

---

16. Struktura realizacji

Realizacja obejmuje baseline receipt oraz dwa średnie, stacked PR-y.

Change Set 0 — baseline receipt

Bez zmiany runtime.

1. Nie resetować istniejącego brudnego worktree.
2. Utworzyć clean worktree z:

a12ef9cfb7199d44841cde27be2ecd8af13e2f3f

3. Zapisać:
   
   - commit SHA;
   - "Cargo.lock" hash;
   - binary hash;
   - config hashes;
   - pełny status testów;
   - istniejące failure signatures;
   - canonical replay checksum;
   - throughput;
   - p99 receive-to-normalize;
   - steady-state RSS;
   - queue high-water marks.

4. Zamrozić differential corpus zawierający:
   
   - raw-only transactions;
   - raw + NLN duplicate;
   - raw/NLN conflict;
   - kilka Pump mutations w jednej signature;
   - create + initial buy;
   - account duplicate;
   - account same-version/different-hash conflict;
   - "write_version=None";
   - queue saturation;
   - writer stall;
   - BuyV2 golden fixture;
   - LegacySell golden fixture;
   - missing anchor;
   - multiple same-slot transactions.

---

PR 1 — INGEST AND STATE INTEGRITY

Cel: exactly-once structural mutation, idempotent AccountStateCore, deterministic provider roles i nieblokujący transport.

PR 1 nie przełącza jeszcze produkcyjnej authority. Nowa ścieżka działa w izolowanym "Observe".

Commit 1A — additive contracts i provider roles

Zmiany:

- dodać source-neutral raw locator;
- dodać canonical order key;
- dodać semantic payload;
- dodać observation provenance;
- dodać jawny primary/secondary provider role;
- propagować "provider_id";
- propagować account "txn_signature";
- zachować backward-compatible deserializację;
- nie zmieniać canonical emission.

PASS:

old JSONL readable
old configs readable
tx_index=0 preserved
txn_signature Some preserved
txn_signature None preserved
baseline authority output unchanged

Commit 1B — nieblokujący transport i gap ledger

Zmiany:

- usunąć blokujące "overflow.send";
- usunąć writer ".send().await" z receive/normalize path;
- dodać typed saturation outcome;
- dodać deterministyczny CoverageGap;
- dodać reconnect/backfill state machine;
- przenieść JSONL za ledger.

PASS:

receiver_blocked_on_writer = 0
receiver_blocked_on_overflow = 0
silent_drop = 0
one saturation episode = one deterministic gap
recovered event applied exactly once

Commit 1C — AccountObservationArbiter

Zmiany:

- zastąpić boolean monotonic guard typed arbitrem;
- oddzielić version key od data hash;
- usunąć "recv_seq" z chain ordering;
- primary raw jako jedyny live account authority;
- secondary raw jako witness;
- oddzielić canonical mutation metrics od provider observation metrics.

PASS:

same version + same hash → duplicate
same version + different hash → conflict
duplicate does not change reserves
duplicate does not change velocity
duplicate does not increment state counters
secondary-first does not mutate live state

Commit 1D — Observation Ledger i raw/NLN reconciliation

Zmiany:

- primary raw tworzy structural canonical mutation;
- parsed NLN staje się witness-only;
- direct NLN → canonical "PoolTransaction" zostaje odłączone od produkcyjnego runtime;
- dodać exact/singleton/ambiguous/unmatchable correlation;
- dodać conflict matrix;
- dodać candidate integrity outcome;
- nie zmieniać quote math.

PASS:

raw + same NLN = one canonical mutation
NLN-first = zero runtime mutation
NLN-only = zero canonical event
two mutations in one signature remain two mutations
raw/NLN material conflict classified
no source waits in hot path
Gatekeeper not invoked for integrity-failed candidate

PR 1 merge gate

unclassified structural differences = 0
duplicate canonical mutations = 0
duplicate account mutations = 0
silent primary gaps = 0
receiver blocked time = 0
unexpected Gatekeeper differences = 0

PR 1 pozostaje shadow/observe-only.

---

PR 2 — TRANSACTION-LOCAL ECONOMICS AUTHORITY

Cel: exact anchored economics, poprawny amount contract i migracja aktywnych authority paths.

Commit 2A — transaction-local anchor

Zmiany:

- zbudować AccountObservationLedger join po exact "txn_signature";
- dodać independent pre-anchor;
- dodać final post-anchor;
- dodać ordered mutation inventory;
- dodać forward trajectory certification;
- zakazać tuple fallback;
- dodać "PendingAnchor / Exact / NonEvaluable / Conflict".

PASS:

no txn_signature → NonEvaluable
no pre-anchor → NonEvaluable
no final anchor → NonEvaluable
unknown mutation → NonEvaluable
same-version provider conflict → Conflict
forward result exact matches post-anchor
no NonEvaluable can become Exact

Commit 2B — typed amount i anchored quote wrapper

Zmiany:

- użyć istniejącego "PumpQuoteV1";
- dodać "AnchoredPumpQuoteV1";
- dodać typed instruction limits;
- dodać signature-level cost ledger;
- przestać używać "sol_amount_lamports" jako authority;
- oddzielić curve amount, wallet settlement i transaction costs;
- dodać "math_version_id";
- rozszerzyć istniejące golden fixtures o pre/post anchor.

PASS:

max wallet debit is not curve input
min wallet credit is not settlement result
transaction costs counted once
program fees counted once
multi-event tx does not duplicate costs
reserve transition exact
wallet settlement exact

Commit 2C — active authority cutover

Zmiany:

- przepiąć active entry quote;
- przepiąć executable valuation;
- przepiąć existing configured TP/SL/emergency evaluation;
- przepiąć exit quote;
- przepiąć terminal accounting;
- dodać CI legacy-math guard;
- odłączyć stary amount projection od authority.

PASS:

active legacy math calls = 0
active ambiguous sol amount reads = 0
missing exact economics fails closed
route authorization unchanged
strategy thresholds unchanged

Commit 2D — Observe → Enforce

Konfiguracja:

enum IngestIntegrityModeV1 {
    Observe,
    Enforce,
}

Nie istnieje "Legacy" runtime mode.

Observe

- stara ścieżka pozostaje authority wyłącznie podczas zamkniętego shadow canary;
- nowa ścieżka działa w izolacji;
- nowa ścieżka nie mutuje produkcyjnego AccountStateCore;
- nowa ścieżka nie steruje Gatekeeperem;
- porównujemy oba wyniki.

Enforce

- tylko nowa ścieżka jest authority;
- parsed NLN canonical emission jest odłączone;
- legacy math nie jest dostępne dla active authority;
- nie ma per-event fallbacku;
- brak exact evidence fail-closuje.

Produkcyjny launcher wymaga jawnego ustawienia mode. Brak wartości jest startup error. Backward-compatible default może istnieć wyłącznie w narzędziach replay/test.

---

17. Testy

17.1. Unit tests

Obowiązkowe:

1. curve i semantic kind nie należą do locatora;
2. source ID nie zmienia locatora;
3. slot i "tx_index" są order, nie identity;
4. "tx_index=0" jest poprawne;
5. dwie mutacje w jednej signature są różne;
6. NLN bez locatora i multi-event transaction jest ambiguous;
7. same account version + same hash jest duplicate;
8. same account version + different hash jest conflict;
9. "write_version=None" nie używa "recv_seq";
10. secondary provider nie mutuje live state;
11. max debit nie jest curve quote;
12. min credit nie jest wallet settlement;
13. transaction costs nie są program fee;
14. transaction costs nie są odejmowane dwa razy;
15. missing anchor nie daje exact;
16. tuple join nie daje exact;
17. unknown route fail-closuje;
18. unknown fee schedule fail-closuje;
19. overflow fail-closuje;
20. deterministic gap ID jest stabilne w replay;
21. receive timestamp nie wpływa na canonical hash;
22. forward trajectory odpowiada finalnemu anchorowi.

17.2. Integration tests

primary raw transaction
→ structural canonical mutation
→ raw AccountUpdate z txn_signature
→ exact trajectory
→ AnchoredPumpQuoteV1
→ AccountStateCore
→ MFS
→ unchanged Gatekeeper verdict

raw trade + identyczny NLN trade
→ jeden canonical event
→ jeden buy/sell count
→ jedna state mutation

raw/NLN conflict przed MFS
→ CandidateIntegrityOutcome::SourceReconciliationConflict
→ brak MFS
→ brak Gatekeeper evaluation

terminal BUY przed submit + konflikt
→ execution intent cancelled
→ capacity released

submit started + konflikt
→ typed unknown
→ confirmation/reconciliation
→ brak fałszywego success/cancel

zatrzymany JSONL writer
→ receiver kontynuuje
→ artifact segment invalid
→ brak raw coverage gap

pełny canonical ingress ring
→ primary raw gap
→ reconnect
→ backfill
→ mutation dokładnie raz

17.3. Differential replay

Dla unaffected records:

canonical event hash identical
MFS hash identical
verdict identical
reason chain identical
PM lifecycle identical
terminal outcome identical

Dla fixed records:

exactly one expected-difference classification
no unrelated field difference
before/after values present
canonical locator present
source observations linked

---

18. CI

Każdy commit i oba PR-y muszą przejść:

cargo fmt --all --check
cargo test -p ghost-core
cargo test -p seer
cargo test -p trigger
cargo test -p ghost-launcher
cargo test --workspace
cargo build --release --workspace
git diff --check

Istniejące baseline failures:

- zostają zapisane w Change Set 0;
- ich signature nie może się zmienić;
- nie są naprawiane przypadkowo w tej serii;
- każdy nowy failure blokuje merge.

---

19. Performance i saturacja

Pomiar:

- 5 warm-up runs;
- 20 measurement runs;
- identyczny corpus;
- identyczny host;
- identyczny release profile;
- jawnie zapisane queue capacities.

Bramki:

throughput ratio new/baseline, lower 95% CI >= 0.98
p99 receive-to-normalize ratio, upper 95% CI <= 1.05
receiver blocked on writer = 0
receiver blocked on overflow = 0
locks held across await = 0
silent drops = 0

RSS może wzrosnąć wyłącznie o jawnie policzony budżet bounded ledgerów i kolejek.

---

20. Rollout

20.1. Dark Observe

Minimum:

30 minut
10 000 successful primary raw Pump mutations
0 live execution

Wymagania:

unclassified differences = 0
duplicate canonical mutation = 0
duplicate account mutation = 0
false Exact = 0
silent primary gap = 0
receiver blocked = 0

20.2. Anchor gate

Wymagania:

100% Exact trajectories pass forward/post-anchor parity
0 missing-anchor record classified as Exact
100% candidate windows reaching execution eligibility have complete exact economics
overall supported mutation anchor coverage >= 99.9%

Brakujące przypadki pozostają "NonEvaluable".

20.3. Shadow Enforce

Minimum:

kolejne 30 minut
albo kolejne 10 000 primary raw mutations
0 live execution

Nowa ścieżka steruje wyłącznie shadow rolloutem.

PASS:

unexpected Gatekeeper differences = 0
unexpected PM lifecycle differences = 0
unexpected accounting differences = 0
legacy authority calls = 0

20.4. Dalsza promocja

Ten plan nie autoryzuje live promotion.

Po PASS shadow enforce należy osobno:

1. zamrozić commit SHA;
2. zamrozić binary hash;
3. zamrozić config hash;
4. zamrozić math version;
5. zamrozić fee registry;
6. przygotować jawny poprzedni binary/config jako rollback;
7. wykonać osobny live-readiness review.

---

21. Rollback

Rollback:

previous frozen binary
+
previous frozen config

Niedozwolone:

- runtime "Legacy" mode;
- fallback pojedynczego eventu;
- fallback z missing anchor do tuple join;
- fallback z unknown fee schedule do stałej;
- fallback z raw conflict do NLN;
- przepisanie historycznych verdictów.

Segment z gapem albo konfliktem pozostaje oznaczony jako niekompletny również po rollbacku.

---

22. Kryterium zakończenia

Problem uznajemy za usunięty, gdy równocześnie:

active ambiguous sol_amount_lamports reads = 0
active legacy simulate_buy/simulate_sell authority calls = 0
active legacy FEE_BPS authority calls = 0
raw + NLN duplicate canonical mutations = 0
duplicate AccountUpdate state applications = 0
receiver blocking on bounded queues = 0
receiver blocking on JSONL writer = 0
silent primary raw coverage gaps = 0
unclassified source conflicts = 0
false Exact transaction trajectories = 0
transaction costs counted more than once = 0
unclassified replay differences = 0
unexpected Gatekeeper changes = 0
unexpected PM lifecycle changes = 0

Dodatkowo:

- każdy canonical event ma source-neutral locator;
- każda obserwacja zachowuje provenance;
- każdy market SOL amount ma curve semantics;
- każdy instruction limit ma limit semantics;
- każdy wallet debit/credit ma settlement semantics;
- każdy transaction cost ma signature/attempt semantics;
- każdy account state update jest applied, duplicate, stale albo conflicted;
- każdy exact quote posiada independent pre-anchor i transaction-local post-anchor;
- parsed NLN pozostaje witness-only;
- żadna niepełna ekonomika nie zasila exact valuation;
- żadna późna obserwacja nie mutuje zamrożonego MFS ani terminalnej historii.

---

23. Kolejność rozpoczęcia prac

Pierwsze działania:

1. utworzyć clean worktree z commita baseline;
2. wykonać Change Set 0;
3. utworzyć branch PR 1;
4. rozpocząć od additive propagation "txn_signature", provider role i locator contracts;
5. dopiero potem zmienić kolejki i AccountUpdate arbiter;
6. zamknąć PR 1 pełnym differential replay;
7. rozpocząć PR 2 dopiero na zielonym PR 1;
8. pozostawić live execution wyłączone przez całą realizację planu.
