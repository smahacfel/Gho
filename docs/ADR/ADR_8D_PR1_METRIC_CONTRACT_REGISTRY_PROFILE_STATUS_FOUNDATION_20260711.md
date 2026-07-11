# ADR-8D: PR1 registry, profile, status i schema foundation kontraktów metryk

Status: `IMPLEMENTED / METRIC_CONTRACTS_V1_1_FOUNDATION_LEGACY_PARITY / READY_FOR_REVIEW`

Typ: ADR-8D / cross-cutting runtime-contract foundation

Data: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

Branch: `agent/metric-contract-pr1-foundation`

Base i merge-base z `origin/main`:
`9ae1a30dd5b681f7777fa8e833e7103fde73a647`

Plan:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

PR0 baseline:
`reports/metric_contracts/baseline_reconciliation_v1.md`

Raport weryfikacyjny:
`reports/metric_contracts/pr1_foundation_verification_v1.md`

Poziom ryzyka: `MEDIUM` — nowe publiczne kontrakty współdzielone przez crates i
fail-closed startup validation; bez zmiany progów, policy authority, verdictów,
reason codes, DecisionLogger v33, replay V3 v1 albo shadow/live behavior.

Uwaga o szablonie: wskazany globalnie plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` oraz lokalny
`docs/ADR/ADR_8D_SZABLON.md` nie istnieją w checkoutach. Dokument zachowuje
sekcyjny format ADR-8D używany w istniejących ADR-ach repo.

## 1. Kontekst i problem

PR0 potwierdził dziesięć rodzin metryk, których historyczne nazwy, skale,
populacje, statusy albo źródła nie są wystarczająco jednoznaczne do bezpiecznego
replayu i późniejszego cutoveru. PR1 ma utworzyć wspólny język kontraktowy przed
jakąkolwiek zmianą producentów lub policy:

```text
registry + surface identity
→ immutable authority profile
→ canonical status/actionability
→ canonical hash/config/identity
→ serde-compatible evidence schema
```

PR1 nie może jeszcze emitować evidence sidecar, aktywować dual compute, zmieniać
`MaterializedFeatureSet`, podnosić decision schema do v34 ani przełączać
jakiegokolwiek terminalnego odczytu na semantycznie nowy sygnał.

## 2. Decyzja architektoniczna

Foundation jest współdzielone w nowym module:

```text
ghost-core::metric_contracts
```

`ghost-core` jest właścicielem typów, ponieważ późniejsze PR2A/PR2B/PR2C będą
potrzebować identycznych struktur w producerach, materializacji, loggerze,
replayu i narzędziach audytowych. Moduł nie czyta live state i nie jest
consumerem Gatekeeper policy.

### 2.1 Registry

`METRIC_CONTRACTS_V1_1` zawiera dokładnie dziesięć rodzin i 32 rozłączne,
surface-qualified powierzchnie:

| Rodzina | Normatywne rozdzielenie |
| --- | --- |
| FTDI | legacy runtime value, typed equivalent value, legacy/corrected actionability i export HHI |
| dev buy | TxIntel first-observed, GatekeeperBuffer primary, dwa MFS surfaces i effective policy read |
| same-ms | full-window exact legacy, typed exact, `<50 ms` cluster i recent exact |
| top3 | preferred ratio, compatibility alias i effective selector |
| flip | frozen legacy slot-gap i evidence-only deterministic hybrid V2 |
| legacy FSC | legacy producer, typed equivalent legacy FSC, FSC v2 readiness i export HHI |
| FSC status | osobna compatibility surface dla `evidence_status.fsc` |
| manipulation | legacy numeric/default, presence-aware numeric, legacy flags i derived flags |
| reserve velocity | legacy scalar i typed interval/status evidence |
| recent buy/sell | legacy logging scalar i typed counts/ratios/share |

Registry zapisuje canonical name, wersję, unit, population/denominator oraz
interpretację. Profile validation odrzuca missing, duplicate, unknown lub
surface-to-contract mismatch.

### 2.2 Globalny ceiling i Profile A

Zdefiniowano:

```text
MetricContractRolloutMode = Legacy | DualCompute | V2
MetricContractProfileIdV1 = metric_contracts_v1_1_profile_a
```

Globalny mode wybiera kolumnę profilu, nie nadaje authority automatycznie.
Każda surface ma:

```text
authority_class
legacy_role
dual_compute_role
v2_role
```

Profile A zachowuje legacy authority w `Legacy` i `DualCompute`. W `V2`
autorytatywne mogą stać się wyłącznie typed representations oznaczone
`EquivalentCutover`. `Compatibility`, `Counterfactual`, `EvidenceOnly`,
`LoggingOnly` i `ExportOnly` nigdy nie stają się policy-authoritative.

W szczególności nie są promowane:

- corrected FTDI unique-buyer actionability;
- dev-primary buy;
- same-ms `<50 ms`;
- flip V2;
- FSC v2;
- coordination HHI;
- reserve velocity evidence;
- recent buy/sell;
- manipulation candidates bez późniejszego formalnego parity proof.

Profile entries mają canonical `(contract_id, surface_id)` order. To usuwa
możliwość otrzymania różnych hashy dla semantycznie identycznej macierzy tylko
przez permutację tablicy.

Profile hash payload zawiera również canonical snapshot dziesięciu definicji
registry: version, name, unit, population/denominator, interpretation i listę
surfaces. Zmiana semantyki registry bez zgodnej zmiany wersjonowanego payloadu
jest odrzucana jako `RegistrySemanticMismatch`; hash nie chroni wyłącznie
routingu authority.

### 2.3 Bezpieczna aktywacja config

Do `GhostBrainConfig` dodano addytywnie i z `#[serde(default)]`:

```toml
metric_contract_rollout_mode = "legacy"
metric_contract_profile = "metric_contracts_v1_1_profile_a"
```

Brak pól w starym TOML daje dokładnie Legacy + Profile A. Nieznany mode lub
profile failuje deserializację. Launcher parsuje te dwa pola osobno przed
historycznym full-config fallbackiem, dlatego fallback unrelated analytical
config nie może ukryć błędnej authority konfiguracji.

PR1 zna wartości `dual_compute` i `v2` na poziomie schema, ale launcher celowo
odrzuca je kodem:

```text
METRIC_CONTRACT_ROLLOUT_MODE_NOT_ACTIVE_IN_PR1
```

Zapobiega to silent no-op, w którym config deklarowałby dual/V2 bez aktywnych
producerów, comparatora i durable evidence.

### 2.4 CanonicalHashV1

Przyjęto normatywnie:

```text
canonicalization = RFC 8785 JCS
encoding = UTF-8
algorithm = SHA-256
digest = lowercase 64-char hex
hash input = typed semantic payload bez self-hash i transport fields
explicit unavailable = wymagany JSON null
omitted required key = error
newline/BOM = poza hashem
NaN/+Inf/-Inf = forbidden
wide u64/u128/i64 = canonical base-10 JSON string
```

Oddzielne payload structs chronią profile, effective config i evidence przed
przypadkowym hashowaniem własnego digestu, writer timestampu albo rotation
metadata. `CanonicalNullableV1<T>` zachowuje różnicę między wymaganym `null` i
brakiem klucza.

Dodano `CanonicalU128StringV1`, ponieważ aktywny early-fingerprint przechowuje
raw token deltas i ich sumy jako `u128`; zawężenie do `u64` byłoby błędem
kontraktu dla legalnego runtime value.

### 2.5 Effective config hash

`ResolvedMetricContractEffectiveConfigV1` ma closed key vocabulary dla
ustawień wpływających na:

- population, success, dust i denominator;
- identity, dedupe, order, capacity, eviction i reconnect;
- time/slot windows i anchors;
- FSC coverage/readiness;
- manipulation presence/threshold provenance;
- reserve clock/fallback;
- recent buy/sell window;
- comparator normalization/status/actionability.

Builder failuje na brakującym lub podwójnym kluczu, złym value kind, NaN/Inf,
ratio poza `[0,1]`, blank enum/text oraz profile-hash mismatch. Entries są
sortowane kanonicznie przed hashem. Custom deserialization ponownie liczy hash i
odrzuca zmieniony transport.

PR1 definiuje typ i algorytm; rzeczywiste resolved producer values zostaną
związane z runtime przez PR2A/PR2B, a durable emission przez PR2C. Sam typ nie
udaje jeszcze runtime provenance.

### 2.6 Record identity kontra source-event identity

Rozdzielono:

```text
duplicate record identity = (run_id, join_key, decision_plane)
stable source event identity = source + tagged stable key
```

Stable key preferuje signature; dopuszczalne fallbacki to jawne
`slot + transaction_index` albo `slot + event_ordinal`. Brak stabilnej identity
ma być unavailable/not-evaluable, nigdy receive-order guess. Ten sam `join_key`
w dwóch runach nie jest samodzielnie duplicate record.

### 2.7 Canonical status envelope i adaptery

Canonical envelope zawiera:

```text
contract_id / contract_version / surface_id
authority_class
availability
measurement_quality
policy_actionable
typed reason_codes
```

Walidacja failuje między innymi na:

- available + `NotApplicable`;
- unavailable + actionable;
- insufficient/stale/legacy-default + actionable;
- non-policy authority class + actionable;
- contract/surface/version mismatch;
- authority/profile mismatch;
- reason family przypiętej do złego kontraktu.

Dodano jawne, exhaustive adaptery dla:

```text
EvidenceStatus
MetricEvidenceQuality
FscEvidenceStatus
FeatureEvidenceStatus
EvidenceDegradedReason
EvidenceUnavailableReason
FscExcludedReason
legacy string reasons
```

Unknown legacy string jest zachowywany jako typed
`UnmappedLegacyString { contract_id, raw }`; nie znika i nie zostaje Clean.

### 2.8 Shared evidence schemas bez aktywacji

Zdefiniowano serde-compatible schemas dla wszystkich dziesięciu rodzin oraz
compact `MetricContractDecisionSummaryV1` zarezerwowany dla v34. Pełny evidence
transport:

- wymaga wszystkich dziesięciu rodzin i wszystkich 32 surface envelopes;
- weryfikuje profile ID/hash oraz effective-config hash reference;
- oddziela semantic evidence hash od writer/rotation metadata;
- odrzuca unknown fields, malformed required-nullable fields i hash mismatch;
- nie jest jeszcze dołączony do `MaterializedFeatureSet` ani DecisionLogger.

Presence-aware manipulation schema rozróżnia missing raw field od measured
zero, ma measured-fields mask i osobne legacy/derived flags. Derived flag
cross-checkuje raw value/status, comparator i threshold.

Flip schema odzwierciedla normatywny automat: owner state, stable anchor i
qualifying-sell identity, slot/timestamp, pre-anchor sells, cumulative `u128`
buys/sells oraz aggregate numerator/denominator. Walidacja sprawdza owner
uniqueness, status completeness, canonical time/slot window i aggregate ratio.

Legacy FSC validator zachowuje realną semantykę kodu: dla mniej niż dwóch known
sources wynik jest `null`, nie `0.0`. `evidence_status.fsc` jest cross-checkowane
z osobnym FSC v2 status i coverage.

Recent buy/sell zachowuje legacy edge case tylko na legacy surface:

```text
sell_count > 0  → buy_count / sell_count
sell_count == 0 i tx_count > 0 → buy_count
tx_count == 0 → null
```

Nowy `buy_to_sell_ratio` jest unbounded i `null` przy zero sells, a `buy_share`
jest osobnym bounded ratio.

