# ADR-8D: PR2B evidence-only producers i atomowa projekcja kontraktów metryk

Status: `IMPLEMENTED / VALIDATED / READY_FOR_RE_REVIEW`

Typ: ADR-8D / cross-cutting SSOT, evidence, materialization i resource contract

Data: 2026-07-12

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2b-evidence-producers`

Base: `55a9c5ce306e3a0c4fceb85015f5b796567073c3`

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Poprzedni etap:
`docs/ADR/ADR_8D_PR2A_METRIC_CONTRACT_PARITY_SENSITIVE_PRODUCERS_20260712.md`

Raport dowodowy:
`reports/metric_contracts/pr2b_evidence_only_producers_verification_v1.md`

Poziom ryzyka: `HIGH`. Zmiana przecina czterech canonical owners, pełny evidence
root, compact projection oraz terminalną materializację MFS. Ryzyko policy jest
ograniczone przez rollout `Legacy`, niezmienione authority assignments, brak
odczytu nowej projection przez Gatekeeper oraz fail-closed typed materialization.

## 1. Kontekst

PR2A dostarczył sześć rodzin parity-sensitive, lecz celowo nie tworzył
niekompletnego root projection. PR2B domyka pozostałe cztery rodziny:

- Flip V2;
- manipulation numeric presence i derived flags;
- reserve velocity typed evidence;
- recent buy/sell typed evidence.

Od tego etapu każdy poprawny terminalny snapshot bieżącego buildu musi zawierać
kompletną dziesięciorodzinną projection. Historyczne rekordy bez nowego pola
pozostają rozróżnialne jako `None`.

## 2. Decyzja architektoniczna

Jedynym dozwolonym przepływem jest:

```text
canonical per-family producers
→ jeden immutable frozen producer input set
→ MetricContractsEvidenceSetV1
→ pure MetricContractDecisionEvidenceProjectionV1
→ MaterializedFeatureSet.metric_contract_decision_projection_v1
```

Pełny evidence set jest lokalnym wynikiem granicy builda. Nie jest kopiowany do
MFS, nie jest zapisywany do pliku i nie tworzy sidecara. Projection nie czyta
raw transakcji, sesji, indeksów, owner state ani zegara. Konwersja przyjmuje
wyłącznie full evidence oraz frozen context: rollout, Profile A,
effective-config i decision cutoff.

`MaterializedFeatureSet` zawiera dokładnie:

```rust
#[serde(default)]
pub metric_contract_decision_projection_v1:
    Option<MetricContractDecisionEvidenceProjectionV1>
