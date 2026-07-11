# ADR-8D: plan integracji Type-5 z Metric Contracts V1.1 i istniejącym Gatekeeperem V3

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

Klasa dokumentu: `DOCUMENTATION_ONLY`

Typ: ADR-8D / cross-cutting architecture reconciliation / plan-only

Data: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

Branch: `agent/type5-metric-contract-integration-reconciliation-t0`

Base i merge-base z `origin/main`:
`f904d5a02283126c599822c8839fdcd5ff1de901`

Plan wykonawczy:
`PLANS/DO_REALIZACJI/PLAN_TYPE5_V3_INTEGRATION_AFTER_METRIC_CONTRACTS_V1_1_20260711.md`

Normatywna macierz zależności i bindingów:
`reports/type5/type5_metric_contract_dependency_matrix_v1.md`

Dokument źródłowy zachowany bez zmian:
`PLANS/PLAN_TYPE_5_2.md`

SHA-256 dokumentu źródłowego w T0:
`5c8c90f918c63994118e0104c4cfc66ce97e27016f455848f64b2e7bfe937c21`

Poziom ryzyka: `HIGH` dla przyszłej implementacji, `LOW` dla tego
dokumentacyjnego T0. Plan przecina SSOT, authority, V3 shadow evaluation,
DecisionLogger/replay i przyszłą walidację statystyczną, lecz ten ADR nie
zmienia kodu, configu ani zachowania runtime.

Uwaga o szablonie: wskazany globalnie plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` oraz lokalny
`docs/ADR/ADR_8D_SZABLON.md` nie są częścią tego checkoutu. Dokument zachowuje
sekcyjny format ADR-8D używany w lokalnym korpusie `docs/ADR/`.

## 0. Historia amendmentu

### T0 initial reconciliation

Pierwsza wersja poprawnie zatrzymała drugi scoring stack, związała Type-5 z
MFS i istniejącym V3 oraz oddzieliła durable evidence od policy promotion.
Pozostawiła jednak zbyt mocne twierdzenie o kompletności bindingów, trzy
fingerprint primitives oznaczone jako missing, retroaktywne rozszerzenie v34 i
niepełny cohort denominator.

Historyczne, wycofane twierdzenie — `NOT_ACTIVE`:

```text
EXACT_SURFACE_BINDINGS_COMPLETE
```

Zastępuje je `EXACT_BINDINGS_REVIEWABLE`: bindingi są dokładne i reviewable,
ale formalna aktywacja compact MFS projection pozostaje zablokowana do osobnego
pre-PR2A amendmentu nadrzędnego planu Metric Contracts.

### T0 semantic amendment

Rewizja rozdzieliła existing fingerprint producers od jednego naprawdę
brakującego primitive, ustaliła exact owner/type/accessor, poprawiła cohort
coverage, zamroziła v34, przeniosła Type-5 durability do v35 i rozdzieliła T3,
T4 oraz T5A/FREEZE/T5B.

### T0 final amendment — niniejsza decyzja

Finalny amendment domyka:

- compact `MetricContractDecisionEvidenceProjectionV1` kontra pełny evidence
  set/transport;
- typed `Type5ResolvedInputEvidenceV1` bez reflection/downcastu;
- `Type5ProducerConfigRefV1` dla Metric Contracts i component producers;
- creation no-clamp i whale unbounded semantics;
- lookback/eviction/checked-arithmetic semantics CrossPoolCohortReuse;
- exact fingerprint binding w każdym wierszu dependency matrix;
- pre-PR2A parent-plan amendment jako twardy prerequisite.

Nie zmienia to Rust, TOML, MFS, loggera, replayu, Gatekeepera ani runtime.

## 1. Kontekst i problem

Źródłowy `PLAN_TYPE_5_2.md` trafnie identyfikuje potrzebę połączenia early-flow,
coordination, sybil, organic broadening i manipulation evidence. Powstał jednak
przed zamknięciem PR1 Metric Contracts V1.1 i proponuje struktury o zbyt
szerokiej odpowiedzialności:

- scoringowe feature bundles w `MaterializedFeatureSet`;
- osobny weighted scoring engine;
- nowy risk/opportunity/confidence stack;
- osobny final shadow arbiter;
- nowe producery dla sygnałów, które mają już canonical ownera;
- przykładowe progi i klasy wykonawcze bez prospective calibration.

Po PR #61 repo ma normatywne:

- 10 `MetricContractId` i 32 `MetricSurfaceId`;
- Profile A oraz jawne role Legacy/DualCompute/V2;
- `MetricAuthorityClass`;
- typed availability, measurement quality i policy actionability;
- `CanonicalHashV1`;
- `metric_contract_effective_config_hash`;
- record identity i stable event identity;
- foundation schema dla metric-contract v34 i pełnego durable evidence;
- istniejący `V3ShadowDecision`, `RiskVerdictStatus`,
  `OpportunityVerdictStatus`, `ConfidenceBreakdown` i `reason_chain`.

Uruchomienie pierwotnego planu bez reconciliation stworzyłoby równoległy język
metryk, zduplikowanych producentów i drugi scoring stack. Z kolei umieszczenie
pełnego `MetricContractsEvidenceSetV1` w każdym MFS rozdęłoby canonical snapshot,
zduplikowałoby sidecar i zmieszało decision-time projection z audit transportem.

## 2. Potwierdzony baseline i korekty semantyczne

PR #61 jest obecny w `origin/main` pod merge commitem:

```text
f904d5a02283126c599822c8839fdcd5ff1de901
```

Baseline zawiera między innymi:

```text
ghost-core/src/metric_contracts/
docs/ADR/ADR_8D_PR1_METRIC_CONTRACT_REGISTRY_PROFILE_STATUS_FOUNDATION_20260711.md
reports/metric_contracts/pr1_foundation_verification_v1.md
```

PR1 jest foundation-only. Istnienie shared schema nie dowodzi istnienia
runtime producerów, projekcji MFS ani durable emission. Sekwencja Metric
Contracts pozostaje:

```text
PR2A — parity-sensitive producers
PR2B — evidence-only producers
PR2C — frozen v34, sidecar, comparator, replay i audit
PR3  — equivalence-only cutover
```

Audyt aktywnego kodu zamknął następujące ustalenia:

1. `PoolObservationSession::materialize_features()` pozostaje canonical granicą
   budowy immutable `MaterializedFeatureSet`.
2. `EarlyFingerprintMetrics` już zawiera i oblicza
   `block0_sniped_supply_pct`, `whale_reversal_ratio_top1` oraz
   `whale_reversal_ratio_top3`.
3. Canonical runtime ownerem fingerprintów jest
   `ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg`,
   implementation type to `seer::early_fingerprint::FingerprintAggregator`,
   a canonical accessor to `TxIntelligenceEngine::fingerprint_metrics()`.
4. Creation-slot supply concentration i whale reversal nie są brakującymi
   algorytmami. Brakuje ich field-level, typed MFS projection.
5. Jedynym naprawdę brakującym Type-5 primitive jest
   `CrossPoolCohortReuse`.
6. Historia signera w cross-pool state nie jest binarnym hit/miss. Trzeba
   rozróżnić known-empty, known-nonempty, unavailable i evicted.
7. Frozen compact v34 nie ma kontraktu na retroaktywne pola Type-5; Type-5
   durable evidence wymaga osobnego v35.
8. `MetricContractsEvidenceSetV1` i transport sidecara zawierają struktury zbyt
   ciężkie dla per-decision MFS, w szczególności owner/event-level payload
   `FlipRatioEvidenceV2`.
9. Sygnały spoza dziesięciu Metric Contracts nie mają prawa podszywać się pod
   `metric_contract_effective_config_hash` ani cały `brain_config_hash`.

## 3. Decyzja architektoniczna

Przyjęto jeden przepływ:

```text
metric_contracts_v1_1
→ canonical producers
→ compact decision-time projection w MaterializedFeatureSet
→ TYPE5_INPUT_BINDINGS_V1
→ EarlyFlowPatternAssessmentV1
  + CoordinationPatternAssessmentV1
