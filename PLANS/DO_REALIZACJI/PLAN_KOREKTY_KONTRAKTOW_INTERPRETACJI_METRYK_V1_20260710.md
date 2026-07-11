# Plan wykonawczy V1.1: korekta kontraktów interpretacyjnych metryk Ghost

Status:

```text
PLAN_V1_1_ACCEPTED
PR0_BASELINE_RECONCILIATION_ALLOWED
RUNTIME_IMPLEMENTATION_BLOCKED_UNTIL_PR0_PASS
```

Wersja kontraktu: `metric_contracts_v1_1`

Data źródłowego audytu: 2026-07-10

Data rewizji V1.1: 2026-07-11

Dokument źródłowy:
`PLANS/AUDYT/RAPORT_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`

Historia: treść V1 została zastąpiona w tym samym pliku na życzenie właściciela
planu. ADR V1 pozostaje historycznym śladem pierwszej wersji, a osobny ADR V1.1
dokumentuje niniejszą rewizję.

Zakres: dokładnie 10 rodzin metryk wskazanych w audycie.

Normatywna lista wejściowa — bez rozszerzania zakresu:

| # | Legacy/public surface z audytu | Klasyfikacja V1.1 |
| ---: | --- | --- |
| 1 | `fee_topology_diversity_index` | runtime value + oddzielna legacy/V2 actionability |
| 2 | `dev_buy_total_sol` | surface-qualified first-observed kontra primary buy |
| 3 | `same_ms_tx_ratio` | exact, cluster i recent jako osobne kontrakty |
| 4 | `top3_volume_pct` | ratio-scale compatibility alias istniejącego preferred field |
| 5 | `flip_ratio_10s` | zamrożony legacy contract; osobny evidence-only flip V2 |
| 6 | `funding_source_concentration` | legacy collision/compression ratio; FSC v2 osobno |
| 7 | `evidence_status.fsc` | compatibility legacy alias; osobne `fsc_legacy` i `fsc_v2` |
| 8 | `ManipulationContradictionFeatures.high_*` | legacy default bools; policy-derived flags z provenance |
| 9 | `reserve_velocity_sol_per_sec` | legacy scalar + typed interval/validity evidence |
| 10 | `buy_sell_ratio_recent` | legacy logging scalar + counts/optional/bounded ratios |

Lista nie obejmuje jedenastej metryki ani nie daje zgody na rozszerzenie policy.

## 1. Podsumowanie rewizji i granice

### 1.1 Strategia

V1.1 zachowuje kierunek:

```text
kontrakt
→ canonical producer
→ MaterializedFeatureSet
→ durable evidence
→ replay/comparator
→ burn-in
→ wyłącznie równoważny cutover
```

Wprowadza siedem obowiązkowych uzupełnień:

1. Baseline reconciliation względem aktualnego target branch i merge-base.
2. Globalny rollout mode jako ceiling oraz osobny, wersjonowany authority profile.
3. Surface-qualified kontrakt dev-buy.
4. Normatywny automat stanów flip V2.
5. Canonical envelope availability/quality/actionability i adaptery legacy.
6. Compact decision v34 oraz pełny evidence sidecar z budżetem zasobów.
7. Wielorunowy burn-in bundle i zamrożenie minimów przed zebraniem danych
   walidacyjnych.

Plan ma trzy główne milestones, ale sześć rzeczywistych PR-ów:

```text
PR0 → PR1 → PR2A → PR2B → PR2C → BURN-IN → PR3
```

To nie jest podział per metryka. PR-y są grupowane według ownership, ryzyka,
reviewability i niezależnego rollbacku.

### 1.2 Potwierdzony stan checkoutu przed V1.1

PR0 musi ponownie potwierdzić te fakty na docelowym merge-base. Obecny
reconnaissance wykazał:

- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 33`;
- `TxIntelFeatures::effective_top3_signer_volume_ratio()` już istnieje;
- TxIntelligence ustawia preferred `top3_signer_volume_ratio` i legacy alias
  `top3_volume_pct`;
- aktywne Gatekeeper callsite'y już używają top3 effective helpera;
- TxIntelligence ustawia dev buy z `stats.first_buy_volume_sol`;
- `GatekeeperBuffer` ma oddzielną create-signature-anchored primary creator buy
  semantics;
- aktywny Phase 5 używa MFS/TxIntel, nie primary-buy GatekeeperBuffer:

```text
evaluate_feature_driven_terminal_verdict
→ GatekeeperBuffer::evaluate_from_features
→ build_assessment_from_features
→ dev_behavior_from_features
→ MaterializedFeatureSet.tx_intel_features.dev_buy_sol
→ min_dev_buy_sol / max_dev_buy_sol
```

- FTDI liczy wartość po unikalnych buyerach, ale clean/degraded opiera na
  całkowitym `buy_sample_count`; dwa unikalne buyery w co najmniej trzech BUY-ach
  mogą uzyskać zbyt optymistyczną legacy actionability;
- repo ma równoległe `EvidenceStatus`, `MetricEvidenceQuality`,
  `FscEvidenceStatus`, `FeatureEvidenceStatus` i stringowe degraded reasons;
- `ManipulationContradictionFeatures` ma `high_*: false` oraz raw numeric
  `f64: 0.0`, więc typed deserialization nie odróżnia zawsze missing od real zero;
- próbka istniejącego v33 runa miała około 102 KB p50, 123 KB p95 i 138 KB p99
  na decision record;
- reconnaissance 8 h miał 8316 decision rows i 8315 dev-known, lecz nie dowodzi
  wykonalności flip V2 ani dev legacy/V2 divergence.

Te liczby są reconnaissance, nie acceptance evidence. PR0 ma je odtworzyć z
SHA wejść.

### 1.3 Nienegocjowalne kontrakty

- `MaterializedFeatureSet` pozostaje jedynym canonical decision snapshot.
- `PoolObservationSession::materialize_features()` pozostaje granicą mutable
  state → immutable evidence.
- Policy i DecisionLogger nie mogą ponownie liczyć authoritative cech z live
  state.
- GatekeeperBuffer nie staje się drugim SSOT.
- Nie zmieniamy thresholdów, wag, phase order, verdict taxonomy ani reason codes.
- Nie promujemy FSC v2, flip V2, reserve velocity ani RCE do aktywnej polityki.
- Nie zmieniamy selector score; top3 otrzymuje wyłącznie audit/guard/parity.
- Nie zmieniamy IWIM, post-buy, sendera, builderów, submit/confirmation ani
  shadow/live boundary.
- Nie przywracamy HyperPrediction, Chaos ani starego scoring path.
- Semantycznie nierównoważna metryka nie może wejść do PR3 tylko dlatego, że na
  burn-inie przypadkiem nie zmieniła verdictu.
- Każdy PR otrzymuje ADR-8D, allowlist staging i osobny scope audit.
- Zakazane jest `git add .`.

## 2. Rollout, authority i canonical evidence

### 2.1 Rollout mode jako ceiling

```rust
pub enum MetricContractRolloutMode {
    Legacy,
    DualCompute,
    V2,
}
```

```toml
metric_contract_rollout_mode = "legacy" | "dual_compute" | "v2"
metric_contract_profile = "metric_contracts_v1_1_profile_a"
```

| Tryb | Zachowanie |
| --- | --- |
| `Legacy` | Obecne authoritative surfaces; candidate policy comparator wyłączony. |
| `DualCompute` | Obecne surfaces pozostają terminalne; candidates są read-only. |
| `V2` | Aktywowane mogą być tylko profile entries oznaczone `EquivalentCutover`. |

`V2` nie oznacza, że wszystkie metryki V2 są policy-authoritative.

TOML wybiera znany, skompilowany profil; nie zawiera luźnej per-field authority
matrix. Wymagania:

- serde default mode: `Legacy`;
- serde default profile: `metric_contracts_v1_1_profile_a`;
- unknown mode/profile: startup failure;
- canonical JSON profilu jest hashowany SHA-256;
- mode/profile ID/hash są w decision summary, sidecarze i replayu;
- hash mismatch jest błędem schema/replay;
- nowy profil wymaga nowego ID, testów i ADR.

### 2.2 Authority classes

```rust
pub enum MetricAuthorityClass {
    Authoritative,
    EquivalentCutover,
    Compatibility,
    Counterfactual,
    EvidenceOnly,
    LoggingOnly,
    ExportOnly,
}
```

- `Authoritative`: obecny terminalny source.
- `EquivalentCutover`: formalnie równoważna nowa reprezentacja.
- `Compatibility`: alias/historyczny replay surface.
- `Counterfactual`: semantycznie inna cecha bez promocji w tym planie.
- `EvidenceOnly`: typed runtime evidence bez policy consumer.
- `LoggingOnly`: offline/logging evidence.
- `ExportOnly`: osobny research sidecar, `score_eligible=false`.

### 2.3 Profile A — normatywna matrix

| Kontrakt | Obecny authority | Candidate V1.1 | Profile A w `V2` | Promocja |
| --- | --- | --- | --- | --- |
| FTDI value | unique-topology ratio | typed, ta sama formuła | `EquivalentCutover` | Tak, tylko wartość |
| FTDI actionability | legacy buy-tx-count gate | unique-buyer gate | legacy authority; V2 counterfactual | Nie |
| Dev buy | MFS TxIntel first-observed | MFS primary creator buy | first-observed authority; primary counterfactual | Nie |
| Same-ms exact | MFS `delta == 0` | typed exact V2 | `EquivalentCutover` | Tak |
| Same-ms `<50 ms` | compat/helper | jawnie nazwany cluster | `EvidenceOnly` | Nie |
| Top3 | preferred ratio + fallback | istniejący helper | już `Authoritative` | Audit/guard |
| Flip V2 | brak V2 authority | deterministic hybrid | `EvidenceOnly` | Nie |
| Legacy FSC | collision/compression ratio | typed legacy | `EquivalentCutover` | Tak |
| FSC v2 | shadow/export | typed readiness | `EvidenceOnly` | Nie |
| `evidence_status.fsc` | legacy scalar presence | `fsc_legacy` + `fsc_v2` | legacy alias compatibility | Nie |
| Manipulation numeric | raw numeric/default zero | presence-aware numeric | `EquivalentCutover` po formalnym proofie | Warunkowo |
| Manipulation `high_*` | default false + numeric OR | policy-derived flags | `EquivalentCutover` po truth-table proofie | Warunkowo |
| Reserve velocity | scalar evidence | interval/status | `EvidenceOnly` | Nie |
| Recent buy/sell | legacy logging scalar | counts + ratios | `LoggingOnly` | Nie |
| Coordination FTDI/FSC | HHI sidecar | typed export | `ExportOnly` | Nie |

Dev-primary i poprawiona FTDI actionability są jawnie wyłączone z PR3.

### 2.4 Canonical status envelope

```rust
pub struct MetricEvidenceEnvelopeV1<R> {
    pub contract_id: MetricContractId,
    pub contract_version: u16,
    pub surface_id: MetricSurfaceId,
    pub authority_class: MetricAuthorityClass,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub policy_actionable: bool,
    pub reason_codes: Vec<R>,
}
```

```rust
pub enum MetricAvailabilityV1 {
    Available,
    Unavailable,
    NotConfigured,
    NotRecordedLegacySchema,
}

