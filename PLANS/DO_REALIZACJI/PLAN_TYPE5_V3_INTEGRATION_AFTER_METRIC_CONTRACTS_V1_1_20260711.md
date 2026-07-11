# Plan wykonawczy: integracja Type-5 z Metric Contracts V1.1 i istniejącym Gatekeeperem V3

Status:

```text
TYPE5_T0_ARCHITECTURE_PASS
TYPE5_T0_SEMANTIC_RECONCILIATION_PASS
EXACT_BINDINGS_REVIEWABLE
NO_DUPLICATE_METRIC_PRODUCERS
NO_SECOND_SCORING_STACK
EXISTING_V3_REUSED
MFS_ASSESSMENT_POLICY_BOUNDARY_EXPLICIT
V34_FROZEN_TYPE5_V35_SEPARATE
T3_IN_MEMORY_T4_DURABLE_BOUNDARY_EXPLICIT
T5A_FREEZE_T5B_SEQUENCE_EXPLICIT
PRE_PR2A_MFS_PROJECTION_AMENDMENT_REQUIRED
TYPE5_RUNTIME_IMPLEMENTATION_BLOCKED
TYPE5_POLICY_PROMOTION_BLOCKED
```

Data: 2026-07-11

Repozytorium: `/root/Gho_dynamic_exit_v1`

Branch T0: `agent/type5-metric-contract-integration-reconciliation-t0`

Audytowany `origin/main`, HEAD i merge-base:
`f904d5a02283126c599822c8839fdcd5ff1de901`

Wersja bazowego rejestru: `metric_contracts_v1_1`

Dokument źródłowy, zachowany bez zmian:
`PLANS/PLAN_TYPE_5_2.md`

SHA-256 dokumentu źródłowego:
`5c8c90f918c63994118e0104c4cfc66ce97e27016f455848f64b2e7bfe937c21`

Normatywna macierz zależności:
`reports/type5/type5_metric_contract_dependency_matrix_v1.md`

ADR:
`docs/ADR/ADR_8D_PLAN_TYPE5_V3_INTEGRATION_AFTER_METRIC_CONTRACTS_V1_1_20260711.md`

## 1. Cel i werdykt T0

Plan zastępuje wykonawczo wcześniejszą koncepcję Type-5 jednym przepływem:

```text
metric_contracts_v1_1
→ canonical producers
→ compact decision-time MFS projections
→ TYPE5_INPUT_BINDINGS_V1
→ MaterializedFeatureSet
→ EarlyFlowPatternAssessmentV1
  + CoordinationPatternAssessmentV1
→ istniejący Gatekeeper V3
→ in-memory observe-only integration
→ compact decision v35 + type5_shadow_assessment_v1.jsonl
→ T5A discovery
→ FREEZE calibration/outcome contracts
→ T5B prospective shadow validation + untouched holdout
→ osobny policy-promotion plan
```

Type-5 nie może stać się drugim scoring stackiem. Nie powstają nowe
odpowiedniki:

- `RiskVerdictStatus`;
- `OpportunityVerdictStatus`;
- `ConfidenceBreakdown`;
- `V3ShadowDecision`;
- final shadow arbiter;
- terminalny Type-5 verdict.

`MaterializedFeatureSet` pozostaje snapshotem canonical measurements i compact
decision evidence. Assessment jest deterministyczną interpretacją snapshotu.
Istniejący Gatekeeper V3 pozostaje właścicielem risk, opportunity, confidence,
reason chain i finalnego shadow verdictu.

T0 jest wyłącznie dokumentacyjny. Nie daje zgody na:

- zmianę Rust lub TOML;
- zmianę MFS, DecisionLoggera, replayu lub runtime;
- zmianę Gatekeepera V2/V2.5 ani aktywnego BUY/REJECT/TIMEOUT;
- zmianę progów, wag, hard-fail classes lub confidence caps;
- promocję FSC v2, dev-primary, corrected FTDI actionability albo flip V2;
- zmianę shadow/live boundary;
- rozpoczęcie Type-5 T1 przed zaakceptowanym pre-PR2A MFS projection
  amendmentem oraz PASS PR2A i PR2B.

## 2. Potwierdzony baseline

PR #61 został zmergowany do `origin/main` pod
`f904d5a02283126c599822c8839fdcd5ff1de901`. PR1 ustanowił:

- 10 rodzin i 32 `MetricSurfaceId`;
- `MetricContractRolloutMode = Legacy | DualCompute | V2`;
- Profile A i klasy authority;
- `CanonicalHashV1`;
- `metric_contract_effective_config_hash` schema;
- canonical availability/quality/actionability envelope;
- record identity kontra stable event identity;
- schema foundation v34 i pełnego metric-contract sidecara bez runtime
  aktywacji.

Aktualny kod potwierdza:

- `MaterializedFeatureSet` jest canonical snapshotem decyzji;
- `PoolObservationSession::materialize_features()` jest granicą mutable state
  → immutable evidence;