→ istniejący Gatekeeper V3
→ compact decision v35 + type5_shadow_assessment_v1.jsonl
→ T5A discovery/feasibility
→ FREEZE calibration/outcome contracts
→ T5B prospective shadow validation + untouched holdout
→ osobny T6 policy-promotion plan
```

### 3.1 `metric_contracts_v1_1` pozostaje zamknięty

T0 nie dodaje jedenastej rodziny ani trzydziestej trzeciej surface do PR1.
CPV, DBIA, SFD, organic broadening i fingerprint primitives nie są fałszywie
wciskane do rejestru naprawy dziesięciu interpretacji.

Dla Type-5 powstaje osobny, wersjonowany `TYPE5_INPUT_BINDINGS_V1`. Każdy
binding wskazuje dokładnie jeden `MetricSurfaceId` albo typed canonical MFS
input. Zakazane są:

- dynamiczny wybór pierwszej `PolicyAuthoritative` surface;
- reflection po stringowym `mfs_path`;
- aliasowanie semantycznie różnych sygnałów;
- promotion-by-binding;
- ponowne liczenie istniejącej canonical metryki.

`mfs_path` jest wyłącznie metadata/debugging stringiem. Wykonanie używa
compile-time accessora zwracającego typed evidence.

### 3.2 MFS otrzymuje wyłącznie kompaktową projekcję decision-time

`MaterializedFeatureSet.metric_contract_evidence_v1` jest nazwą planowanej
granicy logicznej:

```text
MaterializedFeatureSet.metric_contract_evidence_v1
  = PROPOSED_PENDING_METRIC_CONTRACT_PLAN_AMENDMENT