pub enum MetricMeasurementQualityV1 {
    Measured,
    Degraded,
    Insufficient,
    Stale,
    Fallback,
    LegacyDefault,
    NotApplicable,
}
```

Inwarianty:

- unavailable/not-configured/not-recorded zawsze mają `NotApplicable` i
  `policy_actionable=false`;
- available nie oznacza measured;
- measured nie oznacza automatycznie actionable;
- actionable wymaga availability, dozwolonej jakości, authority profile i
  contract-specific sample/readiness gate;
- reason codes są typed per contract;
- legacy string reasons pozostają kompatybilne, ale nie są canonical API.

PR1 tworzy exhaustive mapping matrix:

```text
EvidenceStatus                 → MetricEvidenceEnvelopeV1
MetricEvidenceQuality          → MetricEvidenceEnvelopeV1
FscEvidenceStatus              → MetricEvidenceEnvelopeV1
FeatureEvidenceStatus          → MetricEvidenceEnvelopeV1
legacy string degraded reasons → typed per-contract reasons
```

Istniejące enumy nie są usuwane; otrzymują jawne adaptery i testy.

## 3. Normatywne kontrakty wymagające doprecyzowania

### 3.1 Dev-buy surfaces

| Surface | Semantyka | Rola |
| --- | --- | --- |
| `tx_intel_dev_first_observed_buy_sol` | pierwszy accepted TxIntel buy rozpoznanego deva | legacy producer |
| `gatekeeper_buffer_dev_primary_buy_sol` | create-signature-anchored primary buy | compat/non-terminal |
| `mfs_dev_first_observed_buy_sol` | materialized TxIntel first-observed | aktualny authority |
| `mfs_dev_primary_buy_sol_v1` | successful/deduped/non-dust primary buy | counterfactual |
| `effective_policy_dev_buy_sol` | source wybrany przez profile | Profile A: first-observed |

Istniejące `dev_buy_total_sol`, `dev_buy_sol` i JSON fields pozostają bez zmiany.

Primary selector:

1. Używa canonical creator identity i create signature dostępnych przed
   materializacją.
2. Kandydat musi być BUY, successful, canonical-deduped i non-dust.
3. Preferuje eligible BUY z create signature i najwcześniejszy stabilny TxKey.
4. Bez eligible signature match wybiera najwcześniejszy eligible creator BUY.
5. Brak eligible buy daje `None` + typed reason, nie real zero.
6. Permutacja delivery nie zmienia wyniku przy tych samych order keys.
7. Policy nie czyta GatekeeperBuffer w celu wybrania wartości.
8. Existing GatekeeperBuffer primary behavior pozostaje chronione testem.

Comparator baseline zawsze oznacza aktualny `effective_policy_dev_buy_sol`, a
nie pole wybrane po podobieństwie nazwy.

Evidence zawiera creator-known, create-signature availability/match, selection
mode, selected signature/order key, eligible count, amount, status i reasons.

`dev_volume_ratio` pozostaje gross buy-plus-sell turnover share. Nie oznacza
total buys, holdings, net exposure, supply ownership ani PnL.

### 3.2 FTDI value kontra actionability

Wartość pozostaje:

```text
unique_topology_count / unique_buyer_sample_count
```

PR2A nie zmienia legacy degraded reason konsumowanego przez aktywną politykę.
Równolegle tworzy:

```text
ftdi_value_evidence_v1
ftdi_legacy_actionability
ftdi_unique_buyer_actionability_v2
```

Przypadek 2 unique buyers i co najmniej 3 BUY tx musi dawać osobne wyniki legacy
i V2. Jest to kontrprzykład formalnej równoważności, więc corrected V2
actionability pozostaje counterfactual.

Coordination HHI diversity zachowuje osobną `ExportOnly` surface.

### 3.3 Same-ms i top3

`TxTimingEvidenceV1` rozdziela:

```text
tx_intel_same_ms_collision_ratio_exact
tx_intel_bundle_cluster_ratio_lt_50ms
rce_same_ms_collision_ratio_recent_exact
```

Każdy wariant ma source, population, success/dedupe/dust filters, window,
numerator i denominator. Legacy denominator pozostaje:

```text
adjacent_collision_count / transaction_count
```

Aktywny threshold nadal używa exact `delta == 0`.

Top3:

- nie dodawać helpera ponownie;
- przeprowadzić callsite audit;
- dodać mismatch telemetry dla preferred kontra alias;
- static guard zabrania nowych active reads legacy `top3_volume_pct` poza
  serializerem, adapterem i fixture'ami;
- selector wymaga bit-for-bit parity;
- skala pozostaje 0..1.

### 3.4 Normatywny automat flip V2

Event jest eligible wyłącznie, gdy:

- należy do globalnego pool window `pool_t0 <= ts <= pool_t0 + window_secs`;
- ma `success=true`;
- przeszedł canonical dedupe;
- jest non-dust;
- ma resolved owner i stabilny canonical order key;
- nie cofa czasu ani slotu względem accepted eventów ownera.

Identity: signature; fallback wyłącznie udowodniony unikalny
`slot + transaction_index/event_ordinal`. Brak stabilnej identity/order daje
non-evaluable, nie receive-order guess.

Per-owner state:

```text
anchor_ts
anchor_slot
cumulative_buy_tokens
cumulative_sell_tokens
qualifying_sell_ts
qualifying_sell_slot
status = no_anchor | tracking | flipper | closed_non_flipper
```

Pseudokod:

```text
for event in canonical_order:
    odrzuć duplicate / failed / dust / unresolved / unordered
    jeśli poza globalnym window: pomiń

    state = owner_state[event.owner]

    jeśli SELL bez anchor:
        zwiększ pre_anchor_sell_count
        nie doliczaj do cumulative_sell
        continue

    jeśli BUY bez anchor:
        anchor_ts = event.ts
        anchor_slot = event.slot
        cumulative_buy = amount
        cumulative_sell = 0
        status = tracking
        dodaj ownera raz do denominatora
        continue

    jeśli status jest flipper lub closed_non_flipper:
        continue

    jeśli event.ts < anchor_ts lub event.slot < anchor_slot:
        ordering invalid; cały record non-evaluable
        continue

    jeśli BUY:
        cumulative_buy += amount
        continue

    jeśli SELL:
        cumulative_sell += amount

        within_time = event.ts - anchor_ts <= window_secs
        within_slots = event.slot - anchor_slot <= max_flip_slots
        dump_reached = cumulative_sell >= flip_dump_ratio * cumulative_buy

        jeśli within_time && within_slots && dump_reached:
            status = flipper
            zapisz current sell jako qualifying sell
            zamroź klasyfikację ownera
