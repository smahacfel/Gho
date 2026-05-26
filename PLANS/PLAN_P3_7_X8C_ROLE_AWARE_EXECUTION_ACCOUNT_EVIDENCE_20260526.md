# PLAN P3.7-X8C Role-Aware Execution Account Evidence

Data: 2026-05-26

Status: plan naprawczy po walidacji z aktualnym repo.

Tryb: planning / contract only. Ten dokument nie implementuje zmian runtime.

## 1. Cel

Naprawic luke BCV2 working-builder readiness przez dodanie osobnej, role-aware warstwy evidence dla kont wymaganych przez execution builder.

Docelowy przeplyw:

```text
observed tx / exact-watch / RPC hydration / Yellowstone AccountUpdate
-> ExecutionAccountEvidence(role=BondingCurveV2, account_pubkey=<BCV2>)
-> OracleRuntime ExecutionAccountEvidenceStore
-> exact lookup po (BondingCurveV2, BCV2 pubkey)
-> working-builder diagnostics
-> manifest-ready tylko po execution-load-ready evidence
-> dopiero potem probe/entry/lifecycle denominator
```

Najwazniejszy kontrakt: `AccountStateCore` pozostaje canonical pool-state store keyed by `base_mint`. Nie staje sie role-aware execution-account evidence store.

## 2. Walidacja aktualnego repo

Stan aktualnego checkoutu potwierdza diagnoze z planu wejsciowego, ale wymaga precyzyjniejszego kontraktu implementacyjnego.

### 2.1 AccountStateCore nie jest miejscem na BCV2 role evidence

Aktualne pliki:

- `ghost-core/src/account_state_core/types.rs`
- `ghost-core/src/account_state_core/reducer.rs`
- `ghost-core/src/pool_identity.rs`
- `ghost-launcher/src/oracle_runtime.rs`

Fakty z kodu:

- `AccountStateUpdate` ma pola `pool_amm_id`, `base_mint`, `bonding_curve`, reserves, `slot`, `write_version`, `receive_ts_ms`, `receive_seq`, `curve_finality`, `source`.
- `AccountStateUpdate` nie ma `account_pubkey`, `account_role`, `bonding_curve_v2`, `owner`, `data_len`, ani source-account identity.
- `AccountStateReducer::apply_account_update()` zapisuje stan pod `update.base_mint`.
- `AccountStateReducer::apply_account_update()` przy bootstrapie preferuje `bootstrap.pool_amm_id` i `bootstrap.bonding_curve` zamiast wartosci z update.
- `PoolIdentityRegistry` mapuje tylko `pool_id <-> base_mint <-> bonding_curve`.
- `OracleRuntime::build_account_state_update()` buduje canonical update przez `PoolIdentityRegistry::get_by_base_mint()` albo `get_by_bonding_curve()`, czyli rowniez w modelu classic `bonding_curve`, nie role-aware BCV2.

Wniosek: wpychanie BCV2 do `AccountStateCore` jako `bonding_curve` byloby bledne. Store BCV2 musi byc osobny.

### 2.2 Transaction path zna BCV2 jako osobne execution konto

Aktualne pliki:

- `off-chain/components/seer/src/types.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `off-chain/components/trigger/src/direct_buy_builder.rs`

Fakty z kodu:

- `seer::types::TradeEvent` ma `bonding_curve_v2: Option<Pubkey>` i `bonding_curve_v2_provenance: Option<ObservedAccountMetaProvenance>`.
- `ghost-launcher::events::PoolTransaction` ma analogiczne pola `bonding_curve_v2` i `bonding_curve_v2_provenance`.
- `ghost-launcher/src/components/seer.rs::trade_event_to_pool_transaction()` przenosi BCV2 i provenance z Seer do `PoolTransaction`.
- `DirectBuyBuilder::build_buy_ix_with_accounts_and_bonding_curve_v2()` wstawia `bonding_curve_v2` jako konto instrukcji pod indeksem 16.
- `TriggerComponent::counterfactual_probe_account_role_for()` i `TriggerComponent::counterfactual_probe_required_account_roles()` potrafia sklasyfikowac role `bonding_curve_v2` dla manifestu execution.

Wniosek: builder/manifest role model istnieje, ale brakuje runtime evidence store, ktory zachowuje dokladne `(role, pubkey)`.

### 2.3 Seer exact-watch i hydration istnieja jako markery, nie jako typed evidence

Aktualne pliki:

- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/ipc.rs`

Fakty z kodu:

- `binary_parser.rs::register_route_compatible_observed_bcv2()` rejestruje route-compatible BCV2 przez `AccountRegistry::insert_bcv2()` i loguje `BCV2_EXACT_WATCH_REGISTERED`.
- `Bcv2HydrationService` loguje `BCV2_RPC_HYDRATION_READY` albo `BCV2_RPC_HYDRATION_MISSING`.
- `grpc_connection.rs::AccountRegistry` ma osobna lane `bcv2_accounts`, `bcv2_resub_notify` i `snapshot_by_lane()`.
- `grpc_connection.rs::build_subscribe_request_for_profile()` loguje `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED` oraz `BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED`.
- `grpc_connection.rs::route_update()` loguje `BCV2_ACCOUNT_UPDATE_RECEIVED` gdy account update pubkey jest w `AccountRegistry::bcv2_accounts`.
- `Seer::handle_account_update()` najpierw wywoluje `decode_canonical_account_update(owner, data)`. Jesli BCV2 nie dekoduje sie jako canonical curve/pool update, obecna sciezka konczy sie parse-failed i nie emituje IPC eventu.
- `seer::ipc::SeerEvent` ma tylko `PoolDetected`, `Trade`, `FundingTransfer`, `AccountUpdate`.

Wniosek: X8AS/X8B markery pokazuja transport/diagnostic coverage, ale nie istnieje typed event `ExecutionAccountEvidence`, ktory przechodzi przez IPC i event bus.

### 2.4 Launcher bridge i OracleRuntime nie maja role-aware evidence store

Aktualne pliki:

- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/oracle_runtime.rs`

Fakty z kodu:

- `GhostEvent` nie ma wariantu `ExecutionAccountEvidence`.
- `AccountUpdateEvent` jest canonical reserve update i nie powinien byc rozszerzany o BCV2 role semantics.
- `SessionAccountUpdateBridge` ma `detected_keys: HashMap<Pubkey, Instant>` i bufor `pending_updates`, ale rejestruje/flushuje tylko `pool_amm_id`, `bonding_curve`, `base_mint`.
- `SessionAccountUpdateBridge::refresh_from_trade()` odswieza tylko `[trade.pool_amm_id, trade.mint]`; nie uzywa `trade.bonding_curve_v2`.
- Petla `start_oracle_runtime_task...` obsluguje `GhostEvent::AccountUpdate(event)` i wysyla go do worker queue dla `OracleRuntime::process_account_update_with_explicit_source(...)`.
- Nie ma osobnego pola `execution_account_evidence_store` w `OracleRuntime`.

Wniosek: nawet gdy transport zobaczy BCV2 account update, runtime nie ma typowanego miejsca, gdzie moze zapisac exact `(BondingCurveV2, pubkey)` jako dowod dla working-builder.

### 2.5 P3.7 diagnostics sa bogate, ale nadal nie sa runtime evidence store

Aktualne pliki:

- `ghost-launcher/src/oracle_runtime.rs`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`

Fakty z kodu:

- `oracle_runtime.rs::p37_working_builder_parity_diagnostics()` wylicza pola `working_builder_bcv2_*` z manifestu, precheck diagnostics, observed provenance, MFS/diag/account-state flags.
- `p37_working_builder_bcv2_materialization_diagnostics()` ustawia `subscription_requested = Some(false)`, mimo ze X8AS/X8B maja globalne exact-watch markery. To jest symptom braku typed runtime evidence.
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py::working_builder_parity_payload()` traktuje `working_builder_manifest_ready_rows` jako gotowe dopiero gdy BCV2 ma source-authority + materialization evidence.
- Obecna definicja `bcv2_materialization_evidence_ready()` uznaje `account_state_materialized`, `mfs_materialized`, `diag_materialized` albo `rpc_fetch_ready`. Po X8C readiness musi przejsc na nowy role-aware evidence store i przestac bazowac na canonical `AccountStateCore` jako dowodzie BCV2.

Wniosek: raporty sa dobrym outputem, ale zrodlo prawdy dla BCV2 readiness musi zostac przeniesione do `ExecutionAccountEvidenceStore`.

### 2.6 X8B runtime evidence zmienia blocker

Aktualny lokalny raport:

- `PLANS/AUDYT/RAPORT_P3_7_X8B_BCV2_WORKING_BUILDER_EVIDENCE_GAP_PROVIDER_TIMING_LAYOUT_AUDIT_20260526.md`

Fakty z raportu:

- `20 / 20` working-builder BCV2 pubkeys bylo registered/included/resubscribed.
- `7 / 20` working-builder BCV2 pubkeys mialo same-pubkey `BCV2_ACCOUNT_UPDATE_RECEIVED`.
- `13 / 20` nie mialo same-pubkey update w tym runie.
- `0 / 20` mialo hydration ready; `20 / 20` mialo hydration missing.
- Run ma watchdog caveat, wiec true missing/not-loadable nie jest potwierdzone.
- R18 pozostaje NO-GO.

Wniosek: nastepny krok nie powinien byc R18 ani kolejny fallback. Nastepny krok to typed role-aware evidence path.

## 3. Twarde non-goals

Nie zmieniamy:

- `DirectBuyBuilder` account layout ani discriminator/payload.
- Helius Sender / LiveTxSender / live submit.
- Gatekeeper, scoring, thresholds, V3 policy.
- `MaterializedFeatureSet` jako canonical decision snapshot.
- `AccountStateCore` semantics.
- `AccountUpdateEvent` semantics.
- `legacy_buy` / fallback handoff / R18 / P2/live path.

Nie wolno:

- uznac observed tx meta za execution-load-ready.
- mapowac BCV2 na classic `bonding_curve`.
- odblokowac manifest-ready na `DiscoveryHint`, `ExactWatchRegistered` albo `SubscribeIncluded`.
- traktowac `AccountUpdateReceived` jako final execution-load-ready w pierwszej integracji.
- uruchamiac R18 bez PASS-A z evidence-gated readiness.

## 4. Slownik kontraktu

### 4.1 Evidence vs execution readiness

W kodzie nie uzywac ogolnego pola `ready` bez kwalifikatora.

Wprowadzamy dwa pojecia:

- `evidence_ready`: konto zostalo realnie zobaczone albo pobrane jako konto o danym pubkeyu i roli.
- `execution_load_ready`: evidence spelnia konserwatywna polityke odblokowania manifestu execution.

Minimalnie:

- `ObservedTxMeta`, `ExactWatchRegistered`, `ExactWatchSubscribeIncluded` moga dac tylko discovery/subscription evidence.
- `YellowstoneAccountUpdate` moze dac `evidence_ready=true`, ale w X8C nie daje samo `execution_load_ready=true`.
- `RpcHydration` / `RpcPrecheck` ze statusem ready i owner/data_len daja execution-load-ready, jesli nie ma nowszego missing/conflict/stale.

### 4.2 Role naming

Canonical role names w JSON/logach:

- `bonding_curve_v2`
- `creator_vault`
- `user_ata`
- `associated_bonding_curve`
- `payer`
- `other:<name>`

Rust enum:

```rust
pub enum ExecutionAccountRole {
    BondingCurveV2,
    CreatorVault,
    UserAta,
    AssociatedBondingCurve,
    Payer,
    Other(String),
}
```

`ExecutionAccountRole` musi miec `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`, bo store bedzie indeksowany po `(role, pubkey)`.

### 4.3 Source and status

```rust
pub enum ExecutionAccountEvidenceSource {
    ObservedTxMeta,
    ExactWatchRegistered,
    ExactWatchSubscribeIncluded,
    YellowstoneAccountUpdate,
    RpcHydration,
    RpcPrecheck,
    ManifestPrecheck,
}
```

```rust
pub enum ExecutionAccountEvidenceStatus {
    DiscoveryHint,
    SubscriptionRequested,
    SubscribeIncluded,
    AccountUpdateReceived,
    RpcReady,
    RpcMissing,
    PrecheckReady,
    PrecheckMissing,
    DecodeFailed,
    Unmapped,
}
```

Status precedence ma byc jawna funkcja, nie wynik kolejnosci enum:

```rust
impl ExecutionAccountEvidenceStatus {
    pub fn precedence(self) -> u8 { ... }
    pub fn is_positive_evidence(self) -> bool { ... }
    pub fn is_missing_or_negative(self) -> bool { ... }
}
```

Minimalna precedence:

```text
DiscoveryHint < SubscriptionRequested < SubscribeIncluded < AccountUpdateReceived < RpcReady < PrecheckReady
RpcMissing/PrecheckMissing/DecodeFailed sa negative evidence i nie usuwaja starszego positive evidence.
```

Konflikt nie moze byc kasowany przez overwrite. Store musi zachowac `conflict_status` albo `latest_negative`.

## 5. Nowy core model

Plik:

- `ghost-core/src/execution_account_evidence.rs`

Re-export:

- `ghost-core/src/lib.rs`

Minimalny typ:

```rust
pub struct ExecutionAccountEvidence {
    pub role: ExecutionAccountRole,
    pub account_pubkey: Pubkey,
    pub base_mint: Option<Pubkey>,
    pub pool_id: Option<Pubkey>,
    pub canonical_bonding_curve: Option<Pubkey>,

    pub source: ExecutionAccountEvidenceSource,
    pub status: ExecutionAccountEvidenceStatus,

    pub slot: Option<u64>,
    pub context_slot: Option<u64>,
    pub write_version: Option<u64>,

    pub owner: Option<Pubkey>,
    pub data_len: Option<u64>,

    pub tx_signature: Option<String>,
    pub observed_instruction_index: Option<u32>,
    pub observed_account_position: Option<u32>,
    pub provenance_status: Option<String>,

