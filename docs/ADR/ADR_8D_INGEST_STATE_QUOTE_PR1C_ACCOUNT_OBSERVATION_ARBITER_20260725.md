# ADR-8D: Ingest State Quote PR1C — AccountObservationArbiter

Status: `IMPLEMENTED / FOLLOW-UP REVIEW REMEDIATION IN PROGRESS / BASE COMMIT 42e9db5`

Typ: ADR-8D / PR1C / idempotentna arbitrażowa granica `AccountStateCore`

Data: `2026-07-25`

Repo: `/root/Gho_ingest`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Poprzednik: merge PR #83 / PR1B, commit
`9ebcc40876b7992c3dd249b7de7e05cb91ed1c79`.

Zakres: `PR 1 / Commit 1C` — `AccountObservationArbiter`.

## D0. Decyzja

`MonotonicUpdateGuard`, który przyjmował mutacje z tym samym
`slot`/`write_version` według lokalnego `recv_seq`, zostaje zastąpiony przez
per-mint `AccountObservationArbiter`.

Arbiter rozdziela:

```text
AccountMutationVersionV1 = (account_pubkey, slot, write_version)
AccountObservationIdentityV1 = AccountMutationVersionV1 + data_hash_blake3
AccountProviderObservationIdentityV1 = observation identity
                                       + provider id/role
                                       + txn signature/context
```

`receive_seq` i `receive_ts_ms` pozostają tylko metadanymi transportowymi.
Nie biorą udziału w identity, ordering ani decyzji canonical state.

Jedynie decyzja `AppliedNewMutation` może wywołać redukcję
`CanonicalPoolState`. Wszystkie inne decyzje są no-op dla reserves, velocity,
state-facing counters, session evidence, canonical readiness, ShadowLedger
sync i reconciliation.

## D1. Klasyfikacja i authority

Arbiter wystawia jedną typed decyzję dla każdej odebranej obserwacji:

- `exact_duplicate`;
- `newer_mutation`;
- `older_observation`;
- `same_version_same_hash`;
- `same_version_different_hash_conflict`;
- `write_version_unknown`;
- fail-closed klasy wejściowe: brak provenance, niepoprawny hash, niewspierane
  źródło i conflict identity konta.

Primary raw jest jedynym źródłem mogącym stworzyć canonical mutation.
Secondary raw jest witness-only, także gdy nadejdzie wcześniej. Matching
primary/secondary observation zapisuje agreement. Jeżeli secondary nadejdzie
pierwszy z różnym hashem, późniejszy kwalifikujący się primary nadal stosuje
swój payload dokładnie raz; conflict evidence zachowuje oba fakty. Secondary
nie może uzyskać prawa veta samą kolejnością dostarczenia.

`txn_signature = None` jest poprawnym faktem obserwacyjnym i jest zachowywane
w evidence konfliktu. `None` write version nie jest nigdy równoważne
`Some(0)`.

## D2. Granica aktywnej ścieżki

```text
raw AccountUpdateEvent
  -> OracleRuntime::process_runtime_account_update_event
  -> AccountStateUpdate z provider provenance + captured account data hash
  -> AccountObservationArbiter
  -> AppliedNewMutation only
  -> AccountStateCore / active session / canonical readiness / Shadow sync
```

Parsed `PoolTransaction` może zawierać tuple reserves, ale nie ma raw account
version, captured account payload hash ani provider-role provenance. W PR1C
nie może więc tworzyć `AccountStateCore` ani canonical readiness. Ta ścieżka
zwraca no-op; późniejsza korelacja należy do PR1D.

`apply_rpc_refresh` jest obserwacją diagnostyczną: waliduje identity i
porównuje captured payload hash z raw-primary canonical payloadem, ale nie
mutuje `CanonicalPoolState`, reserves, phase, counterów, velocity, provenance
ani freshness. Nie uczestniczy w raw Geyser arbitration i nie przesuwa
watermarku raw observations. Dzięki temu nie istnieje drugi repair authority
obok raw primary.

