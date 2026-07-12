# PR2A parity-sensitive metric-contract producers — raport weryfikacyjny V1

Status: `VERIFICATION_COMPLETE / READY_FOR_RE_REVIEW / REVIEW_BLOCKERS_CLOSED / ONE_UNRELATED_BASELINE_TEST_FAILURE`

Data: 2026-07-12

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2a-producers`

Base, branch start i merge-base:
`a695efb2e6d3884b9a1a7fc4ff86106f0af3ff64`

ADR:
`docs/ADR/ADR_8D_PR2A_METRIC_CONTRACT_PARITY_SENSITIVE_PRODUCERS_20260712.md`

## 1. Zakres dowodu

Raport weryfikuje:

1. parity legacy FTDI, dev, exact same-ms, top3 i FSC;
2. authority isolation corrected FTDI, dev-primary i FSC v2;
3. compact schema, hash, bounded reasons i per-family builders;
4. rzeczywiste runtime effective-config binding;
5. brak częściowej materializacji root MFS;
6. brak zmian Gatekeeper policy, DecisionLogger v33 i V3 replay v1;
7. compatibility starych konfiguracji i historycznych statusów;
8. fail-closed value/status i family semantic validation przed hashowaniem;
9. ewaluowalność domkniętego recent-exact window o zerowej szerokości.
10. pełne związanie compact family semantics z dokładnym, poprawnie
    zahashowanym `ResolvedMetricContractEffectiveConfigV1`.

Nie jest to burn-in, PR2B, PR2C, PR3 ani Type-5 T1 evidence.

## 2. Test matrix

| Komenda | Wynik | Dowód |
| --- | --- | --- |
| `cargo test -p ghost-core --test metric_contracts_v1_1_foundation` | PASS, 15/15 | registry/profile/hash/status/effective-config foundation |
| `cargo test -p ghost-core --test metric_contracts_v1_1_projection` | PASS, 18/18 | closed root, validated hash, value/status coherence, family/config semantics, context-driven timing/FSC i negative serde/config |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_producers` | PASS, 18/18 | FTDI/dev/timing/top3/FSC producers, runtime config defaults, frozen-boundary config binding i zero-width recent exact |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_static_guards` | PASS, 7/7 | forbidden activation i one-owner guards |
| `cargo test -p ghost-launcher --lib reversed_recent_window_remains_empty` | PASS, 1/1 | `start > end` pozostaje pustym oknem |
| `cargo test -p ghost-launcher --test gatekeeper_policy_tests` | PASS, 46/46 | V2 policy parity |
| `cargo test -p ghost-launcher --test gatekeeper_v25_regression` | PASS, 42/42 | V2.5 parity |
| `cargo test -p ghost-launcher --test refactor_invariants_tests` | PASS, 12/12 | SSOT/top3/schema guards |
| `cargo test -p ghost-launcher --test gatekeeper_v3_tests` | PASS, 9/9 | istniejący V3 behavior |
| `cargo test -p ghost-launcher --test session_lifecycle_tests fsc` | PASS, 2/2 | rozdzielone statusy FSC w materializacji |
| `cargo test -p ghost-launcher --lib same_funder_yields_high_fsc` | PASS, 1/1 | legacy FSC formula |
| `cargo test -p ghost-launcher --lib insufficient_known_sources_returns_reason` | PASS, 1/1 | one-known = null/unavailable |
| `cargo test -p ghost-brain --lib test_gatekeeper_buy_log_file_write` | PASS, 1/1 | v33 write compatibility |
| `cargo test -p ghost-brain --lib replay_payload` | PASS, 5/5 | V3 replay schema v1 compatibility |
| `cargo check -p ghost-launcher` | PASS | launcher build |
| `cargo fmt --all -- --check` | PASS | format |
| `git diff --check` | PASS | whitespace/patch integrity |

Targeted clippy:

```text
cargo clippy -p ghost-core --test metric_contracts_v1_1_projection \
  --no-deps --message-format=short
cargo clippy -p ghost-launcher \
  --test metric_contracts_pr2a_producers \
  --test metric_contracts_pr2a_static_guards \
  --no-deps --message-format=short