```

Semantyka:

- historyczny brak pola deserializuje się do `None`;
- bieżąca skuteczna materializacja tworzy pełne `Some`;
- błąd producenta, configu, evidence, projection lub resource gate zwraca
  `MetricContractMaterializationErrorV1` na aktywnej terminalnej ścieżce;
- nie istnieje częściowe `Some` ani `None` jako fallback bieżącego buildu;
- błąd nie jest mapowany na Gatekeeper verdict, reason, phase ani soft point.

## 3. Canonical owners

| Rodzina | Canonical owner | Nowa reprezentacja | Legacy behavior |
| --- | --- | --- | --- |
| Flip V2 | istniejący `TxIntelligenceEngine` / fingerprint owner | owner-state full evidence + compact aggregates | `flip_ratio_10s` bez zmian |
| Manipulation | frozen V3 materialization owner | 7 presence-aware numeric fields + 6 derived flags | `ManipulationContradictionFeatures` i V3 v1 bez zmian |
| Reserve velocity | `AccountStateReducer` | typed status, reserves, interval, count, receive-time clock | legacy scalar SOL/s bez zmian |
| Recent buy/sell | istniejący RCE recent-window owner | counts, legacy scalar, unbounded ratio, bounded share | legacy scalar bez zmian |

Każdy producer jest wywoływany jeden raz w terminalnej materializacji. Static
call-count guard obejmuje również sześć rodzin PR2A i pojedyncze wywołanie
complete buildera. Test pełnego boundary dowodzi deterministycznej równości
`full evidence → projection` z projection przechowywaną w wyniku granicy.

## 4. Flip V2

Flip V2 jest bounded automatem per owner osadzonym w istniejącym
`TxIntelligenceEngine`. Eligible event wymaga success, non-dust, canonical
window, resolved owner, present slot oraz niezależnie udowodnionych stable
identity i canonical order. `StableEventIdentityV1` wybiera signature, a
dopiero przy jej braku fallback `slot + transaction_index` albo `slot +
event_ordinal`. `CanonicalFlipOrderKeyV1` nigdy nie używa signature: wybiera
`slot + transaction_index`, a przy braku indexu `slot + event_ordinal`.
Signature bez obu order fields daje znaną identity, lecz non-evaluable record.
Receive order, timestamp i leksykograficzna kolejność signature nie zastępują
order proof. Duplicate identity z różnymi order keys i duplicate order key z
różnymi identities są fail-closed conflicts. Timestamp pozostaje
window/consistency checkiem; sprzeczność z canonical slot/order daje
`OutOfOrderEvent`.

Stany:

```text
no_anchor
tracking
flipper
closed_non_flipper
```

Pierwszy eligible BUY tworzy niezmienny anchor. Pre-anchor SELL nie jest
retroaktywny. Kolejne BUY i SELL używają checked cumulative arithmetic.
Pierwszy SELL spełniający jednocześnie wall-clock, slot-gap i dump-ratio zamraża
`flipper`. Denominator liczy ownera raz, a aggregate ratio jest zawsze w 0..1.

Dedupe FIFO, event state i owner set są ograniczone konfiguracją. Dedupe
eviction usuwa także odpowiadający event i degraduje wynik fail-closed. Wallet
cap jest egzekwowany przed zapisaniem nowego ownera. Reconnect/gap, capacity
loss, sprzeczny canonical order i overflow tworzą unavailable/non-evaluable
snapshot z typed reasons i telemetryką eviction.

## 5. Manipulation presence-aware evidence

Full evidence zawiera dokładnie siedem pól:

```text
same_ms_tx_ratio
bundle_suspicion_ratio
top3_signer_volume_ratio
hhi
max_tx_per_signer
dev_volume_ratio
contradiction_score
```

Owner terminalnej materializacji zamraża jednocześnie legacy
`ManipulationContradictionFeatures` oraz typed
`ManipulationProducerSnapshotV2` z tych samych już policzonych źródeł. Typed
snapshot zawiera osobne `value/availability/measurement_quality/reasons` dla
każdego pola; evidence builder nie odzyskuje presence ze scalarów legacy ani z
group statusu.

`Null` nie jest zerem. Legacy/default zero pozostaje wyłącznie w legacy lane;
explicit measured zero pozostaje `Value(0)`. Brak preferred/fallback top3,
evaluable signer population, denominatora albo wymaganych składowych
contradiction score pozostaje unavailable, nigdy measured zero. Maska
`measured_fields_mask` odpowiada dokładnie polom o jakości `Measured` albo
`Degraded`. Group quality jest wyłącznie górnym ograniczeniem jakości pola:
`Degraded` dopuszcza mixed presence, natomiast `Clean` z missing required field
jest downgradowane na owner boundary lub odrzucane fail closed.

Sześć derived high flags zachowuje field ID, raw value/status, comparator,
threshold, wynik, exact policy-stage/version i effective-config hash. Comparator
tej wersji to strict `GreaterThan`; equality jest false. Brak raw value daje
`Null`, nigdy false. Projection zachowuje wyłącznie pola numeryczne i maski;
pełne derived provenance pozostaje w full evidence.

## 6. Reserve velocity

`AccountStateReducer` jest jedynym właścicielem typed snapshotu. Session nie
rekonstruuje velocity z MFS scalar. Owner zwraca jawnie:

- `Measured` — co najmniej dwa canonical updates, dodatni interval, exact
  reserves/formula i finite SOL/s;
- `FirstUpdate` — current reserve obecne, brak interval/value;
- `ZeroDeltaTime` — reserves obecne, interval zero, brak measured value;
- `BootstrapFallback` — owner-known bootstrap, brak measured value;
- `Unavailable` — brak owner evidence albo fail-closed arithmetic state.

Źródłem czasu jest wyłącznie `receive_time`; fallback i first update nie mogą
udawać measured zero. Pełny evidence i compact validator niezależnie sprawdzają
count/presence/formula parity.

## 7. Recent buy/sell

Canonical owner korzysta z successful-only RCE window z inclusive start/end.
Snapshot zachowuje checked counts oraz liczbę odrzuconych failed events.

```text
transaction_count = buy_count + sell_count