- V3 ma już `V3ShadowDecision`, `RiskVerdictStatus`,
  `OpportunityVerdictStatus`, `ConfidenceBreakdown` i `reason_chain`;
- `MetricContractDecisionSummaryV1` v34 jest zamknięty przez
  `#[serde(deny_unknown_fields)]`;
- `MetricContractsEvidenceSetV1` jest pełnym semantic audit payloadem;
- `MetricContractEvidenceTransportV1` dodaje transport metadata;
- `FlipRatioEvidenceV2` zawiera detail-heavy owner/event evidence;
- `EarlyFingerprintMetrics` już oblicza `block0_sniped_supply_pct`,
  `whale_reversal_ratio_top1` i `whale_reversal_ratio_top3`;
- `AlphaFingerprintFeatures` nie materializuje jeszcze tych trzech wartości;
- obecny CPV dzieli przez wszystkich current signers i nie odróżnia clean-empty
  history od lookup miss/eviction;
- pole `MaterializedFeatureSet.metric_contract_evidence_v1` nie istnieje i
  wymaga osobnego amendmentu nadrzędnego planu przed PR2A.

## 3. Reconciliation wcześniejszego planu

Zachowane zostają:

- shadow-first rollout;
- MFS jako jedyny assessment snapshot;
- deterministic replay;
- typed reason codes i jawna missingness;
- rozdzielenie decision evidence od durable audit detail;
- addytywna kompatybilność;
- prospective validation i untouched holdout;
- osobny promotion decision.

Odrzucone zostają:

| Propozycja źródłowa | Decyzja T0 |
| --- | --- |
| `EarlyBuyerSignatureFeatures` w MFS ze score/severity/taxonomy | Zastępuje go pure `EarlyFlowPatternAssessmentV1`. |
| `CoordinationFusionFeatures` w MFS z fused scores | Zastępuje go pure `CoordinationPatternAssessmentV1`. |
| Nowy risk/opportunity/confidence stack | Reuse istniejącego V3. |
| Nowy final shadow arbiter | Jedynym arbitrem pozostaje `V3ShadowDecision`. |
| Pełny Type-5 payload w decision row | Compact v35 ref/hash + pełny Type-5 sidecar. |
| Pełny `MetricContractsEvidenceSetV1` w MFS | Odrzucony; MFS dostaje osobny compact projection. |
| Rozszerzenie frozen v34 o Type-5 | Odrzucone; Type-5 durable collection używa v35. |
| Przykładowe executable thresholds | `UNFROZEN_PENDING_CALIBRATION`. |
| Dynamiczne wyszukiwanie pierwszej authoritative surface | Zakazane; exact static binding. |

Historyczne `OrganicBroadeningFeatures.broadening_score`,
`ManipulationContradictionFeatures.contradiction_score` i legacy `high_*` nie
stają się canonical Type-5 inputs.

## 4. Compact Metric Contract projection w MFS

### 4.1 Twarda granica projection kontra sidecar

```text
MaterializedFeatureSet.metric_contract_evidence_v1
nie oznacza MetricContractsEvidenceSetV1
i nie oznacza MetricContractEvidenceTransportV1.
```

Pole nie jest aliasem, wrapperem ani kopią pełnego evidence setu i nie jest
kopią `metric_contract_evidence_v1.jsonl`.

Pre-PR2A amendment nadrzędnego planu ma zatwierdzić exact field/type:

```text
MaterializedFeatureSet.metric_contract_evidence_v1
  = PROPOSED_PENDING_METRIC_CONTRACT_PLAN_AMENDMENT

field type
  = MetricContractDecisionEvidenceProjectionV1
```

Docelowy kontrakt:

```rust
pub struct MetricContractDecisionEvidenceProjectionV1 {
    pub schema_version: u16,
    pub rollout_mode: MetricContractRolloutMode,
    pub profile_id: MetricContractProfileIdV1,
    pub profile_hash: CanonicalHashV1,
    pub metric_contract_effective_config_hash: CanonicalHashV1,
    pub fee_topology_diversity_index: FtdiDecisionProjectionV1,
    pub dev_buy: DevBuyDecisionProjectionV1,
    pub same_ms_tx_ratio: TxTimingDecisionProjectionV1,
    pub top3_signer_volume_ratio: Top3DecisionProjectionV1,
    pub flip_ratio: FlipDecisionProjectionV1,
    pub funding_source_concentration: FundingDecisionProjectionV1,
    pub fsc_evidence_status: FscStatusDecisionProjectionV1,
    pub manipulation_contradiction: ManipulationDecisionProjectionV1,
    pub reserve_velocity: ReserveVelocityDecisionProjectionV1,
    pub recent_buy_sell: RecentBuySellDecisionProjectionV1,
}
```

Projection zawiera wyłącznie canonical decision-time values, wymagane bounded
counts, compact availability/quality/actionability envelopes, schema-bounded
typed reason summaries oraz producer/config/cutoff provenance wymagane przez
policy, comparator albo Type-5.

Zakazane w MFS:

