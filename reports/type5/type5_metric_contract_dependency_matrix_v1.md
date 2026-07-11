# Type-5 T0 — normatywna macierz zależności Metric Contracts V1.1

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

Audytowany `origin/main` / merge-base:
`f904d5a02283126c599822c8839fdcd5ff1de901`

Rejestr bazowy: `metric_contracts_v1_1`

Profil: `metric_contracts_v1_1_profile_a`

Dokument nadrzędny:
`PLANS/DO_REALIZACJI/PLAN_TYPE5_V3_INTEGRATION_AFTER_METRIC_CONTRACTS_V1_1_20260711.md`

## 1. Reguły normatywne

### 1.1 Binding i result classes

`TYPE5_INPUT_BINDINGS_V1` nie jest drugim authority profilem. Każdy input
używa exact `MetricContractSurface`, exact `CanonicalMfsInput` albo jawnego
`MissingPrimitive`. `mfs_path` jest tylko metadata; runtime używa exhaustive
compile-time accessor.

Role Profile A:

- `A` — `PolicyAuthoritative`;
- `C` — `PolicyComparator`;
- `N` — `NonPolicy`;
- `N/A` — input poza `metric_contracts_v1_1`.

Dozwolone klasy wyniku:

```text
EXISTS_ACTIVE_LEGACY
PR1_SCHEMA_ONLY_NO_PRODUCER
PR2A_PRODUCER_REQUIRED
PR2B_PRODUCER_REQUIRED
PR2C_DURABLE_EVIDENCE_REQUIRED
EXISTING_V3_COMPONENT_REUSE
EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED
TRULY_MISSING_TYPE5_PRIMITIVE
DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED
POLICY_PROMOTION_REQUIRED_SEPARATELY
```

### 1.2 Exact fingerprint owner/type/accessor

Każdy fingerprint row zapisuje bez skrótu ten sam exact binding:

```text
canonical owner:
ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg

implementation type:
seer::early_fingerprint::FingerprintAggregator

canonical accessor:
TxIntelligenceEngine::fingerprint_metrics()
```

Runtime-local aggregatory w `OracleRuntime` są compatibility/logging-only.

### 1.3 Typed accessor result

Każdy binding zwraca normatywnie:

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

`Type5ResolvedInputValueV1` jest zamkniętym input-specific enumem. Zakazane są
`serde_json::Value`, dynamiczny downcast i string reflection. Runtime resolution
jest exhaustive matchem po `Type5InputIdV1`; `mfs_path` pozostaje wyłącznie
metadata/debugging stringiem. `reason_codes` mają schema-bounded limit.

Dla Metric Contracts accessor zachowuje exact `MetricContractId`, exact
`MetricSurfaceId`, `MetricAuthorityClass`, bieżący `MetricRolloutRoleV1`,
`policy_actionable`, profile ID/hash i
`metric_contract_effective_config_hash`. Dla non-metric inputów zachowuje exact
`Type5InputIdV1`, `Type5InputUseV1`, field-level quality/reasons, config ref i
cutoff bez fikcyjnego `MetricContractId`.

Brak config ref lub cutoff proof wyklucza evaluability. Group status jest
ceiling, nie dowodem field-level measurement: non-clean group nie daje
field-level `Measured/Clean`, `None` nie jest zerem, a legacy zero/false nie
dowodzi pomiaru. Organic scalar jest evaluable tylko przy
`sequence_available=true` i zgodnym field/group statusie. `broadening_score`,
`contradiction_score` i legacy `high_*` nie są canonical Type-5 inputs.

Config ref ma dokładnie dwa warianty:

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

Config refs:

```text
MC_CONFIG = Type5ProducerConfigRefV1::MetricContractEffectiveConfig
FP_CONFIG = ComponentResolvedConfig(FingerprintResolvedConfigV1)
CPV_CONFIG = ComponentResolvedConfig(CrossPoolResolvedConfigV1)
SYBIL_CONFIG = ComponentResolvedConfig(SybilResolvedConfigV1)
ORGANIC_CONFIG = ComponentResolvedConfig(OrganicBroadeningResolvedConfigV1)
```

Żaden component ref nie może używać całego `brain_config_hash` ani dowolnego
wycinka TOML. Każdy component payload ma schema version, zamknięty typed zestaw
resolved pól i canonical field order. Dostępny input bez zgodnego config ref
nie jest evaluable.

Pierwsze wymagane payload families:

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
    + compiled constants
    + population/sample rules

OrganicBroadeningResolvedConfigV1
  = segment materialization semantics version
    + resolved segment/window/sample settings