```

Reguły:

- anchor to pierwszy eligible BUY; brak re-anchor;
- selle przed anchorem nie są retroaktywnie doliczane;
- kolejne BUY-e przed qualifying sell zwiększają cumulative buy i threshold;
- pierwszy qualifying sell zamraża wynik;
- denominator = unique owners z anchor BUY;
- numerator = unique owners z qualifying sell;
- wynik 0..1;
- wallet cap, identity/order/reconnect gap wykluczają Clean/Evaluable;
- parametry V1: 10 s, 0.50, 20 slotów; są config-driven, logowane i hashowane;
- Flip V2 pozostaje evidence-only.

### 3.5 FSC

Legacy FSC pozostaje:

```text
1 - distinct_known_sources / known_source_samples
```

Nie jest HHI, top1 share ani volume concentration.

Materialized evidence otrzymuje `fsc_legacy` i `fsc_v2`. Existing `fsc` pozostaje
legacy compatibility aliasem. `fsc_v2` jest mapowane wyłącznie z decision-time
FSC v2 status/coverage/capture/index/gap/excluded/lane provenance.

`GatekeeperV3EvidenceRequirements.fsc_v2` ma `#[serde(default)] = false`.
Bazowy profil pozostaje `fsc=false`, `fsc_v2=false`. Nie ma FSC v2 policy
promotion.

