# ADR-8D: Ingest State Quote PR1E — aktywacja runtime authority i kwalifikacja PR1

Status:
`IMPLEMENTED / REVIEW REMEDIATION COMPLETE LOCALLY / FINAL LOCAL MATRIX PENDING / DRAFT PR`

Typ: ADR-8D / PR1E / aktywna granica integralności nowych kandydatów

Data: `2026-07-27`

Repo: `/root/Gho_ingest`

Gałąź: `agent/ingest-state-quote-boundary-pr1e-20260727`

Base: `103212b16bfc059db367e1ceb3c7d00fd307d6c5` (merge PR #85 / PR1D)

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Zakres: `PR 1 / PR1E` — executable qualification oraz produkcyjny cutover
authority z parentowego wrappera na canonical decision istniejącego
`PumpObservationLedgerV1`.

## D0. Decyzja

PR1E zmienia aktywną ścieżkę nowych kandydatów z:

```text
primary wrapper -> runtime
ledger -> evidence
```

na:

```text
primary raw wrapper + aligned ObservedPumpMutationV1
  -> PumpObservationLedgerV1
  -> CanonicalRuntimePermitV1
  -> istniejący rich wrapper
  -> Event Bus
  -> rzeczywisty downstream apply
  -> CandidateIntegrity::Ready
  -> MFS
  -> Gatekeeper
  -> guarded submit
```

Wrapper pozostaje nośnikiem kompletnego payloadu, ale nie jest authority.
Brak observation, mismatch, brak canonical mutation albo błąd integrity
oznacza zero strukturalnych emisji. Nie istnieje per-event fallback do
parentowej emisji.

## D1. Authority i typed admission

Launcherowy Seer używa prywatnego:

```text
CanonicalRuntimeAdmissionV1
  = Apply(CanonicalRuntimePermitV1)
  | NoApply(CanonicalRuntimeNoApplyReasonV1)
  | Blocked(CandidateIntegrityOutcomeV1)
```

Permit zawiera:

- exact `CanonicalMutationApplyReceiptV1`;
- `authority_epoch_id`;
- exact `RawPumpMutationLocatorV1`;
- BLAKE3 primary captured payload.

`Apply` jest możliwe tylko dla canonical mutation zwróconej przez produkcyjny
ledger. `ExactDuplicate`, secondary raw, parsed NLN, ambiguous/unmatchable
witness, `ContinuityOnly` oraz `Suppressed` mają zero nowych strukturalnych
emisji.

`NewPoolDetected` i `PoolTransaction` przenoszą permit prywatnie w obrębie
launchera. Publiczne konstruktory kompatybilnościowe tworzą event bez permitu;
wszyscy produkcyjni odbiorcy odrzucają taki event jako bypass.

## D2. Boundary consistency

Przed wydaniem permitu granica porównuje wrapper i observation:

- provider role;
- provider ID;
- candidate pool/mint;
- signature;
- source-neutral locator;
- canonical order;
- mutation family;
- materialne structural claims.

Mismatch daje `PrimaryRawCoverageIncomplete` i zero runtime emission.
Secondary raw i parsed NLN pozostają witness-only niezależnie od arrival
order.

Jedna signature nie jest kluczem dedupe. Każdy exact locator zachowuje osobny
permit i osobny apply proof.

## D3. Buforowanie i downstream apply

Trade przychodzący przed rejestracją puli jest przechowywany jako
`BufferedCanonicalTradeV1`:

- oryginalny `TradeEvent`;
- dokładnie pierwotny permit;
- czas buforowania;
- structural dedupe key.

Replay nie tworzy nowego permitu. Expiry, eviction, duplicate buffer replay,
full/closed per-pool mpsc i Event Bus send failure failują receipt.

Event Bus send i enqueue nie są apply. Tylko prywatny
`CanonicalMutationApplyOutcomeV1::AppliedNewMutation` może wywołać
`mark_canonical_apply_succeeded()`.

Macierz downstream:

| Apply outcome | Receipt |
|---|---|
| `AppliedNewMutation` | acknowledge exactly once |
| `Duplicate` | fail closed |
| `Ignored` | fail closed |
| `Terminal` | fail closed, terminal history bez zmian |
| `Failed` | fail closed |

`Unkeyable` i dust/not-counted nie są apply proof. InitializePool potwierdza
apply wyłącznie po nowej, jednoznacznej instalacji identity/runtime state.

## D4. CandidateIntegrity jako aktywny gate

Istniejący registry i jeden prywatny bounded `CanonicalApplyFenceV1` są
współdzielone przez Seer i OracleRuntime.

`Ready` może opublikować wyłącznie `publish_ready_with_cas()` po:

1. zamknięciu kompletnego raw transaction inventory;
2. zastosowaniu wszystkich exact locatorów oczekiwanych dla konkretnego
   kandydata;
3. braku failure/conflict;
4. zgodności generation/CAS;
5. zachowaniu fazy `PreMfs`.

Inny candidate/curve/mint w tej samej signature nie może spełnić proofu.
Unknown inventory może utworzyć structural canonical mutation, ale nigdy
`Ready`.

Przed MFS:

```text
evaluation_guard
-> check_ready
-> build immutable MFS
-> check_ready
-> mark_mfs_materialized
```

Evaluation ponownie sprawdza generation przed publikacją terminalu.
Integrity failure jest technical termination bez strategicznego
`REJECT`, `TIMEOUT` ani policy reason.

BUY uzyskuje istniejący `CandidateIntegritySubmitGuardV1`. Konflikt przed
`try_begin_submit()` anuluje intent bez sender call. Konflikt po rozpoczęciu
submitu ustawia reconciliation; nie udaje anulowania ani sukcesu. Potwierdzona
pozycja zachowuje monitoring i protective exits, a późny witness jest
quarantined.

Globalne zamknięcie candidate admission jest osobną technical authority
generation. Każdy evaluation oraz submit guard przechowuje generation wydaną
w chwili utworzenia. `close_candidate_admission()` atomowo zamyka admission i
zwiększa generation, więc żaden wcześniej wydany guard nie może już:

- rozpocząć MFS;
- rozpocząć evaluation;
- opublikować nowego BUY;
- rozpocząć submitu.

Wyjątkiem jest submit, który rzeczywiście już osiągnął `SubmitStarted`:
nie jest on fałszywie anulowany, lecz przechodzi istniejącą ścieżkę
confirmation/reconciliation. Confirmed position zachowuje protective exits.

Wspólnym linearization point dla `close_candidate_admission()` i każdego
guard phase transition jest mutex `CandidateIntegrityRegistry::state`.
Transition ponownie sprawdza admission generation po przejęciu tego locka i
bezpośrednio przed commit phase; close pod tym samym lockiem zamyka admission
i inkrementuje generation. Zatem submit może wygrać tylko, gdy jego transition
już przeszedł drugi check i posiada lock; jeżeli close inkrementuje generation
wcześniej, `try_begin_submit()` kończy się typed `AdmissionClosed`.

## D5. Startup, epoch i fail-closed health

Startup nowych candidate admissions wymaga:

- aktywnego Yellowstone gRPC/Geyser gRPC;
- dokładnie jednego stabilnego primary provider ID;
- unikalnych secondary provider ID;
- niezerowych bounded capacities;
- aktywnego Event Bus receivera;
- dostępnego i otwartego CandidateIntegrity registry;
- tej samej instancji registry w Seer i OracleRuntime;
- wyłączonego gRPC-to-legacy-websocket fallbacku.

Walidacja odbywa się synchronicznie w `main` przed spawnem Seera oraz ponownie
na granicy komponentu.

Każdy start tworzy `Pr1AuthorityEpochV1` z hashem finalnego binary, hashem
configu, czasem startu i niezerowym epoch ID. Exactly-once jest ograniczone do
tego runtime epoch i retained bounded state; PR1E nie deklaruje durable
cross-restart deduplication.

Poison, capacity exhaustion, ambiguity albo niedostępność registry zamyka
globalne admission nowych kandydatów. Nie włącza fallbacku i nie zatrzymuje
AccountUpdate/protective handling istniejących potwierdzonych pozycji.

Primary local coverage gap z bounded Yellowstone ingress albo Seer IPC jest
aktywną technical failure, nie tylko metryką. Niezależny control-plane notice
dociera do launcherowego Seera poza business FIFO i wykonuje globalne
zamknięcie admission z invalidacją mutable candidates. Gap secondary/NLN
pozostaje audit-only. Test przechodzi rzeczywiście przez IPC saturation →
watch notice → launcher handler → CandidateIntegrity guard denial.

Control plane zachowuje bounded, monotoniczny zbiór distinct notices zamiast
jednego nadpisywalnego `watch<Option<_>>`: późniejszy secondary notice nie
może ukryć wcześniejszego primary gap. Overflow samego bounded control plane
jest osobnym fail-closed degradation, ponieważ runtime nie potrafi już
dowieść, że nie utracił primary coverage; zwykły, zidentyfikowany gap
secondary/NLN nadal jest audit-only.

Candidate-admission `PoolDetected` wymusza `BackpressurePolicy::Block`
niezależnie od ustawionego `DropNew`; nasycenie tej structural lane zawsze
tworzy primary coverage-gap notice. `ContinuityOnly` i `Suppressed` zachowują
skonfigurowaną politykę, ponieważ nie są nową admission structural mutation.

## D6. Continuity i AccountStateCore

`PoolDetectionRuntimeDispositionV1` ma produkcyjnie tylko:

- `CandidateAdmission`;
- `ContinuityOnly`;
- `Suppressed`.

Historyczne `"observe"` deserializuje się przez alias do
`CandidateAdmission`, ale w kodzie nie istnieje runtime variant `Observe`.

`ContinuityOnly` tworzy zero `NewPoolDetected`, zero `PoolTransaction`, zero
nowych MFS i zero BUY. Ochrona odtworzonej/potwierdzonej pozycji pozostaje w
istniejących monitoringach, raw AccountUpdate i protective-exit lifecycle.

PR1C pozostaje jedyną granicą AccountUpdate. Tylko
`AccountObservationOutcomeV1::AppliedNewMutation` zmienia AccountStateCore.
CandidateIntegrity nie blokuje raw-primary AccountUpdate potwierdzonej
pozycji.

## D7. Executable qualification i differential

Commit 1E-A zamraża:

- baseline receipt dla `103212b`;
- autorytatywną mapę emitterów, buforów i state owners;
- manifest istniejących corpusów PR1B/PR1C/PR1D;
- 23 brakujące cross-layer scenarios;
- runner korzystający z produkcyjnego Ledgera, registry i Event Bus adaptera.

Finalna kwalifikacja musi wykazać:

- unchanged existing corpus digests;
- zero fake Ledgera i fake CandidateIntegrity;
- zero unclassified differences;
- zero duplicate canonical applies;
- zero witness canonical emissions;
- zero false Ready;
- untouched primary payload/state/MFS/Gatekeeper parity.

Fault injection obejmuje missing observation, provider mismatch, duplicate,
conflict, incomplete inventory, bus without apply, duplicate downstream
apply, poison, capacity exhaustion, lifecycle conflicts, writer stall,
queue saturation i AccountUpdate duplicate.

Zamrożony JSONL pozostaje niezmiennym inventory scenariuszy. Każdy jego
`scenario_id` jest wykonywany przez production `PumpObservationLedgerV1`,
typed admission/permit, Event Bus adapter i realny
`PoolObservationSession::ingest_transaction_with_apply_result`; test porównuje
faktyczne canonical emissions, downstream applies, Ready i klasyfikację
różnicy z oczekiwaniem fixture. Nie jest to już test wyłącznie hasha/schematu.

Każdy rekord fixture zawiera i asercjuje także `ready_publications`,
`mfs_materializations`, `gatekeeper_invocations` oraz `sender_calls`.
`primary_create_session_apply_ready` wykonuje `InitializePool` → canonical
`NewPoolDetected` → `OracleRuntime::register_new_pool_with_apply_outcome` →
realne `SessionManager::open_session`; `create_and_initial_buy_one_signature`
łączy `InitializePool` i `Trade` pod jednym inventory. `writer_stall`
zatrzymuje rzeczywisty bounded NLN artifact-writer worker przed odbiorem,
nasyca jego kolejkę i potwierdza canonical apply przed zwolnieniem oraz
fizycznym appendem. `queue_saturation` nasyca realne IPC i odbiera
control-plane notice, zaś `conflict_race_with_submit` używa dwóch wątków i
bariery wokół production submit transition. MFS count pochodzi z
`try_materialize_features()`, a sender count z instrumentowanego adaptera
wywołującego rzeczywisty `CandidateIntegritySubmitGuardV1`.

Pełny parent-versus-Enforce run na tym samym production-like input pozostaje
osobną, niezamkniętą bramką przed merge. Nie wolno opisywać committed
execution corpus jako zastępstwa credentialed parent/current differential.

## D8. Telemetry

Cutover publikuje liczniki/gauge:

- permit issued;
- canonical apply succeeded/failed;
- duplicate/witness suppression;
- bypass, missing observation i wrapper mismatch;
- incomplete inventory;
- block before MFS i evaluation abort;
- cancel before submit, post-submit reconciliation i confirmed quarantine;
- candidate admission closed;
- pending permits i oldest pending permit age.

Metryki i receipts wiążą się z `authority_epoch_id`.

## D9. Granice zakresu

PR1E nie zmienia:

- `PumpQuoteV1`, quote/fee math ani costs;
- sizing, TP/SL, valuation ani PnL;
- strategy score, progów i kolejności Gatekeeper policy;
- `MaterializedFeatureSet` schema;
- Position Manager policy;
- route authorization;
- shadow/live config;
- PR2 transaction-local anchors.

Zmiana polega na aktywacji technicznej integralności istniejącej ścieżki, nie
na zmianie ekonomiki.

## D10. Rollback

Rollback jest atomowy:

```text
previous frozen binary + previous frozen config
```

Nie wolno dodawać:

- runtime toggle do Legacy;
- missing-observation fallback;
- registry-unavailable fallback;
- conflict-to-parent fallback;
- dwóch równoległych aktywnych authority.

Przed rolloutem receipt musi zawierać poprzedni i nowy binary/config hash oraz
finalny authority epoch.

## D11. Stan walidacji

Targeted ledger, CandidateIntegrity, downstream-apply ordering, startup,
buffer, fault-injection i active-guard testy są uruchamiane ponownie po
review remediation. Dopóki pełna macierz nie zostanie ponownie zapisana dla
finalnego SHA, ten ADR nie deklaruje finalnego offline/differential PASS.

Pełne package/workspace suites historycznego receipt'u pozostają czerwone
wyłącznie przez exact historyczne failure classes zamrożone na `103212b`.
Historyczny release workspace build przeszedł. Formalny 5-warmup/20-pair
performance protocol dla wcześniejszego SHA przeszedł bez waivera:

- throughput lower one-sided 95% CI: `1.018097198`;
- receive-to-normalize p99 upper one-sided 95% CI: `0.958477979`;
- missing events: `0`;
- silent drops: `0`;
- receiver saturation blocking: `0`.

Wartości te nie są finalnym receipt'em aktualnego remediacyjnego diffu; po
zacommitowaniu finalnego SHA wymagają ponownego uruchomienia zgodnie z planem.

Parser V1/V2 digests muszą pozostać niezmienione. Szczegóły, log hashes,
rollback identity oraz status niezamkniętych external gates znajdują się w
`PLANS/DO_REALIZACJI/QUALIFICATION_RECEIPT_PR1E_1EB_20260727.md`.

Zamknięty 30-min/10k-mutation run pozostaje twardą bramką przed merge.
Środowisko implementacyjne nie posiada Yellowstone endpointu ani tokenu, więc
ten gate ma status `EXTERNAL ENVIRONMENT BLOCKED`; nie został zastąpiony
symulacją. Draft PR nie autoryzuje merge ani live execution.

## D12. Runtime disposition test contract

Stary-slot raw carrier pozostaje widoczny na IPC integrity boundary z
dyspozycją `Suppressed`. Nie jest to strukturalna runtime emission.
Launcherowy Ledger/admission zatrzymuje go przed Event Busem.

Nowy, prawidłowy raw carrier używa `CandidateAdmission`, ale sama dyspozycja
nie nadaje authority: nadal wymagany jest aligned canonical permit.

## D13. Bounded terminal retirement

PR1E aktywnie używa bounded state, dlatego terminal retention nie może być
ani nieograniczona, ani automatycznym licznikiem do trwałego zamknięcia
admission.

- aktywny `PumpObservationLedgerV1` zachowuje wyłącznie bieżące canonical
  records;
- po bezpiecznym terminalnym cleanupie Oracle CandidateIntegrity przenosi
  record do bounded FIFO tombstone i przekazuje bounded retirement handoff do
  Seer-owned Ledgera;
- Ledger przenosi canonical locator do własnego bounded FIFO terminal lane;
- tombstone zachowuje exact duplicate/late witness classification, ale nigdy
  nie przywraca canonical authority;
- pełny tombstone FIFO evictuje wyłącznie terminal evidence i nigdy aktywnego
  primary record; first eviction oraz counters pozostają audytowalne;
- unresolved canonical receipt nie może być wycofany: terminal cleanup
  fail-closes new admission zamiast go silently evictować.

Pierwszy terminalny technical failure bez unresolved receipt i bez Oracle
session przechodzi identyczną bounded retirement ścieżkę. Dzięki temu missing
observation, boundary mismatch i pre-session conflict nie pozostają trwale w
aktywnych indeksach `records`, `by_pool` i `by_mint`; ich późne evidence
pozostaje wyłącznie w bounded immutable tombstone history.

W ten sposób `max_candidates` i `max_primary_canonical_mutations` pozostają
bounded protection przed aktywnym overloadem, a nie deterministycznym
samozamknięciem runtime po liczbie historycznych, już zakończonych kandydatów.