```

Nie oznacza pełnego `MetricContractsEvidenceSetV1` ani
`MetricContractEvidenceTransportV1`; nie jest ich aliasem, wrapperem ani kopią
`metric_contract_evidence_v1.jsonl`.

Przed PR2A/PR2B nadrzędny plan Metric Contracts musi dostać normatywny amendment,
który zatwierdzi kompaktowy typ:

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

Projection może zawierać wyłącznie:

- canonical decision-time values wymagane przez policy/comparator/Type-5;
- wymagane bounded counts;
- compact availability, measurement quality i actionability envelopes;
- bounded typed reason summaries;
- exact contract/surface provenance;
- authority i rollout role;
- producer/config/hash references;
- decision cutoff niezbędny do oceny braku leakage.

Pełne owner-level, event-level i audit-detail evidence pozostaje wyłącznie w:

```text
metric_contract_evidence_v1.jsonl
```

Zakazane jest kopiowanie do MFS:

- `FlipRatioEvidenceV2.owners`;
- per-owner anchors;
- qualifying sell identities;
- cumulative owner token flows;
- pełnych event lists;
- pełnych FSC transfer candidates;
- writer timestamp i rotation metadata;
- pełnego transport envelope sidecara.

Type-5 bindingi wskazują pola kompaktowej projekcji, nigdy pełny transport.
Historyczny brak pola oznacza typed `Unavailable/NotRecordedLegacySchema`, nie
measured zero. Projection i pełny sidecar muszą pochodzić z jednego frozen
producer snapshotu bez recompute. Type-5 accessors czytają wyłącznie compact
projection w MFS, nigdy sidecar.

Pre-PR2A parent-plan amendment jest twardym prerequisite i musi zatwierdzić:

- exact per-family projection fields;
- split PR2A/PR2B;
- serde oraz historical-missing semantics;
- boundedness;
- projection build/serialization budget;
- replay i hash semantics;
- dowód `one producer → one frozen snapshot → two representations`;
- zakaz reconstruction pełnego audit payloadu z projection.

Ten ADR nie wykonuje amendmentu nadrzędnego planu Metric Contracts.

### 3.3 Każdy accessor zwraca typed resolved evidence

Normatywny wynik accessora:

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
`serde_json::Value`, dynamiczny downcast i string reflection. Runtime używa
exhaustive match po `Type5InputIdV1`; `mfs_path` jest tylko
metadata/debugging stringiem. Reason list ma schema-bounded limit.

Dla Metric Contracts wynik zachowuje exact `MetricContractId`, exact
`MetricSurfaceId`, `MetricAuthorityClass`, bieżący `MetricRolloutRoleV1`,
`policy_actionable`, profile ID/hash i effective-config provenance. Dla
non-metric inputs zachowuje exact `Type5InputIdV1`, `Type5InputUseV1`,
field-level quality/reasons/config/cutoff bez fikcyjnego `MetricContractId`.

Wspólny group-level status, serde-default scalar albo legacy aggregate string
nie wystarczają do uznania konkretnego pola za evaluable. Non-clean group nie
daje field-level `Measured/Clean`, `None` nie jest zerem, a legacy zero/false
nie dowodzi pomiaru. Organic scalar wymaga `sequence_available=true` oraz
zgodnego field/group statusu. `broadening_score`, `contradiction_score` i
legacy `high_*` nie są canonical Type-5 inputs.

Pełna historyczna nazwa pola:

```text
legacy aggregate string EarlyFingerprintMetrics.fingerprint_reason
```

Assessment nie może go parsować. Brak jednoznacznej atrybucji daje typed
`ReasonAttributionUnavailable` i pozostawia pole `Degraded/Unavailable`.

### 3.4 Producer config provenance ma dwa jawne warianty

`Type5ProducerConfigRefV1` rozróżnia:

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

Pierwszy wariant jest dozwolony wyłącznie dla producentów objętych dziesięcioma
Metric Contracts. Drugi obejmuje CPV, DBIA, SFD, fingerprint, organic
broadening oraz inne canonical component producers spoza rejestru.

`ComponentResolvedConfig.hash` jest SHA-256/JCS zamkniętego typed payloadu
faktycznie resolved ustawień wpływających na producenta. Payload ma własny
`schema_version`, zamknięty zestaw pól i canonical field order. Zakazane jest
użycie:

- całego `brain_config_hash`;
- dowolnego wycinka TOML;
- runtime-defaultów nieobecnych w typed resolved payload;
- hasha niezwiązanego z wersją schema producenta.

Dostępny input bez poprawnego config ref nie jest evaluable. Pierwsze wymagane
payload families to:

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

`type5_assessment_effective_config_hash` referuje używane config refs, lecz nie
kopiuje payloadów ani nie miesza producer settings z assessment thresholds.

### 3.5 Canonical fingerprint producer jest jednoznaczny

Normatywny binding brzmi:

```text
canonical owner:
ghost-launcher::tx_intelligence::TxIntelligenceEngine::fingerprint_agg

implementation type:
seer::early_fingerprint::FingerprintAggregator

canonical accessor:
TxIntelligenceEngine::fingerprint_metrics()
```

Inne runtime-local aggregatory pozostają compatibility/logging-only i nie mogą
stać się źródłem Type-5.

### 3.6 T1 nie implementuje ponownie istniejących algorytmów

Klasyfikacja jest zamrożona:

```text
CreationSlotSupplyConcentration
  = EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED

WhaleReversal
  = EXISTING_CANONICAL_PRODUCER_MFS_PROJECTION_REQUIRED

CrossPoolCohortReuse
  = TRULY_MISSING_TYPE5_PRIMITIVE
```

Dla pierwszych dwóch T1 dodaje wyłącznie:

- MFS materialization;
- field-level availability i quality;
- exact producer provenance;
- typed per-field reasons;
- config hash reference;
- deterministic parity tests z canonical accessorem.

Nie wolno przepisać ich algorytmów.

### 3.7 Creation-slot supply ratio nie jest clampowane

Normatywny binding:

```text
Type5InputIdV1::CreationSlotSupplyConcentration
source = EarlyFingerprintMetrics.block0_sniped_supply_pct
target = alpha_fingerprint.creation_slot_sniped_supply_ratio_v1
unit = ratio 0..1
```

Historyczny suffix `pct` nie oznacza skali `0..100`. Canonical producer wylicza
`tokens_bought_in_creation_slot / supply_raw` i zwraca brak wartości dla
nieznanego creation slot albo supply. Projection zachowuje bit-for-bit parity
tylko dla wartości finite w przedziale `[0, 1]`; `None` pozostaje `None`.

Wartość non-finite lub poza `[0, 1]`:

- ma `value=null`, `availability=Unavailable` i
  `measurement_quality=NotApplicable`;
- finite poza zakresem otrzymuje `CreationSlotSupplyRatioOutOfRange`;
- non-finite otrzymuje `CreationSlotSupplyRatioNonFinite`;
- nie jest clampowana ani prezentowana Type-5 jako evaluable measurement;
- może pozostać jako raw producer value wyłącznie w audit evidence.

Naprawianie `1.07` do `1.0` jest zakazane, ponieważ ukrywa naruszenie
inwariantu i łamie parity. Zakazane są `clamp`, `min(1.0)`, saturating
conversion i error-to-zero. Non-finite diagnostic zapisuje raw IEEE-754 bits
jako integer/string field, nigdy jako nielegalny JSON float.

### 3.8 Whale reversal pozostaje istniejącą, unbounded projection

Normatywne bindingi:

```text
Type5InputIdV1::WhaleReversalTop1
source = EarlyFingerprintMetrics.whale_reversal_ratio_top1
target = alpha_fingerprint.whale_reversal_sell_to_buy_ratio_top1_v1

Type5InputIdV1::WhaleReversalTop3
source = EarlyFingerprintMetrics.whale_reversal_ratio_top3
target = alpha_fingerprint.whale_reversal_sell_to_buy_ratio_top3_v1
```

Oba pola są unbounded sell/buy ratios. Nie wolno zakładać `[0,1]`, clampować
ani traktować braku denominatora jako zero. Brak owner deltas lub denominatora
daje typed `Unavailable`; wallet-cap degradation daje co najmniej `Degraded`.
T1 kopiuje canonical output i dowodzi source-to-MFS parity, bez zmiany
`ingest()`, `finalize()` lub tworzenia nowego aggregatora.

### 3.9 CrossPoolCohortReuse ma coverage-aware denominator

`CrossPoolCohortReuse` nie jest aliasem CPV. Normatywny typ
`CohortSignerHistoryStateV1` nadaje historii każdego current successful BUY
signera na decision cutoff dokładnie jeden stan:

```text
KnownEmptyHistory
KnownNonEmptyHistory
UnavailableHistory
EvictedHistory
```

Tylko signery z `KnownEmptyHistory` albo `KnownNonEmptyHistory` są evaluable.
`KnownEmptyHistory` wymaga decision-safe lookup obejmującego pełny lookback i
cutoff. `KnownNonEmptyHistory` wymaga co najmniej jednego prior other-pool ID.
`UnavailableHistory` oznacza brak readiness, continuity, cutoff proof albo
jednoznacznego lookup result. `EvictedHistory` wymaga bounded tombstone
dowodzącego usunięcia relewantnej historii przez global/per-signer cap.

Lookup miss nigdy nie jest automatycznie `KnownEmptyHistory`. Lookback expiry
może dać known-empty wyłącznie przy continuity proof i dowodzie, że wszystkie
usunięte wpisy były poza bieżącym lookbackiem.

Denominatory i coverage brzmią:

```text
all_current_signer_pair_count = checked_choose_2(current_signer_count)
eligible_pair_count           = checked_choose_2(eligible_history_signer_count)
recurring_pair_count          = eligible pairs z niepustym przecięciem
                                prior other-pool sets
cohort_reuse_ratio            = recurring_pair_count / eligible_pair_count
pair_coverage                 = eligible_pair_count /
                                all_current_signer_pair_count
signer_history_coverage       = eligible_history_signer_count /
                                current_signer_count