### 3.6 Presence-aware manipulation evidence

```rust
pub struct MeasuredMetricValueV1<T> {
    pub value: Option<T>,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub reason_codes: Vec<ManipulationEvidenceReasonV1>,
}
```

`ManipulationNumericEvidenceV2` obejmuje same-ms, bundle ratio, top3 ratio, HHI,
max tx per signer, dev volume ratio i contradiction score.

Pełny per-field status trafia do sidecar; compact v34 ma `measured_fields_mask`.
Replay v1 sprawdza raw JSON field presence przed typed deserialization:

- brak pola: `NotRecordedLegacySchema`;
- obecne i odtwarzalne: właściwa jakość;
- obecne bez dowodu pomiaru: `LegacyDefault`;
- explicit zero nie jest automatycznie missing;
- historyczny verdict używa frozen V1 semantics.

Policy-derived `high_*` zapisuje field ID, raw value/status, comparator, threshold,
result, stage, policy version i config hash. Missing raw daje unavailable, nie
`false`.

### 3.7 Reserve velocity i recent buy/sell

Reserve evidence zachowuje legacy f64 i dodaje delta SOL, interval ms,
`source_clock=receive_time`, update counts i status:

```text
measured | first_update | bootstrap_fallback | zero_delta_time
```

Real measured zero wymaga co najmniej dwóch canonical updates, dodatniego
interval i delta zero.

Recent window zachowuje legacy scalar i dodaje raw buy/sell/total counts,
`buy_to_sell_ratio: Option`, bounded `buy_share: Option` i denominator status.
Zero sells daje `buy_to_sell_ratio=None`. Feature pozostaje logging-only.

## 4. Logging, replay i budżet zasobów

### 4.1 Compact v34 plus sidecar

Decision v34 zawiera tylko compact summary:

```text
metric_contract_schema_version
rollout_mode
profile_id/profile_hash
evidence_record_id/evidence_sha256/evidence_schema
authoritative/comparator contract sets
equivalence verdict/reason/phase/soft-point deltas
counterfactual_delta_present
comparator_elapsed_us
metric_contract_serialize_us
measured_fields_mask
```

Pełny payload trafia do `metric_contract_evidence_v1.jsonl`.

Join identity:

```text
(run_id, join_key, decision_plane)
```

Sidecar zawiera wszystkie 10 typed contracts, profile/config/schema versions,
authoritative result, equivalence candidate, semantic counterfactual i payload
SHA-256.

Decision summary i sidecar row są przekazywane jednym logicznym commandem do
bounded queue. Zapis do dwóch plików nie jest udawany jako atomowy; orphan/missing
pair dyskwalifikuje run. Send/writer/ENOSPC/missing-pair mają osobne liczniki.

Wszystkie rotowane parts mają row/byte counts i SHA-256 w manifeście. Replay v2
łączy sidecar po identity/hash. Replay v1 pozostaje zgodny ze starym payloadem.

### 4.2 Resource acceptance

```text
comparator_elapsed_us p99 <= 1_000 us
metric_contract_build_and_serialize_us p99 <= 1_000 us
logger_enqueue_wait_us p99 <= 1_000 us
writer_queue_high_water < 80% capacity
dropped rows = 0
writer failures = 0
orphan summaries/evidence = 0
```

Rozmiar:

```text
compact v34 p95 increase <= 8 KiB względem paired v33
compact v34 p95 increase <= 10% względem paired v33
sidecar p95 <= 24 KiB
sidecar p99 <= 48 KiB
combined GB/hour delta <= 25% względem paired v33 plane
```

Oba limity compact muszą przejść jednocześnie. Przekroczenie daje
`FAIL_RESOURCE_BUDGET`; nie wolno usuwać critical provenance dla sztucznego PASS.

## 5. Sekwencja sześciu PR-ów

### 5.1 PR0 — baseline reconciliation i feasibility preflight

Dokumentacyjny PR bez kodu runtime. Tworzy:

```text
reports/metric_contracts/baseline_reconciliation_v1.md
reports/metric_contracts/historical_feasibility_preflight_v1.md
```

Dla każdego kontraktu raportuje requirement, current definition/producer/MFS,
active i legacy consumers, schema fields, klasyfikację oraz implementation delta.

Dozwolone klasy:

```text
ALREADY_SATISFIED
PARTIALLY_SATISFIED
INCORRECT
MISSING
LEGACY_ONLY
SHADOW_ONLY
EXPORT_ONLY
LOGGING_ONLY
```