```

Obie komendy kończą się kodem 0 i nie raportują nowych ostrzeżeń w liniach
zmienionych przez amendment; pozostałe ostrzeżenia są istniejącym baseline’em
crates.

## 3. Projection foundation proof

Pokrycie testów:

- deterministic hash i mutacja każdego semantic leaf zmienia hash;
- hash jest dostępny przez `validated_canonical_hash(context)` i nie powstaje,
  gdy `validate_context()` odrzuca payload;
- `deny_unknown_fields` na root i typach składowych;
- partial root jest odrzucany;
- missing producer/config/cutoff, wrong surface/role/profile/mode są odrzucane;
- missing, duplicate, wrong-kind i non-finite effective config są odrzucane;
- reasons są unikalne, bounded do 8 i mają jawny omitted count;
- generic field/surface validation odrzuca `Available + Null`,
  non-available + value oraz `Available + NotApplicable`;
- jawne family validators odrzucają niespójne FTDI/dev/timing/top3/FSC
  dependencies także po deserializacji albo ręcznej mutacji root;
- FTDI, dev, timing, top3 i FSC builders odrzucają representation drift;
- compact types nie zawierają owner lists, event lists, anchors, cumulative
  flows ani transport metadata;
- producer call-count pozostaje 1 przy wielokrotnej konwersji tego samego
  frozen evidence;
- PR2B family types nie mają builderów;
- `MaterializedFeatureSet` nie ma root projection field.

### 3.1 Zamknięcie pierwszych dwóch blokerów review

Bloker 1 miał dwie przyczyny źródłowe:

- compact field/surface walidowały reason/envelope/provenance, ale nie
  koherencję `value ↔ availability ↔ measurement_quality`;
- family builders sprawdzały część parity podczas konwersji, lecz root
  `validate_context()` nie odtwarzał wszystkich zależności semantycznych, a
  dawny `canonical_hash()` sprawdzał tylko `schema_version`.

Naprawa:

```text
MetricDecisionFieldValueV1::validate
MetricDecisionSurfaceValueV1::validate
FtdiDecisionProjectionV1::validate_semantics
DevBuyDecisionProjectionV1::validate_semantics
TxTimingDecisionProjectionV1::validate_semantics
Top3DecisionProjectionV1::validate_semantics
FundingDecisionProjectionV1::validate_semantics
FscStatusDecisionProjectionV1::validate_semantics
MetricContractDecisionEvidenceProjectionV1::validated_canonical_hash
```

Negatywne testy obejmują: `Available + Null`, `Unavailable/NotConfigured/`
`NotRecordedLegacySchema + Value`, `Available + NotApplicable`, timing
`numerator > denominator`, ratio niezgodne z counts, złą population, zły recent
window, drift FTDI value/counts, top3 effective/fallback, legacy FSC value przy
jednej known próbce oraz hash semantycznie nieważnego root.

Bloker 2 wynikał z warunku `start_ts_ms >= end_ts_ms` w
`rce_window_stats()`. Domknięty przedział `[T,T]` może jednak zawierać wiele
zdarzeń. Warunek zmieniono na `start_ts_ms > end_ts_ms`; testy dowodzą:

```text
1 successful @ T => numerator=0, denominator=1, ratio=0.0
2 successful @ T => numerator=1, denominator=2, ratio=0.5
3 successful @ T => numerator=2, denominator=3, ratio=2/3
failed @ T       => wykluczony z successful denominatora
start > end      => pusty wynik
```

Recent exact nadal jest `LoggingOnly`; static guard zabrania jego odczytu w
aktywnym Gatekeeper policy.

### 3.2 Zamknięcie effective-config/projection parity

Po pierwszym amendmentcie poprawnie zahashowany config mógł nadal przeczyć
compact projection, ponieważ family validators używały lokalnych stałych dla
recent window i legacy FSC minimum. Naprawa:

```text
TxTimingDecisionProjectionV1::validate_semantics(context)
FundingDecisionProjectionV1::validate_semantics(context)
DevBuyDecisionProjectionV1::validate_semantics(context)
Top3DecisionProjectionV1::validate_semantics(context)
FtdiDecisionProjectionV1::validate_semantics(context)
FscStatusDecisionProjectionV1::validate_semantics(funding, context)
MetricContractProjectionErrorV1::EffectiveConfigParity
```

Core projection nie zawiera już
`TX_TIMING_RECENT_EXACT_WINDOW_MS_V1` ani lokalnego
`FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1`. `SameMsRecentWindowMs` jest czytany
jako `WideUnsigned`, checked-konwertowany do `u32` i dokładnie porównywany z
non-null `recent_exact.window_ms`. `FscLegacyMinKnownSourceSamples` pozostaje
`u64` podczas porównania z `u32` count i steruje granicą null/measured.

Testy z nowym poprawnym config hash dowodzą:

```text
config window=9_999, projection window=10_000 => EffectiveConfigParity
config FSC min=3, measured legacy FSC przy 2 samples => EffectiveConfigParity
config window=9_999, projection window=9_999 => validate + validated hash PASS
config window > u32::MAX => checked-representation error
```

Audyt pozostałych zależności jawnie wiąże population/denominator FTDI i timing,
dev eligibility/anchors/selection, top3 preferred/fallback/scale/mismatch oraz
FSC formula/unavailable/status mappings. Producer-only dust/dedupe/capacity i
source settings nie są dopisywane do compact payload: obowiązkowy frozen
producer boundary porównuje je z effective-config. Osobny test zmienia je,
przebudowuje config/hash i otrzymuje `ProducerConfigMismatch` dla każdej rodziny.

## 4. Parity proofs per family

### FTDI

```text
3 successful BUY tx
2 unique signers
first sample per signer
1 unique topology
legacy value = 1 / 2 = 0.5
legacy buy-tx actionable = true
corrected unique-buyer actionable = false
```

To jest normatywny counterexample: corrected actionability różni się, ale jego
surface pozostaje `Counterfactual` i `policy_actionable=false`. Coordination HHI
ma `ExportOnly`.

### Dev buy

Test obejmuje:

- creator unknown;
- creator bez eligible buy;
- first-observed failed legacy buy;
- create-signature dust exclusion;
- późniejszy successful create-signature match;
- multiple creator buys i duplicate ingest;
- stable-key fallback niezależny od delivery order;
- candidate history completeness;
- bit-for-bit effective-policy = first-observed MFS;
- dev-primary `Counterfactual` i zero policy actionability.

### Timing

Testy rozdzielają:

- exact same-ms extras;
- adjacent `<50 ms` cluster;
- recent successful 10 s window;
- domknięte zero-width recent window z 1/2/3 successful tx;
- failed tx w tym samym timestampie wykluczony z recent denominatora;
- full observation denominator;
- duplicate transaction;
- missing timestamp fallback;
- missing stable ordering identity;
- strict granicę `0/1/49/50 ms`;
- truncated source quality.

Legacy i exact V1 mają identyczne numerator, denominator i ratio bits. Tylko
legacy exact surface pozostaje aktywna w bieżącej policy.

### Top3

Testy potwierdzają preferred, alias fallback, mismatch telemetry, skalę 0..1 i
bit-for-bit effective selector. Static guard wykazuje dokładnie jeden helper
`effective_top3_signer_volume_ratio()` i brak bezpośredniego active policy read
`top3_volume_pct`.

### FSC

Testy pokrywają zero known, one known, dwa i więcej known oraz same-funder
concentration. Dla
`known_source_sample_count < FscLegacyMinKnownSourceSamples` value jest null i
`Unavailable/NotApplicable`. Od skonfigurowanego minimum builder/validator
wymaga bit-for-bit:

```text
1 - distinct_known_source_count / known_source_sample_count
```

FSC v2 status i coverage są osobne; compatibility FSC status nie dowodzi v2
measurement. V3 requirement `fsc_v2` domyślnie pozostaje false.

## 5. Effective-config proof

Runtime resolver jest zasilany rzeczywistymi Gatekeeper V2, TxIntelligence,
fingerprint i FundingSource/FSC v2 settings. Testy wykazują:

- identyczne settings dają identyczny hash;
- zmiana producenta zmienia hash;
- niespójny Gatekeeper/TxIntel config jest odrzucany;
- snapshot dust/capacity/window niezgodny z hash jest odrzucany;
- FundingSourceConfig niezgodny z hash jest odrzucany;
- poprawnie przebudowany i ponownie zahashowany config sprzeczny z compact
  family semantics jest odrzucany przez `EffectiveConfigParity`;
- klucze bez compact representation są fail-closed na frozen producer
  boundary, również po poprawnym rehashu configu;
- missing/duplicate/wrong-kind/non-finite/profile mismatch failuje zamknięcie.

Resolved config jest przechowywany w `OracleRuntimeConfig`, ale PR2A nie
uruchamia root buildera i nie emituje projection.

## 6. Forbidden-scope proof

Następujące pliki mają identyczny SHA-256 jak base:

```text
5ec6a766e8bb2d9b0cacdc39d7146b5fac056e0cf57042b75b4077a82fd9210c
  ghost-launcher/src/components/gatekeeper_policy.rs