    pub detected_at_ms: u64,
    pub received_at_ms: u64,
    pub evidence_ready: bool,
    pub reason: Option<String>,
}
```

Store:

```rust
pub struct ExecutionAccountEvidenceStore {
    by_role_pubkey: DashMap<(ExecutionAccountRole, Pubkey), ExecutionAccountEvidenceRecord>,
    by_base_mint_role: DashMap<(Pubkey, ExecutionAccountRole), Vec<Pubkey>>,
    by_pool_role: DashMap<(Pubkey, ExecutionAccountRole), Vec<Pubkey>>,
}
```

`ExecutionAccountEvidenceRecord` powinien zawierac:

```rust
pub struct ExecutionAccountEvidenceRecord {
    pub best_positive: Option<ExecutionAccountEvidence>,
    pub latest_negative: Option<ExecutionAccountEvidence>,
    pub latest: ExecutionAccountEvidence,
    pub conflict: Option<ExecutionAccountEvidenceConflict>,
}
```

Minimalne API store:

```rust
impl ExecutionAccountEvidenceStore {
    pub fn new() -> Self;
    pub fn upsert(&self, evidence: ExecutionAccountEvidence) -> UpsertExecutionAccountEvidenceResult;
    pub fn get(&self, role: ExecutionAccountRole, account_pubkey: &Pubkey) -> Option<ExecutionAccountEvidenceRecord>;
    pub fn find_by_base_mint_role(&self, base_mint: &Pubkey, role: ExecutionAccountRole) -> Vec<Pubkey>;
    pub fn find_by_pool_role(&self, pool_id: &Pubkey, role: ExecutionAccountRole) -> Vec<Pubkey>;
    pub fn snapshot_counts(&self) -> ExecutionAccountEvidenceSnapshotCounts;
}
```

Wazne:

- `RpcMissing` nie usuwa `AccountUpdateReceived`; ustawia konflikt albo latest negative.
- `RpcReady` / `PrecheckReady` wygrywaja nad `DiscoveryHint`.
- lookup dla readiness zawsze uzywa exact `(role, pubkey)`.
- indeksy `by_base_mint_role` i `by_pool_role` sa pomocnicze; nie moga zastapic exact lookup.

## 6. Etapy wykonawcze

### X8C-PR1 - Core model and store

Cel:

Dodac core typy i store bez IPC, bez Seer producerow, bez OracleRuntime wiring i bez readiness unlock.

Pliki:

- `ghost-core/src/execution_account_evidence.rs`
- `ghost-core/src/lib.rs`
- `ghost-core/tests/execution_account_evidence_tests.rs` albo testy inline w module

Zakres:

- enumy role/source/status.
- `ExecutionAccountEvidence`.
- `ExecutionAccountEvidenceRecord`.
- `ExecutionAccountEvidenceStore`.
- jawna precedence/status merge.
- snapshot counts.

Acceptance:

```bash
cargo test -p ghost-core execution_account_evidence -- --nocapture
cargo check -p ghost-core
cargo fmt --check
git diff --check
```

Testy krytyczne:

- zapisuje evidence pod exact `(role, pubkey)`.
- `BondingCurveV2` nie kolapsuje do classic `bonding_curve`.
- `RpcReady` bije `DiscoveryHint`.
- `RpcMissing` nie usuwa starszego `AccountUpdateReceived`, tylko tworzy konflikt/latest negative.
- `find_by_base_mint_role(base_mint, BondingCurveV2)` zwraca exact BCV2 pubkey.
- `find_by_pool_role(pool_id, BondingCurveV2)` zwraca exact BCV2 pubkey.

### X8C-PR2 - IPC and launcher event schema

Cel:

Przeniesc typed evidence przez Seer IPC i Ghost event bus bez zmiany semantyki `AccountUpdateEvent`.

Pliki:

- `off-chain/components/seer/src/ipc.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`

Nowy IPC payload:

```rust
pub struct DetectedExecutionAccountEvidenceEvent {
    pub evidence: ghost_core::ExecutionAccountEvidence,
    pub sequence_number: u64,
    pub priority: EventPriority,
}
```

Warianty:

```rust
SeerEvent::ExecutionAccountEvidence(DetectedExecutionAccountEvidenceEvent)
GhostEvent::ExecutionAccountEvidence(ExecutionAccountEvidenceEvent)
```

Launcher event powinien uzywac core typu, np.:

```rust
pub struct ExecutionAccountEvidenceEvent {
    pub evidence: ghost_core::ExecutionAccountEvidence,
    pub sequence_number: u64,
}
```

Dodac helpery:

```rust
IpcSender::send_execution_account_evidence(...)
GhostEvent::execution_account_evidence(...)
emit_execution_account_evidence_to_event_bus(...)
```

Zakaz:

- nie dodawac BCV2 pol do `DetectedAccountUpdateEvent`.
- nie dodawac BCV2 pol do `AccountUpdateEvent`.
- nie route'owac evidence przez canonical `AccountUpdate`.

Acceptance:

```bash
cargo test -p seer ipc -- --nocapture
cargo test -p ghost-launcher --lib seer -- --nocapture
cargo check -p seer
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
```

Testy krytyczne:

- IPC roundtrip dla `ExecutionAccountEvidence`.
- launcher bridge emituje `GhostEvent::ExecutionAccountEvidence`.
- `SeerEvent::AccountUpdate` i `GhostEvent::AccountUpdate` pozostaja canonical reserve update.
- `ExecutionAccountEvidence` nie przechodzi przez `AccountUpdateEvent`.

### X8C-PR3 - Seer producers for BCV2 evidence

Cel:

Seer ma emitowac typed BCV2 evidence z istniejacych miejsc, ktore dzis tylko loguja markery.

Pliki:

- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/ipc.rs`