```

`KnownEmptyHistory` jest realnym evaluable negative. `UnavailableHistory` i
`EvictedHistory` nie mogą udawać clean no-recurrence. Niska coverage daje typed
`Degraded` albo `Unavailable`.

Oba pair counts są liczone przez checked `choose(n, 2)`. Overflow,
przekroczenie bounded signer cap albo brak możliwości udowodnienia pełnego
denominatora daje `Unavailable`, nigdy saturating clean count. Current pool oraz
eventy po decision cutoff są wykluczone.

```text
value_u128 = u128(n) * u128(n - 1) / 2
result = checked u64 conversion
```

Zakazane są `saturating_mul`, `saturating_add` i saturating pair count. Mniej
niż dwóch current signers daje `Insufficient`; zero evaluable pairs,
overflow/conversion failure, truncated/capped current signer set albo brak
invariant proof daje `Unavailable`. Signer `EvictedHistory` jest wyłączony z
eligible denominatora i wymusza co najmniej `Degraded`. Clean wymaga
`pair_coverage == 1`, pełnego cutoff proof i braku gap/eviction.

T1 rozszerza istniejący bounded `CrossPoolVelocityIndex`; drugi globalny pair
index jest zakazany. Tombstones mają TTL/cap/overflow behavior zapisane w
`CrossPoolResolvedConfigV1`; tombstone overflow albo utrata eviction proof daje
`UnavailableHistory`. MFS zachowuje tylko bounded
`CrossPoolCohortReuseEvidenceV1`, nigdy pełną listę par ani prior-pool sets.

### 3.10 MFS, assessment i policy pozostają oddzielne

Przyjęto bez wyjątku:

```text
MFS        = canonical measurements, evidence i provenance
assessment = deterministyczna interpretacja immutable MFS
policy     = decyzja istniejącego Gatekeepera V3
```

T2 tworzy:

```text
EarlyFlowPatternAssessmentV1
CoordinationPatternAssessmentV1
```

Oba assessmenty:

- konsumują wyłącznie `&MaterializedFeatureSet`;
- rozwiązują inputy przez exact typed bindings;
- nie czytają raw tx, session state, RPC, rolling indexes ani wall clock;
- nie recompute'ują canonical metrics;
- propagują availability/quality/provenance;
- nie zwracają osobnego final verdictu ani final arbitra;
- przed calibration freeze mają `UNFROZEN_PENDING_CALIBRATION`.

Do MFS nie mogą wejść sniping score, weighted score, final severity,
threshold-dependent taxonomy, policy-profile-dependent class, final confidence,
Type-5 verdict ani coordination penalty.

### 3.11 Istniejący Gatekeeper V3 pozostaje jedynym arbitrem

T3 może addytywnie rozszerzyć istniejący V3 o in-memory assessment context, ale
nie tworzy nowych odpowiedników:

- `V3ShadowDecision`;
- `RiskVerdictStatus`;
- `OpportunityVerdictStatus`;
- `ConfidenceBreakdown`;
- `reason_chain`;
- final shadow arbiter.

Przed calibration freeze obowiązuje:

```text
HOOKS_WITHOUT_PRECALIBRATION_INFLUENCE
```

T3 ma udowodnić bit-for-bit parity wszystkich istniejących pól V3 i zapewnia:

- wyłącznie in-memory context i hooks;
- parity tests;
- zero durable Type-5 emission;
- zero wpływu Type-5 na risk, opportunity, confidence, reason chain i verdict;
- zero wpływu na aktywny Gatekeeper V2/V2.5.

### 3.12 Frozen v34 pozostaje bez zmian, Type-5 używa v35

Metric-contract burn-in zachowuje frozen compact v34. Type-5 nie dodaje do v34
żadnych pól ani optional extensions po zamrożeniu schema.

T4 wprowadza osobne:

```text
TYPE5_DECISION_SCHEMA_VERSION_V35 = 35

decision schema v35
  = frozen v34 fields at unchanged paths
  + compact Type-5 status/ref/hash summary

type5_shadow_assessment_v1.jsonl
  = pełne pattern assessments, component breakdown,
    exact input IDs/surfaces, authority/use classes, quality,
    reason chain, profile/config/calibration/outcome refs