- `FlipRatioEvidenceV2.owners`;
- owner anchors, qualifying sell identities i cumulative owner flows;
- pełne event lists;
- pełne FSC candidates lub transfer histories;
- writer timestamp i rotation metadata;
- pełny `MetricContractsEvidenceSetV1`;
- `MetricContractEvidenceTransportV1`;
- reconstruction pełnego audit payloadu z projection.

Każda kolekcja w projection musi być schema-bounded. Projection i pełny audit
payload powstają z jednego frozen producer snapshotu, bez podwójnego compute.

Pełne metric-contract evidence pozostaje wyłącznie w:

```text
metric_contract_evidence_v1.jsonl
```

Historyczny brak projection mapuje się na `NotRecordedLegacySchema`, nie na
measured zero. Type-5 accessor odczytuje wyłącznie tę kompaktową projekcję w
MFS; nigdy nie czyta pełnego sidecara.

Osobny dokumentacyjny amendment nadrzędnego planu Metric Contracts jest twardym
prerequisite przed PR2A. Musi zatwierdzić:

- dokładne pola per-family projections;
- podział odpowiedzialności PR2A/PR2B;
- serde i historical-missing semantics;
- boundedness wszystkich kolekcji i reason summaries;
- projection build/serialization budget;
- replay i hash semantics;
- dowód `one producer → one frozen snapshot → two representations`;
- zakaz reconstruction pełnego audit payloadu z projection.

Ten T0 amendment nie zmienia nadrzędnego planu Metric Contracts i nie wykonuje
opisanego pre-PR2A amendmentu.

### 4.2 `TYPE5_INPUT_BINDINGS_V1`

`metric_contracts_v1_1` pozostaje zamknięty. Type-5 używa osobnego rejestru:

```rust
pub enum Type5InputRefV1 {
    MetricContractSurface {
        contract_id: MetricContractId,
        surface_id: MetricSurfaceId,
    },
    CanonicalMfsInput {
        input_id: Type5InputIdV1,
        mfs_path: &'static str,
        producer: &'static str,
        evidence_status_path: &'static str,
        semantics_version: u16,
    },
    MissingPrimitive {
        primitive_id: Type5PrimitiveIdV1,
    },
}

pub struct Type5InputBindingV1 {
    pub input_id: Type5InputIdV1,
    pub input_ref: Type5InputRefV1,
    pub intended_semantics: &'static str,
    pub allowed_type5_use: Type5InputUseV1,
    pub recompute_forbidden: bool,
}
```

`mfs_path` jest wyłącznie metadata/debugging stringiem. Runtime resolution jest
exhaustive matchem po `Type5InputIdV1`, bez string reflection, dynamicznego
downcastu ani dynamicznego wyszukiwania surface.

Każdy accessor zwraca:

```rust
pub struct Type5ResolvedInputEvidenceV1 {
    pub input_id: Type5InputIdV1,
    pub input_ref: Type5InputRefV1,
    pub value: CanonicalNullableV1<Type5ResolvedInputValueV1>,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub reason_codes: Vec<Type5InputReasonCodeV1>,
    pub producer_id: Type5ProducerIdV1,
    pub producer_config_ref: Type5ProducerConfigRefV1,
    pub source_cutoff: Type5SourceCutoffV1,
    pub authority_or_use_class: Type5AuthorityOrUseClassV1,
}
```

`Type5ResolvedInputValueV1` jest zamkniętym input-specific enumem. Nie używa
`serde_json::Value`. `reason_codes` są bounded limitem zapisanym w schema.

Dla Metric Contracts accessor zachowuje exact `MetricContractId`, exact
`MetricSurfaceId`, `MetricAuthorityClass`, bieżący `MetricRolloutRoleV1`,
`policy_actionable`, profile ID/hash i
`metric_contract_effective_config_hash`. Wartość pochodzi wyłącznie z
`MetricContractDecisionEvidenceProjectionV1` w MFS.

Dla non-metric inputs accessor zachowuje exact `Type5InputIdV1`,
`Type5InputUseV1`, field-level availability/quality/reasons, producer/config
provenance i cutoff, bez fikcyjnego `MetricContractId`.

Group-level status jest wyłącznie górnym ograniczeniem jakości:

- non-clean group nie daje field-level `Measured/Clean`;
- `None` nie staje się zero;
- legacy default zero/false nie dowodzi pomiaru;
- organic scalar jest evaluable tylko przy `sequence_available=true` i zgodnym
  field/group statusie;
- brak config ref lub cutoff proof wyklucza evaluability.

`OrganicBroadeningFeatures.broadening_score`,
`ManipulationContradictionFeatures.contradiction_score` i legacy `high_*` nie
są canonical Type-5 inputs.

### 4.3 Producer config references

```rust
pub enum Type5ProducerConfigRefV1 {
    MetricContractEffectiveConfig {
        hash: CanonicalHashV1,
    },
    ComponentResolvedConfig {
        producer_id: Type5ProducerIdV1,
        schema_version: u16,
        hash: CanonicalHashV1,
    },
}
```

