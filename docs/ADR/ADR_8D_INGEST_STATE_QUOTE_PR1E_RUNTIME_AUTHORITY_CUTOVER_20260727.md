# ADR-8D: Ingest State Quote PR1E — aktywacja runtime authority i kwalifikacja PR1

Status:
`IMPLEMENTED / OFFLINE QUALIFICATION PASS / CLOSED RUNTIME RUN PENDING / DRAFT PR`

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
buffer, fault-injection i active-guard testy przeszły.

Pełne package/workspace suites pozostają czerwone wyłącznie przez exact
historyczne failure classes zamrożone na `103212b`. Release workspace build
przeszedł. Formalny 5-warmup/20-pair performance protocol przeszedł bez
waivera:

- throughput lower one-sided 95% CI: `1.018097198`;
- receive-to-normalize p99 upper one-sided 95% CI: `0.958477979`;
- missing events: `0`;
- silent drops: `0`;
- receiver saturation blocking: `0`.

Parser V1/V2 digests pozostały niezmienione. Szczegóły, log hashes i rollback
identity znajdują się w
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