```

`type5_assessment_effective_config_hash` referuje używane config refs, nie
kopiuje payloadów i nie miesza producer settings z assessment thresholds.

## 2. Compact MFS projection kontra pełny transport

```text
MaterializedFeatureSet.metric_contract_evidence_v1
  = PROPOSED_PENDING_METRIC_CONTRACT_PLAN_AMENDMENT

field type
  = MetricContractDecisionEvidenceProjectionV1
```

Docelowy compact type:

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

Pole nie jest `MetricContractsEvidenceSetV1` ani
`MetricContractEvidenceTransportV1`; nie jest ich aliasem, wrapperem ani kopią
sidecara. Zawiera tylko canonical decision-time values, wymagane counts,
compact envelopes, bounded typed reason summaries i producer/config/cutoff
provenance. Pełny audit detail pozostaje wyłącznie w
`metric_contract_evidence_v1.jsonl`.

Zakazane w MFS:

- `FlipRatioEvidenceV2.owners`;
- per-owner anchors, qualifying sell identities i cumulative owner token flows;
- pełne event/candidate/history lists;
- pełne FSC transfer candidates;
- writer/rotation metadata;
- transport wrapper;
- reconstruction pełnego sidecara.

Wszystkie `metric_contract_evidence_v1.*` paths poniżej oznaczają pola
kompaktowej `MetricContractDecisionEvidenceProjectionV1`, a nie pełnego
semantic/transport payloadu. Bindingi pozostają `EXACT_BINDINGS_REVIEWABLE`,
ale formalne utworzenie pola wymaga pre-PR2A amendmentu.

Projection i pełny sidecar pochodzą z jednego frozen producer snapshotu, bez
metric recompute. Historyczny brak projection mapuje się na
`NotRecordedLegacySchema`, nigdy measured zero. Type-5 accessors czytają
wyłącznie compact projection w MFS, nigdy pełny sidecar.

Pre-PR2A parent-plan amendment jest twardym prerequisite i musi zatwierdzić:
exact per-family fields, split PR2A/PR2B, serde/historical-missing semantics,
boundedness, build/serialization budget, replay/hash semantics, dowód
`one producer → two representations` oraz zakaz reconstruction pełnego audit
payloadu. Ten T0 nie wykonuje amendmentu nadrzędnego planu.

## 3. Metric-contract surfaces

Każdy compact path w poniższych wierszach ma statyczny root:

```text
MaterializedFeatureSet.metric_contract_evidence_v1
  : MetricContractDecisionEvidenceProjectionV1