Dziesięć Metric Contracts używa `metric_contract_effective_config_hash`.
Fingerprint, CPV/cohort, DBIA/SFD/DES i organic broadening używają
`ComponentResolvedConfig`: hash zamkniętego typed payloadu rzeczywiście
resolved ustawień producenta. Nie wolno użyć całego `brain_config_hash` ani
dowolnego wycinka TOML.

Każdy component payload ma własny `schema_version`, zamknięty zestaw pól,
canonical field order i `CanonicalHashV1`. Dostępny input bez poprawnego,
zgodnego z producerem config ref nie jest evaluable.

Pierwsze payload families:

```text
FingerprintResolvedConfigV1
  = wszystkie resolved EarlyFingerprintConfig fields
    + fingerprint algorithm/semantics version

CrossPoolResolvedConfigV1
  = CrossPoolVelocityConfig
    + cohort state-machine version
    + tombstone cap/TTL/overflow behavior

SybilResolvedConfigV1
  = DBIA/SFD/DES algorithm versions
    + używane compiled constants
    + population/sample rules

OrganicBroadeningResolvedConfigV1
  = segment materialization semantics version
    + resolved segment/window/sample settings
```

Compiled constants są literalnie obecne w typed payloadzie. `brain_config_hash`
może pozostać wyłącznie pomocniczym transport provenance.

`type5_assessment_effective_config_hash` referuje wykorzystane component refs i
`metric_contract_effective_config_hash`, ale nie kopiuje producer payloadów i
nie miesza producer settings z assessment thresholds.

## 5. Fingerprint projections i CrossPoolCohortReuse

### 5.1 Klasyfikacja T1

```text
CreationSlotSupplyConcentration
  = EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED

WhaleReversal
  = EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED

CrossPoolCohortReuse
  = TRULY_MISSING_TYPE5_PRIMITIVE
```

Canonical fingerprint source jest jednoznaczny:

```text
canonical owner:
ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg

implementation type:
seer::early_fingerprint::FingerprintAggregator

canonical accessor:
TxIntelligenceEngine::fingerprint_metrics()
```

Runtime-local aggregatory w `OracleRuntime` są compatibility/logging-only.

T1 nie zmienia `FingerprintAggregator::ingest()` ani formuł `finalize()`.
Kopiuje existing producer outputs do MFS i dodaje field-level typed evidence,
config ref i cutoff.

Pełna nazwa historycznego pola brzmi:

```text
legacy aggregate string EarlyFingerprintMetrics.fingerprint_reason
```

Assessment nie może parsować tego stringa. T1 dodaje typed per-field reasons
na producer/materialization boundary. Brak jednoznacznej atrybucji daje
`ReasonAttributionUnavailable` oraz field-level `Degraded/Unavailable`, nie
zgadywanie.

### 5.2 Creation-slot supply concentration bez clampowania

```text
binding:
Type5InputIdV1::CreationSlotSupplyConcentration

source:
EarlyFingerprintMetrics.block0_sniped_supply_pct

target:
alpha_fingerprint.creation_slot_sniped_supply_ratio_v1

unit:
ratio 0..1
```

Historyczny suffix `pct` nie oznacza skali `0..100`.

- `None` dla nieznanego creation slot/supply pozostaje `None`;
- finite `[0,1]` jest kopiowane bit-for-bit;
- wartość nie jest clampowana;
- finite poza `[0,1]` lub non-finite nie jest evaluable;
- projection zapisuje `value=null`, `availability=Unavailable`,
  `measurement_quality=NotApplicable`;
- typed reasons:
  - `CreationSlotSupplyRatioOutOfRange`;
  - `CreationSlotSupplyRatioNonFinite`.

Raw invalid producer value może pozostać wyłącznie w pełnym audit evidence.
Non-finite diagnostyka używa raw IEEE-754 bits zapisanych jako integer/string
diagnostic field, nigdy jako nielegalny JSON float.

Zakazane są `clamp`, `min(1.0)`, saturating conversion i zastąpienie błędu
zerem.

### 5.3 Whale reversal projections

```text
binding top1:
Type5InputIdV1::WhaleReversalTop1

source top1:
EarlyFingerprintMetrics.whale_reversal_ratio_top1

target top1:
alpha_fingerprint.whale_reversal_sell_to_buy_ratio_top1_v1

binding top3:
Type5InputIdV1::WhaleReversalTop3

source top3:
EarlyFingerprintMetrics.whale_reversal_ratio_top3

target top3:
alpha_fingerprint.whale_reversal_sell_to_buy_ratio_top3_v1
```

Wartości są projection istniejącego producenta, nie nowym algorytmem. Nie
zakłada się bounded ratio: whale reversal jest unbounded sell/buy ratio.
Zakazane są clampowanie i interpretacja braku denominatora jako zero. Missing
owner deltas lub denominator dają `Unavailable`; wallet-cap degradation daje co
najmniej `Degraded` i typed reason.

### 5.4 CrossPoolCohortReuse