22bf380765fb9dfa5ae38e9f7340b9d9c5ba3a467d0b2eca783344787bf64425
  ghost-brain/src/oracle/decision_logger.rs

feca4ba45ac4242c32e3a7ba3a7f70b571bd0cc8ee4883985b8652af728cb74b
  ghost-launcher/src/bin/v3_replay.rs

479d370849981b7b8789982a04246b37794779e451e5ef8f96c09398f0911373
  ghost-launcher/src/components/gatekeeper_v3.rs
```

Static guards potwierdzają także:

- brak aktywacji DualCompute/V2;
- brak MFS root projection;
- brak PR2B family builders;
- brak Type-5 runtime symbols;
- brak alternative producers;
- brak live-state read/recompute w projection builderze.

## 7. Znany baseline failure

Komenda:

```text
cargo test -p ghost-brain --lib selector_shadow_score
```

Wynik:

```text
8 passed
1 failed: test_selector_shadow_score_filters_non_finite_feature_values
```

Klasyfikacja: `UNCHANGED_PRE_EXISTING_FAILURE`.

Dowód blob/diff:

```text
working-tree blob = 790db5300db39036a6309552d75578ebeac2aa1c
base blob         = 790db5300db39036a6309552d75578ebeac2aa1c
git diff base -- ghost-brain/src/oracle/decision_logger.rs = empty
```

Failure i jego assertion są takie same jak w PR1 verification. PR2A nie zmienia
selector score, mapping, logger ani execution eligibility. Naprawa pozostaje
poza zakresem.

## 8. Zmienione powierzchnie

Kod/schema:

```text
ghost-core/src/metric_contracts/{projection,effective_config,status,mod}.rs
ghost-core/src/checkpoint/types.rs
ghost-brain/src/config/gatekeeper_v3_config.rs
ghost-launcher/src/metric_contracts/{mod,effective_config,pr2a}.rs
ghost-launcher/src/tx_intelligence/{engine,funding_source,sybil_metrics,mod}.rs
ghost-launcher/src/session/observation.rs
ghost-launcher/src/components/gatekeeper.rs
ghost-launcher/src/{lib,main,oracle_runtime}.rs
```

Testy:

```text
ghost-core/tests/metric_contracts_v1_1_projection.rs
ghost-launcher/tests/metric_contracts_pr2a_producers.rs
ghost-launcher/tests/metric_contracts_pr2a_static_guards.rs
ghost-launcher/tests/{gatekeeper_v3_tests,session_lifecycle_tests}.rs
```

Dokumenty:

```text
docs/ADR/ADR_8D_PR2A_METRIC_CONTRACT_PARITY_SENSITIVE_PRODUCERS_20260712.md
reports/metric_contracts/pr2a_parity_sensitive_producers_verification_v1.md
```

## 9. Ograniczenia i kolejny gate

- Brak durable full evidence i v34 — PR2C.
- Brak kompletnego root MFS projection — PR2B.
- Brak flip/manipulation/reserve/recent family builders — PR2B.
- Brak DualCompute/V2 — późniejszy sekwencyjny gate.
- Brak policy promotion corrected FTDI/dev-primary/FSC v2.
- Brak Type-5 runtime.

Kolejny etap po review i merge PR2A: `PR2B`.

## 10. Werdykt

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