```

Wiersz `sidecar only` nie tworzy alternatywnego MFS inputu. Formalna aktywacja
żadnego projection field nie jest w tym T0 zakończona.

| Sygnał Type-5 | Semantyka | `MetricContractId` | Exact `MetricSurfaceId` | Authority | L / D / V2 | Canonical owner | Compact MFS path / durable path | Config ref | Milestone | Type-5 use | Recompute | Klasa wyniku |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Exact same-ms legacy | Adjacent exact timestamp collisions / transaction count, full TxIntel window. | `SameMsTxRatio` | `TxIntelSameMsCollisionRatioExact` | `Authoritative` | `A/A/N` | `TxIntelligenceEngine` | existing `tx_intel_features.same_ms_tx_ratio`; proposed compact `metric_contract_evidence_v1.same_ms_tx_ratio.legacy_exact` | `MC_CONFIG` | existing + PR2A compact adapter | active provenance/parity | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Exact same-ms typed | Ta sama exact semantyka z numerator/denominator/dedupe/status. | `SameMsTxRatio` | `TxTimingExactSameMsEvidenceV1` | `EquivalentCutover` | `N/C/A` | `TxIntelligenceEngine` | proposed compact `.same_ms_tx_ratio.exact_v1`; full detail sidecar | `MC_CONFIG` | PR2A | shadow/comparator; PR3-only authority | forbidden | `PR2A_PRODUCER_REQUIRED` |
| Sub-50ms cluster | Adjacent gap `<50 ms`; nie exact same-ms. | `SameMsTxRatio` | `TxIntelBundleClusterRatioLt50Ms` | `EvidenceOnly` | `N/N/N` | TxIntel timing producer | proposed compact `.same_ms_tx_ratio.cluster_lt_50ms` | `MC_CONFIG` | PR2A | shadow evidence | forbidden | `PR2A_PRODUCER_REQUIRED` |
| Recent exact same-ms | Exact collisions w successful-only recent RCE window. | `SameMsTxRatio` | `RceSameMsCollisionRatioRecentExact` | `LoggingOnly` | `N/N/N` | RCE recent-window producer | proposed compact `.same_ms_tx_ratio.recent_exact`; durable PR2C | `MC_CONFIG` | PR2A/PR2C | logging/calibration only | forbidden | `PR2A_PRODUCER_REQUIRED` |
| Top3 preferred | Top-three signer absolute volume / total absolute signer volume, ratio `0..1`. | `Top3SignerVolumeRatio` | `TxIntelTop3SignerVolumeRatioPreferred` | `Authoritative` | `A/A/A` | `TxIntelligenceEngine` | existing `tx_intel_features.top3_signer_volume_ratio`; proposed compact preferred | `MC_CONFIG` | existing + PR2A telemetry | canonical concentration | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Top3 alias | Historyczny ratio-scale `top3_volume_pct`. | `Top3SignerVolumeRatio` | `TxIntelTop3VolumePctCompatibilityAlias` | `Compatibility` | `N/N/N` | `TxIntelligenceEngine` | existing alias; compact compatibility only | `MC_CONFIG` | existing | frozen replay only | forbidden | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| Top3 effective selector | Preferred + istniejący legacy fallback helper. | `Top3SignerVolumeRatio` | `TxIntelTop3EffectiveSelector` | `Authoritative` | `A/A/A` | `TxIntelFeatures::effective_top3_signer_volume_ratio()` | existing accessor; proposed compact effective ratio | `MC_CONFIG` | existing + PR2A guard | default top3 input | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Legacy flip | Slot-gap fingerprint; nie literalne wall-clock 10 s. | `FlipRatio` | `EarlyFingerprintFlipRatioLegacySlotGap` | `Compatibility` | `N/N/N` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | proposed compact `.flip_ratio.legacy_slot_gap_ratio`; full audit sidecar | `MC_CONFIG` | PR2B | parity/history only | forbidden | `PR2B_PRODUCER_REQUIRED` |
| Flip V2 | First eligible BUY anchor + cumulative flows + time/slot constraints. | `FlipRatio` | `FlipRatioHybridEvidenceV2` | `EvidenceOnly` | `N/N/N` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | compact ratio/count/status only; `owners` wyłącznie sidecar | `MC_CONFIG` | PR2B | shadow evidence | forbidden | `PR2B_PRODUCER_REQUIRED` |
| FTDI legacy value | Unique topologies / unique successful buyers. | `FeeTopologyDiversityIndex` | `TxIntelFeeTopologyDiversityLegacy` | `Authoritative` | `A/A/N` | sybil/TxIntel producer | existing `sybil_resistance.fee_topology_diversity_index`; compact legacy | `MC_CONFIG` | existing + PR2A adapter | active provenance | forbidden | `EXISTS_ACTIVE_LEGACY` |
| FTDI typed value | Typed equivalent legacy formula. | `FeeTopologyDiversityIndex` | `FtdiValueEvidenceV1` | `EquivalentCutover` | `N/C/A` | same FTDI producer | proposed compact `.fee_topology_diversity_index.value_v1` | `MC_CONFIG` | PR2A | shadow/parity | forbidden | `PR2A_PRODUCER_REQUIRED` |
| FTDI legacy actionability | Historyczny buy-transaction-count quality gate. | `FeeTopologyDiversityIndex` | `FtdiLegacyBuyTxActionability` | `Authoritative` | `A/A/A` | FTDI evidence adapter | proposed compact legacy actionability | `MC_CONFIG` | PR2A | current actionability provenance | forbidden | `PR2A_PRODUCER_REQUIRED` |
| FTDI corrected actionability | Unique-buyer quality/actionability. | `FeeTopologyDiversityIndex` | `FtdiUniqueBuyerActionabilityV2` | `Counterfactual` | `N/C/C` | same FTDI producer | proposed compact counterfactual actionability | `MC_CONFIG` | PR2A | counterfactual only | forbidden | `POLICY_PROMOTION_REQUIRED_SEPARATELY` |
| Coordination FTDI HHI | HHI rozkładu topologii; nie runtime FTDI. | `FeeTopologyDiversityIndex` | `CoordinationFeeTopologyHhiExportV1` | `ExportOnly` | `N/N/N` | coordination export helper | sidecar only; brak runtime assessment path | `MC_CONFIG` | PR2C | offline only | forbidden | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| Legacy FSC scalar | `1 - distinct_known_sources / known_source_samples`. | `FundingSourceConcentration` | `TxIntelFundingSourceConcentrationLegacy` | `Authoritative` | `A/A/N` | `FundingSourceIndex` | existing scalar; proposed compact legacy | `MC_CONFIG` | existing + PR2A adapter | active provenance | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Typed legacy FSC | Ta sama legacy formula z counts/status. | `FundingSourceConcentration` | `FundingSourceConcentrationLegacyEvidenceV1` | `EquivalentCutover` | `N/C/A` | `FundingSourceIndex` | proposed compact `.funding_source_concentration.legacy_v1` | `MC_CONFIG` | PR2A | shadow/parity | forbidden | `PR2A_PRODUCER_REQUIRED` |
| FSC v2 | Readiness/coverage/attribution niezależne od legacy scalar. | `FundingSourceConcentration` | `FundingSourceV2ReadinessEvidence` | `EvidenceOnly` | `N/N/N` | `FundingSourceIndex` | existing `sybil_resistance.funding_source_v2`; compact status/coverage only; details sidecar | `MC_CONFIG` | existing + PR2A cross-check + PR2C | shadow coordination | forbidden | `PR2C_DURABLE_EVIDENCE_REQUIRED` |
| FSC status compatibility | Legacy scalar availability nie dowodzi FSC v2 quality. | `FscEvidenceStatus` | `MaterializedFscStatusCompatibility` | `Compatibility` | `N/N/N` | MFS evidence materializer | existing `evidence_status.fsc`; compact cross-check | `MC_CONFIG` | PR2A | compatibility only | forbidden | `PR2A_PRODUCER_REQUIRED` |
| Coordination FSC HHI | Export HHI; nie legacy FSC/FSC v2 readiness. | `FundingSourceConcentration` | `CoordinationFundingSourceHhiExportV1` | `ExportOnly` | `N/N/N` | coordination export helper | sidecar only | `MC_CONFIG` | PR2C | offline only | forbidden | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| Manipulation raw legacy | Legacy numeric/default zero bez presence proof. | `ManipulationContradiction` | `MfsManipulationNumericLegacyDefaults` | `Authoritative` | `A/A/A` | V3 materializer | existing raw bundle | `MC_CONFIG` | existing + PR2B adapter | frozen V3 parity | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Manipulation numeric V2 | Presence-aware per-field numeric evidence. | `ManipulationContradiction` | `ManipulationNumericEvidenceV2` | `EquivalentCutover` | `N/C/C` | V3 materializer + PR2B adapter | proposed compact measured values/mask; full detail sidecar | `MC_CONFIG` | PR2B | raw assessment inputs | forbidden | `PR2B_PRODUCER_REQUIRED` |
| Legacy manipulation `high_*` | Default false nie dowodzi pomiaru. | `ManipulationContradiction` | `MfsManipulationHighFlagsLegacyDefaults` | `Authoritative` | `A/A/A` | V3 materializer | existing compatibility only | `MC_CONFIG` | existing | Type-5 use rejected | forbidden | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| Derived manipulation flags V2 | Present raw + comparator/threshold/config. | `ManipulationContradiction` | `PolicyDerivedManipulationHighFlagsV2` | `EquivalentCutover` | `N/C/C` | policy-stage adapter | proposed compact counterfactual summary; detail sidecar | `MC_CONFIG` | PR2B | not primitive input | forbidden | `POLICY_PROMOTION_REQUIRED_SEPARATELY` |
| Dev first-observed TxIntel | Pierwszy zaobserwowany creator BUY. | `DevBuy` | `TxIntelDevFirstObservedBuySol` | `Authoritative` | `A/A/A` | `TxIntelligenceEngine` | producer source + proposed compact | `MC_CONFIG` | existing + PR2A | provenance/parity | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Dev first-observed MFS | Aktualny policy input. | `DevBuy` | `MfsDevFirstObservedBuySol` | `Authoritative` | `A/A/A` | TxIntel → materializer | existing `tx_intel_features.dev_buy_sol`; proposed compact | `MC_CONFIG` | existing + PR2A | exact active input | forbidden | `EXISTS_ACTIVE_LEGACY` |
| GatekeeperBuffer dev-primary | Create-signature-anchored primary buy, osobna surface. | `DevBuy` | `GatekeeperBufferDevPrimaryBuySol` | `Compatibility` | `N/N/N` | `GatekeeperBuffer` | producer source; nie Type-5 MFS authority | `MC_CONFIG` | existing | parity only | forbidden | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| MFS dev-primary V1 | Counterfactual primary creator buy. | `DevBuy` | `MfsDevPrimaryBuySolV1` | `Counterfactual` | `N/C/C` | GatekeeperBuffer → materializer | proposed compact `.dev_buy.mfs_primary_v1` | `MC_CONFIG` | PR2A | counterfactual only | forbidden | `POLICY_PROMOTION_REQUIRED_SEPARATELY` |
| Effective policy dev buy | Faktyczny current policy read. | `DevBuy` | `EffectivePolicyDevBuySol` | `Authoritative` | `A/A/A` | current policy accessor | proposed compact effective policy value | `MC_CONFIG` | PR2A | active provenance | forbidden | `PR2A_PRODUCER_REQUIRED` |
| Reserve velocity legacy | Per-update rate; fallback zero nie dowodzi real zero. | `ReserveVelocity` | `AccountStateReserveVelocityScalarLegacy` | `EvidenceOnly` | `N/N/N` | `AccountStateCore` | existing scalar | `MC_CONFIG` | existing + PR2B mirror | compatibility context | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Reserve velocity V1 | Optional rate + reserves/interval/count/status. | `ReserveVelocity` | `ReserveVelocityEvidenceV1` | `EvidenceOnly` | `N/N/N` | `AccountStateCore` | proposed compact summary; detail sidecar | `MC_CONFIG` | PR2B | shadow evidence | forbidden | `PR2B_PRODUCER_REQUIRED` |
| Recent buy/sell legacy | Sell=0 scalar jest buy count, nie bounded ratio. | `RecentBuySell` | `RceBuySellRatioRecentLegacy` | `LoggingOnly` | `N/N/N` | RCE producer | proposed compact legacy scalar | `MC_CONFIG` | existing + PR2B/PR2C | logging only | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Recent buy/sell V1 | Raw counts + optional unbounded ratio + bounded buy share. | `RecentBuySell` | `RecentBuySellEvidenceV1` | `LoggingOnly` | `N/N/N` | RCE producer | proposed compact counts/ratios | `MC_CONFIG` | PR2B/PR2C | calibration covariate | forbidden | `PR2B_PRODUCER_REQUIRED` |

## 4. Canonical MFS inputs poza `metric_contracts_v1_1`

| Sygnał | Semantyka | Exact `Type5InputIdV1` / MFS path | Canonical owner | Evidence/status | Config ref | Milestone | Type-5 use | Recompute | Klasa wyniku |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CPV signer velocity | Other-pool velocity current successful BUY signers. | `CpvSignerCrossPoolVelocity` → `sybil_resistance.signer_cross_pool_velocity` | `CrossPoolVelocityIndex` | `cpv_evidence` + `evidence_status.cpv` | `CPV_CONFIG` | existing + PR2C | coordination evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| CPV other-pool activity | Other-pool activity intensity. | `CpvOtherPoolActivity` → `sybil_resistance.cpv_other_pool_activity` | `CrossPoolVelocityIndex` | `cpv_evidence` + `evidence_status.cpv` | `CPV_CONFIG` | existing + PR2C | coordination evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| DBIA | Dev-buyer infrastructure affinity. | `DevBuyerInfrastructureAffinity` → `sybil_resistance.dev_buyer_infrastructure_affinity` | `compute_sybil_resistance` | `evidence_status.sybil` + reasons | `SYBIL_CONFIG` | existing | raw evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| SFD | Spend-fraction divergence. | `SpendFractionDivergence` → `sybil_resistance.spend_fraction_divergence` | `compute_sybil_resistance` | `evidence_status.sybil` + reasons | `SYBIL_CONFIG` | existing | raw evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| DES | Demand elasticity score. | `DemandElasticityScore` → `sybil_resistance.demand_elasticity_score` | `compute_sybil_resistance` | `evidence_status.sybil` + reasons | `SYBIL_CONFIG` | existing | raw observation | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Organic raw sequence | Counts/growth/HHI fields; excludes `broadening_score`. | `OrganicBroadeningRawV1` → `organic_broadening` | `materialize_v3_organic_broadening` | bundle status + `sequence_available` | `ORGANIC_CONFIG` | existing | opportunity observations | forbidden | `EXISTING_V3_COMPONENT_REUSE` |
| Manipulation contradiction bundle | Existing V3 baseline; excludes `high_*`/score from Type-5 input. | `ExistingV3ManipulationContradictionBundle` → `manipulation_contradictions` | `materialize_v3_manipulation_contradictions` | bundle/evidence status | `MC_CONFIG` | PR2B raw operands | existing V3 only | forbidden | `EXISTING_V3_COMPONENT_REUSE` |
| Early top3 dominance | Early top3 BUY volume ratio. | `EarlyTop3BuyVolumeRatio` → `alpha_fingerprint.early_top3_buy_volume_pct_3s` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + alpha ceiling | `FP_CONFIG` | existing | early-flow evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Early-slot dominance | Earliest-slot BUY volume ratio. | `EarlySlotVolumeDominanceBuy` → `alpha_fingerprint.early_slot_volume_dominance_buy` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + alpha ceiling | `FP_CONFIG` | existing | early-flow evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Fee template reuse | Static fee-profile ratio. | `StaticFeeProfileRatio` → `alpha_fingerprint.static_fee_profile_ratio` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | early/coordination evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| CU template reuse | CU cluster dominance. | `ComputeUnitClusterDominance` → `alpha_fingerprint.compute_unit_cluster_dominance` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | early/coordination evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Fixed-size pattern | Fixed-size BUY ratio. | `FixedSizeBuyRatio` → `alpha_fingerprint.fixed_size_buy_ratio` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | early-flow evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Flipper presence | Legacy fingerprint flipper presence. | `FingerprintFlipperPresenceRatio` → `alpha_fingerprint.flipper_presence_ratio` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | compatibility evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Inner IX count | Average bounded inner IX count. | `AverageInnerInstructionCount` → `alpha_fingerprint.avg_inner_ix_count_50tx` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | supporting evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| CPI depth | Average bounded CPI depth. | `AverageCpiDepth` → `alpha_fingerprint.avg_cpi_depth_50tx` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | supporting evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Jito intensity | Known Jito tip intensity. | `JitoTipIntensity` → `alpha_fingerprint.jito_tip_intensity` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field presence + typed reasons | `FP_CONFIG` | existing | supporting evidence | forbidden | `EXISTS_ACTIVE_LEGACY` |
| Decision sample counts | Tx/buy/sell/unique signer counts at cutoff carried by compact per-family projections. | `DecisionSampleCounts` → exact count fields in `MetricContractDecisionEvidenceProjectionV1` | `TxIntelligenceEngine` | per-field compact envelope/status | `MC_CONFIG` | PR2A/PR2B projection | denominator/quality only | forbidden | `PR2A_PRODUCER_REQUIRED` |
| Segment same-size streak | Per-segment streak. | `SegmentSameSizeStreak` → `tx_segment_sequence.*.same_size_streak` | `GatekeeperBuffer` segment materializer | `evidence_status.tx_segments` | `ORGANIC_CONFIG` | existing | supporting sequence | forbidden | `EXISTS_ACTIVE_LEGACY` |

## 5. Existing-producer projections i jedyny missing primitive

| Sygnał | Source → target | Exact owner | Evidence/config/cutoff | Algorytm brakujący? | Milestone | Klasa wyniku |
| --- | --- | --- | --- | --- | --- | --- |
| Creation-slot supply concentration | `Type5InputIdV1::CreationSlotSupplyConcentration`: `EarlyFingerprintMetrics.block0_sniped_supply_pct` → `alpha_fingerprint.creation_slot_sniped_supply_ratio_v1` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field status/reasons + `FP_CONFIG` + cutoff | nie; projection only | T1 po PR2A/PR2B | `EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED` |
| Whale reversal top1 | `Type5InputIdV1::WhaleReversalTop1`: `EarlyFingerprintMetrics.whale_reversal_ratio_top1` → `alpha_fingerprint.whale_reversal_sell_to_buy_ratio_top1_v1` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field status/reasons + `FP_CONFIG` + cutoff | nie; projection only | T1 po PR2A/PR2B | `EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED` |
| Whale reversal top3 | `Type5InputIdV1::WhaleReversalTop3`: `EarlyFingerprintMetrics.whale_reversal_ratio_top3` → `alpha_fingerprint.whale_reversal_sell_to_buy_ratio_top3_v1` | owner: `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`;<br>type: `seer::early_fingerprint::FingerprintAggregator`;<br>accessor: `TxIntelligenceEngine::fingerprint_metrics()` | field status/reasons + `FP_CONFIG` + cutoff | nie; projection only | T1 po PR2A/PR2B | `EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED` |
| CrossPoolCohortReuse | new checked pair recurrence aggregate → `sybil_resistance.cross_pool_cohort_reuse_ratio_v1` | existing `CrossPoolVelocityIndex` | four history states + coverages + `CPV_CONFIG` + cutoff | tak | T1 po PR2A/PR2B | `TRULY_MISSING_TYPE5_PRIMITIVE` |

Creation projection rules:

- unit = ratio `0..1`; historyczny suffix `pct` nie oznacza `0..100`;
- valid finite `[0,1]` copied bit-for-bit;
- `None` remains unavailable;
- finite out-of-range: `value=null`, `availability=Unavailable`,
  `measurement_quality=NotApplicable`,
  `reason=CreationSlotSupplyRatioOutOfRange`;
- non-finite: `value=null`, `availability=Unavailable`,
  `measurement_quality=NotApplicable`,
  `reason=CreationSlotSupplyRatioNonFinite`;
- zakaz `clamp`, `min(1.0)`, saturating conversion i error-to-zero;
- raw invalid value jest audit-sidecar-only; non-finite diagnostic zapisuje raw
  IEEE-754 bits jako integer/string, nie nielegalny JSON float.

Pełna nazwa pola:

```text
legacy aggregate string EarlyFingerprintMetrics.fingerprint_reason
```

Assessment nie może go parsować. T1 dostarcza typed per-field reasons; brak
jednoznacznej atrybucji daje `ReasonAttributionUnavailable` i
`Degraded/Unavailable`, nie zgadywanie.

Whale reversal top1/top3 jest unbounded sell/buy ratio. Nie wolno zakładać
`[0,1]`, clampować ani mapować braku denominatora na zero. Brak owner deltas lub
denominatora daje `Unavailable`; wallet-cap degradation daje co najmniej
`Degraded`.

CrossPoolCohortReuse używa normatywnego enumu
`CohortSignerHistoryStateV1`:

```text
history states = KnownEmptyHistory / KnownNonEmptyHistory /
                 UnavailableHistory / EvictedHistory