PR0 potwierdza dev callsite, top3 helper/readers, FTDI mismatch, status enums,
FSC boundaries, schema versions, logger/rotation i dostępne historyczne runy.
Liczy v33 bytes/record, GB/hour, duration, decisions i dev-known. Ocenia, czy raw
payload pozwala później odtworzyć primary dev i flip V2.

Brak danych daje `NOT_MEASURABLE_PRE_IMPLEMENTATION`, nie zgadywanie.

Acceptance:

```text
BASELINE_RECONCILIATION_PASS
```

PR1 jest zablokowany do PASS.

### 5.2 PR1 — registry/profile/status foundation

- normatywny `METRIC_CONTRACTS_V1_1`;
- rollout/profile/authority types i hash;
- canonical status envelope i typed reasons;
- exhaustive legacy adapters;
- serde-compatible shared evidence structs;
- old TOML jako Legacy + Profile A;
- top3 callsite audit/static guard;
- schema compatibility types bez aktywacji v34/dual compute.

Acceptance:

```text
METRIC_CONTRACTS_V1_1_FOUNDATION_LEGACY_PARITY
```

Old TOML/v33/V3 v1, Gatekeeper verdict/reason/phase/soft points i selector output
muszą pozostać identyczne.

### 5.3 PR2A — active/parity-sensitive producers

- FTDI typed value + oddzielne legacy/V2 actionability;
- surface-qualified dev first-observed i primary evidence;
- MFS dev-primary counterfactual;
- same-ms exact/cluster/recent typed evidence;
- top3 mismatch telemetry;
- typed legacy FSC;
- `fsc_legacy`, `fsc_v2`, compatibility `fsc`;
- `fsc_v2` requirement default false.

Legacy terminal behavior pozostaje niezmienione. Dev-primary i corrected FTDI
actionability nie wpływają na Phase 5/Sybil policy.

Acceptance:

```text
METRIC_CONTRACTS_V1_1_PARITY_SENSITIVE_PRODUCERS_READY
```

### 5.4 PR2B — evidence-only producers

- flip V2 state machine;
- reserve velocity evidence;
- recent buy/sell evidence;
- manipulation numeric presence i derived flags;
- bounded state/dedupe/order/reconnect diagnostics.

Acceptance:

```text
METRIC_CONTRACTS_V1_1_EVIDENCE_PRODUCERS_READY
```

Wymagane: zero Gatekeeper V2 influence, frozen V3 v1 replay, deterministic V3 v2
candidate, no live reads, no unbounded state, no saturating order concealment.

### 5.5 PR2C — v34, sidecar, comparator, replay i audit CLI

- compact v34 i pełny sidecar;
- paired writer i health counters;
- v1/v2 replay;
- equivalence i semantic-counterfactual comparators;
- single-run i bundle audit CLI;
- manifests/rotation/SHA/resource telemetry;
- historical feasibility po istnieniu referencyjnych V2 producerów;
- zamrożenie `BURN_IN_CONTRACT_V1` przed prospektywnymi runami.

Comparator używa tego samego frozen MFS/config. Nie czyta live state, nie trzyma
locka przez await, nie emituje drugiego terminal eventu, nie uruchamia IWIM ani
execution i nie zmienia authority.

Acceptance:

```text
METRIC_CONTRACTS_V1_1_DUAL_COMPUTE_READY_FOR_PROSPECTIVE_BURN_IN
```

### 5.6 Burn-in bundle

Burn-in startuje dopiero po zamrożeniu contractu. Niezmienna struktura:

- co najmniej 3 immutable, niepokrywające się runy;
- każdy run minimum 1 h;
- co najmniej dwa `utc_4h_bucket = floor(run_start_ms / 14_400_000)`;
- ten sam build commit, profile ID/hash, metric schema i Gatekeeper config hash;
- każdy run osobno przechodzi full replay/schema/hash/resource gates;
- minima agregowane dopiero po per-run PASS;
- duplicate join key w runie lub bundle dyskwalifikuje bundle;
- feasibility data nie wchodzą do validation counts.

### 5.7 PR3 — equivalence-only cutover

PR3 może aktywować tylko entries z `EquivalentCutover` i:

```text
formal_equivalence_proof_id
runtime_replay_parity_proof_id
burn_in_bundle_id
```

Dozwolone: typed FTDI value bez zmiany actionability, same-ms exact, existing
preferred top3, typed legacy FSC oraz manipulation V2 tylko po formalnym
truth-table proofie.

Niedozwolone: corrected FTDI actionability, dev-primary, `<50 ms`, flip V2,
FSC v2, reserve, RCE i coordination-risk.

Rollback: `metric_contract_rollout_mode = "dual_compute"`.

Acceptance:

```text
METRIC_CONTRACTS_V1_1_EQUIVALENCE_ONLY_CUTOVER_COMPLETE
```