## 3. Top3 callsite reconciliation

Nie utworzono drugiego helpera. Istniejący
`TxIntelFeatures::effective_top3_signer_volume_ratio()` pozostaje jedynym
preferred-plus-fallback selectorem.

Dwa bezpośrednie odczyty legacy TxIntel aliasu w materializacji manipulation
contradictions zostały zastąpione tym helperem. Dla bieżącego producera oba
pola są bit-for-bit równe; dla starego payloadu brak preferred field korzysta z
legacy fallbacku. Static guard obejmuje aktywne materialization, policy i
Gatekeeper assessment/log adaptery.

Pole historyczne `top3_volume_pct` pozostaje w MFS/log schema jako ratio-scale
compatibility alias. Nie dokonano destructive rename.

## 4. Jawne granice i preserved invariants

PR1 nie zmienia:

- `MaterializedFeatureSet` layout ani feature ownership;
- Gatekeeper V2/V2.5 thresholds, weights, phase order, hard/soft gates;
- BUY/REJECT/TIMEOUT verdict ani reason-code taxonomy;
- IWIM, post-buy, Trigger, sender, simulation/live behavior;
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 33`;
- V3 replay payload schema v1;
- selector model, score, mapping albo sidecar format;
- legacy producer formulas;
- active authority source;
- `fsc_v2`, flip V2, reserve, recent ratio lub coordination HHI policy role.

`git diff origin/main` jest pusty dla:

```text
ghost-launcher/src/components/gatekeeper_policy.rs
ghost-launcher/src/components/gatekeeper.rs
ghost-brain/src/oracle/decision_logger.rs
ghost-core/src/checkpoint/types.rs
ghost-launcher/src/bin/v3_replay.rs
```

## 5. Wykryte podczas review i skorygowane luki

Ręczny contract review po pierwszym zielonym teście wykrył:

1. FSC validator początkowo traktował jeden known source jako measured `0.0`.
   Aktywny producer zwraca `None` dla `<2`; validator i regresja zostały
   poprawione.
2. Flip cumulative token amounts początkowo używały `u64`. Aktywny
   early-fingerprint używa `u128`; schema otrzymała canonical `u128` string.
3. Pierwszy flip schema nie utrwalał kompletnej stable identity i czasu
   qualifying sell. Dodano event identity, slot i timestamp oraz state
   invariants.
4. Typed reason enum dopuszczał syntaktycznie poprawną rodzinę reason przy
   niewłaściwym kontrakcie. Deserializacja i profile validation teraz failują.
5. Manipulation derived flags nie cross-checkowały raw field i comparator truth
   table. Walidacja została domknięta.

Te korekty nie dotknęły active policy; zapobiegają wprowadzeniu wadliwego
evidence contract do PR2.

## 6. Weryfikacja

Pełne wyniki i komendy są w
`reports/metric_contracts/pr1_foundation_verification_v1.md`.

Najważniejsze wyniki:

```text
ghost-core foundation tests                 15/15 PASS
Gatekeeper V2 integration parity            46/46 PASS
Gatekeeper V2.5 regression parity           42/42 PASS
top3 focused parity                           4/4 PASS
PR1 static/refactor invariants               12/12 PASS
old/current TOML foundation compatibility     3/3 PASS
launcher PR1 startup guard                    1/1 PASS
v33 DecisionLogger write                      1/1 PASS
V3 replay-payload compatibility               5/5 PASS
cargo check -p ghost-launcher                     PASS
cargo fmt --all -- --check                        PASS
targeted ghost-core clippy                        PASS
git diff --check                                  PASS
```

Selector logging sweep: 8/9 PASS, z jednym istniejącym baseline test failure
`test_selector_shadow_score_filters_non_finite_feature_values`. PR1 nie zmienia
tego pliku; working-tree i `origin/main` mają identyczny blob
`790db5300db39036a6309552d75578ebeac2aa1c`. Test oczekuje missing first-price,
podczas gdy istniejący helper jawnie wybiera pierwszy finite price. Nie
rozszerzono PR1 o naprawę unrelated selector contract.

Repo ma liczne istniejące warnings. Targeted clippy dla nowych foundation tests
kończy się kodem 0 i nie raportuje ostrzeżeń w `metric_contracts/*`.

## 7. Pliki PR1

Nowe:

- `ghost-core/src/metric_contracts/mod.rs`
- `ghost-core/src/metric_contracts/canonical_hash.rs`
- `ghost-core/src/metric_contracts/registry.rs`
- `ghost-core/src/metric_contracts/identity.rs`
- `ghost-core/src/metric_contracts/effective_config.rs`
- `ghost-core/src/metric_contracts/status.rs`
- `ghost-core/src/metric_contracts/evidence.rs`
- `ghost-core/tests/metric_contracts_v1_1_foundation.rs`
- `reports/metric_contracts/pr1_foundation_verification_v1.md`
- `docs/ADR/ADR_8D_PR1_METRIC_CONTRACT_REGISTRY_PROFILE_STATUS_FOUNDATION_20260711.md`

Zmodyfikowane:

- `ghost-core/Cargo.toml`
- `ghost-core/src/lib.rs`
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/src/config/mod.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/tests/refactor_invariants_tests.rs`

`Cargo.lock` nie wymaga zmiany, ponieważ wybrana biblioteka JCS już występowała
w lockfile jako zależność tranzytywna.

Niezwiązane istniejące zmiany użytkownika, configs, selector reports i scripts
nie należą do PR1 i nie były modyfikowane ani usuwane.

## 8. Rollback i następny handoff

Rollback PR1 polega na odwróceniu wyłącznie plików z sekcji 7. Nie wymaga
migracji logów, ponieważ v34 i evidence sidecar nie są aktywne, a v33/V3-v1
pozostają bez zmian.

Po review i przyjęciu tego milestone następny zakres to wyłącznie PR2A:

```text
active/parity-sensitive producers
FTDI + dev surfaces + same-ms + top3 mismatch telemetry + typed legacy FSC
legacy terminal behavior unchanged
```

PR2B, PR2C, burn-in i PR3 pozostają zablokowane do sekwencyjnego acceptance.

```yaml
delegation_trace:
  task_classification: cross-cutting metric-contract foundation implementation
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - SSOT Feature Materialization Guardian
    - Config Rollout Safety Reviewer
    - Decision Logging Replay Analyst
    - Gatekeeper Policy Auditor
  specialist_docs_loaded:
    - docs/agents/ghost-runtime-coordinator.md
    - docs/agents/ssot-feature-materialization-guardian.md
    - docs/agents/config-rollout-safety-reviewer.md
    - docs/agents/decision-logging-replay-analyst.md
  specialist_docs_not_loaded:
    - name: Gatekeeper Policy Auditor
      reason: policy source was protected by empty-diff and parity suites; no policy edit was authorized
    - name: Seer Ingest Event Integrity Specialist
      reason: PR1 defines identity types but does not change ingest or event ordering
    - name: Solana Execution Path Engineer
      reason: execution path and shadow/live behavior are outside PR1
  skills_used:
    - ghost-execution
    - rust-master
    - abstract-reasoning
  fast_path_used: false
  contracts_checked:
    - MaterializedFeatureSet remains the decision SSOT
    - no competing policy feature computation
    - active authority remains legacy in PR1
    - old TOML defaults and unknown-value fail-closed behavior
    - RFC 8785 canonical hashing and self-hash exclusion
    - record identity versus stable source-event identity
    - canonical availability, quality and actionability
    - typed reason family ownership
    - decision schema v33 and V3 replay v1 freeze
    - Gatekeeper verdict, reason, phase and soft-point parity
    - selector output path unchanged
    - shadow/live separation
  unresolved_routing_uncertainty: []
```