Jedyny naprawdę brakujący primitive rozszerza `CrossPoolVelocityIndex`:

```rust
pub enum CohortSignerHistoryStateV1 {
    KnownEmptyHistory,
    KnownNonEmptyHistory,
    UnavailableHistory,
    EvictedHistory,
}
```

- `KnownEmptyHistory`: decision-safe lookup obejmuje lookback/cutoff, a prior
  other-pool set jest pusty;
- `KnownNonEmptyHistory`: decision-safe set jest niepusty;
- `UnavailableHistory`: brak readiness, cutoff proof, continuity albo
  jednoznacznego lookup result;
- `EvictedHistory`: bounded tombstone dowodzi relewantnej utraty historii przez
  global albo per-signer cap.

Lookup miss nigdy nie jest automatycznie `KnownEmptyHistory`. Lookback expiry
może dać `KnownEmptyHistory` wyłącznie wtedy, gdy istnieje dowód ciągłości
źródła i dowód, że wszystkie usunięte wpisy leżały poza aktualnym lookbackiem.

```text
current_signers = unique successful BUY signers at cutoff
all_current_signer_pair_count = checked_choose_2(current_signer_count)
eligible_history_signers = KnownEmpty + KnownNonEmpty
eligible_pair_count = checked_choose_2(eligible_history_signer_count)
recurring_pair_count = eligible pairs with non-empty prior-pool intersection
cohort_reuse_ratio = recurring_pair_count / eligible_pair_count
signer_history_coverage = eligible_history_signer_count / current_signer_count
pair_coverage = eligible_pair_count / all_current_signer_pair_count
```

`checked_choose_2` używa `u128(n) * u128(n - 1) / 2` i checked conversion do
`u64`:

```text
value_u128 = u128(n) * u128(n - 1) / 2
result = checked u64 conversion
```

Zakazane są `saturating_mul`, `saturating_add` i jakikolwiek saturating pair
count.

Failure semantics:

- mniej niż dwóch current signers → `Insufficient`;
- overflow/conversion failure → `Unavailable`;
- obcięty current signer set lub niepełny denominator proof → `Unavailable`;
- zero evaluable pairs → `Unavailable`;
- partial pair coverage albo history eviction przy kompletnym current set →
  `Degraded`;
- signer z `EvictedHistory` jest wykluczony z eligible denominatora, a jego
  obecność daje co najmniej `Degraded`;
- lookup miss nigdy nie jest `KnownEmptyHistory`;
- clean wymaga `pair_coverage == 1`, pełnego cutoff proof i braku gap/eviction;
- `eligible_pair_count <= all_current_signer_pair_count`, inaczej
  `Unavailable`.

MFS przechowuje wyłącznie bounded aggregate:

```rust
pub struct CrossPoolCohortReuseEvidenceV1 {
    pub ratio: CanonicalNullableV1<f64>,
    pub current_signer_count: u64,
    pub all_current_signer_pair_count: u64,
    pub eligible_history_signer_count: u64,
    pub eligible_pair_count: u64,
    pub recurring_pair_count: u64,
    pub signer_history_coverage: CanonicalNullableV1<f64>,
    pub pair_coverage: CanonicalNullableV1<f64>,
    pub known_empty_count: u64,
    pub known_non_empty_count: u64,
    pub unavailable_count: u64,
    pub evicted_count: u64,
    pub availability: MetricAvailabilityV1,
    pub measurement_quality: MetricMeasurementQualityV1,
    pub reason_codes: Vec<Type5InputReasonCodeV1>,
    pub producer_config_ref: Type5ProducerConfigRefV1,
    pub source_cutoff: Type5SourceCutoffV1,
}
```

Nie materializuje się par ani prior-pool sets. Tombstones są bounded i mają
TTL/cap/overflow semantics w `CrossPoolResolvedConfigV1`. Tombstone overflow
daje `UnavailableHistory`, nigdy clean empty.

T1 rozszerza istniejący bounded `CrossPoolVelocityIndex`. Utworzenie drugiego
globalnego pair indexu jest zakazane.

## 6. MFS / assessment / policy i istniejący V3

```text
MFS        = compact canonical measurements/evidence/provenance
assessment = deterministic interpretation of immutable MFS
policy     = existing Gatekeeper V3 shadow decision
```

Do MFS nie trafiają score, final severity, policy taxonomy, final confidence,
Type-5 verdict ani coordination penalty.

T2 projektuje:

```rust
pub struct EarlyFlowPatternAssessmentV1 {
    pub schema_version: u16,
    pub status: Type5AssessmentStatusV1,
    pub input_bindings_hash: CanonicalHashV1,
    pub assessment_effective_config_hash: CanonicalHashV1,
    pub inputs: Vec<Type5ResolvedInputEvidenceV1>,
    pub observations: Vec<EarlyFlowPatternObservationV1>,
    pub reason_codes: Vec<Type5AssessmentReasonCodeV1>,
}

pub struct CoordinationPatternAssessmentV1 {
    pub schema_version: u16,
    pub status: Type5AssessmentStatusV1,
    pub input_bindings_hash: CanonicalHashV1,
    pub assessment_effective_config_hash: CanonicalHashV1,
    pub inputs: Vec<Type5ResolvedInputEvidenceV1>,
    pub observations: Vec<CoordinationPatternObservationV1>,
    pub reason_codes: Vec<Type5AssessmentReasonCodeV1>,
}
```