#### 6.3.1 Observed TX route-compatible BCV2

Aktualna funkcja:

- `binary_parser.rs::register_route_compatible_observed_bcv2(...)`

Zmiana:

- po pozytywnym provenance `route_compatible` wyslac `ExecutionAccountEvidence`.
- zrodlo: `ObservedTxMeta`.
- status: `DiscoveryHint`.
- `evidence_ready=false`.
- `account_pubkey=trade.bonding_curve_v2`.
- `base_mint=Some(trade.mint)`.
- `pool_id=Some(trade.pool_amm_id)`.
- `tx_signature`, `observed_instruction_index`, `observed_account_position`, `provenance_status` z `ObservedAccountMetaProvenance`.

Zakaz:

- observed tx meta nie odblokowuje readiness.
- non-route-compatible provenance nie emituje evidence.

#### 6.3.2 Exact-watch registered / subscribe included

Aktualne miejsca:

- `AccountRegistry::insert_bcv2()`
- `register_route_compatible_observed_bcv2()`
- `build_subscribe_request_for_profile()`
- `maybe_send_resubscribe(...)`

Minimalnie:

- `ExactWatchRegistered` event z `register_route_compatible_observed_bcv2()`.
- `SubscribeIncluded` jako typed event tylko jesli mozna podac exact pubkey; counters-only log marker nie jest wystarczajacy jako per-pubkey evidence.

Jesli w PR3 nie da sie bezpiecznie podac pubkey dla `SubscribeIncluded`, zostaje marker audit-only i plan musi zapisac to w raporcie PR3.

#### 6.3.3 RPC hydration

Aktualne miejsce:

- `Bcv2HydrationService`
- `run_bcv2_hydration_worker(...)`
- `record_bcv2_rpc_hydration_evidence(...)`

Zmiana:

- `Bcv2HydrationService` dostaje opcjonalny callback albo `IpcSender`.
- `RpcReady` wysyla:
  - `source=RpcHydration`
  - `status=RpcReady`
  - `evidence_ready=true`
  - `owner=Some(owner)`
  - `data_len=Some(data_len)`
  - `context_slot=Some(context_slot)`
- `RpcMissing` wysyla:
  - `source=RpcHydration`
  - `status=RpcMissing`
  - `evidence_ready=false`
  - `reason=error_class`
  - `context_slot` jesli dostepny

#### 6.3.4 Yellowstone AccountUpdate received

Aktualne miejsca:

- `grpc_connection.rs::route_update(...)` loguje `BCV2_ACCOUNT_UPDATE_RECEIVED`.
- `Seer::handle_account_update(...)` w `off-chain/components/seer/src/lib.rs` dekoduje canonical layout i moze odrzucic BCV2 przed IPC.

Zmiana:

- BCV2 account-update evidence musi byc emitowane przed `decode_canonical_account_update(owner, data)`.
- `Seer::handle_account_update()` powinien sprawdzic:

```rust
let is_bcv2 = self
    .grpc_connection
    .as_ref()
    .map(|conn| conn.account_registry().contains_bcv2(&pubkey.to_string()))
    .unwrap_or(false);
```

- Jesli `is_bcv2`, emitowac:
  - `source=YellowstoneAccountUpdate`
  - `status=AccountUpdateReceived`
  - `evidence_ready=true`
  - `owner`
  - `data_len`
  - `slot`
  - `write_version`
  - `base_mint/pool_id` tylko jesli znane z dodatkowego registry/context. Jesli nieznane, zostaja `None`, a OracleRuntime moze nadal exact-lookup po `(BondingCurveV2, pubkey)`.

Zakaz:

- nie wymuszac canonical decode dla BCV2.
- nie wysylac BCV2 przez `send_account_update(...)`.
- canonical `AccountUpdate` path musi zostac bez zmian po BCV2 evidence emission.

Acceptance:

```bash
cargo check -p seer
cargo test -p seer binary_parser -- --nocapture
cargo test -p seer grpc_connection -- --nocapture
cargo test -p seer ipc -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo fmt --check
git diff --check
```

