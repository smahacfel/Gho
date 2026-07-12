# ADR-8D: PR2A parity-sensitive producers kontraktów metryk

Status: `IMPLEMENTED / READY_FOR_RE_REVIEW / METRIC_CONTRACTS_V1_1_PARITY_SENSITIVE_PRODUCERS_READY / PR2A_REVIEW_BLOCKERS_CLOSED`

Typ: ADR-8D / cross-cutting producer, evidence i SSOT contract

Data: 2026-07-12

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2a-producers`

Base i merge-base z `origin/main`:
`a695efb2e6d3884b9a1a7fc4ff86106f0af3ff64`

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Foundation:
`docs/ADR/ADR_8D_PR1_METRIC_CONTRACT_REGISTRY_PROFILE_STATUS_FOUNDATION_20260711.md`

Raport dowodowy:
`reports/metric_contracts/pr2a_parity_sensitive_producers_verification_v1.md`

Poziom ryzyka: `HIGH` — zmiana przecina producentów TxIntelligence, sybil/FSC,
kompaktowe shared schema, startup-resolved config i materializowany status FSC.
Ryzyko policy jest ograniczone przez Legacy-only startup, brak root projection w
`MaterializedFeatureSet`, brak zmian Gatekeeper policy oraz source-hash/static
guards dla loggera i replayu.

## 1. Kontekst

PR1 ustanowił registry, Profile A, authority classes, status taxonomy,
`CanonicalHashV1` i zamknięty effective-config vocabulary. Nie wiązał jednak
tych kontraktów z rzeczywistymi producer settings ani z frozen outputs
aktywnych metryk. PR2A dostarcza wyłącznie rodziny wrażliwe na parity:

```text
FTDI
dev buy
same-ms / timing
top3 signer volume
legacy FSC + FSC readiness/status
```

Zasada nadrzędna:

```text
canonical producer state
→ jeden frozen producer snapshot
→ typed full-family evidence
→ pure compact family projection
```

PR2A nie materializuje jeszcze kompletnego root projection w MFS. Brak czterech
rodzin PR2B nie jest uzupełniany placeholderami.

## 2. Decyzja

### 2.1 Common compact projection foundation

W `ghost-core::metric_contracts` zdefiniowano zamknięty kontrakt:

```text
MetricContractDecisionEvidenceProjectionV1
MetricDecisionEnvelopeV1
MetricDecisionSurfaceValueV1<T>
MetricDecisionFieldValueV1<T>
MetricDecisionRatioV1
MetricContractProducerIdV1
MetricContractDecisionSourceCutoffV1
MetricDecisionReasonSummaryV1
```

Root ma wszystkie dziesięć required family keys i `deny_unknown_fields`, ale
PR2A implementuje buildery wyłącznie dla:

- `FtdiDecisionProjectionV1`;
- `DevBuyDecisionProjectionV1`;
- `TxTimingDecisionProjectionV1`;
- `Top3DecisionProjectionV1`;
- `FundingDecisionProjectionV1`;
- `FscStatusDecisionProjectionV1`.

Typy slotów PR2B są schema-only. Nie mają builderów i nie są tworzone w
runtime. `MaterializedFeatureSet` celowo nie ma jeszcze pola
`metric_contract_decision_projection_v1`.

Każda compact surface zachowuje exact `MetricSurfaceId`, authority class,
rollout role dla bieżącego profilu, availability, measurement quality,
actionability, producer ID/schema, source cutoff oraz ograniczony summary
powodów. Limit wynosi osiem unikalnych reason codes; nadmiar ma jawny
`omitted_count`.

Projection hash jest deterministycznym `CanonicalHashV1` pełnego semantic root.
Publiczny kontrakt projection udostępnia wyłącznie
`validated_canonical_hash(context)`: przed serializacją do canonical JSON i
SHA-256 wykonuje pełne `validate_context()`, w tym walidację status/value oraz
semantykę rodzin PR2A związaną z tym samym
`ResolvedMetricContractEffectiveConfigV1`, którego hash znajduje się w root.
Usunięto wcześniejszą metodę, która przed hashowaniem sprawdzała tylko wersję
schema. Projection nie zawiera self-hash i nie zastępuje przyszłego SHA pełnego
sidecara.

### 2.2 Frozen inputs i zakaz recompute

`Pr2aFrozenProducerInputsV1` oraz per-family buildery przyjmują wyłącznie
gotowe wyniki producentów. Nie odpytują live state, `GatekeeperBuffer`, raw
eventów ani indeksów. Builder:

- waliduje exact profile/mode/effective-config hash;
- porównuje frozen producer settings z resolved config;
- sprawdza count/value/selector parity;
- mapuje typed quality i reasons;
- nie wywołuje producenta ponownie.

Test call-count buduje producer output raz i konwertuje go wielokrotnie bez
dodatkowego wywołania producenta.

### 2.3 Fail-closed validation compact projection

Każdy publiczny `MetricDecisionFieldValueV1<T>` i
`MetricDecisionSurfaceValueV1<T>` egzekwuje ten sam kontrakt koherencji:

```text
Available     => Value + quality inna niż NotApplicable
non-Available => Null + NotApplicable
```

`non-Available` obejmuje `Unavailable`, `NotConfigured` i
`NotRecordedLegacySchema`. Tym samym żaden deserializowany albo ręcznie
złożony compact payload nie może przejść jako `Available + Null`,
`Unavailable + Value` albo `Available + NotApplicable`.

Root `validate_context()` uruchamia następnie jawne walidatory rodzin PR2A:

- FTDI: value/count/parity, population/first-sample/denominator config oraz
  oddzielne legacy i corrected actionability;
- dev-buy: first-observed/effective parity oraz anchor/eligibility/selection
  config;
- timing: count/ratio i population/denominator/window config;
- top3: preferred/fallback/effective oraz field/alias/scale config;
- funding: legacy FSC formula i minimum pobrane z effective-config, counts, v2
  coverage i authority isolation;
- FSC status: cross-check legacy presence oraz v2 readiness z funding family i
  status mappings z effective-config.

Rozjazd tych semantyk ma dedykowany błąd `EffectiveConfigParity`. Klucze
producer-only, których wartości nie mają reprezentacji w compact payload
(np. dust, dedupe i capacity), są bezwarunkowo sprawdzane na frozen producer
boundary przed utworzeniem full evidence; test zmienia taki config, przebudowuje
jego hash i dowodzi odrzucenia przez `ProducerConfigMismatch`.

Invarianty rodzin PR2B pozostają ich gate'em implementacyjnym, natomiast
generic status/value coherence już obejmuje także ich publiczne field/surface
typy.

## 3. Producer ownership i authority

| Rodzina / surface | Canonical owner | Authority w Legacy | Decyzja PR2A |
| --- | --- | --- | --- |
| `TxIntelFeeTopologyDiversityLegacy` | `compute_ftdi_from_buys` | Authoritative | wartość i legacy actionability bez zmiany |
| `FtdiValueEvidenceV1` | ten sam FTDI snapshot | EquivalentCutover / inactive | bit-for-bit typed representation |
| `FtdiUniqueBuyerActionabilityV2` | ten sam FTDI snapshot | Counterfactual | zero policy influence |
| `CoordinationFeeTopologyHhiExportV1` | ten sam FTDI snapshot | ExportOnly | full evidence only |
| `TxIntelDevFirstObservedBuySol` | `TxIntelligenceEngine` | Authoritative | legacy first accepted, non-dust creator BUY |
| `GatekeeperBufferDevPrimaryBuySol` | istniejący `GatekeeperBuffer` | Compatibility | osobny compatibility snapshot |
| `MfsDevFirstObservedBuySol` | TxIntel snapshot projection | Authoritative | parity z aktywnym `dev_buy_sol` |
| `MfsDevPrimaryBuySolV1` | TxIntel primary candidate state | Counterfactual | successful, non-dust, stable-key selection |
| `EffectivePolicyDevBuySol` | legacy TxIntel first-observed | Authoritative | aktywne Phase 5 bez zmiany |
| `TxIntelSameMsCollisionRatioExact` | `TxIntelligenceEngine` | Authoritative | exact `delta == 0`, legacy denominator |
| `TxTimingExactSameMsEvidenceV1` | ten sam timing snapshot | EquivalentCutover / inactive | bit-for-bit parity |
| `TxIntelBundleClusterRatioLt50Ms` | ten sam timing state | EvidenceOnly | jawnie oddzielone od exact |
| `RceSameMsCollisionRatioRecentExact` | istniejący RCE window producer | LoggingOnly | successful recent 10 s, bounded provenance |
| `TxIntelTop3SignerVolumeRatioPreferred` | `TxIntelFeatures` | Authoritative | preferred ratio 0..1 |
| `TxIntelTop3VolumePctCompatibilityAlias` | istniejący alias | Compatibility | fallback only |
| `TxIntelTop3EffectiveSelector` | istniejący helper | Authoritative | jeden selector, mismatch telemetry |
| `TxIntelFundingSourceConcentrationLegacy` | `FundingSourceIndex` | Authoritative | istniejąca formuła bez zmiany |
| `FundingSourceConcentrationLegacyEvidenceV1` | ten sam FSC computation | EquivalentCutover / inactive | typed value/count parity |
| `FundingSourceV2ReadinessEvidence` | istniejący FSC v2 computation | EvidenceOnly | brak policy promotion |
| `MaterializedFscStatusCompatibility` | materialization status adapter | Compatibility | legacy, v2 i alias rozdzielone |
| `CoordinationFundingSourceHhiExportV1` | FSC v2 computation | ExportOnly | full evidence only |

## 4. Kontrakty rodzin

### 4.1 FTDI

Legacy runtime pozostaje:

```text
population = successful BUY
sample = first BUY per unique signer
value = unique_topologies / unique_first_signer_samples
legacy clean/actionable gate = buy_transaction_sample_count >= 3
```

PR2A dodaje jawne counts, wartość typed, osobny corrected gate
`unique_buyer_sample_count >= 3` i coordination HHI. Corrected gate jest
`Counterfactual`, HHI jest `ExportOnly`; żaden nie trafia do policy.

### 4.2 Dev buy

Rozdzielono fizyczne powierzchnie, aby podobna nazwa nie łączyła różnych
semantyk:

- TxIntel first-observed zachowuje aktywną semantykę, w tym accepted failed;
- GatekeeperBuffer primary jest compatibility evidence;
- TxIntel primary V1 wymaga successful, non-dust i stable `TxKey`;
- create-signature match ma pierwszeństwo, fallback wybiera najwcześniejszy
  eligible creator BUY po stabilnym kluczu;
- candidate history jest ograniczona przez istniejącą capacity; truncation
  daje fail-closed, nie niepełny measured value.

Late creator identity przekazuje również create signature do TxIntelligence.
Aktywny effective-policy surface nadal kopiuje first-observed. Dev-primary nie
wpływa na Phase 5, Sybil, soft points ani verdict.

### 4.3 Same-ms i timing

Zamrożono trzy różne populacje:

```text
legacy/exact V1: accepted non-dust transactions, full observation
cluster:          accepted non-dust transactions, adjacent delta < 50 ms
recent exact:     successful transactions, recent effective-config RCE window
```

Exact numerator pozostaje liczbą dodatkowych zdarzeń w tym samym ms, a legacy
denominator pozostaje transaction count. Dedupe, dust, source capacity,
timestamp fallback, missing stable ordering identity i truncation mają jawne
provenance/quality. Aktywny threshold nadal czyta exact `same_ms_tx_ratio`.

Recent exact używa domkniętego przedziału czasu. `start_ts_ms == end_ts_ms`
jest prawidłowym, ewaluowalnym oknem: jeden successful tx daje `0/1`, dwa
`1/2`, a trzy `2/3`. Tylko `start_ts_ms > end_ts_ms` zwraca pusty wynik.
Failed tx nie wchodzi do successful denominatora. Surface pozostaje
`LoggingOnly` i jest objęty statycznym zakazem aktywnego policy read.
Compact validator nie ma fallbacku do 10 s: pobiera
`SameMsRecentWindowMs` z przekazanego effective-config, wykonuje checked
conversion `u64 -> u32` i wymaga dokładnej zgodności z `recent_exact.window_ms`.
Bieżący runtime resolver nadal materializuje 10 000 ms.

### 4.4 Top3

Nie dodano drugiego helpera. Producer snapshot pobiera:

```text
preferred = top3_signer_volume_ratio
alias = top3_volume_pct
effective = preferred.or(alias)
unit = ratio 0..1
```

Builder sprawdza bit-for-bit selector parity i spójność flag fallback/mismatch.
Telemetry rozróżnia preferred, compatibility fallback i mismatch. Static guard
blokuje nowe aktywne bezpośrednie odczyty aliasu.

### 4.5 Legacy FSC i FSC status

Legacy FSC pozostaje:

```text
1 - distinct_known_sources / known_source_samples
```

Minimum known samples jest częścią effective-config hash i jest jedynym źródłem
granicy compact validation. Poniżej `FscLegacyMinKnownSourceSamples` legacy FSC
musi być null i non-available; od minimum wzwyż musi mieć dokładną wartość
formuły. Porównanie wykonywane jest jako `u64`, bez truncation ani saturating
conversion. Bieżący runtime resolver nadal materializuje minimum 2.

`MaterializedEvidenceStatus` ma addytywne:

```text
fsc          compatibility legacy alias
fsc_legacy   legacy scalar status
fsc_v2       actual FSC v2 readiness/coverage status
```

Historyczne rekordy bez nowych pól deserializują je przez `serde(default)` do
fail-closed unavailable. `GatekeeperV3EvidenceRequirements.fsc_v2` ma default
`false`. FSC v2 nie jest promowane do policy.

## 5. Effective config

Launcher buduje `ResolvedMetricContractEffectiveConfigV1` z rzeczywistych,
resolved ustawień:

- Gatekeeper V2;
- TxIntelligence;
- defaultowego, faktycznie używanego `EarlyFingerprintConfig`;
- FundingSource/FSC v2;
- skompilowanych semantyk FTDI, timing i FSC legacy.

PR2A uzupełnia vocabulary między innymi o dust/dedupe/capacity dla timing,
capacity dev-primary oraz minimum znanych próbek legacy FSC. Resolver odrzuca
missing/duplicate/wrong-kind/non-finite/profile mismatch i rozjazd ustawień
Gatekeeper–TxIntelligence. Wynik jest przechowywany w `OracleRuntimeConfig` dla
PR2B, lecz nie jest jeszcze materializowany.

Launcher nadal failuje dla trybu innego niż Legacy. PR2A nie aktywuje
`DualCompute` ani `V2`.

## 6. Compatibility i niezmienniki

- Stary TOML pozostaje Legacy + Profile A.
- Nowe FSC v2 requirement ma `serde(default = false)`.
- `MaterializedFeatureSet` nie ma root projection; historyczne MFS są zgodne.
- DecisionLogger pozostaje v33.
- V3 replay payload pozostaje schema v1.
- Gatekeeper V2/V2.5 policy source jest byte-identical z base.
- V3 evaluator source jest byte-identical z base.
- Brak zmian execution, Jito, sender, IWIM, post-buy i shadow/live.
- Brak Type-5 runtime.

### 6.1 Amendment zamykający blokery review

Review ujawniło trzy luki, które nie zmieniały aktywnej authority, ale
pozwalały utworzyć niepoprawny dowód compact:

1. `MetricDecisionFieldValueV1<T>` sprawdzał wyłącznie reason bounds, a
   `MetricDecisionSurfaceValueV1<T>` wyłącznie envelope/provenance. Root nie
   weryfikował wszystkich zależności między polami rodzin, a stary hash path
   akceptował dowolny root z poprawną `schema_version`.
2. RCE `rce_window_stats()` traktował `start_ts_ms == end_ts_ms` jak pusty
   przedział, mimo że domknięte okno mogło zawierać wiele successful tx w tej
   samej milisekundzie.
3. Po pierwszym amendmentcie root wiązał się z poprawnym hashem effective-config,
   lecz timing window i legacy FSC minimum były nadal walidowane względem
   lokalnych stałych. Inne semantyczne klucze population/denominator,
   anchor/selection, top3 mapping/scale i FSC formula również nie były jawnie
   cross-checkowane w compact family validators.

Poprawki wprowadzają fail-closed generic coherence, context-aware family
semantic validators, validated-only projection hash oraz zmieniają pusty-window
guard z `start >= end` na `start > end`. Ostatni amendment usuwa z core
projection lokalne `10_000` i `2`, a timing/FSC pobierają wartości wyłącznie z
tego samego contextu. Testy A/B przebudowują i ponownie hashują config przed
wykazaniem błędu family/config; test C dowodzi, że spójne `9_999/9_999`
przechodzi. Regresje recent exact nadal obejmują 1/2/3 successful tx w tym samym
timestampie, failed exclusion oraz odwrócony przedział.

## 7. Odrzucone warianty

1. Częściowy root MFS z placeholderami PR2B — odrzucony, bo udawałby kompletne
   evidence.
2. Nowy top3 selector — odrzucony, bo tworzyłby drugą authority.
3. Dev-primary jako aktywny read — odrzucony, bo jest semantycznie
   counterfactual.
4. FSC v2 jako zamiennik legacy FSC/status — odrzucony, bo wymaga osobnej
   policy promotion.
5. Projection builder czytający raw session state — odrzucony przez SSOT i
   replay determinism.
6. Naprawa znanego selector testu — odrzucona jako unrelated scope.

## 8. Znane ograniczenia i następny gate

- PR2A nie emituje pełnego `MetricContractsEvidenceSetV1` ani compact root do
  MFS.
- Full sidecar, v34, replay/comparator/audit CLI należą do PR2C.
- Projection resource gate jest wykonywany dopiero na kompletnym root w PR2B.
- Corrected FTDI, dev-primary i FSC v2 pozostają nieactionable.
- Znany selector test pozostaje baseline failure przy identycznym blobie
  `decision_logger.rs`.

Następny dopuszczalny milestone po akceptacji PR2A:

```text
PR2B — evidence-only producers + atomowa kompletna materializacja root MFS
```

PR2C, PR3 i Type-5 T1 pozostają zablokowane przez sekwencyjne acceptance gates.

## 9. Kryteria decyzji

```text
METRIC_CONTRACTS_V1_1_PARITY_SENSITIVE_PRODUCERS_READY
FTDI_LEGACY_PARITY_PASS
DEV_LEGACY_EFFECTIVE_POLICY_PARITY_PASS
SAME_MS_EXACT_PARITY_PASS
TOP3_SELECTOR_PARITY_PASS
LEGACY_FSC_PARITY_PASS
CORRECTED_FTDI_ACTIONABILITY_COUNTERFACTUAL_ONLY
DEV_PRIMARY_COUNTERFACTUAL_ONLY
FSC_V2_EVIDENCE_ONLY
PROJECTION_COMMON_SCHEMA_READY
PROJECTION_PR2A_FAMILY_BUILDERS_READY
MFS_ROOT_PROJECTION_NOT_ACTIVATED_IN_PR2A
DECISION_LOGGER_V33_UNCHANGED
V3_REPLAY_V1_UNCHANGED
DUAL_COMPUTE_NOT_ACTIVATED
TYPE5_RUNTIME_NOT_IMPLEMENTED
PR2B_PLUS_BLOCKED_UNTIL_SEQUENTIAL_ACCEPTANCE
PR2A_PROJECTION_VALUE_STATUS_INVARIANTS_PASS
PR2A_FAMILY_SEMANTIC_VALIDATION_PASS
PR2A_VALIDATED_HASH_PATH_PASS
PR2A_EFFECTIVE_CONFIG_PROJECTION_PARITY_PASS
PR2A_VALIDATED_HASH_CONTEXT_BINDING_PASS
PR2A_RECENT_EXACT_ZERO_WIDTH_WINDOW_PASS
GATEKEEPER_POLICY_UNCHANGED
PR2A_REVIEW_BLOCKERS_CLOSED
PR2A_READY_FOR_RE_REVIEW
```