Inwarianty:

- input wyłącznie `&MaterializedFeatureSet`;
- exact static binding resolution;
- brak raw tx/session/RPC/live-index/wall-clock reads;
- brak metric recompute;
- missing/degraded nie oznacza pattern absent;
- pre-freeze status `UNFROZEN_PENDING_CALIBRATION`;
- brak final score, severity, confidence i verdictu;
- ten sam MFS + hashes daje identyczny assessment.

Istniejący V3 pozostaje jedynym ownerem:

- `V3ShadowDecision`;
- `RiskVerdictStatus`;
- `OpportunityVerdictStatus`;
- `ConfidenceBreakdown`;
- `reason_chain` i `reason_code`.

## 7. Frozen v34, Type-5 v35 i durable evidence

Metric-contract burn-in zachowuje frozen v34 bez Type-5 pól.

T4 wprowadza:

```text
TYPE5_DECISION_SCHEMA_VERSION_V35 = 35
v35 = frozen v34 fields at unchanged paths + compact Type-5 refs
```

Compact Type-5 v35 fields:

```text
type5_assessment_schema_version
type5_assessment_status
type5_input_bindings_id/hash
type5_assessment_effective_config_hash
type5_assessment_record_identity
type5_assessment_sha256
type5_assessment_sidecar_schema
type5_v3_integration_mode
type5_calibration_contract_ref
type5_outcome_contract_ref
```

`TYPE5_DECISION_SCHEMA_VERSION_V35 = 35` jest osobną wersją dispatchu. Żaden
Type-5 field, nawet optional, nie może zostać dodany do historycznego v34.

Pełny payload:

```text
type5_shadow_assessment_v1.jsonl
```

zawiera assessments, compact resolved input evidence, exact surfaces/classes,
quality, reasons, config refs, cutoffs i V3 projection. Nie duplikuje pełnego
owner/event-level metric-contract evidence; referuje jego identity/hash.

Replay dispatchuje v33, frozen v34 i v35 osobno. v34 nigdy nie oczekuje pól
Type-5. T4 three-way join obejmuje v35 decision, metric-contract sidecar i
Type-5 sidecar. Missing/orphan/hash mismatch dyskwalifikuje run.

## 8. Calibration, outcome i resource contracts

Wszystkie pattern cutoffs, sample minima, mappings, weights, severity bands i
confidence caps pozostają:

```text
UNFROZEN_PENDING_CALIBRATION
```

`TYPE5_CALIBRATION_CONTRACT_V1` zawiera version/hash/`frozen_at`, binding/config
refs, prospective eligibility, untouched holdout manifest, split i zakaz
post-hoc zmian.

`TYPE5_OUTCOME_CONTRACT_V1` zamraża entry cutoff, notional, exit policy,
execution evidence grade, fees/impact assumptions, tail-risk metrics,
session/time split i missing handling. T0 nie ustala wartości.

Zakazane domyślne labels: arbitrary short-horizon final PnL, mark-only MFE,
last price i synthetic quote bez kosztów.

Przed T4 zamrażany jest `TYPE5_ARTIFACT_RESOURCE_CONTRACT_V1` z:

- dropped rows = 0;
- writer failures = 0;
- orphan refs = 0;
- rotation manifests/SHA;
- p50/p95/p99 bytes per record;
- serialization/enqueue p99;
- queue high-water;
- GB/hour delta;
- projection-size i v35-vs-v34 delta.

## 9. Sekwencja PR1–PR3 i T0–T6

```text
PR1 MERGED
   │
   ▼
pre-PR2A metric-contract plan amendment
   │
   ▼
PR2A → review/acceptance
   │
   ▼
PR2B → review/acceptance
   ├──────────────────────► PR2C ─────────────────┐
   │                                               │
   ▼                                               │
T1 bindings + projections + cohort primitive       │
   │                                               │
   ▼                                               │
T2 pure assessments                                │
   │                                               │
   ▼                                               │
T3 in-memory V3 hooks, ObserveOnly, no durability  │
   └───────────────────────────────────────────────┤
                                                   ▼
                                  T4 v35 durable shadow evidence
                                                   │
                                                   ▼
                                      T5A discovery/feasibility
                                                   │
                                                   ▼
                                    FREEZE owner-approved contracts
                                                   │
                                                   ▼
                                   T5B prospective shadow validation
                                                   │
                                                   ▼
                                  T6 separate policy-promotion plan
```

Równoległa ścieżka Metric Contracts:

```text
PR2C → frozen v34 burn-in → PR3 equivalence-only cutover
```

PR3 nie jest prerequisite dla Type-5 shadow assessmentów.

