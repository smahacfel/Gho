PLAN WYKONAWCZY NAPRAWY GRANICY INGEST–STATE–QUOTE

Status: "PR1E READY FOR IMPLEMENTATION / RUNTIME AUTHORITY CUTOVER / LIVE EXECUTION DISABLED"
Data: "2026-07-24 / rozszerzenie PR1E 2026-07-27"
Repozytorium: "smahacfel/Gho"
Baseline: "88aa1b775d51f4a1b3e512b1aaf05663e7af6db1"
Zakres: siedem błędów integralności ingestu, stanu i ekonomiki
Struktura realizacji: baseline receipt + PR1A–PR1E; PR2 pozostaje odłożony

Aktualny baseline planu został formalnie przesunięty z
`a12ef9cfb7199d44841cde27be2ecd8af13e2f3f` na
`88aa1b775d51f4a1b3e512b1aaf05663e7af6db1`, ponieważ implementacja PR1A
powstała na tym późniejszym commicie i review wymagało jednoznacznej decyzji
baseline przed zatwierdzeniem bramek. Ten commit stanowi bazę planowanej
realizacji PR1A od tej aktualizacji.

Receipt dla tej podstawy:
`PLANS/DO_REALIZACJI/BASELINE_RECEIPT_INGEST_STATE_QUOTE_88AA1B7_20260724.md`.

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
    semantic_event_ordinal: u32,
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
    semantic_event_ordinal: u32,
}

Zasady:

- order pochodzi wyłącznie z primary raw;
- "tx_index=0" pozostaje "0";
- order nie może być syntetyzowany z czasu;
- order nie może być syntetyzowany z receive sequence;
- rozbieżny slot albo "tx_index" dla tego samego locatora daje typed provider conflict;
- event bez pełnego raw order może pozostać obserwacją, ale nie uzyskuje statusu exact ordered mutation.

6.3. Semantic claims

struct PumpMutationClaimsV1 {
    curve: Option<Pubkey>,
    mint: Option<Pubkey>,
    route_variant: Option<PumpRouteVariant>,
    side: Option<PumpTradeSideV1>,
    success: Option<bool>,
    token_amount_units: Option<u64>,
    instruction_limit: Option<PumpInstructionLimitV1>,
    reported_curve_quote_lamports: Option<u64>,
    reported_wallet_delta_lamports: Option<u64>,
    reported_fee_breakdown: Option<Vec<ProgramFeeCharge>>,
}

Każde `None` znaczy wyłącznie `Unknown`: provider nie znał pola albo go nie
raportował. Nie znaczy wartości domyślnej i nie jest samodzielnym konfliktem.
Konflikt powstaje dopiero, gdy dwa źródła zgłoszą konkretne, różne wartości.

Pola `reported_*` są obserwacją providera. Nie stają się authority bez
transaction-local certification. Rygorystyczny
`ValidatedPumpMutationFactV1`, którego pola będą bezwarunkowe po walidacji
primary raw, jest świadomie odroczony poza PR1A (najwcześniej 1D/2A).

6.4. Provenance

struct ObservationProvenanceV1 {
    source_family: ObservationSourceFamilyV1,
    source_id: String,
    provider_id: String,
    schema_id: String,
    payload_hash_blake3: [u8; 32],
    received_at_monotonic_ns: u64,
}

`ObservationSourceFamilyV1` opisuje rodzinę dowodu (`RawYellowstone` albo
`ParsedNln`) i celowo nie używa nazwy istniejącego transportowego
`SourceKind`. Nie opisuje socketu, adaptera ani endpointu providera.

`payload_hash_blake3` to BLAKE3 captured provider payload bytes przekazanych
przez adapter do normalizacji. `source_family` i `schema_id` określają ich
reprezentację. Dla Yellowstone są to bajty prost-encoded zdekodowanego
`SubscribeUpdateTransaction`, nie oryginalna ramka gRPC: envelope ani
nieznane pola wire nie muszą zostać zachowane. Hash służy do identyfikacji
jednej obserwacji i audytu; nie jest hashem zgodności semantycznej między raw
protobuf i parsed JSON.

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
    claims: PumpMutationClaimsV1,
    provenance: ObservationProvenanceV1,
}

8.1. Faza pierwsza — structural canonical

Primary raw po pełnej walidacji tworzy:

struct StructuralCanonicalPumpMutationV1 {
    locator: RawPumpMutationLocatorV1,
    order: CanonicalPumpOrderKeyV1,
    claims: PumpMutationClaimsV1,
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

Pierwotna realizacja obejmowała baseline receipt oraz dwa średnie, stacked
PR-y. Po scaleniu PR1A–PR1D plan zostaje rozszerzony o wykonawczy PR1E, który
kwalifikuje cały PR1 end-to-end i przełącza runtime authority bez rozpoczynania
PR2 ani zmiany ekonomiki.

Change Set 0 — baseline receipt

Bez zmiany runtime.

1. Nie resetować istniejącego brudnego worktree.
2. Utworzyć clean worktree z:

88aa1b775d51f4a1b3e512b1aaf05663e7af6db1

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

### 16.0a. Review decision: deferred hard gates for non-CI Change Set 0 work

Dla addytywnego PR1A receipt CI względem wybranego baseline jest wystarczający
do domknięcia formalnej bramki baseline. Brak istniejącego harnessu i corpus
ingest/state/quote nie jest waiverem ani deklaracją parity — jest oznaczony w
receipt jako `DEFERRED HARD GATE`.

Przed pierwszą zmianą zachowania transportu w 1B trzeba:

1. przygotować jeden identyczny harness i workload;
2. uruchomić go w clean worktree rodzica PR1A oraz na diffie 1B;
3. zapisać baseline throughput, p99 receive-to-normalize, RSS i queue
   behavior;
4. porównać wyniki przed commitem 1B.

Przed 1C/1D trzeba zamrozić odpowiedni differential corpus dla account
duplicates, provider conflicts i raw/NLN reconciliation. Te warunki nie mogą
być pominięte przez fakt, że PR1A jest addytywny; nie wymagają jednak osobnego
piątego PR-a przed 1A.

---

PR 1 — INGEST AND STATE INTEGRITY

Cel: exactly-once structural mutation, idempotent AccountStateCore, deterministic provider roles i nieblokujący transport.

PR1A–PR1D nie przełączają jeszcze produkcyjnej authority. Nowa ścieżka działa
w izolowanym "Observe". PR1E zamyka tę granicę i przełącza authority nowych
kandydatów na Observation Ledger oraz CandidateIntegrity, przy nadal
wyłączonym live execution.

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

Commit 1B — single-pass ingest, nieblokujące granice i local gap

Kolejność realizacji jest normatywna:

1. Najpierw usunąć self-generated backlog:

   - nie używać ponownie zakodowanego protobufu jako transportu live;
   - przenieść zdekodowaną transakcję Yellowstone do normalizacji;
   - capture tworzyć opcjonalnie, najwyżej raz i dopiero po kolejce ingress;
   - wykonać jedno przejście outer + inner instructions;
   - wyprowadzić CREATE i wszystkie TRADE z jednego parsed bundle.

2. Następnie odizolować niezależne wolne sinki:

   - fizyczny WAL append przenieść do jednego bounded writera;
   - JSON, Base58 i evidence hash przenieść do bounded evidence writera;
   - oczekiwanie na downstream IPC przenieść do jednego bounded egress
     dispatchera;
   - canonical `AccountUpdate` zachować w osobnej bounded FIFO wszystkich
     przyjętych obserwacji, bez deduplikacji i freshness arbitration w PR1B,
     aby pełna wspólna kolejka business IPC nie usuwała primary state feedu i
     aby PR1C otrzymał `None`, `Some(0)`, multi-provider oraz
     same-version/different-hash;
   - sequence number nadawać atomowo pod tym samym lockiem co enqueue, a obie
     lane scalać według ich frontowego sequence number;
   - event worker używa wyłącznie nieblokujących enqueue;
   - wszystkie dispatchery mają stałą liczbę workerów i bounded capacity;
   - każdy dispatcher musi mieć `stop accepting -> drain -> flush -> join` i
     raportować typed failure, jeżeli zaakceptowana praca nie została
     dostarczona lub utrwalona.

3. Dopiero po usunięciu powielonej pracy i blokujących sinków obsłużyć
   rzeczywiste przeciążenie:

   - zastąpić układ "fast + overflow + blocking overflow.send" jedną bounded
     FIFO ingress;
   - dodać typed local saturation outcome;
   - jeden ciągły epizod tworzy jeden deterministyczny LocalCoverageGap;
   - gap zapisuje `missing_event_count`, `first_dropped` i `last_dropped`;
   - ingress, WAL, evidence i IPC przekazują markery do jednego centralnego
     audit routera i rezerwowanej ścieżki WAL, niezależnej od normalnej kolejki
     `WalJob`;
   - segment z nieodzyskaną luką jest fail-closed i NonEvaluable;
   - local processing gap nie jest provider slot gap i sam nie uruchamia
     reconnectu ani backfillu.

Pojemność ingress jest konfigurowalna przez serde-default
`ingress_queue_capacity`; wartość domyślna wynosi 2 048 eventów. Pierwotne
wyliczenie `średni throughput × 250 ms` było nieprawidłowe i nie jest już
podstawą capacity. Równoległy zamrożony protobuf replay ma workload 3 072
eventów (24 × 128 co 50 ms), czyli nie jest równy capacity. Z rzeczywistym
konsumentem normalizacji/parsera zmierzył peak batch 73 873,871 eventów/s,
operational ingress 2 535,427 eventów/s, sustained drain 2 442,683 eventów/s,
high-water 134 i zero utraconych eventów przy capacity 2 048. Twarde bramki:
queue dwell p99 47 209 510 ns <= 250 ms oraz oldest age 54 277 899 ns <=
500 ms. WAL, evidence i IPC mają jawne bounded kolejki; nie ma kaskadowych
overflow queues ani per-event task spawning.

Status PR1B po korekcie review (2026-07-25):

- live Yellowstone: 0 application-level prost decode;
- capture off: 0 prost encode; capture wymagany: najwyżej 1 prost encode;
- 1 pełny scan outer + inner na transakcję;
- pełny `CanonicalParserParitySnapshotV1` digest zachowany na B0 i finalnym
  PR1B:
  `549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202`;
- receiver i parser worker nie blokują na ingress, WAL, evidence ani IPC;
- pełna normalna kolejka IPC nie usuwa canonical `AccountUpdate`;
- AccountUpdate FIFO nie usuwa obserwacji tej samej wersji/krzywej i nie
  wykonuje arbitrażu przed PR1C;
- 64-producer test potwierdza globalny sequence ordering obu IPC lane;
- saturacja ma trwały missing count, pierwszą/ostatnią odrzuconą granicę i
  deterministyczny local gap;
- wszystkie cztery domeny gapów trafiają do niezależnego audit WAL;
- WAL/evidence/IPC/audit mają kontrolowany drain, final flush i join;
- shutdown ma wspólny deadline i typed timeout; IPC nie używa blocking_send;
- diagnostyczny evidence jest globalnym warunkiem runu tylko przy
  `artifact_required_for_run = true`;
- brak dowodu lokalnego recovery pozostawia segment niewiarygodny;
- authority, strategia, MFS, Gatekeeper i quote math pozostają bez zmian.

PASS:

```text
receiver_blocked_on_writer = 0
receiver_blocked_on_overflow = 0
parser_worker_blocked_on_ipc = 0
canonical_account_update_lost_on_full_business_ipc = 0
live_transaction_prost_decode = 0
full_instruction_scans_per_transaction = 1
silent_drop_reported_as_success = 0
one saturation episode = one deterministic local gap
gap_missing_event_count_and_boundaries = durable
all_local_gap_domains = reserved audit WAL
accepted_dispatcher_jobs = drained, flushed, joined
unrecovered local gap = non-evaluable
```

Reconnect/backfill state machine dla dowodliwego provider gap oraz odzyskiwanie
lokalnej luki nie są implementowane w PR1B. Nie wolno ich zastępować reconnectem
spowodowanym lokalnym writer stall. Zakres 1C (AccountObservationArbiter) i 1D
(Observation Ledger oraz raw/NLN reconciliation) pozostaje świadomie odłożony.

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

Commit 1E — aktywacja integralności ingest–state w runtime i kwalifikacja
end-to-end całego PR1

Status: READY FOR IMPLEMENTATION
Charakter: poboczny PR rozszerzający PR1A–PR1D
Repozytorium: `smahacfel/Gho`
Base: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`
PR bazowy: #85 — PR1D
Head PR1D przed merge: `a982157f499313eb8f9b42326e67d495ace6224d`
Struktura: jeden niewielki/średni PR, maksymalnie dwa logiczne commity
Finalny stan: runtime authority aktywne, brak produkcyjnego `Observe` i brak
per-event legacy fallbacku.

PR #85 został zmergowany, ale jego jawna granica pozostawia
`PumpObservationLedgerV1`, canonical apply proof i `CandidateIntegrity` w
trybie dowodowym. Parent primary wrapper nadal steruje aktywną emisją, a
CandidateIntegrity nie blokuje MFS, Gatekeepera ani submitu.

PR1E zamyka dokładnie tę granicę. Jest wykonawczym rozszerzeniem pierwotnego
planu naprawy ingest–state–quote, bez rozpoczynania PR2 i bez zmian ekonomiki.

### 1E.1. Cel nadrzędny

PR1E ma odpowiedzieć wykonawczo na dwa pytania:

1. Czy komplet PR1A–PR1D działa wspólnie w realnej ścieżce:

   `provider → Seer parser → IPC → Observation Ledger → Event Bus → session
   → AccountStateCore → MFS → Gatekeeper → execution handoff`?

2. Czy można bezpiecznie odebrać authority starej emisji i przekazać je
   istniejącym mechanizmom PR1?

Po merge PR1E aktywny przepływ ma wyglądać tak:

```text
PRIMARY RAW YELLOWSTONE WRAPPER
        +
ALIGNED ObservedPumpMutationV1
                │
                ▼
       PumpObservationLedgerV1
                │
        ┌───────┴────────┐
        │                │
        ▼                ▼
canonical mutation   duplicate / witness-only
        │                │
        │                └── zero runtime emission
        ▼
canonical runtime permit
        │
        ▼
existing rich wrapper as data carrier
        │
        ▼
NewPoolDetected / PoolTransaction
        │
        ▼
actual downstream apply receipt
        │
        ▼
CandidateIntegrity::Ready
        │
        ▼
MFS → Gatekeeper → guarded submit
```

Najważniejsza zasada:

> Oryginalny primary wrapper pozostaje nośnikiem kompletnego payloadu, ale
> przestaje być źródłem authority. Prawo emisji uzyskuje wyłącznie przez
> canonical decision Observation Ledgera.

### 1E.2. Namacalny efekt po merge

PR1E ma sprawić, że w działającym Ghostcie:

- identyczna mutacja raw odebrana ponownie nie trafi drugi raz do runtime;
- raw + NLN nie utworzą dwóch `PoolTransaction`;
- NLN-only nie utworzy kandydata ani transakcji;
- secondary raw nie zmutuje runtime state;
- primary wrapper bez prawidłowej obserwacji nie ominie Ledgera;
- wrapper niezgodny z obserwacją zostanie zatrzymany;
- niepełne transaction inventory nie odblokuje MFS;
- konflikt providera przed MFS zablokuje ocenę;
- konflikt podczas evaluation przerwie ocenę bez strategicznego `REJECT`;
- konflikt po BUY, ale przed submit, anuluje execution intent;
- konflikt po rozpoczęciu submitu nie zostanie fałszywie uznany za anulowanie;
- potwierdzona pozycja zachowa protective exits pomimo późnego konfliktu
  witnessa;
- duplicate AccountUpdate nie zmieni rezerw, velocity ani liczników;
- awaria Ledgera albo CandidateIntegrity zamknie dopływ nowych kandydatów,
  ale nie wyłączy ochrony istniejących pozycji.

### 1E.3. Twarde granice zakresu

PR1E zmienia:

- authority emisji `NewPoolDetected`;
- authority emisji `PoolTransaction`;
- obsługę wyniku `PumpObservationLedgerV1`;
- przepływ canonical apply receipt;
- obsługę buforowanych i replayowanych trade’ów;
- obowiązkowe sprawdzenie CandidateIntegrity przed MFS;
- obowiązkowe sprawdzenie CandidateIntegrity podczas evaluation;
- obowiązkowy submit guard;
- zachowanie przy niedostępności Ledgera albo registry;
- produkcyjny startup contract;
- end-to-end qualification harness;
- differential proof i fault injection;
- telemetry cutoveru.

PR1E nie zmienia:

- `PumpQuoteV1`;
- `sol_amount_lamports`;
- quote math;
- fee schedules;
- entry sizing;
- TP/SL;
- executable valuation;
- PnL;
- transaction costs;
- strategy scoring;
- progów Gatekeepera;
- kolejności Gatekeeper policy;
- schematu `MaterializedFeatureSet`;
- Position Manager policy;
- shadow/live mode;
- route authorization;
- PR2 transaction-local anchors.

Każda zmiana w powyższych elementach oznacza scope violation.

### 1E.4. Nienaruszalne inwarianty

1. Exactly-once dotyczy jednego runtime epoch i retained bounded ledgeru.
2. PR1E nie deklaruje durable cross-restart deduplication.
3. Primary raw Yellowstone pozostaje jedynym structural authority.
4. Wrapper `provider_role=PrimaryAuthority` sam w sobie nie daje prawa emisji.
5. Każdy nowy primary pool/trade musi posiadać aligned
   `ObservedPumpMutationV1`.
6. Każda aktywna emisja wymaga canonical mutation z Ledgera.
7. Exact duplicate nie może uzyskać drugiego runtime permitu.
8. Secondary raw i parsed NLN mają zawsze zero canonical runtime emissions.
9. Jedna signature z kilkoma locatorami zachowuje kilka odrębnych mutacji.
10. Canonical raw mutation może zostać zastosowana do strukturalnego state,
    nawet gdy witness zgłosi konflikt, ale kandydat nie może wtedy przejść do
    MFS.
11. `CandidateIntegrity::Ready` jest wymagane przed MFS i pozostaje ważne
    wyłącznie dla tej samej generation.
12. Żaden technical integrity failure nie staje się Gatekeeper `REJECT`,
    `TIMEOUT` ani policy reason.
13. Bus enqueue nie jest dowodem downstream apply.
14. Tylko `AppliedNewMutation` potwierdza canonical apply.
15. `Duplicate`, `Ignored`, `Terminal`, `Failed` nie potwierdzają apply.
16. Brak registry, poisoned lock, capacity exhaustion lub brak permitu blokuje
    nowego kandydata.
17. Awaria integralności nowych kandydatów nie zatrzymuje protective exitów
    potwierdzonych pozycji.
18. Nie istnieje produkcyjny per-event fallback do parent emission.
19. Nie istnieje produkcyjny tryb `Legacy`.
20. Rollback oznacza cały poprzedni binary i config.

### 1E.5. Minimalny kontrakt aktywnego admission

W `ghost-launcher/src/components/seer.rs` należy zastąpić obecne:

```text
ingest ledger
→ ewentualnie zapisz shadow evidence
→ primary wrapper i tak kontynuuje
```

kontraktem:

```rust
enum CanonicalRuntimeAdmissionV1 {
    Apply(CanonicalRuntimePermitV1),
    NoApply(CanonicalRuntimeNoApplyReasonV1),
    Blocked(CandidateIntegrityOutcomeV1),
}
```

Przykładowe `NoApply`:

```rust
enum CanonicalRuntimeNoApplyReasonV1 {
    ExactDuplicate,
    SecondaryWitnessOnly,
    ParsedWitnessOnly,
    AmbiguousWitness,
    UnmatchableWitness,
    ContinuityOnly,
    Suppressed,
}
```

`CanonicalRuntimePermitV1` ma być typem prywatnym dla launchera i zawierać co
najmniej:

```rust
struct CanonicalRuntimePermitV1 {
    apply_receipt: CanonicalMutationApplyReceiptV1,
    authority_epoch_id: u64,
    locator: RawPumpMutationLocatorV1,
    primary_payload_hash_blake3: [u8; 32],
}
```

Permit nie tworzy nowego modelu eventu. Upoważnia istniejący `CandidatePool`
albo `TradeEvent` do przejścia przez dotychczasowy adapter.

### 1E.6. Zmiana semantyki `ingest_pump_observation`

Obecnie kod jawnie kontynuuje parent emission przy:

- braku transportowej obserwacji;
- poisoned ledger mutex;
- nieudanym stagingu receiptu;
- błędzie sealowania inventory.

PR1E usuwa tę semantykę.

Primary event bez observation:

```text
brak observation
→ PrimaryRawCoverageIncomplete
→ zero NewPoolDetected/PoolTransaction
→ zamknięcie admission dla tego kandydata
```

Nie wolno emitować parent wrappera.

Wrapper/observation mismatch obejmujący provider role, provider ID, candidate
identity, signature albo locator/order daje:

```text
PrimaryRawCoverageIncomplete
→ zero runtime emission tego wrappera
```

Ledger unavailable nie może kontynuować parent emission:

```text
new_candidate_admission_open = false
runtime health = degraded/fail-closed
existing confirmed positions continue
protective exits continue
```

Nie należy zabijać całego procesu, jeśli istnieją pozycje wymagające ochrony.

Tylko obecna canonical mutation tworzy permit. Jeżeli canonical mutation nie
istnieje, nie ma emisji niezależnie od roli zapisanej w wrapperze.

### 1E.7. Rich wrapper jako carrier, nie authority

`StructuralCanonicalPumpMutationV1` nie musi powielać wszystkich pól
istniejącego `TradeEvent` albo `CandidatePool`.

Dlatego PR1E nie rekonstruuje pełnego eventu z claims. Zamiast tego:

1. Ledger potwierdza identity, source role, order i canonical uniqueness.
2. Boundary consistency sprawdza zgodność canonical mutation z wrapperem.
3. Permit upoważnia oryginalny wrapper do dalszego przejścia.
4. Existing adapter nadal mapuje wrapper do `DetectedPool` lub
   `PoolTransaction`.

Pozwala to uniknąć duplikowania parsera, utraty istniejących pól, nowego event
schema, szerokiego rewrite’u oraz różnic w untouched primary events.

### 1E.8. Buforowane trade’y

Żaden trade z canonical permitem nie może zostać zbuforowany jako sam
`TradeEvent`, ponieważ późniejszy replay mógłby ominąć canonical apply proof.

Bufor musi przechowywać:

```rust
struct BufferedCanonicalTradeV1 {
    trade: TradeEvent,
    permit: CanonicalRuntimePermitV1,
    buffered_at: Instant,
}
```

Zasady:

- replay używa dokładnie pierwotnego permitu;
- replay nie tworzy nowego permitu;
- expiry bufora wywołuje `fail_canonical_apply`;
- eviction wywołuje `fail_canonical_apply`;
- duplicate replay nie potwierdza apply;
- replay bez permitu jest runtime bypass attempt i zostaje odrzucony;
- `replay_ready` zachowuje canonical order.

Analogicznie należy sprawdzić każdy inny lokalny bufor lub retry lane
przenoszący `PoolDetected` albo `Trade`.

### 1E.9. Downstream apply receipt

PR1D wprowadził już oddzielny wynik faktycznego session apply:

```rust
enum CanonicalMutationApplyOutcomeV1 {
    AppliedNewMutation,
    Duplicate,
    Ignored,
    Terminal,
    Failed,
}
```

oraz `ingest_transaction_with_apply_result()`. PR1E czyni ten wynik
autorytatywnym.

| Downstream outcome | Działanie |
| --- | --- |
| `AppliedNewMutation` | `mark_canonical_apply_succeeded` |
| `Duplicate` | fail receipt; typed divergence |
| `Ignored` | fail receipt |
| `Terminal` | fail receipt; terminal history bez zmian |
| `Failed` | fail receipt |
| brak receiptu | runtime bypass, event odrzucony |
| kilka pasujących receiptów | registry unavailable, fail-closed |

Samo `event_bus_tx.send(...)` nie może potwierdzać apply.

Otwarcie/rejestracja sesji dla `NewPoolDetected` musi zwrócić analogiczny typed
apply result. Za poprawny apply uznajemy wyłącznie sytuację, gdy candidate
identity została jednoznacznie przyjęta, sesja została utworzona albo
prawidłowo skorelowana z istniejącą tą samą sesją, nie wystąpił alias conflict,
a event nie był terminalnym albo compatibility no-op.

Canonical primary transaction, której downstream nie potrafi jednoznacznie
zkeyować, nie może uzyskać `Ready`:

```text
Unkeyable under canonical permit
→ PrimaryRawCoverageIncomplete
→ zero MFS
```

Replay/diagnostic compatibility może nadal odczytać historyczny event, ale nie
może odblokować produkcyjnej oceny.

### 1E.10. CandidateIntegrity jako aktywny gate

Registry ma już generation, CAS, evaluation guard, submit guard, phase
transitions i conflict matrix. PR1E nie tworzy drugiego lifecycle’u. Włącza
istniejący.

Przed MFS:

```text
CandidateIntegrity != Ready
→ brak MaterializedFeatureSet
→ brak Gatekeeper invocation
→ brak strategicznego verdictu
```

Guard należy pobrać bezpośrednio przed rozpoczęciem materializacji.

Materializacja MFS:

```text
evaluation_guard()
→ check_ready()
→ build immutable MFS
→ check_ready()
→ mark_mfs_materialized()
```

Drugie `check_ready()` chroni przed konfliktem, który nadszedł podczas
budowania MFS. Jeżeli generation się zmieniła:

- MFS nie jest publikowany downstream;
- lokalnie utworzony snapshot zostaje orphaned;
- nie wolno go mutować ani ponownie wykorzystać;
- wykonywany jest technical cleanup.

Evaluation:

```text
check_ready()
→ mark_evaluation_running()
→ evaluate policy
→ check_ready()
→ publish terminal
```

Konflikt podczas evaluation oznacza technical abort, brak
`BUY/REJECT/TIMEOUT`, brak policy reason oraz cleanup pre-buy resources.

Jeżeli repo nie posiada odpowiedniego prywatnego cleanup receiptu, należy dodać
wyłącznie wewnętrzny:

```rust
enum TechnicalEvaluationTerminationV1 {
    CandidateIntegrityInvalidated,
}
```

Nie jest to Gatekeeper verdict, strategy outcome ani terminal trading result.
Ma jedynie zatrzymać evaluation, zwolnić pre-buy resources, zachować evidence
i nie zanieczyszczać statystyk strategii.

Późny konflikt po opublikowanym `REJECT/TIMEOUT` nie zmienia terminalu, nie
uruchamia ponownej oceny i zapisuje audit marker.

BUY musi zwrócić jeden `CandidateIntegritySubmitGuardV1`. Bez guardu execution
handoff jest niemożliwy.

Konflikt przed `try_begin_submit()` anuluje execution intent, zwalnia capacity
dokładnie raz i daje sender call count = 0.

Po `try_begin_submit()` konflikt nie oznacza anulowania ani sukcesu, capacity
nie jest zwalniane, ustawiane jest `reconciliation_required`, a potwierdzenie
transakcji zachowuje authority.

Późny konflikt dla confirmed position powoduje quarantine witnessa, nie
uruchamia automatycznego close, nie zatrzymuje raw AccountUpdate, a protective
exits i monitoring działają dalej.

### 1E.11. AccountStateCore

PR1C pozostaje jedyną granicą AccountUpdate. Tylko
`AccountObservationOutcomeV1::AppliedNewMutation` może zmienić
AccountStateCore.

Stanu nie mogą zmieniać duplicate, stale, provider conflict, secondary
witness, unorderable without write version ani observation-only `RpcRefresh`.

CandidateIntegrity nie może zatrzymywać raw-primary AccountUpdate dla już
potwierdzonej pozycji.

Rozdzielenie odpowiedzialności:

```text
AccountObservationArbiter
→ decyduje, czy stan on-chain może się zmienić

CandidateIntegrity
→ decyduje, czy nowy kandydat może dojść do MFS/evaluation/submit
```

### 1E.12. Startup i continuity boundary

Produkcyjny startup wymaga:

- dokładnie jednego primary raw providera;
- stabilnego `provider_id`;
- dostępnego Pump Observation Ledgera;
- dostępnego CandidateIntegrity registry;
- niezerowych bounded capacities;
- tego samego registry instance w Seer i OracleRuntime;
- aktywnego downstream apply acknowledgement;
- aktywnego AccountObservationArbiter;
- wyłączonej direct NLN canonical emission;
- wyłączonej secondary raw canonical emission.

Brak któregokolwiek warunku jest startup error dla nowych candidate
admissions.

Obecna nazwa `Observe` nie może opisywać finalnej aktywnej ścieżki. Docelowo:

```rust
enum PoolDetectionRuntimeDispositionV1 {
    CandidateAdmission,
    ContinuityOnly,
    Suppressed,
}
```

Historyczna deserializacja może zachować:

```rust
#[serde(alias = "observe")]
CandidateAdmission
```

ale produkcyjny kod nie posiada trybu `Observe`.

`ContinuityOnly` służy wyłącznie pozycjom odtworzonym sprzed authority epoch,
istniejącym monitoringom, protective state hydration i terminal
reconciliation. Nie może tworzyć nowej sesji wejściowej, MFS, Gatekeepera ani
nowego BUY.

### 1E.13. Authority epoch

Każdy start runtime tworzy:

```rust
struct Pr1AuthorityEpochV1 {
    epoch_id: u64,
    binary_hash: [u8; 32],
    config_hash: [u8; 32],
    started_at_unix_ms: u64,
}
```

Cel:

- jawnie ograniczyć exactly-once do runtime epoch;
- odróżnić nowe candidate admissions od continuity state;
- uniemożliwić fałszywe twierdzenie o durable cross-restart dedupe;
- powiązać metrics i receipts z konkretnym binary/configiem.

PR1E nie tworzy durable globalnego Observation Ledgera.

### 1E.14. Zachowanie przy awarii integralności

Globalna awaria obejmująca ledger unavailable, registry unavailable, capacity
exhausted, impossible receipt ambiguity albo internal identity contradiction
powoduje:

```text
new_candidate_admission_open = false
new Gatekeeper evaluations = 0
new submits = 0
runtime health = critical/degraded
existing confirmed positions continue
protective exits continue
```

Nie wolno kontynuować parent emission, automatycznie przełączyć się na legacy,
wyłączyć protective exits ani udawać poprawnego działania.

### 1E.15. Struktura PR1E

#### Commit 1E-A — executable PR1 end-to-end qualification

Bez zmiany aktywnej authority.

Zakres:

- utworzyć baseline receipt dla `103212b16bfc059db367e1ceb3c7d00fd307d6c5`;
- zbudować jeden runner przechodzący przez production adapters;
- połączyć istniejące frozen corpora PR1B, PR1C i PR1D przez manifest;
- nie kopiować istniejących fixture’ów;
- dodać tylko brakujące cross-layer scenarios;
- zamrozić manifest hash;
- zamrozić runner hash;
- uruchamiać ten sam input na merged PR1D parent behavior i planowanej ścieżce
  Enforce.

Manifest ma odwoływać się między innymi do:

- PR1B canonical parity digest;
- PR1C AccountObservationArbiter corpus;
- PR1D V1 corpus digest;
- PR1D V2 corpus digest.

Pełne właściwe wartości digestów należy pobrać z zamrożonych receiptów, a nie
przepisywać ręcznie z opisu PR.

Dodatkowe cross-layer scenarios:

1. primary create → session open → apply ack → Ready;
2. create + initial buy w jednej signature;
3. dwie trade mutations w jednej signature;
4. duplicate raw po pierwszym apply;
5. raw + NLN agreement;
6. NLN-first, raw-second;
7. NLN-only;
8. raw/NLN conflict przed MFS;
9. konflikt podczas MFS;
10. konflikt podczas evaluation;
11. konflikt BUY-before-submit;
12. konflikt race z submit;
13. konflikt po submit;
14. konflikt po confirmation;
15. missing transport observation;
16. wrapper/observation provider mismatch;
17. buffered canonical trade replay;
18. buffered trade expiry;
19. AccountUpdate duplicate;
20. same version/different hash;
21. continuity-only restored position;
22. writer stall;
23. queue saturation.

PASS 1E-A:

```text
all existing corpus digests unchanged
production adapters used
test-only fake ledger = 0
test-only fake CandidateIntegrity = 0
unclassified differences = 0
duplicate canonical applies = 0
witness canonical emissions = 0
false Ready = 0
```

#### Commit 1E-B — production authority cutover

Zakres:

- zmienić `ingest_pump_observation` na typed admission;
- usunąć fail-open parent emission;
- wprowadzić private canonical runtime permit;
- wymagać permitu w pool/trade emitters;
- zachowywać permit w buforach;
- wymagać downstream apply receipt;
- aktywować MFS/evaluation guard;
- aktywować submit guard;
- dodać technical abort cleanup;
- zamknąć candidate admission przy niedostępności integrity subsystemu;
- zachować confirmed-position continuity;
- zmienić produkcyjny disposition z `Observe` na `CandidateAdmission`;
- dodać startup preflight;
- dodać runtime health i metrics;
- nie dodawać produkcyjnego mode switcha.

PASS 1E-B:

```text
primary wrapper without canonical permit emits 0 events
exact duplicate emits 0 additional events
secondary raw emits 0 events
parsed NLN emits 0 events
canonical unique primary emits exactly 1 event
canonical apply requires actual downstream mutation
Ready requires complete inventory and all applies
non-Ready reaches MFS = 0
integrity-failed candidate reaches Gatekeeper = 0
pre-submit invalidated BUY reaches sender = 0
confirmed position protective tick continues
legacy parent bypass calls = 0
```

### 1E.16. Differential contract

Dla poprawnych, unikalnych primary raw events następujące elementy pozostają
identyczne:

```text
NewPoolDetected payload
PoolTransaction payload
session state hash
MFS hash
Gatekeeper verdict
reason chain
PM handoff
terminal outcome
```

Każda dozwolona różnica musi mieć jedną klasyfikację:

```text
DUPLICATE_PRIMARY_SUPPRESSED
SECONDARY_WITNESS_SUPPRESSED
PARSED_WITNESS_SUPPRESSED
PRIMARY_BOUNDARY_INCOMPLETE
SOURCE_RECONCILIATION_BLOCK
ACCOUNT_DUPLICATE_SUPPRESSED
ACCOUNT_PROVIDER_CONFLICT
EVALUATION_TECHNICALLY_ABORTED
EXECUTION_CANCELLED_BEFORE_SUBMIT
POST_SUBMIT_RECONCILIATION_REQUIRED
CONFIRMED_POSITION_WITNESS_QUARANTINED
BUFFERED_CANONICAL_APPLY_EXPIRED
```

Twarde bramki:

```text
unclassified_differences = 0
unexpected_verdict_differences = 0
unexpected_reason_chain_differences = 0
unexpected_position_lifecycle_differences = 0
unexpected_account_state_differences = 0
```

### 1E.17. Fault injection

Code review nie jest wystarczającym dowodem PR1E. Obowiązkowo należy celowo
zepsuć po jednym elemencie i wykazać fail-closed:

| Wstrzyknięty błąd | Wymagany wynik |
| --- | --- |
| usunięta observation z primary wrappera | zero emission |
| zmieniony provider ID | zero emission |
| drugi identyczny raw | jedna łączna mutacja |
| różny payload tego samego locatora | conflict |
| brak jednego locatora inventory | brak Ready |
| bus send bez downstream apply | brak Ready |
| duplicate downstream apply | brak Ready |
| poisoned registry | candidate admission closed |
| pełna receipt capacity | candidate admission closed |
| konflikt po MFS | evaluation abort |
| konflikt przed submit | sender nieuruchomiony |
| konflikt po submit | reconciliation required |
| konflikt confirmed position | protective exits działają |
| zatrzymany writer | receiver działa |
| duplicate AccountUpdate | state hash bez zmiany |

Test musi wykazać również, że usunięcie samego guard checku powoduje czerwony
test. To zabezpiecza przed testem przechodzącym niezależnie od implementacji.

### 1E.18. CI i performance

Każdy commit:

```text
cargo fmt --all --check
git diff --check
cargo test -p ghost-core
cargo test -p seer
cargo test -p ghost-launcher
cargo test --workspace
cargo build --release --workspace
```

Dodatkowo:

- dedicated PR1E end-to-end corpus;
- active authority bypass guard;
- CandidateIntegrity lifecycle tests już jako active semantics, nie `would_*`;
- fault injection;
- startup contract tests;
- buffered permit tests;
- continuity tests.

Performance protocol:

- 5 warm-up runs;
- 20 measurement runs;
- ten sam host;
- ten sam release profile;
- identyczny input;
- base: merged PR1D `103212b16bfc059db367e1ceb3c7d00fd307d6c5`;
- branch: finalny PR1E.

Bramki:

```text
missing events = 0
receiver blocked on writer = 0
receiver blocked on overflow = 0
silent drops = 0
throughput lower one-sided 95% CI >= 0.98
receive-to-normalize p99 upper one-sided 95% CI <= 1.05
pending canonical permits at clean shutdown = 0
unacknowledged canonical applies at clean shutdown = 0
```

Performance waiver PR1D nie przechodzi automatycznie na PR1E.

### 1E.19. Zamknięty runtime qualification run

Przed merge finalnego PR1E:

```text
minimum 30 minut
minimum 10 000 successful primary raw Pump mutations
final release binary
final production-like config
live execution disabled
```

Nowa ścieżka ma sterować rzeczywistym izolowanym runtime aż do Gatekeepera.
Nie wystarczy policzyć równoległych metrics.

Wymagania:

```text
primary unique canonical apply ratio = 100%
duplicate second apply = 0
secondary/NLN canonical emissions = 0
false Ready = 0
unclassified integrity outcomes = 0
integrity-failed Gatekeeper invocations = 0
unexpected unaffected verdict differences = 0
pending permit leak = 0
registry unavailable = 0
ledger unavailable = 0
receiver blocked = 0
```

Po pozytywnym runie finalny binary zostaje zmergowany i uruchomiony z aktywnym
PR1 authority. Nie ma długotrwałego produkcyjnego `Observe`.

### 1E.20. Telemetry

Obowiązkowe liczniki:

```text
pr1_runtime_canonical_permit_issued_total
pr1_runtime_canonical_apply_succeeded_total
pr1_runtime_canonical_apply_failed_total
pr1_runtime_duplicate_suppressed_total
pr1_runtime_witness_suppressed_total
pr1_runtime_bypass_attempt_total
pr1_runtime_missing_observation_total
pr1_runtime_wrapper_mismatch_total
pr1_runtime_inventory_incomplete_total
pr1_runtime_integrity_block_before_mfs_total
pr1_runtime_evaluation_abort_total
pr1_runtime_execution_cancel_before_submit_total
pr1_runtime_post_submit_reconciliation_total
pr1_runtime_confirmed_witness_quarantine_total
pr1_runtime_candidate_admission_closed
pr1_runtime_pending_permits
pr1_runtime_oldest_pending_permit_age_ms
```

Każdy counter musi zawierać `authority_epoch_id` w receiptach lub w powiązanym
manifest identity.

### 1E.21. Rollback

Przed wdrożeniem zamrozić:

- poprzedni binary hash;
- poprzedni config hash;
- finalny PR1E binary hash;
- finalny PR1E config hash;
- authority epoch receipt.

Rollback:

```text
previous frozen binary
+
previous frozen config
```

Niedozwolone:

- runtime toggle do `Legacy`;
- per-event fallback;
- fallback przy missing observation;
- fallback przy unavailable registry;
- fallback z conflict do parent wrapper;
- równoległe dwóch aktywnych authority.

### 1E.22. Definition of Done

PR1E jest zakończony wyłącznie wtedy, gdy:

```text
active primary wrapper bypass = 0
active direct NLN canonical emission = 0
active secondary raw canonical emission = 0
canonical event without permit = 0
duplicate canonical runtime mutation = 0
duplicate AccountUpdate state mutation = 0
false CandidateIntegrity Ready = 0
non-Ready MFS materialization = 0
integrity-failed Gatekeeper invocation = 0
pre-submit invalidated sender call = 0
post-submit false cancellation = 0
confirmed-position protective-exit interruption = 0
unclassified differential differences = 0
unexpected unaffected verdict changes = 0
pending permit leaks = 0
receiver blocking = 0
```

Dodatkowo:

- wszystkie korpusy PR1A–PR1D są wykonywalne;
- jeden end-to-end runner używa production code paths;
- new candidate admission jest fail-closed;
- existing confirmed positions zachowują continuity;
- runtime działa bez produkcyjnego `Observe`;
- PR2 pozostaje nietknięty.

### 1E.23. Pierwsza kolejność prac

1. Utworzyć clean worktree z `103212b16bfc059db367e1ceb3c7d00fd307d6c5`.
2. Zapisać baseline receipt.
3. Zmapować wszystkie emitery `NewPoolDetected` i `PoolTransaction`.
4. Zmapować wszystkie session/replay buffery.
5. Udowodnić, że nie istnieje drugi production emitter.
6. Zbudować 1E-A end-to-end runner.
7. Zamrozić corpus manifest.
8. Dopiero wtedy zmienić emission authority.
9. Wprowadzić permit do buforów.
10. Włączyć downstream apply acknowledgement.
11. Włączyć MFS/evaluation/submit guards.
12. Uruchomić fault injection.
13. Uruchomić differential i performance protocol.
14. Uruchomić zamknięty qualification run.
15. Po pełnym PASS zmergować i wdrożyć cały binary.

### 1E.24. Ostateczna decyzja architektoniczna

PR1E nie ma dodawać kolejnego „obserwatora”. Ma wykonać jeden konkretny
cutover:

```text
BYŁO:
primary wrapper → runtime
ledger → evidence

MA BYĆ:
primary wrapper + observation
→ ledger
→ canonical permit
→ runtime
```

Po PR1E Observation Ledger, AccountObservationArbiter i CandidateIntegrity nie
są aparaturą diagnostyczną. Są aktywną granicą integralności nowych
kandydatów.

PR2 może zostać odłożony. Ghost nadal będzie używał dotychczasowej ekonomiki,
ale będzie już działał na jednej canonical structural mutation, jednym
primary raw authority, idempotentnym AccountStateCore, witness-only NLN,
aktywnym technical integrity gate i dokładnie raz zastosowanym event flow.

PR 1 merge gate

unclassified structural differences = 0
duplicate canonical mutations = 0
duplicate account mutations = 0
silent primary gaps = 0
receiver blocked time = 0
unexpected Gatekeeper differences = 0

Po PR1D PR1 pozostaje shadow/observe-only. Po PR1E authority integralności
nowych kandydatów jest aktywne w runtime, podczas gdy live execution pozostaje
wyłączone i nie jest autoryzowane przez ten plan.

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