## 6. Feasibility i anty-post-hoc gate

Początkowa hipoteza minimów:

```text
8 h aggregate duration
700 unique decisions
100 dev-known
100 clean flip-v2 evaluable
30 real dev legacy/v2 feature divergences
```

Po PR2B/PR2C historyczny feasibility audit może je kontrolowanie potwierdzić,
podnieść, obniżyć lub rozłożyć na per-run/aggregate minima — wyłącznie przed
zebraniem danych walidacyjnych.

Procedura:

1. Historical dataset otrzymuje `FEASIBILITY_ONLY` i manifest SHA.
2. Audit generuje exact minima oraz uzasadnienie.
3. Właściciel planu jawnie zatwierdza `BURN_IN_CONTRACT_V1`.
4. Contract otrzymuje version/hash/`frozen_at`.
5. Do bundle kwalifikują się tylko decyzje po `frozen_at`.
6. Feasibility rows nigdy nie zwiększają validation counts.

Zakazane jest obniżenie minimum po zobaczeniu niekorzystnego validation result.
Zmiana gate po starcie validation daje:

```text
old bundle = INVALIDATED_BY_GATE_CHANGE
burn_in_contract_version += 1
new hash and frozen_at
collect entirely new prospective runs
```

Implementator nie może sam zatwierdzić minimów.

## 7. Replay/comparator acceptance

Każdy run wymaga wszystkich terminal rows i rotowanych parts, 100% full replay,
zero malformed/truncated/duplicate/missing identity, zero schema/deser/MFS/config/
profile hash mismatch, zero runtime-replay mismatch, zero candidate error oraz
komplet summary-sidecar pairs.

Equivalence lane wymaga exact zero drift:

```text
verdict
primary reason code
stable reason chain identity
phase pass vector
soft points
selector soft score
hard-fail classification
```

Dowolna delta daje `FAIL_POLICY_DRIFT`.

Semantic counterfactual lane obejmuje dev-primary i corrected FTDI actionability.
Feature/policy delta jest dopuszczalną obserwacją, ale wymaga deterministic replay,
pełnego provenance i zera wpływu na authoritative verdict. `30 dev divergences`,
jeśli pozostanie po feasibility, jest coverage minimum, nie proof equivalence.

Terminalne klasy:

```text
PASS_CUTOVER_READY
NOT_EVALUABLE
FAIL_SCHEMA_OR_REPLAY
FAIL_POLICY_DRIFT
FAIL_RESOURCE_BUDGET
```

PASS zawsze ma:

```text
cutover_scope = metric_contracts_v1_1_profile_a_equivalence_only
```

`COUNTERFACTUAL_POLICY_DELTA_OBSERVED` jest diagnostyką, nie FAIL, jeśli authority
i equivalence lane pozostają niezmienione.

## 8. Macierz testów

### Profile/status/config

- old TOML; unknown mode/profile; deterministic profile hash;
- każda authority entry zmienia hash;
- exhaustive status adapters i invalid combinations;
- v33/V3 v1 nie dostają optymistycznego Measured.

### FTDI/dev

- 2 buyers/2 buys i 2 buyers/3+ buys;
- missing topology i coordination export-only;
- aktywny dev min/max callsite guard;
- GatekeeperBuffer primary regression;
- create-signature/fallback/failed/dust/duplicate/permutation/no-buy;
- counterfactual dev nie zmienia Phase 5.

### Same-ms/top3

- delty 0/1/49/50 ms i denominator tx count;
- active exact semantics;
- top3 one-helper guard, preferred/alias mismatch i selector parity.

### Flip

- `buy1 → buy2 → sell1 → buy3 → sell2`;
- sell before anchor, multiple buys, freeze, no re-anchor;
- exact 10 s/20 slots i 21 slots;
- global/owner window, duplicate, fallback/missing identity;
- reconnect burst, wallet cap, out-of-order timestamp/slot;
- property `0 <= ratio <= 1` i bounded cleanup.

### FSC/manipulation

- neutral-only, low coverage, cold index, not-ready, unavailable, gap, clean;
- legacy alias nie ustawia V2 clean;
- absent kontra explicit zero, LegacyDefault kontra Measured;
- threshold boundary, stage/config hash, v1 frozen replay, v2 parity i truth table.

### Reserve/RCE

- first, measured nonzero, measured zero, zero-delta-time, fallback;
- 6/0, 1/1, 0/0, failed exclusion i reconstruction legacy scalar.

### Logger/replay/bundle

- v33 read/v34 round-trip;
- hash mismatch, missing pair, orphan, truncated line, rotated parts;
- mixed profile/config/run, queue pressure, ENOSPC/writer disable;
- per-run PASS before aggregate, cross-run duplicate;
- mniej niż 3 runs/2 buckets;
- row przed `frozen_at` i gate-change invalidation;
- equivalence drift failuje, counterfactual drift nie zmienia authority.