```

Pełny payload Type-5 nie trafia do decision row. v35, metric-contract sidecar i
Type-5 sidecar są łączone przez record identity:

```text
(run_id, join_key, decision_plane)
```

`stable_event_identity`, jeśli dostępne, służy do detekcji cross-run collision,
nie zastępuje record identity. Hash/ref mismatch, orphan, truncated row albo
paired-writer failure dyskwalifikuje run.

Replay dispatchuje v33, v34 i v35 osobno. Historyczny v34 nigdy nie oczekuje
Type-5 fields, a resource sizing T4 porównuje v35 z frozen v34.

### 3.13 Type-5 config hash referuje producer hashes, lecz ich nie dubluje

`type5_assessment_effective_config_hash` obejmuje wyłącznie:

- assessment schema;
- Type-5 bindings ID/hash;
- pattern-rule definitions;
- taxonomy mapping;
- normalizacje;
- risk/opportunity component definitions;
- confidence-cap definitions;
- calibration contract version/hash albo jawne `null`;
- referencje do odpowiednich `Type5ProducerConfigRefV1`;
- referencję do Metric Contracts profile ID/hash i
  `metric_contract_effective_config_hash`.

Nie kopiuje producer windows, dedupe, dust, retention ani całych configów. Nie
obejmuje run/writer metadata ani własnego digestu.

### 3.14 Kalibracja ma sekwencję T5A → FREEZE → T5B

Definition parameters pozostają oddzielone od policy/calibration parameters.
Wszystkie progi, sample minima, taxonomy cutoffs, contributions, weights,
severity bands, confidence caps i hard-fail mappings mają status:

```text
UNFROZEN_PENDING_CALIBRATION
```

Etapy są rozdzielone:

```text
T5A — discovery / feasibility / contract proposal
      używa wyłącznie T4 evidence;
      nie wpływa na V3;
      wykonuje robustness/regime analysis;
      przygotowuje calibration i outcome contracts;
      nie używa przyszłego holdoutu.

FREEZE — owner approval;
         version/hash/frozen_at;
         frozen rules/mappings/normalizacje/component definitions/caps;
         frozen outcome i execution-cost semantics;
         prospective eligibility;
         untouched holdout manifest;
         dowód, że holdout nie był użyty w T5A.

T5B — prospective shadow validation;
      używa wyłącznie rows po frozen_at;
      może zasilać V3 shadow według frozen mappingu;
      nadal nie wpływa na aktywny Gatekeeper V2/V2.5;
      zmiana calibration/outcome hash unieważnia T5B.
```

`TYPE5_CALIBRATION_CONTRACT_V1` i `TYPE5_OUTCOME_CONTRACT_V1` są osobnymi,
wersjonowanymi kontraktami. Outcome contract zamraża entry cutoff, notional,
exit policy, execution evidence grade, fees/impact assumptions, tail-risk
metrics i session/time split. T0 nie ustala ich wartości i zabrania domyślnej
walidacji na arbitralnym final PnL, mark-only MFE, last price albo synthetic
quote bez jawnych kosztów.

Zmiana po freeze unieważnia walidację i wymaga nowej wersji oraz nowych
prospective runs. Post-hoc zmiana gate po zobaczeniu wyniku jest zakazana.

## 4. Sekwencja wdrożenia i zależności

Przyjęto następujący DAG:

```text
PR1 MERGED
   │
   ▼
pre-PR2A Metric Contracts plan amendment
   │  definiuje compact MetricContractDecisionEvidenceProjectionV1
   │  i MFS resource budget
   ▼
PR2A parity-sensitive producers/projection
   │
   ▼
PR2B evidence-only producers/projection
   ├────────────────► PR2C frozen v34 + durable MC evidence ───────┐
   │                                                               │
   ▼                                                               │
T1 existing fingerprint projections                               │
   + exact bindings                                                │
   + one new CrossPoolCohortReuse primitive                        │
   │                                                               │
   ▼                                                               │
T2 pure assessments                                                │
   │                                                               │
   ▼                                                               │
T3 in-memory V3 hooks, no emission, zero influence                 │
   └───────────────────────────────────────────────────────────────┤
                                                                   ▼
                                                        T4 v35 durable shadow
                                                                   │
                                                                   ▼
                                                        T5A discovery/proposal
                                                                   │
                                                                   ▼
                                                                 FREEZE
                                                                   │
                                                                   ▼
                                                        T5B prospective shadow
                                                                   │
                                                                   ▼
                                                     T6 policy-promotion plan