legacy:
  transaction_count == 0 → null
  sell_count == 0        → buy_count
  otherwise              → buy_count / sell_count

buy_to_sell_ratio:
  sell_count == 0 → null
  otherwise       → buy_count / sell_count

buy_share:
  transaction_count == 0 → null
  otherwise               → buy_count / transaction_count
```

Nowa surface pozostaje `LoggingOnly`, `NonPolicy` i nieactionable. Static guard
potwierdza brak consumera w aktywnych plikach Gatekeeper policy i V3.

## 8. Effective-config coverage

Zamknięta tabela `PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1` wyprowadza oczekiwany
zbiór z `METRIC_EFFECTIVE_CONFIG_KEYS_V1` dla czterech kontraktów PR2B. Każdy
klucz jest przypisany dokładnie raz.

`FrozenProducerBoundaryValidated`:

- `FlipCandidateDustThresholdSol`;
- `FlipCandidateDedupeKey`;
- `FlipCandidateDedupeCapacity`;
- `FlipCandidateEvictionPolicy`;
- `FlipCandidateMaxWallets`;
- `FlipCandidateReconnectBehavior`.

Pozostałe PR2B keys są `CompactValidated`, ponieważ ich wartości są
reprezentowane przez aggregate fields albo są stałą semantyką zamkniętej
compact schema. Family builders sprawdzają te same frozen context values przed
utworzeniem evidence; `validate_context()` i validated hash sprawdzają je
ponownie bez dostępu do producer state.

## 9. Compact schema i resource gate

Projection zachowuje zatwierdzone exact field sets. Nie zawiera owner IDs,
anchorów, qualifying sells, event identities, raw collections, full field
lists ani derived provenance arrays. Wszystkie dziesięć family keys jest
wymagane, a unknown/partial root jest odrzucany.

Domain `MetricContractDecisionEvidenceProjectionV1` i wszystkie jego exact Rust
field-sets pozostają bez zmian. Nie otrzymały globalnych serde aliases ani
compact `Serialize`. Oddzielny, lossless
`MetricContractDecisionProjectionWireV1` jest wyłącznie transportowym adapterem
MFS:

```json
{"w":1,"d":["fixed-position domain projection payload"]}
```

Field-level serde na
`Option<MetricContractDecisionEvidenceProjectionV1>` koduje `Some` przez Wire
V1. Brak historycznego pola nadal daje `None`; present field oznacza wyłącznie
Wire V1. Unsupported version, missing/extra key lub slot, wrong tuple length i
invalid enum/reason code są odrzucane. Verbose projection object i jawne `null`
w present field nie mają fallbacku.

### 9.1. Normatywne pozycje Wire V1

Indeks w każdej tablicy jest częścią wersji. Zamknięte źródło wykonywalnego
mappingu to `metric_contract_projection_wire_v1_tuple_layouts()` w
`ghost-core/src/metric_contracts/projection_wire.rs`; golden fixture ma BLAKE3
`be965cdbfabffc8690a256574334ddd628414d2423a24cd5e81900ec32f4b566`.

| Tuple | Pozycje 0..N mapowane na domain fields |
| --- | --- |
| wire object | `w=wire_schema_version`, `d=projection_root` |
| root | `schema_version`, `rollout_mode`, `profile_id`, `profile_hash`, `metric_contract_effective_config_hash`, `fee_topology_diversity_index`, `dev_buy`, `same_ms_tx_ratio`, `top3_signer_volume_ratio`, `flip_ratio`, `funding_source_concentration`, `fsc_evidence_status`, `manipulation_contradiction`, `reserve_velocity`, `recent_buy_sell` |
| FTDI | `legacy_value`, `value_v1`, `unique_topology_count`, `unique_buyer_sample_count`, `buy_transaction_sample_count`, `legacy_buy_tx_actionability`, `unique_buyer_actionability_v2` |
| dev-buy | `tx_intel_first_observed`, `mfs_first_observed`, `mfs_primary_v1`, `effective_policy`, `creator_known`, `create_signature_matched`, `primary_selection_mode`, `primary_eligible_buy_count` |
| timing | `legacy_exact`, `exact_v1`, `cluster_lt_50ms`, `recent_exact` |
| top3 | `preferred`, `compatibility_alias`, `effective`, `preferred_alias_bitwise_equal`, `used_compatibility_fallback` |
| flip | `legacy_slot_gap_ratio`, `hybrid_v2_ratio`, `eligible_buyer_count`, `flipper_count`, `wall_clock_window_ms`, `max_slot_gap`, `dump_ratio` |
| funding | `legacy_source`, `legacy_v1`, `distinct_known_source_count`, `known_source_sample_count`, `fsc_v2`, `known_coverage`, `non_neutral_known_coverage`, `known_buyer_count`, `total_buyer_count` |
| FSC status | `compatibility_status`, `legacy_scalar_present`, `legacy_feature_status`, `fsc_v2_status`, `fsc_v2_coverage` |
| manipulation | `legacy_numeric_envelope`, `numeric_v2_envelope`, `measured_fields_mask`, seven named numeric fields in §2.6.2 order, `legacy_high_recorded_mask`, `legacy_high_true_mask`, `derived_high_evaluable_mask`, `derived_high_true_mask` |
| reserve | `legacy_velocity`, `velocity_v1`, `previous_real_sol_reserves_lamports`, `current_real_sol_reserves_lamports`, `interval_ms`, `accepted_update_count`, `source_clock`, `status` |
| recent | `legacy_scalar`, `v1_envelope`, `window_ms`, `buy_count`, `sell_count`, `transaction_count`, `buy_to_sell_ratio`, `buy_share` |
| envelope | `contract_id`, `contract_version`, `surface_id`, `authority_class`, `rollout_role`, `availability`, `measurement_quality`, `policy_actionable`, `reasons` |
| surface | `envelope`, nullable literal `value`, `producer_id`, `producer_schema_version`, `source_cutoff` |
| field | nullable literal `value`, `availability`, `measurement_quality`, `reasons` |
| cutoff | canonical integer-string `decision_timestamp_ms`, nullable canonical integer-string `decision_slot` |
| reason summary | `codes`, `omitted_count` |
| ratio | `surface`, `numerator`, `denominator`, `population`, nullable `window_ms` |

### 9.2. Normatywne enum i reason codes

Każdy mały integer jest indeksem w jawnej, zamrożonej tabeli. Funkcja
`metric_contract_projection_wire_v1_mapping_tables()` publikuje dokładnie 28
tabel: contract ID, wszystkie surface ID, rollout/profile, authority/role,
availability/quality, producer, dev selection, timing population, evidence/FSC
status, reserve clock/status, reason family oraz 12 family-specific reason
detail tables. `UnmappedLegacyString` zachowuje typed contract code i pełny raw
tekst. `MetricDecisionReasonSummaryV1` zachowuje `omitted_count`; tylko
istniejący bounded reason omission jest dozwolony. Static guard wymaga
unikalnych niepustych tabel, a round-trip/golden test zamraża ich interpretację.
Zmiana pozycji, wartości lub kodu wymaga Wire V2.

Canonical hash nadal używa canonical domain JSON i jest dostępny wyłącznie
przez `validated_canonical_hash(context)`. Nie hashujemy wire bytes. Test
wymaga identycznego `CanonicalHashV1` przed i po Wire V1 round-trip. Input-head
`fddd4d3a…` fixture oraz bieżący domain fixture mają ten sam semantic hash
`61cf0429a8dd042070f18cf426f37f27983d055b91d4033df3a8311a78e5a09e`.

Normatywne `metric_contract_projection_serialized_bytes` to długość dokładnego,
nieskompresowanego `serde_json::to_vec(Wire V1)`, używanego przez MFS field
serializer, runtime hard gate, testy, telemetrykę i release harness. Bincode i
verbose domain JSON pozostają wyłącznie osobno nazwanymi diagnostykami.
Oversized bounded-reason payload jest odrzucany przez `ProjectionTooLarge`,
którego `actual_bytes` raportuje exact Wire V1 JSON bytes. Budżety pozostają
p95 `12 KiB` i hard max `16 KiB`; kompresja nie bierze udziału w gate.

Family builders zachowują fail-closed public context validation, natomiast
root builder używa prywatnej, typowo ograniczonej ścieżki dla już
zwalidowanego immutable contextu. Eliminuje to ponowne liczenie hashy profile i
effective-config dla każdego compact envelope bez osłabienia publicznej
granicy. Release resource harness raportuje osobno build/validation, wire
serialization i combined path. Po 32-iteracyjnym warm-upie steady-state wyniki
p50/p95/p99 wynoszą odpowiednio: build/validation `515/619/738 us`, Wire V1
serialization `49/66/79 us`, combined `575/674/708 us`. Standard Wire V1 ma
`2339 B`, duży poprawny fixture `8487 B`; diagnostyczny verbose domain JSON ma
`20332 B`, a diagnostyczny bincode `2780 B`.

## 10. Zakres wyłączony

Bez zmian pozostają Gatekeeper V2/V2.5 i V3 policy, thresholds, weights,
phases, soft points, verdicts, reasons, aktywny dev source, DecisionLogger v33,
V3 replay v1, selector score, IWIM, post-buy, sender, Jito, execution oraz
live/shadow boundary. Rollout pozostaje `Legacy`.

PR2B nie implementuje v34, sidecara, writera, comparatora, replayu v2, audit
CLI, burn-in, Type-5, DualCompute, V2 rollout ani PR3 cutover.

## 11. Konsekwencje

Pozytywne:

- MFS ma jeden atomowy, kompletny i replay-safe compact contract snapshot;
- pełne evidence i compact projection pochodzą z tych samych frozen inputs;
- brak pola historycznego nie jest mylony z measured zero;
- owner/event detail nie obciąża MFS;
- config drift, arithmetic loss i partial evidence są fail-closed.

Koszt:

- terminal materialization może zwrócić typed error zamiast emitować snapshot;
- runtime wykonuje dodatkową pure validation i canonical-hash proof;
- pełna durability evidence pozostaje świadomie odłożona do PR2C.

## 12. Walidacja i decyzja końcowa

Pełna macierz PR2A/PR2B, Gatekeeper V2/V2.5/V3, session lifecycle, logger,
replay v1, checks, targeted Clippy, rustfmt i diff checks przeszła. Jedyny
uruchomiony failure to niezmieniony baseline
`test_selector_shadow_score_filters_non_finite_feature_values`; jego owner file
`decision_logger.rs` ma identyczny SHA-256 i pusty diff względem base.

Zamrożone pliki policy, V3, loggera, replayu, IWIM, sendera, post-buy, Jito i
execution mają identyczne SHA-256 jak base. Rollout pozostaje `Legacy`.
PR2C, PR3 oraz Type-5 T1 nie zostały rozpoczęte.

Markery akceptacyjne po pełnej walidacji amendmentu:

```text
METRIC_CONTRACT_PROJECTION_WIRE_V1_PLAN_AMENDMENT_PASS
PR2B_COMPACT_JSON_WIRE_V1_ROUNDTRIP_PASS
PR2B_SEMANTIC_HASH_INDEPENDENT_OF_WIRE_PASS
PR2B_ACTUAL_MFS_SERIALIZATION_RESOURCE_GATE_PASS
PR2B_MANIPULATION_TRUE_PER_FIELD_PRESENCE_PASS
PR2B_FLIP_IDENTITY_ORDER_SEPARATION_PASS
PR2B_REVIEW_BLOCKERS_CLOSED
PR2B_READY_FOR_RE_REVIEW
```