Minimalne checks obejmują `cargo fmt --check`, targeted checks/tests dotkniętych
crate'ów, serde/replay fixtures, audit CLI unittesty i `git diff --check`.
Filtr uruchamiający 0 testów nie jest PASS.

## 9. Scope audit każdego PR

Potwierdzić:

- brak threshold/weight/phase/reason zmian;
- brak selector change poza top3 parity/guard;
- brak flip/FSC v2/coordination/RCE/reserve promotion;
- brak IWIM/post-buy/sender/live change;
- brak live-state policy read lub MFS bypass;
- brak legacy revival;
- brak destructive schema/TOML changes;
- nowe config fields mają safe defaults;
- typed verdict/reason pozostają;
- staging obejmuje tylko allowlistę;
- unrelated user changes pozostają nietknięte.

## 10. Definition of Done i routing

Plan jest wykonany dopiero, gdy:

1. PR0–PR3 wykonano kolejno.
2. Baseline odróżnia już gotowe, częściowe, błędne i brakujące elementy.
3. Profile A rozdziela authority/equivalence/counterfactual/evidence/log/export.
4. Każdy z 10 kontraktów ma envelope albo jawny legacy adapter.
5. Dev surfaces nie kolidują authority.
6. Flip implementuje dokładny automat.
7. Manipulation missing nie udaje zero.
8. v34 jest compact, pełne evidence jest hashowanym sidecarem.
9. Resource budgets przechodzą.
10. Minima zamrożono przed prospective validation.
11. Bundle ma minimum 3 pełne runy i 2 UTC buckets.
12. Equivalence lane ma zero drift.
13. Counterfactual lane jest deterministyczna i decision-neutral.
14. PR3 obejmuje tylko formalnie równoważne entries.
15. Dev-primary, corrected FTDI actionability, flip V2 i FSC v2 pozostają poza
    active policy.
16. Old TOML/v33/V3 v1 są replayable.
17. Rollback do DualCompute jest przetestowany.
18. Final scope audit nie wykazuje zmian poza planem.

```yaml
task_classification: cross-cutting metric-contract architecture and guarded rollout
primary_specialist: Ghost Runtime Coordinator
supporting_specialists:
  - SSOT Feature Materialization Guardian
  - Gatekeeper Policy Auditor
  - Decision Logging Replay Analyst
  - Config Rollout Safety Reviewer
  - Seer Ingest Event Integrity Specialist
  - Statistical Research Engine
skills_used:
  - ghost-execution
  - abstract-reasoning
  - statistical-research-engine
active_or_legacy_path: mixed active MFS, compat GatekeeperBuffer, V3 shadow, export-only and logging-only
contracts_at_risk:
  - MaterializedFeatureSet SSOT
  - deterministic terminal decisions
  - schema/config compatibility
  - event identity and ordering
  - replay completeness
  - logger backpressure and artifact integrity
  - prospective validation discipline
  - shadow/live and active/legacy separation
risk_level: high
```

```yaml
delegation_trace:
  task_classification: cross-cutting architecture and rollout plan revision
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - SSOT Feature Materialization Guardian
    - Gatekeeper Policy Auditor
    - Decision Logging Replay Analyst
    - Config Rollout Safety Reviewer
    - Seer Ingest Event Integrity Specialist
    - Rust Hotpath Concurrency Reviewer
    - Statistical Research Engine
  specialist_docs_loaded:
    - docs/agents/ghost-runtime-coordinator.md
    - docs/agents/ssot-feature-materialization-guardian.md
    - docs/agents/gatekeeper-policy-auditor.md
    - docs/agents/decision-logging-replay-analyst.md
    - docs/agents/config-rollout-safety-reviewer.md
    - docs/agents/seer-ingest-event-integrity-specialist.md
  specialist_docs_not_loaded:
    - name: Oracle Session Runtime Engineer
      reason: session scheduling, deadlines and routing remain unchanged
    - name: Solana Execution Path Engineer
      reason: transaction construction, submit, confirmation and live execution are out of scope
    - name: Rust Hotpath Concurrency Reviewer
      reason: mandatory implementation review is assigned to PR2B and PR2C
  skills_used:
    - ghost-execution
    - abstract-reasoning
    - statistical-research-engine
  fast_path_used: false
  contracts_checked:
    - MaterializedFeatureSet SSOT
    - producer ownership and surface qualification
    - active versus compat versus counterfactual authority
    - deterministic policy and typed verdict preservation
    - config serde defaults and rollback
    - decision schema and replay compatibility
    - event identity, ordering and duplicate handling
    - evidence writer backpressure and artifact integrity
    - prospective validation and anti-post-hoc discipline
    - shadow/live and legacy separation
  unresolved_routing_uncertainty: []
```