Testy krytyczne:

- route-compatible observed BCV2 emituje `DiscoveryHint`.
- non-route-compatible BCV2 nie emituje evidence.
- `Bcv2HydrationService` emituje `RpcReady`.
- `Bcv2HydrationService` emituje `RpcMissing`.
- BCV2 `AccountUpdate` emituje `AccountUpdateReceived` nawet gdy canonical decode failuje.
- canonical `DetectedAccountUpdateEvent` nadal powstaje tylko dla canonical reserve update.

### X8C-PR4 - OracleRuntime evidence store and diagnostics

Cel:

`GhostEvent::ExecutionAccountEvidence` trafia do `OracleRuntime` store. Jeszcze nie odblokowuje readiness.

Pliki:

- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-core/src/execution_account_evidence.rs`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`

Zmiany w `OracleRuntime`:

```rust
execution_account_evidence_store: Arc<ExecutionAccountEvidenceStore>
```

Constructor:

- zainicjalizowac `Arc::new(ExecutionAccountEvidenceStore::new())` w `OracleRuntime::new_with_config(...)`.

Event loop:

```rust
GhostEvent::ExecutionAccountEvidence(event) => {
    oracle_runtime.record_execution_account_evidence(event.evidence);
}
```

Helpery:

```rust
pub fn execution_account_evidence_store(&self) -> Arc<ExecutionAccountEvidenceStore>;
pub fn record_execution_account_evidence(&self, evidence: ExecutionAccountEvidence) -> UpsertExecutionAccountEvidenceResult;
pub fn lookup_bcv2_evidence(&self, pubkey: &Pubkey) -> Option<ExecutionAccountEvidenceRecord>;
pub fn bcv2_evidence_status(&self, pubkey: &Pubkey) -> Bcv2EvidenceStatus;
```

Metrics/log:

```text
execution_account_evidence_upsert_total{role,status,source,outcome}
```

Diagnostics added to P3.7 rows:

- `working_builder_bcv2_evidence_status`
- `working_builder_bcv2_evidence_source_counts`
- `working_builder_bcv2_evidence_ready`
- `working_builder_bcv2_evidence_owner`
- `working_builder_bcv2_evidence_data_len`
- `working_builder_bcv2_evidence_slot`
- `working_builder_bcv2_evidence_context_slot`
- `working_builder_bcv2_evidence_reason`
- `working_builder_bcv2_evidence_conflict`

Acceptance:

```bash
cargo check -p ghost-launcher
cargo test -p ghost-launcher --lib execution_account_evidence -- --nocapture
cargo test -p ghost-launcher --lib p37_working_builder -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo fmt --check
git diff --check
```

PASS-B smoke after PR4:

- `working_builder_bcv2_evidence_rows > 0`
- `working_builder_bcv2_evidence_ready_rows > 0` albo jednoznaczne `RpcMissing/DecodeFailed`
- `working_builder_manifest_ready_rows` moze nadal byc `0`
- R18 nadal `NO-GO`

### X8C-PR5 - Session bridge BCV2 key / flush

Cel:

Nie pozwolic, aby evidence/update keyed by BCV2 wisialo w buforze do TTL, gdy pozniejszy `PoolDetected` albo `Trade` zna tylko pool/base_mint/classic curve.

Pliki:

- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`

Nowy key role:

```rust
enum SessionDetectedKeyRole {
    Pool,
    BaseMint,
    BondingCurve,
    BondingCurveV2,
}
```

Minimalna implementacja:

- zastapic `detected_keys: HashMap<Pubkey, Instant>` struktura, ktora zachowuje role w logach i testach.
- dodac mapping pomocniczy:

```rust
bcv2_by_pool: HashMap<Pubkey, HashSet<Pubkey>>
bcv2_by_base_mint: HashMap<Pubkey, HashSet<Pubkey>>
```

Kiedy rejestrowac BCV2 key:

- `TradeEvent.bonding_curve_v2` tylko gdy `bonding_curve_v2_provenance.provenance_status == Some("route_compatible")`.
- `PoolTransaction.bonding_curve_v2` z route-compatible provenance.
- `ExecutionAccountEvidence(role=BondingCurveV2, account_pubkey=...)`.

Flush:

- `register_detected_pool(candidate)` flushuje `pool_amm_id`, `bonding_curve`, `base_mint` oraz znane BCV2 powiazane przez `pool_id/base_mint`.
- evidence keyed by BCV2 moze zostac forwardowane, ale nie moze zostac skonwertowane na canonical `AccountUpdate`.

Markery:

- `BCV2_SESSION_KEY_REGISTERED`
- `BCV2_SESSION_PENDING_FLUSHED`
- `BCV2_SESSION_PENDING_EXPIRED`

Acceptance:

```bash
cargo test -p ghost-launcher --lib seer -- --nocapture
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
```

Testy krytyczne:

- evidence keyed by BCV2 przed `PoolDetected` jest buforowane.
- `PoolDetected` z tym samym `base_mint` i znanym BCV2 flushuje pending evidence.
- `refresh_from_trade` rejestruje BCV2 tylko dla route-compatible provenance.
- non-route-compatible BCV2 nie tworzy session key.
- canonical `AccountUpdate` forward semantics pozostaja bez zmian.

### X8C-PR6 - Working-builder evidence-gated readiness

Cel:

Dopiero teraz working-builder readiness korzysta z `ExecutionAccountEvidenceStore`.

Pliki:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/components/trigger/component.rs` tylko jesli potrzebny test manifest roles; nie zmieniac builder behavior.
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`

Config:

Dodac do `P37ShadowProbeConfig`:

```rust
#[serde(default = "default_p37_execution_account_evidence_freshness_ms")]
pub execution_account_evidence_freshness_ms: u64,
```

Domyslnie np. `10_000`. Walidacja: jesli `p37_shadow_probe.enabled=true`, wartosc musi byc `> 0`.

Minimalna polityka execution-load-ready:

```text
role == BondingCurveV2
account_pubkey == working_builder_bcv2_pubkey
status in {RpcReady, PrecheckReady}
owner present
data_len present
not contradicted by newer RpcMissing/PrecheckMissing
not stale relative to execution_account_evidence_freshness_ms
```

`AccountUpdateReceived` w X8C:

- moze ustawic `evidence_seen=true`.
- moze ustawic `local_coverage/evidence_status`.
- nie ustawia samodzielnie `execution_load_ready=true`.

Nowe pola audytowe:

- `working_builder_bcv2_execution_evidence_status_counts`
- `working_builder_bcv2_execution_evidence_source_counts`
- `working_builder_bcv2_execution_evidence_ready_rows`
- `working_builder_bcv2_execution_evidence_conflict_rows`
- `working_builder_bcv2_execution_evidence_stale_rows`
- `working_builder_bcv2_execution_evidence_exact_pubkey_match_rows`

Zmiana w skrypcie:

- `working_builder_manifest_ready_rows` nie moze juz bazowac na `working_builder_bcv2_account_state_materialized`, `mfs_materialized` albo `diag_materialized` jako gotowosci BCV2.
- Gotowosc BCV2 dla manifest-ready ma bazowac na `working_builder_bcv2_execution_evidence_ready == true`.

Acceptance:

```bash
cargo check -p ghost-launcher
cargo test -p ghost-launcher --lib p37_working_builder -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
cargo fmt --check
git diff --check
```

PASS-A smoke:

- `working_builder_bcv2_execution_evidence_ready_rows > 0`
- `working_builder_manifest_ready_rows > 0`
- `successful_probe_entry_rows > 0` albo `active_shadow_successful_entry_rows > 0`
- legacy/fallback/handoff/live invariants clean
- `post_simulation_account_not_found_rows = 0`

PASS-B smoke:

- role-aware evidence path dziala,
- readiness nadal fail-closed z konkretna przyczyna:
  - `RpcMissing`
  - `PrecheckMissing`
  - `stale`
  - `conflict`
  - `provider_timeout`
  - `decode_failed`

FAIL:

- observed tx meta odblokowuje readiness.
- BCV2 jest traktowane jako classic `bonding_curve`.
- legacy/fallback/handoff wraca.
- live Sender albo submitted path pojawia sie bez jawnego scope.
- `post_simulation AccountNotFound` wraca po manifest-ready.

### X8C-PR7 - Provider/transport hardening diagnostics

Cel:

X8AS/X8B mial watchdog caveat. Nie wolno robic twardych timing/true-missing wnioskow bez rozdzielenia provider stall od naszego request churn/watchdog.

Pliki:

- `off-chain/components/seer/src/grpc_connection.rs`
- `ghost-launcher/src/config.rs`
- `configs/rollout/*` tylko nowe, allowlistowane X8C smoke configi.
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`

Zakres:

- rozdzielic stall global stream vs funding lane przez `source_label`.
- liczyc stall wzgledem ostatniego tx/account/entry osobno.
- dodac `source_label` do X8C report.
- logowac BCV2 evidence przed reconnect i po reconnect.
- dodac retry/reconnect diagnostics bez zmiany execution policy.

Acceptance:

```bash
cargo check -p seer
cargo test -p seer grpc_connection -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo fmt --check
git diff --check
```

Smoke PASS:

- 30-min smoke bez fatal watchdog; albo
- watchdog failure z jednoznaczna klasyfikacja `provider_stall/source_label/message_family`.

### X8C-PR8 - Clean executable denominator gate

Cel:

Wrocic do P3.7 executable lifecycle denominator dopiero po evidence-gated PASS-A.

Warunki wejscia do R18:

- `working_builder_manifest_ready_rows > 0`
- `working_builder_bcv2_execution_evidence_ready_rows > 0`
- `successful_probe_entry_rows > 0` lub `active_shadow_successful_entry_rows > 0`
- `lifecycle_eligible_rows > 0`
- `legacy_buy_route_attempted_rows = 0`
- `selected_route_handoff_mismatch_rows = 0`
- `post_simulation_account_not_found_rows = 0`
- `send_transaction/SUBMITTED/live Sender = 0`
- BCV2 readiness oparta o `ExecutionAccountEvidenceStore`, nie observed tx meta

Jesli dowolny warunek nie jest spelniony:

```text
R18 = NO-GO
```

## 7. Kolejnosc realizacji

Rekomendowana kolejnosc:

1. X8C-PR1: `ghost-core` model/store.
2. X8C-PR2: IPC/event schema.
3. X8C-PR3: Seer producers dla observed/RPC/AccountUpdate BCV2.
4. X8C-PR4: OracleRuntime evidence store + diagnostics, bez readiness unlock.
5. X8C-PR5: session bridge BCV2 key/flush.
6. X8C-PR6: working-builder evidence-gated readiness.
7. X8C-PR7: transport/watchdog diagnostics.
8. X8C-PR8: R18 gate only after PASS-A.

Mozna polaczyc X8C-PR1 + X8C-PR2 w jeden pierwszy commit tylko jesli diff pozostaje maly i testy sa osobno czytelne. Nie laczyc X8C-PR3 z X8C-PR6, bo to grozi ukrytym readiness unlock bez runtime proofu.

## 8. Pierwsze zadanie implementacyjne

Pierwsze zadanie powinno brzmiec:

```text
P3.7-X8C-PR1 - Add role-aware ExecutionAccountEvidence model and IPC/event schema.
```

Scope pierwszego zadania:

- `ghost-core/src/execution_account_evidence.rs`
- `ghost-core/src/lib.rs`
- `off-chain/components/seer/src/ipc.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`
- minimalne testy IPC/store/event bridge

Out of scope pierwszego zadania:

- Seer producers.
- `Bcv2HydrationService` callback.
- `Seer::handle_account_update()` BCV2 emission.
- OracleRuntime store.
- session bridge BCV2 flush.
- working-builder readiness.
- smoke/runtime/R18.

## 9. Kontrakty, ktore trzeba zachowac w kazdym PR

- `MaterializedFeatureSet` pozostaje canonical decision snapshot.
- `AccountStateCore` pozostaje canonical pool-state keyed by `base_mint`.
- `PoolIdentityRegistry` nie staje sie role-aware execution-account registry w PR1-PR6.
- `AccountUpdateEvent` pozostaje canonical reserve update.
- observed tx meta jest discovery/provenance hint, nie load-ready.
- `ExecutionAccountEvidenceStore` jest addytywny i inert do PR6.
- `legacy_buy` pozostaje diagnostic-only / unsupported fallback, zgodnie z E5B/E6.
- shadow/live boundary pozostaje nienaruszona.
- kazdy readiness unlock musi byc fail-closed i audytowalny.

## 10. Delegation trace dla tego planu

```yaml
delegation_trace:
  task_classification: "cross_component_repair_plan"
  routing_performed: true
  primary_specialist: "ghost-runtime-coordinator"
  supporting_specialists_considered:
    - "seer-ingest-event-integrity-specialist"
    - "oracle-session-runtime-engineer"
    - "solana-execution-path-engineer"
    - "decision-logging-replay-analyst"
    - "config-rollout-safety-reviewer"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/seer-ingest-event-integrity-specialist.md"
    - "docs/agents/oracle-session-runtime-engineer.md"
    - "docs/agents/solana-execution-path-engineer.md"
  specialist_docs_not_loaded:
    - name: "decision-logging-replay-analyst"
      reason: "Plan references audit fields/scripts but does not change DecisionLogger JSONL schema directly."
    - name: "config-rollout-safety-reviewer"
      reason: "Config impact is limited to a future P37ShadowProbeConfig freshness field with serde default; no config implementation in this plan."
    - name: "gatekeeper-policy-auditor"
      reason: "Gatekeeper/scoring policy is explicit non-goal."
  skills_used:
    - "ghost-execution"
    - "solana-pumpfun-architect"
  fast_path_used: false
  contracts_checked:
    - "SSOT / MaterializedFeatureSet"
    - "AccountStateCore canonical base_mint ownership"
    - "Seer IPC event semantics"
    - "GhostEvent bus semantics"
    - "OracleRuntime event dispatch"
    - "session bridge buffering/flush"
    - "working-builder manifest readiness"
    - "shadow/live separation"
    - "legacy_buy unsupported/fallback boundary"
  unresolved_routing_uncertainty: []
```