### T0 — reconciliation amendment

Trzy dokumenty, zero runtime changes. DoD: exact owners/bindings/milestones,
compact MFS boundary, v34/v35 split, poprawiony cohort denominator, V3 reuse i
blokady runtime/promotion.

### T1 — bindings, existing-producer projections i CrossPoolCohortReuse

Entry:

- accepted pre-PR2A projection amendment;
- PR2A PASS;
- PR2B PASS;
- T0 accepted.

Zakres:

- `TYPE5_INPUT_BINDINGS_V1`;
- typed resolver/config refs/cutoffs;
- creation-slot and whale existing-producer projections;
- jeden nowy CrossPoolCohortReuse primitive;
- zero threshold/taxonomy/score/policy influence;
- zero wpływu na Gatekeeper V2, V2.5 i istniejący V3.

### T2 — pure pattern assessments

Entry: T1 PASS. Zakres: dwa pure assessments, exact binding resolution,
typed reasons, determinism i no recompute.

### T3 — in-memory V3 integration

Entry: T2 PASS. Tryb `ObserveOnly`. Additive assessment context, compatibility
wrapper parity, zero JSONL, zero decision fields i zero V3 influence. Test-only
serialization fixtures są dozwolone. T3 nie wymaga PR2C ani PR3.

### T4 — v35 durable shadow collection

Entry: T3 PASS, PR2C PASS, frozen resource contract. Zakres: v35, Type-5
sidecar, bounded three-way writer/join (`v35 decision + metric-contract
evidence + Type-5 evidence`), hashes/manifests/rotation/orphan detection,
replay/audit. Nadal
`ObserveOnly`.

### T5A — discovery / feasibility / contract proposal

Entry: T4 data-quality PASS. T5A używa wyłącznie T4 evidence, nie wpływa na V3
i wykonuje discovery, feasibility, robustness oraz regime analysis. Przygotowuje
calibration/outcome contracts i future split/holdout rules, ale nie używa
przyszłego holdoutu.

### FREEZE

Owner approval zamraża versions/hashes/`frozen_at`, pattern mappings,
pattern rules, normalizacje, risk/opportunity component definitions,
confidence caps, outcome semantics, execution-cost assumptions, prospective
eligibility i untouched holdout manifest. Freeze zawiera dowód, że holdout nie
był użyty w T5A.

### T5B — prospective calibrated shadow validation

Entry: FREEZE PASS. Wyłącznie rows po `frozen_at`. Tryb `CalibratedShadow` może
wpływać tylko na istniejący V3 shadow. Active Gatekeeper pozostaje bez zmian.
Zmiana hash unieważnia T5B i wymaga nowego freeze/runs.

### T6 — osobny policy-promotion plan

Entry: prospective untouched-holdout PASS. T6 jest dokumentacyjny i ustala
component-by-component eligibility, authority prerequisites, rollback i
canary/reconciliation. Nie ma automatycznej promocji.

## 10. Test plan przyszłych etapów

### T1

- compact projection boundedness i zakaz heavy detail;
- one-snapshot projection/full-sidecar parity;
- creation valid/None/out-of-range/non-finite/no-clamp;
- whale source-to-MFS bitwise parity;
- exact fingerprint owner guard;
- typed config-ref hash determinism/mismatch;
- resolver None kontra measured zero i group-status ceiling;
- cohort KnownEmpty/KnownNonEmpty/Unavailable/Evicted;
- checked pair overflow/cap/truncation;
- partial/zero coverage, current-pool exclusion i cutoff;
- tombstone cap/TTL/overflow;
- zero V2/V2.5/V3 influence.

### T2

- same MFS/hashes → same assessment;
- binding/config/cutoff mismatch fail-closed;
- unavailable/degraded propagation;
- brak raw/live reads i score/verdict fields.

### T3

- compatibility wrapper parity;
- zero durable Type-5 rows;
- V3 risk/opportunity/confidence/reason/verdict parity;
- active Gatekeeper parity;
- no new arbiter.

### T4

- frozen v34 round-trip;
- separate v35 round-trip;
- sidecar hashes i three-way orphan detection;
- corrupt/truncated/rotation/duplicate identity;
- queue pressure/writer failure/resource budget.

### T5A/FREEZE/T5B

- prospective-only rows;
- untouched holdout;
- explicit outcome evidence/costs;
- owner freeze proof;
- pre-freeze row rejection;
- hash-change invalidation;
- no post-hoc adjustment;
- zero active policy influence.

## 11. T0 verification i Definition of Done

Przed przyjęciem dokumentów:

1. Potwierdzić PR #61, HEAD/origin/main/merge-base.
2. Potwierdzić niezmieniony source-plan SHA-256.
3. Potwierdzić dokładnie trzy dokumenty T0 w allowliście.
4. Potwierdzić brak Rust/TOML/runtime diff.
5. Potwierdzić wszystkie wymagane matrix rows.
6. Potwierdzić brak MFS mapping do pełnego evidence set/transport.
7. Potwierdzić brak `FlipRatioEvidenceV2.owners` w MFS contract.
8. Potwierdzić creation no-clamp i typed invariant reasons.
9. Potwierdzić `Type5ProducerConfigRefV1` i zakaz brain-config substitution.
10. Potwierdzić dokładnie jedną truly-missing family.
11. Potwierdzić exact fingerprint owner/type/accessor.
12. Potwierdzić checked cohort pair arithmetic.
13. Potwierdzić frozen v34 i osobne v35.
14. Potwierdzić T3 in-memory/T4 durable split.
15. Potwierdzić T5A/FREEZE/T5B bez cyklu.
16. Sprawdzić Markdown tables/fences/trailing whitespace i `git diff --check`.

Acceptance:

```text
TYPE5_T0_ARCHITECTURE_PASS                         PASS
TYPE5_T0_SEMANTIC_RECONCILIATION_PASS              PASS
EXACT_BINDINGS_REVIEWABLE                          PASS
NO_DUPLICATE_METRIC_PRODUCERS                      PASS
NO_SECOND_SCORING_STACK                            PASS
EXISTING_V3_REUSED                                 PASS
MFS_ASSESSMENT_POLICY_BOUNDARY_EXPLICIT            PASS
V34_FROZEN_TYPE5_V35_SEPARATE                      PASS
T3_IN_MEMORY_T4_DURABLE_BOUNDARY_EXPLICIT          PASS
T5A_FREEZE_T5B_SEQUENCE_EXPLICIT                    PASS
PRE_PR2A_MFS_PROJECTION_AMENDMENT_REQUIRED          ENFORCED
TYPE5_RUNTIME_IMPLEMENTATION_BLOCKED               ENFORCED
TYPE5_POLICY_PROMOTION_BLOCKED                     ENFORCED
```

## 12. Świadomie odroczone decyzje

Odroczone do właściwych bramek, nie do implementatora T1:

- formalne zatwierdzenie exact compact projection subfields w pre-PR2A
  amendment;
- wykonanie PR2A/PR2B/PR2C;
- executable pattern parameters (`UNFROZEN_PENDING_CALIBRATION`);
- outcome values;
- numeric Type-5 sidecar budgets po sizing preflight;
- jakakolwiek active-policy promotion.

Nie pozostaje nieustalony owner, semantyka, denominator, schema boundary ani
milestone. Następną implementacją po dokumentacyjnej bramce jest PR2A, następnie
review i PR2B — nie Type-5 T1.

```yaml
task_classification: cross-cutting documentation-only Type-5 semantic reconciliation
primary_specialist: Ghost Runtime Coordinator
supporting_specialists:
  - SSOT Feature Materialization Guardian
  - Decision Logging Replay Analyst
  - Gatekeeper Policy Auditor
  - Statistical Research Engine
skills_used:
  - ghost-execution
  - abstract-reasoning
runtime_area_touched:
  - none in T0
contracts_at_risk:
  - MaterializedFeatureSet compact SSOT
  - canonical producer ownership
  - decision projection versus durable audit evidence
  - frozen v34 replay
  - V3 shadow determinism
  - prospective calibration discipline
active_or_legacy_path: documentation over active MFS/V3 shadow and future v35
recommended_action: accept T0, record pre-PR2A projection amendment, then execute PR2A and PR2B
verification_steps:
  - verify base and source-plan SHA
  - validate dependency matrix and ADR consistency
  - validate compact projection versus full sidecar boundary
  - validate no runtime or policy changes
risk_level: high
```

```yaml
delegation_trace:
  task_classification: cross-cutting Type-5 T0 documentation amendment
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - SSOT Feature Materialization Guardian
    - Decision Logging Replay Analyst
    - Config Rollout Safety Reviewer
    - Gatekeeper Policy Auditor
    - Statistical Research Engine
  specialist_docs_loaded:
    - docs/agents/ghost-runtime-coordinator.md
    - docs/agents/ssot-feature-materialization-guardian.md
    - docs/agents/gatekeeper-policy-auditor.md
    - docs/agents/decision-logging-replay-analyst.md
  specialist_docs_not_loaded:
    - name: Config Rollout Safety Reviewer
      reason: T0 nie zmienia TOML ani defaults
    - name: Solana Execution Path Engineer
      reason: execution i shadow-live boundary są poza zakresem
  skills_used:
    - ghost-execution
    - abstract-reasoning
  fast_path_used: false
  contracts_checked:
    - MaterializedFeatureSet compact decision snapshot
    - MetricContractsEvidenceSetV1 versus MetricContractEvidenceTransportV1
    - FlipRatioEvidenceV2 owner-level detail
    - exact fingerprint owner/type/accessor
    - creation no-clamp invariant
    - component resolved config provenance
    - checked cohort denominator
    - existing V3 arbiter reuse
    - frozen v34 and separate v35
    - T3/T4 and T5A/FREEZE/T5B boundaries
    - runtime and policy-promotion blocks
  unresolved_routing_uncertainty: []
```