```

PR3 biegnie osobno po PR2C i metric-contract burn-in. Nie jest warunkiem T1,
T2, T3, T4 ani T5, jeśli nie dostarcza wymaganych danych. T6 ustala per
surface, czy equivalence-only PR3 wystarcza, czy potrzebny jest odrębny plan
promocji semantycznie nowego Counterfactual/EvidenceOnly signal.

T0 nie autoryzuje T1. Bez zaakceptowanego pre-PR2A amendmentu i wymaganych PASS
PR2A/PR2B runtime implementation pozostaje zablokowana.

Entry T1 jest koniunkcją: T0 accepted, pre-PR2A MFS projection amendment
accepted, PR2A PASS i PR2B PASS. T1 zapewnia source-to-MFS parity dla dwóch
istniejących fingerprint projections, dokładnie jeden truly missing primitive,
typed resolver/config refs i zero wpływu na V2/V2.5/V3.

T3 nie wymaga PR2C ani PR3: działa tylko in-memory `ObserveOnly`, dopuszcza
test-only serialization fixtures, nie emituje JSONL ani durable decision fields
i zachowuje pełną V3 parity. T4 wymaga T3 PASS i PR2C PASS; dopiero ono tworzy
v35, Type-5 sidecar oraz bounded three-way writer/join dla `v35 decision +
metric-contract evidence + Type-5 evidence`. T4 nadal pozostaje `ObserveOnly`.

## 5. Świadome non-decisions i blokady

T0 nie ustala i nie autoryzuje:

- progów, wag ani sample minima;
- confidence caps ani hard-fail classes;
- mappingu assessment → V3 contribution;
- aktywnej FSC v2;
- dev-primary authority;
- corrected FTDI actionability;
- flip V2 authority;
- zmiany Gatekeepera V2/V2.5;
- aktywnego BUY/REJECT/TIMEOUT;
- implementacji detectora/scoringu;
- implementation paths T1–T5;
- policy promotion.

Obowiązują:

```text
PRE_PR2A_MFS_PROJECTION_AMENDMENT_REQUIRED
TYPE5_RUNTIME_IMPLEMENTATION_BLOCKED
TYPE5_POLICY_PROMOTION_BLOCKED
```

## 6. Rozważone i odrzucone alternatywy

### 6.1 Pełny Metric Contracts evidence set lub transport w MFS

Odrzucone. Rozdęłoby MFS, zduplikowało sidecar, zwiększyło clone/serialization
cost i wprowadziło nieograniczone owner/event detail do decision snapshotu.

### 6.2 Clampowanie creation-slot ratio

Odrzucone. Clamp ukrywa naruszenie semantycznego inwariantu i łamie
bit-for-bit parity z canonical producerem.

### 6.3 Cały `brain_config_hash` jako provenance component producerów

Odrzucone. Jest za szeroki, nie dowodzi resolved ustawień konkretnego
producenta i może zmieniać się z powodów niezwiązanych z Type-5 inputem.

### 6.4 Surowe string paths albo dynamiczny first-authoritative lookup

Odrzucone. Uniemożliwiają compile-time guards i mogą cicho zmienić semantykę po
rolloucie lub zmianie ścieżki.

### 6.5 Nowe producery creation-slot, whale reversal lub innych istniejących metryk

Odrzucone. Naruszałyby SSOT i tworzyły drift między runtime, replay oraz
assessmentem.

### 6.6 Wszystkie current signer pairs jako cohort denominator

Odrzucone. Miesza unavailable/evicted history z realnym known-empty negative i
systematycznie zaniża recurrence przy partial coverage.

### 6.7 Saturating pair counts

Odrzucone. Saturation może stworzyć pozornie clean denominator. Overflow lub
bounded-cap violation musi zdegradować albo unieważnić evidence.

### 6.8 Rozszerzenie frozen v34 o Type-5

Odrzucone. Retroaktywna zmiana unieważniłaby metric-contract burn-in i replay
parity. Type-5 używa osobnego v35.

### 6.9 Scoring bundles w MFS i drugi final stack

Odrzucone. Score, severity, taxonomy, confidence i verdict należą do
assessment/policy, a istniejący V3 pozostaje jedynym arbitrem.

### 6.10 Pełny Type-5 payload w decision row

Odrzucone. Compact v35 refs + hashed sidecar zachowują replayability bez
obciążania głównego rekordu pełnym payloadem.

### 6.11 Ustalenie progów lub zmiana gate po zobaczeniu wyniku

Odrzucone. Parametry policy pozostają `UNFROZEN_PENDING_CALIBRATION`, a
walidacja po FREEZE jest prospective i używa untouched holdout.

## 7. Konsekwencje i ryzyka

Pozytywne konsekwencje:

- jeden canonical measurement snapshot;
- surface-qualified provenance;
- kompaktowa, budżetowana projection zamiast pełnego transportu w MFS;
- brak duplikacji producentów;
- jednoznaczne rozdzielenie authority od shadow use;
- ponowne użycie istniejącego V3;
- deterministyczne assessmenty;
- zamrożone v34 i jawnie wersjonowane v35;
- prospective calibration z untouched holdout;
- osobna bramka policy promotion.

Koszty i ryzyka przyszłej implementacji:

- pre-PR2A amendment musi zamrozić exact projection schema i resource budget;
- PR2A/PR2B muszą projektować evidence w MFS bez podwójnego compute;
- T1 wymaga per-field statusów dla legacy default/aggregate surfaces;
- CrossPoolCohortReuse wymaga bounded rolling state i jawnego eviction/gap
  status;
- T3 musi udowodnić bit-for-bit parity bez durable emission;
- T4 wymaga v35, paired-writer integrity, replay i resource sizing;
- zmiana bindingu, producer config, assessment config albo calibration hash
  unieważnia porównywalność runów;
- brak execution-grade outcome contractu blokuje wnioski o edge.

Każde ryzyko ma fail-closed milestone. Nie może być rozwiązane przez zgadywanie
progów, ukryty fallback ani wcześniejszą promocję.

## 8. Pliki T0

Pełny T0 documentation chain obejmuje dokładnie trzy dokumenty:

- `PLANS/DO_REALIZACJI/PLAN_TYPE5_V3_INTEGRATION_AFTER_METRIC_CONTRACTS_V1_1_20260711.md`;
- `reports/type5/type5_metric_contract_dependency_matrix_v1.md`;
- `docs/ADR/ADR_8D_PLAN_TYPE5_V3_INTEGRATION_AFTER_METRIC_CONTRACTS_V1_1_20260711.md`.

Dokument źródłowy `PLANS/PLAN_TYPE_5_2.md` pozostaje bez zmian. T0 nie zmienia
Rust, TOML, MFS, DecisionLoggera, replayu, runtime, configu ani Gatekeepera.

## 9. Weryfikacja i rollback

Weryfikacja T0 obejmuje:

1. obecność PR #61 w `origin/main` i zgodność base/merge-base;
2. SHA-256 źródłowego planu przed i po pracy;
3. exact owner/surface/path/milestone matrix;
4. literalne role Profile A i brak dynamic authority lookup;
5. compact projection zamiast pełnego evidence transportu w MFS;
6. exact fingerprint owner/type/accessor;
7. dokładnie dwa existing-producer projections i jeden truly missing primitive;
8. no-clamp creation-slot invariant;
9. coverage-aware, checked cohort denominator;
10. typed resolved input i producer config refs;
11. brak drugiego scoring stacku;
12. istniejący V3 jako jedyny arbiter;
13. T3 in-memory i T4 durable boundary;
14. frozen v34 i osobne v35;
15. T5A → FREEZE → T5B bez cyklu;
16. brak wykonawczych przykładowych progów;
17. brak zmian poza trzema allowlist documents;
18. whitespace i Markdown sanity.

Ponieważ zmiana jest dokumentacyjna, testy Rust nie są dowodem tego T0.
Obowiązują statyczne kontrole zakresu, spójności i provenance.

Rollback polega na wycofaniu wyłącznie trzech dokumentów z sekcji 8. Nie wymaga
migracji configu, logów, schema ani stanu runtime.

## 10. Acceptance markers

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

Marker `EXACT_BINDINGS_REVIEWABLE` oznacza, że każdy istotny input ma jawnego
ownera, exact surface albo typed MFS input i milestone w macierzy. Nie jest to
approval implementacji ani policy promotion.

## 11. Handoff

Kolejność następnych decyzji:

1. przyjąć T0 semantic amendment;
2. wprowadzić i zatwierdzić pre-PR2A Metric Contracts plan amendment dla
   `MetricContractDecisionEvidenceProjectionV1` oraz resource budget;
3. ukończyć i osobno zatwierdzić PR2A;
4. ukończyć i osobno zatwierdzić PR2B;
5. dopiero wtedy rozważyć osobno ograniczony T1;
6. wykonać T2 i T3 z zerowym pre-calibration influence i zerową durable emisją
   w T3;
7. po PR2C wykonać T4 jako v35 + paired Type-5 sidecar;
8. wykonać T5A, owner-approved FREEZE i dopiero T5B na prospective rows;
9. użyć untouched holdout;
10. przygotować osobny T6 policy-promotion plan.

Następny właściwy etap programu pozostaje:

```text
pre-PR2A projection amendment → PR2A → review → PR2B
```

Nie rozpoczyna się jeszcze Type-5 T1.

```yaml
delegation_trace:
  task_classification: cross-cutting documentation-only Type-5 architecture reconciliation
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - SSOT Feature Materialization Guardian
    - Gatekeeper Policy Auditor
    - Decision Logging Replay Analyst
    - Config Rollout Safety Reviewer
    - Statistical Research Engine
  specialist_docs_loaded:
    - docs/agents/ghost-runtime-coordinator.md
    - docs/agents/ssot-feature-materialization-guardian.md
    - docs/agents/gatekeeper-policy-auditor.md
    - docs/agents/decision-logging-replay-analyst.md
  specialist_docs_not_loaded:
    - name: Config Rollout Safety Reviewer
      reason: T0 nie zmienia TOML ani defaults; definiuje jedynie przyszłe typed config refs
    - name: Oracle Session Runtime Engineer
      reason: runtime implementation pozostaje jawnie zablokowana
    - name: Seer Ingest Event Integrity Specialist
      reason: T0 nie zmienia ingest identity ani event ordering
    - name: Solana Execution Path Engineer
      reason: execution, submit, confirmation i shadow/live execution są poza zakresem
  skills_used:
    - ghost-execution
    - abstract-reasoning
  fast_path_used: false
  contracts_checked:
    - MaterializedFeatureSet SSOT
    - compact decision-time projection versus full audit transport
    - canonical materialization boundary
    - exact MetricSurfaceId and typed MFS input binding
    - Profile A authority and rollout roles
    - no duplicate metric producer
    - exact fingerprint owner, implementation type and accessor
    - field-level quality and no-clamp semantic invariant
    - coverage-aware checked cohort denominator
    - existing V3 risk, opportunity, confidence, reason and final arbiter reuse
    - frozen v34 versus separate Type-5 v35
    - T3 in-memory versus T4 durable evidence
    - CanonicalHashV1 and typed producer config references
    - record identity versus stable event identity
    - T5A, FREEZE, T5B and untouched holdout
    - active versus shadow versus evidence-only separation
    - source plan and unrelated worktree preservation
  unresolved_routing_uncertainty: []
```