all pairs = checked_choose_2(current_signer_count)
eligible signers = KnownEmpty + KnownNonEmpty
eligible pairs = checked_choose_2(eligible_signer_count)
recurrence = intersecting eligible prior-pool sets / eligible pairs
signer coverage = eligible signers / current signers
pair coverage = eligible pairs / all pairs
```

`checked_choose_2` używa `u128` intermediate i checked `u64` conversion.
Overflow, truncated current signer set, incomplete denominator proof albo zero
eligible pairs daje `Unavailable`. Partial coverage/eviction daje `Degraded`.
Lookup miss nigdy nie jest clean empty. Saturating pair count jest zabroniony.

State semantics:

- `KnownEmptyHistory`: decision-safe lookup pokrywa pełny lookback i cutoff,
  lecz prior other-pool set jest pusty;
- `KnownNonEmptyHistory`: decision-safe lookup ma prior other-pool ID;
- `UnavailableHistory`: brak readiness, continuity, cutoff proof lub
  jednoznacznego lookup result;
- `EvictedHistory`: bounded tombstone dowodzi usunięcia relewantnej historii
  przez global/per-signer cap.

Lookup miss nie staje się `KnownEmptyHistory`. Lookback expiry może być
known-empty tylko przy continuity proof i dowodzie, że usunięte wpisy były w
całości poza bieżącym lookbackiem.

```text
value_u128 = u128(n) * u128(n - 1) / 2
result = checked u64 conversion
```

Zakazane są `saturating_mul`, `saturating_add` i saturating pair count. Mniej
niż dwóch current signers daje `Insufficient`; signer `EvictedHistory` jest
wyłączony z denominatora i wymusza co najmniej `Degraded`; clean wymaga
`pair_coverage == 1`, pełnego cutoff proof i braku gap/eviction. Tombstones są
bounded, mają TTL/cap/overflow behavior w `CrossPoolResolvedConfigV1`;
tombstone overflow lub utrata eviction proof daje `UnavailableHistory`.

T1 rozszerza istniejący bounded `CrossPoolVelocityIndex`. Drugi globalny pair
index jest zakazany. MFS przechowuje wyłącznie bounded
`CrossPoolCohortReuseEvidenceV1`, bez list par i prior-pool sets.

## 6. Assessments, V3 i durability

| Komponent | Owner/miejsce | Milestone | Dozwolone użycie | Durable? | Klasa wyniku |
| --- | --- | --- | --- | --- | --- |
| Źródłowy `EarlyBuyerSignatureFeatures` scoring bundle | odrzucony | T0 | brak | nie | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| `EarlyFlowPatternAssessmentV1` | pure function nad MFS/exact bindings | T2 | observations/reasons, no verdict | T4 only | `PR1_SCHEMA_ONLY_NO_PRODUCER` |
| Źródłowy `CoordinationFusionFeatures` scoring bundle | odrzucony | T0 | brak | nie | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| `CoordinationPatternAssessmentV1` | pure function nad MFS/exact bindings | T2 | observations/reasons, no verdict | T4 only | `PR1_SCHEMA_ONLY_NO_PRODUCER` |
| V3 statuses/confidence/reason chain | existing `gatekeeper_v3.rs` | existing/T3 hook | jedyny V3 stack | existing | `EXISTING_V3_COMPONENT_REUSE` |
| `V3ShadowDecision` | existing V3 arbiter | existing/T3 | jedyny final shadow verdict | existing | `EXISTING_V3_COMPONENT_REUSE` |
| Nowy weighted scorer/final arbiter | zakazany | nigdy | brak | nie | `DUPLICATE_OR_SEMANTIC_ALIAS_REJECTED` |
| Compact Type-5 v35 | v35 = frozen v34 + Type-5 refs | T4 po PR2C | join/audit/replay | tak | `PR2C_DURABLE_EVIDENCE_REQUIRED` |
| `type5_shadow_assessment_v1.jsonl` | bounded three-way writer | T4 | full Type-5 assessments, compact input evidence, refs do metric sidecara | tak | `PR2C_DURABLE_EVIDENCE_REQUIRED` |

T3 jest wyłącznie in-memory `ObserveOnly`: zero JSONL, zero v35 i zero wpływu
na V3. T4 jest jedynym ownerem Type-5 durability. v34 pozostaje frozen i nigdy
nie otrzymuje Type-5 pól.

```text
TYPE5_DECISION_SCHEMA_VERSION_V35 = 35
```

v35 zachowuje wszystkie pola v34 pod niezmienionymi ścieżkami i znaczeniem.
Dodaje wyłącznie compact assessment status/schema, binding ID/hash, assessment
effective-config hash, Type-5 sidecar identity/hash/schema, V3 integration mode
oraz calibration/outcome refs. Pełny Type-5 payload pozostaje w
`type5_shadow_assessment_v1.jsonl`, a pełny Metric Contracts payload w
`metric_contract_evidence_v1.jsonl`. Replay dispatchuje v33/v34/v35 osobno;
resource sizing T4 porównuje v35 z frozen v34.

## 7. Zależności milestone

| Etap | PR2A | PR2B | PR2C | PR3 | Dodatkowy gate | Znaczenie |
| --- | --- | --- | --- | --- | --- | --- |
| T0 amendment | nie | nie | nie | nie | brak | Dokumentacja, runtime blocked. |
| Pre-PR2A amendment | przed | nie | nie | nie | owner acceptance | Zatwierdza compact projection contract. |
| T1 | tak | tak | nie | nie | T0 + pre-PR2A PASS | Bindings, two projections, one missing primitive. |
| T2 | tak | tak | nie | nie | T1 PASS | Pure assessments. |
| T3 | tak | tak | nie | nie | T2 PASS | In-memory ObserveOnly hooks, no durability. |
| T4 | tak | tak | tak | nie | resource contract | v35 + sidecar/replay/writer. |
| T5A | tak | tak | tak | nie | T4 data-quality PASS | Discovery, zero V3 influence. |
| FREEZE | tak | tak | tak | nie | owner approval | Frozen calibration/outcome/holdout. |
| T5B | tak | tak | tak | nie | FREEZE PASS | Prospective CalibratedShadow only. |
| T6 | tak | tak | tak | per surface | untouched holdout PASS | Separate promotion plan. |

T1 entry jest koniunkcją: T0 accepted, pre-PR2A MFS projection amendment
accepted, PR2A PASS i PR2B PASS. T3 nie wymaga PR2C ani PR3 i dopuszcza tylko
test-only serialization fixtures. T4 wymaga T3 PASS oraz PR2C PASS i jest
bounded three-way writer/join dla `v35 decision + metric-contract evidence +
Type-5 evidence`.

T5A używa tylko danych T4, nie wpływa na V3 i nie ogląda przyszłego holdoutu.
FREEZE wymaga owner approval, version/hash/`frozen_at`, zamrożonych rules,
mappings, normalizacji, component definitions, confidence caps, outcome/cost
semantics, prospective eligibility, untouched holdout manifest i dowodu braku
użycia holdoutu w T5A. T5B używa wyłącznie rows po `frozen_at`, może wpływać
tylko na istniejący V3 shadow i jest unieważniane przez zmianę
calibration/outcome hash.

## 8. Zamknięcie T0

Każdy istotny input ma reviewable owner, exact ref/path, config-ref family,
cutoff contract i milestone. Formalna MFS projection activation pozostaje
zablokowana do pre-PR2A amendmentu.

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

Zmiana bindingu, ownera, config-ref payloadu, projection field lub primitive
semantics wymaga nowej wersji/hash i ADR. Nie wolno jej wprowadzić post hoc po
wynikach calibration/holdout.