Tożsamość mintu nie jest dożywotnio przywiązana do pierwszego pubkeya.
Dozwolone jest wyłącznie jawne przejście raw-primary `Pump.fun bonding curve`
→ `PumpSwap pool`, po uprzedniej canonical completion Pump.fun i przy nowszym
version key. Nieznany owner/program, secondary albo nieudowodniona zmiana
pubkeya fail-close jako `account_identity_conflict`. Po migracji phase nie
może zostać zdegradowana z `Migrated` przez layout-local `is_complete = 0`.

## D3. Differential corpus — hard gate

Przed implementacją zamrożono transport-neutral corpus:

`ghost-core/tests/fixtures/account_observation_arbiter_v1/account_observation_differential_corpus_v1.jsonl`

Digest BLAKE3 v1:

```text
12472c3e8f43f28185b520f3c93a3c1e04d376a46347f42c5935c6b53665d706
```

Corpus v1 obejmuje dokładnie:

- duplicate jednego providera i reconnect/replay duplicate;
- ten sam payload od primary i secondary;
- ten sam version/hash z różnymi signature;
- ten sam version z różnymi hashami;
- starszy write version po nowszym;
- `write_version = None`, w tym `None` kontra `Some(0)`;
- secondary-first agreement i secondary-first conflict;
- kilka mutacji jednego konta w serii.

Review ujawnił, że v1 przypisywał błędny outcome scenariuszowi
`secondary_first_then_primary_conflicts`. V1 pozostaje niezmieniony jako
historyczny receipt; jego digest i inventory są nadal testowane.

Wersją wykonywalną jest nowy, pełny corpus v2:

`ghost-core/tests/fixtures/account_observation_arbiter_v1/account_observation_differential_corpus_v2.jsonl`

Digest BLAKE3 v2:

```text
63839d047310638fe0d8643ee6c71148ac292f4390fc9098a2e573ce0ac1e051
```

V2 zachowuje ten sam complete inventory, lecz potwierdza poprawną regułę:
secondary-first conflict zachowuje evidence, a późniejszy primary stosuje
swoją canonical mutation dokładnie raz. Test replay sprawdza typed outcome
oraz to, że każdy no-op zachowuje niezmienione canonical state i
reserve-velocity evidence.

## D3A. Bounded evidence i restart

PR1C nie tworzy Observation Ledgera. Exactly-once oznacza wyłącznie
*in-process* dla życia jednego `AccountObservationArbiter`; po restarcie
watermark nie jest hydratowany i PR1D musi wprowadzić durable reconciliation.
Jest to jawnie przetestowane, a nie deklarowane jako globalna gwarancja.

Evidence jest ograniczone na mint w niezależnych pasach authority/evidence:
64 primary-capable version records oraz osobno 64 witness-only version records.
W ramach jednego version key jest do 32 unique observations primary i osobno
do 32 unique observations secondary. Exact replay nie zużywa nowego wpisu.
Non-conflict primary versions starsze od current latest mogą zostać jawnie
pruned z licznikiem; witness records nie są cicho wyrzucane.

Nasycenie pasa secondary zapisuje typed `evidence_capacity_exceeded`, ustawia
`secondary_evidence_complete = false` i zachowuje pełny immutable snapshot
pierwszej odrzuconej obserwacji (hash, provider id/role, signature oraz
context). Nie odbiera to capacity z pasa primary: późniejszy kwalifikujący się
primary jest nadal zapisywany i może zastosować canonical mutation dokładnie
raz. Nasycenie samego pasa primary pozostaje fail-closed, ale nie jest veto
sterowanym przez secondary.

## D4. Poza zakresem

PR1C nie zawiera:

- NLN, Observation Ledger ani raw/NLN correlation;
- zmian modelu, cech, `MaterializedFeatureSet` albo Gatekeepera;
- strategii, quote math ani execution;
- proof-based recovery local gap.

