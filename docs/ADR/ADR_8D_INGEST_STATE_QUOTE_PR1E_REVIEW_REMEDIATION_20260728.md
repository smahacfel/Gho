# ADR-8D: PR1E — remediacja executable corpus, coverage gap i lifecycle admission

Status:
`IMPLEMENTED LOCALLY / TARGETED VALIDATION PASS / FINAL MATRIX PENDING / DRAFT PR`

Data: `2026-07-28`

Repo: `/root/Gho_ingest`

Gałąź: `agent/ingest-state-quote-boundary-pr1e-20260727`

Base PR1E: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`

Parent review head: `757f9ba19cd6f090c20a91a40df2f40b99d24cf6`

Normatywny plan:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Powiązany receipt:
`PLANS/DO_REALIZACJI/QUALIFICATION_RECEIPT_PR1E_1EB_20260727.md`

## D0. Cel wąskiej remediacji

Niezależny review wykazał pięć lokalnych luk PR1E przed uruchomieniem
zewnętrznej kwalifikacji:

1. zamrożony JSONL obejmował 23 nazwy, ale część nazw nie uruchamiała
   odpowiadających im produkcyjnych sekwencji;
2. structural `PoolDetected` dziedziczył konfigurowalne `DropNew` i mógł
   utworzyć primary coverage gap bez control-plane notice;
3. semantyka wyścigu globalnego close admission z submit transition nie była
   jawnie zlinearyzowana ani deterministycznie testowana;
4. pierwsze technical failures sprzed utworzenia Oracle session mogły
   zajmować aktywną registry capacity do globalnego zamknięcia;
5. receipt mieszał historyczne wyniki z wcześniejszego SHA z aktualną
   remediacją.

Remediacja nie zmienia quote math, strategii, Gatekeeper policy,
`MaterializedFeatureSet` schema, Position Manager policy ani PR2.

## D1. Structural PoolDetected nie może DropNew

`IpcSender::send_with_observation_and_disposition()` wymusza teraz
`BackpressurePolicy::Block` dla `CandidateAdmission`. W konsekwencji pełna
IPC kolejka dla primary `PoolDetected` tworzy
`LocalCoverageGapReasonV1::IpcEgressQueueSaturated` i niezależny notice do
launchera zamiast cichego `EventDropped`.

`ContinuityOnly` i `Suppressed` pozostają non-admission traffic i dalej
używają konfiguracji, ponieważ nie mogą tworzyć nowego kandydata.

Inwariant:

```text
primary CandidateAdmission PoolDetected + full IPC + config DropNew
  -> coverage-gap control plane
  -> candidate admission closed
  -> MFS/Gatekeeper/new submit = 0
```

## D2. Linearizacja global close admission i submit

Jedynym wspólnym linearization lockiem jest istniejący
`CandidateIntegrityRegistry::state`.

```text
transition_guard_phase:
  check admission generation
  lock state
  re-check admission generation
  commit phase

close_candidate_admission:
  lock state
  close admission + increment authority_admission_generation
  unlock
```

Wynik jest jawny:

- transition, który po drugim checku posiada lock, może ukończyć
  `SubmitStarted`; późniejszy close wymaga reconciliation;
- close, który pod tym lockiem inkrementuje generation wcześniej, powoduje
  typed `AdmissionClosed`; sender nie wystartuje;
- confirmed-position continuity nie jest anulowana.

Dwa testy z barierami wymuszają oba warianty. Nie dodano drugiego lifecycle,
kolejki ani globalnego stanu.

## D3. Bounded pre-session technical retention

Pierwszy non-Ready `record_signal()` bez unresolved canonical receipt jest
natychmiast przenoszony z aktywnych `records` / `by_pool` / `by_mint` do
istniejącego bounded terminal tombstone FIFO. Dotyczy to między innymi:

- missing transport observation;
- wrapper/observation mismatch;
- pre-session provider conflict;
- buffered canonical failure przed pool birth.

Jeżeli receipt pozostaje unresolved, rekord nie może zostać wycofany i
pozostaje active proof obligation. Brak capacity tombstone/handoff
fail-closes nowe admission; nie następuje cicha evikcja unresolved evidence.

## D4. Executable 23-scenario corpus

Każdy rekord
`ghost-launcher/tests/fixtures/pr1e/pr1e_cross_layer_scenarios_v1.jsonl`
jest wykonywany przez produkcyjne granice PR1E. Fixture asercjuje:

- canonical emissions;
- downstream applies;
- Ready publications;
- real MFS materializations;
- Gatekeeper invocations;
- sender calls;
- false Ready;
- typed difference classification.

Szczególne przypadki są wykonywane tak:

- create/session: `InitializePool` → canonical `NewPoolDetected` → actual
  `OracleRuntime::register_new_pool_with_apply_outcome` →
  `SessionManager::open_session` → receipt acknowledge;
- create+buy: `InitializePool` i `Trade` mają wspólną signature oraz
  inventory, a Ready pojawia się dopiero po obu apply;
- writer stall: rzeczywisty bounded `NlnArtifactWriter` worker jest
  zatrzymany przed odbiorem, jego kolejka jest pełna, canonical primary path
  nadal dokonuje apply przed zwolnieniem workera, a po zwolnieniu następuje
  rzeczywisty artifact append;
- queue saturation: rzeczywisty IPC jest pełny, a runner odbiera realny
  watch notice przed zamknięciem registry;
- submit race: dwa wątki i bariera wykonują rzeczywisty submit transition i
  conflict; sender adapter inkrementuje licznik tylko po `StartedNow`;
- MFS: wywoływane jest `PoolObservationSession::try_materialize_features()`,
  a konflikt pomiędzy build i publication orphanuje snapshot bez MFS publish.

Nie utworzono fake Ledgera, fake Registry ani drugiego authority path.

## D5. Stan dowodu i granice

Zmienione zostały digesty PR1E corpus manifestu oraz corpus scenariuszy;
wartości są zamrożone w testach i receipcie. Istniejące PR1B/PR1C/PR1D digesty
pozostają nietknięte.

Aktualny lokalny SHA-256 source runnera wynosi
`117fa9d5783e487aca3075645b6b1ca7304c38117b8e9b0255324b2554038c1e`.
Po commicie finalny receipt musi powiązać go z commit SHA i końcową macierzą;
sam hash source nie stanowi merge qualification.

Celowane testy lokalne potwierdzają poprawki, ale ten ADR nie deklaruje
`OFFLINE PASS`, `DIFFERENTIAL PASS`, zielonego CI ani merge qualification.
Potwierdzone bezpośrednio na local remediation diffie: `cargo check -p
ghost-launcher --lib`, 27 testów `candidate_integrity`, 4 testy
`pr1e_qualification` oraz dokładnie jeden test IPC `DropNew` structural
PoolDetected. Nadal wymagane przed merge są:

1. pełna package/workspace/release matrix na finalnym, zacommitowanym SHA;
2. machine-readable parent-versus-Enforce differential na production-like
   input;
3. credentialed 30-min / 10k-primary closed runtime run;
4. authority-epoch receipt z finalnego binary/configu;
5. niezależna klasyfikacja aktualnych czerwonych GitHub Actions.

PR pozostaje Draftem. Brak closed-run credentials nie jest zastępowany testem,
symulacją ani historycznym receiptem.