Nie ma nowej ścieżki wyboru policy przez `provider_role`. Rola jest użyta
wyłącznie na granicy arbitrażu account observation.

## D5. Dowód i status weryfikacji

Świeżo wykonano na roboczym diffie po follow-up review:

```text
PASS  cargo fmt --all --check
PASS  git diff --check
PASS  cargo test -p ghost-core account_state_core::observation_arbiter --lib -- --nocapture
      (13 testów: secondary-first conflict bez veta, nasycenie witness lane
      per-version i per-index bez veta, retained first rejected overflow
      provenance dla secondary i primary, in-process exactly-once,
      `None != Some(0)` oraz backward-compatible serde)
PASS  cargo test -p ghost-core --test account_observation_arbiter_corpus_tests --test account_state_core_tests -- --nocapture
      (2 replay corpus + 9 testów integracyjnych AccountStateCore, w tym
      kontrolowana migracja Pump.fun -> PumpSwap)
PASS  cargo check -p ghost-launcher --lib
PASS  cargo check -p ghost-brain --tests
PASS  timeout 900s cargo build --release --workspace

`cargo check -p ghost-launcher --test session_lifecycle_tests` nadal kończy
się przed wykonaniem testów na znanej baseline E0063 w initializerze
`PoolTransaction` (brak starszych pól `complete`, `real_sol_reserves`,
`real_token_reserves` i kolejnych) w `session_lifecycle_tests.rs:28`.
Naprawiony helper `test_account_update` jest w tym samym targetcie, ale testy
sesji nie mogą zostać uruchomione, dopóki ten niezwiązany fixture transakcji
nie zostanie formalnie odblokowany poza zakresem PR1C.

`cargo check -p ghost-launcher --test gatekeeper_policy_tests` ma tę samą
baseline klasę E0063 w `gatekeeper_policy_tests.rs:845`. Żaden z tych
`PoolTransaction` fixture’ów nie jest przedstawiany jako zielona bramka ani
nie został przypadkowo naprawiony w PR1C.
```

Poniższe pełne package/workspace testy należą do historycznego baseline
receipt PR1C. Nie zostały ponownie uruchomione w tej lokalnej korekcie; nadal
nie są przedstawiane jako zielone ani jako waiver:

```text
BASELINE FAIL  cargo test -p ghost-core
               ghost-core/tests/pr1_contracts_foundations.rs:
               foundational_types_serialize_and_deserialize_roundtrip
               -> InvalidTagEncoding(104)

BASELINE FAIL  cargo test -p trigger
               istniejące testy statusu Jito oraz ograniczenia presigned
               transaction

BASELINE FAIL  cargo check -p ghost-launcher --tests
               istniejące E0063 w fixture'ach PDD / temporal carry-forward /
               TAS, z brakującymi starszymi polami PoolTransaction

BASELINE FAIL  timeout 300s cargo test -q --workspace
               istniejące E0063 dla PoolTransaction w
               oracle_continuous_sampling.rs oraz
               oracle_event_bus_integration.rs; exit 101

BASELINE BLOCKED  cargo test -p seer
                  bieżący proces testowy nie zakończył się przed przerwaniem
                  ręcznym po braku ograniczenia czasu. Nie klasyfikujemy tego
                  jako PASS; historyczny receipt baseline dokumentuje
                  istniejący timeout i failure signatures Seera.
```

Żaden znany baseline failure nie jest przedstawiany jako zielona bramka. Zielony
release build jest istotny: wybrany baseline ma tę samą zieloną bramkę, a PR1C
nie wprowadza regresji kompilacji workspace w profilu release.

## D6. Rollback

Rollbackiem jest revert finalnego commita PR1C. Nie ma migracji danych,
trwałego Observation Ledger ani zmiany konfiguracji wymagającej migracji.
