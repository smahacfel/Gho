# PLAN WYKONAWCZY GHOST DECISION STACK V3 P0

> Data: 2026-05-13  
> Status: plan wykonawczy do implementacji  
> Zakres: P0 shadow/evidence plane + roadmapa P1/P2  
> Repo baseline: `/root/Gho`, branch `main`, HEAD `d96aba8`  
> Zrodlo koncepcyjne: `V3.md` w root repo  

---

## 1. Executive Verdict

Ghost Decision Stack V3 nalezy wdrozyc jako addytywny, rownolegly
shadow/evidence plane nad obecnym pipeline'em. P0 nie jest aktywna promocja
nowej polityki i nie jest rewrite Gatekeepera.

Docelowy przeplyw P0:

```text
PoolObservationSession::materialize_features()
-> MaterializedFeatureSet
-> aktywny Gatekeeper V2/V2.5 bez zmian
-> rownolegly V3 shadow evaluator
-> addytywne v3_shadow_* fields w JSONL
-> raport current-vs-V3
```

P0 ma wyliczac i logowac rownolegly `v3_shadow_verdict`,
`v3_shadow_risk_status`, `v3_shadow_opportunity_status`,
`v3_evidence_status`, confidence breakdown oraz reason codes. Wynik V3 P0 nie
moze zmieniac aktywnego BUY/REJECT/TIMEOUT, IWIM, execution path ani shadow/live
transportu.

Najwazniejsza decyzja architektoniczna: V3 konsumuje wylacznie
`MaterializedFeatureSet`. Wszystko, co wymaga raw tx/ring buffer/session state,
musi zostac policzone w `PoolObservationSession::materialize_features()`.

---

## 2. Stan Repo Potwierdzony

Aktualny stan kodu pasuje do zalozen V3 shadow-first:

- `MaterializedFeatureSet` jest w `ghost-core/src/checkpoint/types.rs` i ma juz
  `account_features`, `tx_intel_features`, `checkpoint_features`, `risk_flags`,
  `session_metadata`, `curve_readiness`, `sybil_resistance`,
  `alpha_fingerprint`, `tx_segment_sequence`.
- `PoolObservationSession::materialize_features()` w
  `ghost-launcher/src/session/observation.rs` jest obecna granica SSOT i juz
  materializuje curve readiness, alpha fingerprint, sybil/CPV/FSC oraz segment
  sequence.
- Aktywna polityka Gatekeeper idzie przez
  `build_assessment_from_features()` i `evaluate_policy_from_assessment()` w
  `ghost-launcher/src/components/gatekeeper_policy.rs`.
- Aktywny runtime wywoluje `GatekeeperBuffer::evaluate_from_features(...)` w
  `ghost-launcher/src/oracle_runtime.rs`.
- `GatekeeperBuyLog` jest w `ghost-brain/src/oracle/decision_logger.rs`, schema
  obecnie `19`; zawiera juz `decision_plane`, `rollout_profile`, `config_hash`
  i pola V2.5 shadow.
- Reason code taxonomy jest w `ghost-brain/src/oracle/reason_code.rs`; brak
  obecnie kodow V3.
- Nie istnieje jeszcze `ghost-launcher/src/components/gatekeeper_v3.rs`.
- Config shadow-burnin jest shadow-only:
  - `configs/rollout/shadow-burnin.toml`: `entry_mode = "shadow_only"`,
  - `configs/rollout/shadow-burnin.toml`: `execution_mode = "shadow"`,
  - `ghost-brain/ghost_brain_config.toml`: `v25.shadow_enabled = true`,
  - `ghost-brain/ghost_brain_config.toml`: `v25.live_execution_enabled = false`,
  - `ghost-brain/ghost_brain_config.toml`: `v25.require_promotion_adr = true`.

Wniosek: P0 moze zostac wdrozone addytywnie bez zmiany aktywnego Gatekeepera.

---

## 3. Niezmienniki P0

Te reguly sa warunkiem merge-ready dla calego P0:

1. `MaterializedFeatureSet` pozostaje jedynym snapshotem decyzyjnym V3.
2. `gatekeeper_v3.rs` nie czyta raw tx, `GatekeeperBuffer`, mutable session
   state, runtime locks, RPC, IWIM ani execution state.
3. `PoolObservationSession::materialize_features()` jest jedynym miejscem,
   gdzie wolno policzyc V3 feature groups wymagajace `tx_buffer`.
4. Brak albo degradacja danych nigdy nie oznacza `clean`.
5. Nie wolno dodawac globalnego `EvidenceStatus::is_actionable()` jako skrotu
   polityki. Actionability jest liczona per feature group i stage.
6. V3 `Pending` pozostaje shadow-only i nie mapuje sie na aktywny
   `PendingCurve`.
7. V3 reason codes nie zastepuja aktywnego `reason_code`; sa logowane jako
   shadow/evidence plane.
8. `DecisionLogger` w `ghost-brain` nie moze zalezec od typow z
   `ghost-launcher`.
9. Wszystkie nowe pola serializowane addytywnie maja `#[serde(default)]`; pola
   opcjonalne maja `skip_serializing_if`.
10. P0 nie zmienia `DirectBuyBuilder`, `DirectSellBuilder`, `LiveTxSender`,
    blockhash/retry, IWIM policy ani trigger execution handoff.
11. P0 nie reaktywuje HyperPrediction, Chaos, `score_pool()` ani legacy
    `PoolScored` jako aktywnych dependencies Gatekeepera.

---

## 4. Zakres P0

P0 obejmuje:

- nowe addytywne typy w `ghost-core`,
- materializacje V3 evidence fields w `PoolObservationSession`,
- nowy pure evaluator `gatekeeper_v3.rs`,
- addytywne reason codes i JSONL fields,
- integracje logowania V3 shadow bez zmiany active verdictu,
- raport `scripts/v3_shadow_report.py`,
- testy serde, evaluator, logger, active-verdict invariance i raport.

P0 nie obejmuje:

- aktywnego przepisania Gatekeeper verdictu,
- aktywacji V3 BUY/REJECT/TIMEOUT jako live policy,
- zmian IWIM ordering/policy,
- zmian execution/live sender/shadow transport semantics,
- config-driven pelnej kalibracji progow,
- ML/statystycznego kalibratora,
- promocji hard gates bez ADR.

---

## 5. Workstream 0 - Freeze Kontraktu

### Cel

Zamrozic semantyke P0 przed implementacja, zeby uniknac mieszania active plane,
V2.5 shadow plane i nowego V3 shadow plane.

### Kroki

1. Potwierdzic w PR opisie albo osobnym ADR, ze P0 jest shadow/evidence only.
2. Wskazac, ze aktywny Gatekeeper V2/V2.5 pozostaje source of active runtime
   verdict.
3. Zapisac zakazy P0:
   - brak zmian `GatekeeperBuffer::evaluate_from_features()` semantics,
   - brak zmian IWIM,
   - brak execution path changes,
   - brak live sender changes,
   - brak legacy scoring revival.
4. Zapisac rollback: odciecie V3 call site i ignorowanie `v3_shadow_*` fields
   przy zachowaniu dotychczasowej aktywnej polityki.

### Acceptance

- PR/ADR jasno rozdziela `legacy_live`, `v25_shadow`, `v3_shadow`.
- Nie ma zmian aktywnego execution/live path.
- Operator nie moze zinterpretowac V3 P0 jako promotion-ready live policy.

---

## 6. Workstream 1 - Typy SSOT w ghost-core

### Cel

Dodac kontrakt danych V3 do `MaterializedFeatureSet`, tak aby V3 evaluator
mogl dzialac deterministycznie bez czytania runtime state.

### Pliki

- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- testy w `ghost-core/tests/feature_builder_tests.rs`
- testy w `ghost-core/tests/pr1_contracts_foundations.rs`

### Typy do dodania

```rust
EvidenceStatus
EvidenceDegradedReason
EvidenceUnavailableReason
MaterializedEvidenceStatus
OrganicBroadeningFeatures
ManipulationContradictionFeatures
```

Wymagany model `EvidenceStatus`:

- `Clean`
- `Degraded`
- `Unavailable`
- `InsufficientSample`
- `Stale`
- `Fallback`
- `ShadowOnly`
- `NotConfigured`

Default nie moze byc `Clean`. Bezpieczny default dla oczekiwanej grupy danych
to `Unavailable`; dla modulow celowo wylaczonych `NotConfigured`.

### Pola do dodania w `MaterializedFeatureSet`

```rust
#[serde(default)]
pub v3_evidence_status: MaterializedEvidenceStatus,

#[serde(default)]
pub v3_organic_broadening: OrganicBroadeningFeatures,

#[serde(default)]
pub v3_manipulation_contradictions: ManipulationContradictionFeatures,
```

Jesli implementacja wybierze `Option<T>`, musi uzyc:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
```

### Minimalne pola `MaterializedEvidenceStatus`

Statusy per grupa:

- `identity`
- `account_state`
- `curve`
- `tx_intel`
- `trajectory`
- `pdd_sequence`
- `alpha`
- `sybil`
- `cpv`
- `fsc`
- `organic_broadening`
- `manipulation_contradiction`
- `execution`

Kazda grupa musi niesc:

- `status`,
- `reason` albo `reasons`,
- opcjonalnie `observed`,
- opcjonalnie `required`,
- opcjonalnie `source`.

### Minimalne pola `OrganicBroadeningFeatures`

P0:

- `t0_tx_count`
- `t1_tx_count`
- `t2_tx_count`
- `t0_unique_signers`
- `t1_unique_signers`
- `t2_unique_signers`
- `signer_growth_t2_t0`
- `hhi_delta_t2_t0`
- `tx_count_growth_vs_signer_growth`
- `new_signer_ratio_t2`
- `broadening_score`
- `status`
- `degraded_reasons`

P1:

- `top3_delta_t2_t0`
- `repeat_buyer_pressure`

### Minimalne pola `ManipulationContradictionFeatures`

P0:

- `momentum_without_broadening`
- `volume_spike_without_new_signers`
- `high_buy_pressure_with_high_top3`
- `fixed_size_or_ramping_pattern`
- `timing_bundle_concentration`
- `early_top3_concentration`
- `contradiction_score`
- `status`
- `reasons`

P1:

- `static_fee_cu_similarity`
- bardziej granularne alpha/funding contradiction fields.

### Acceptance

- Stare JSON snapshots `MaterializedFeatureSet` deserializuja sie bez V3 fields.
- Domyslne V3 evidence nie jest `Clean`.
- `feature_builder.materialize(...)` wypelnia V3 fields defaultami.
- Re-exporty w `checkpoint/mod.rs` sa kompletne.
- Nie ma nowych zaleznosci z `ghost-core` do `ghost-launcher`.

---

## 7. Workstream 2 - Materializacja V3 w Sesji

### Cel

Policzyc V3 feature groups na granicy SSOT, bez przenoszenia raw tx computation
do polityki.

### Pliki

- `ghost-launcher/src/session/observation.rs`
- ewentualnie lokalne prywatne helpery w tym samym module
- testy w `ghost-launcher/tests/session_lifecycle_tests.rs`
- testy w `ghost-launcher/tests/gatekeeper_v25_regression.rs`

### Helpery do dodania

```rust
fn materialize_v3_evidence_status(...) -> MaterializedEvidenceStatus
fn materialize_v3_organic_broadening(...) -> OrganicBroadeningFeatures
fn materialize_v3_manipulation_contradictions(...) -> ManipulationContradictionFeatures
```

### Zasady

- `organic_broadening` i `manipulation_contradictions` wolno liczyc z
  `tx_buffer` tylko w `PoolObservationSession::materialize_features()`.
- `gatekeeper_v3.rs` dostaje gotowe pola i nie rekonstruuje segmentow z raw tx.
- Missing alpha/CPV/FSC/curve/PDD sequence ma byc odroznione od clean pass.
- Curve/account fallback ma byc jawnie widoczny w evidence status.
- Execution evidence w pure P0 evaluatorze ma byc `Unavailable` albo
  `NotConfigured` z reasonem `execution_not_run`.

### Minimalna logika P0

Organic broadening:

- policzyc segmentowe unique signer counts dla T0/T1/T2,
- porownac signer growth z tx growth,
- wykorzystac `tx_segment_sequence` dla HHI i tx counts,
- jesli segment sequence jest niedostepna, ustawic
  `InsufficientSample` albo `Unavailable`, nie `Clean`.

Manipulation contradiction:

- high momentum + brak broadening,
- volume spike + brak nowych signerow,
- high buy pressure + high top3,
- fixed-size/ramping na bazie alpha + segment same-size/ramping,
- same-ms/bundle concentration z `TxIntelFeatures`,
- early top3 concentration z `AlphaFingerprintFeatures`.

### Acceptance

- V3 materializacja jest czescia `materialize_features()`.
- V3 materializacja nie zmienia istniejacych pol `MaterializedFeatureSet`.
- Brak segmentow albo brak alpha daje jawny evidence status.
- Test potwierdza, ze V3 materializacja nie mutuje aktywnego Gatekeeper state.

---

## 8. Workstream 3 - Pure Evaluator V3

### Cel

Dodac deterministyczny evaluator V3 P0 konsumujacy tylko
`MaterializedFeatureSet` i config.

### Pliki

- nowy `ghost-launcher/src/components/gatekeeper_v3.rs`
- `ghost-launcher/src/components/mod.rs`
- testy unit albo integration w `ghost-launcher`

### API

```rust
pub fn evaluate_v3_from_features(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
    deadline_elapsed: bool,
) -> V3ShadowDecision
```

P0 moze uzywac `GatekeeperV2Config` i V2.5 thresholds jako inputu. Pelny
`GatekeeperV3Config` nalezy zostawic do P1.

### Typy lokalne w launcherze

```rust
DecisionStage
RiskVerdict
RiskVerdictStatus
OpportunityVerdict
OpportunityVerdictStatus
ConfidenceBreakdown
V3ShadowDecision
V3ShadowVerdict
```

Te typy nie moga byc importowane przez `ghost-brain`.

### Decision Tree P0

1. Snapshot invalid albo critical evidence unavailable:
   - przed deadline: `PendingV3WaitEvidence`,
   - po deadline: `TimeoutV3DegradedEvidence`.
2. Identity/protocol contradiction:
   - `Reject` z precyzyjnym reasonem, jesli dowod jest clean.
3. Hard risk actionable:
   - `Reject`.
4. Sample insufficient:
   - przed deadline: `PendingV3WaitSample`,
   - po deadline: `TimeoutV3DegradedEvidence`.
5. Manipulation contradiction severe:
   - `RejectV3ManipulationContradiction`.
6. Low organic broadening przy pump-like activity:
   - `RejectV3LowOrganicBroadening`.
7. Opportunity sufficient + risk clean + confidence above stage threshold:
   - `BuyV3*`.
8. Opportunity weak przy wystarczajacym clean sample:
   - `RejectV3LowOpportunity`.
9. Confidence unresolved:
   - przed deadline: `PendingV3WaitEvidence`,
   - po deadline: `TimeoutV3UnresolvedConfidence`.

### Confidence P0

P0 ma logowac breakdown, nawet jesli progi sa shadow-only:

- `opportunity_score`
- `risk_penalty`
- `confidence_raw`
- `confidence_after_risk`
- `confidence_after_stage`
- `confidence_cap`
- `confidence_cap_reasons`
- `confidence_final`

Execution multiplier w P0 nie moze oznaczac sukcesu. Ma byc
`execution_not_run` i nakladac cap tylko w V3 shadow.

### Acceptance

- API nie przyjmuje `GatekeeperBuffer`, raw tx ani session refs.
- Ten sam snapshot + config daje ten sam V3 output.
- Hard risk wygrywa z opportunity.
- Missing critical evidence nie daje BUY.
- V3 Pending nie dotyka aktywnego `PendingCurve`.

---

## 9. Workstream 4 - Reason Codes i JSONL

### Cel

Dac V3 shadow verdict audytowalny reason code i trwale pola JSONL bez
cyklicznych zaleznosci crate'ow.

### Pliki

- `ghost-brain/src/oracle/reason_code.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- testy w tych modulach

### Reason codes P0

Dodac warianty enum:

```text
BUY_V3_NORMAL_CONFIRMED_OPPORTUNITY
BUY_V3_EARLY_CLEAN_MOMENTUM
BUY_V3_EXTENDED_RECOVERED_EVIDENCE
REJECT_V3_MANIPULATION_CONTRADICTION
REJECT_V3_LOW_ORGANIC_BROADENING
REJECT_V3_LOW_OPPORTUNITY
TIMEOUT_V3_DEGRADED_EVIDENCE
TIMEOUT_V3_UNRESOLVED_CONFIDENCE
PENDING_V3_WAIT_EVIDENCE
PENDING_V3_WAIT_SAMPLE
```

Podbic `GatekeeperReasonCode::version()` tylko jesli PR swiadomie zmienia
taxonomy version. W takim przypadku dodac test wersji.

### `GatekeeperBuyLog` addytywne pola P0

Dodać:

```text
v3_shadow_verdict
v3_shadow_stage
v3_shadow_reason_code
v3_shadow_secondary_reason_codes
v3_shadow_risk_status
v3_shadow_risk_primary_reason
v3_shadow_risk_penalty
v3_shadow_opportunity_status
v3_shadow_opportunity_score
v3_shadow_confidence_raw
v3_shadow_confidence_after_risk
v3_shadow_confidence_after_stage
v3_shadow_confidence_cap
v3_shadow_confidence_cap_reasons
v3_shadow_confidence_final
v3_evidence_status
v3_organic_broadening
v3_manipulation_contradictions
```

Rekomendowany typ w `ghost-brain`: prymitywy JSONL:

- `Option<String>`
- `Option<f64>`
- `Option<bool>`
- `Option<Vec<String>>`
- opcjonalnie `serde_json::Value` dla map evidence/statusow

Nie importowac `ghost-launcher::components::gatekeeper_v3::*`.

### Schema

Po dodaniu pol bump:

```rust
GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 20
```

Warunek: test backward compatibility dla starych rekordow schema v19.

### Acceptance

- V3 shadow row ma typed reason code.
- Stare logi bez `v3_shadow_*` deserializuja sie poprawnie.
- `DecisionLogger` nie zalezy od `ghost-launcher`.
- Plane routing nie gubi V3 row z powodu braku `reason_code`.
- `decision_verdict_buy=true` nie jest ustawiane dla V3 shadow przez przypadek.

---

## 10. Workstream 5 - Integracja Runtime/Logger

### Cel

Wywolac V3 evaluator jako sidecar i dolaczyc wynik do logow bez zmiany aktywnej
decyzji.

### Pliki

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_v3.rs`
- ewentualnie lokalny adapter flat log fields

### Preferowany model integracji

1. Aktywny runtime materializuje features jak dotad.
2. Aktywny Gatekeeper produkuje verdict jak dotad.
3. V3 evaluator jest wywolywany z tym samym `MaterializedFeatureSet`.
4. Wynik V3 jest konwertowany do flat/log-safe fields.
5. `GatekeeperBuyLog` dostaje `v3_shadow_*`.
6. Aktywny verdict, reason chain, IWIM i execution handoff pozostaja bez zmian.

### Miejsca, ktorych nie wolno semantycznie zmieniac

- `GatekeeperBuffer::evaluate_from_features()`
- `build_assessment_from_features()`
- `evaluate_policy_from_assessment()`
- `evaluate_curve_gate()`
- `GatekeeperBuffer::try_shadow_evaluate()`
- `PoolObservationSession::legacy_test_verdict_from_transaction()`
- `OracleRuntime::score_pool()`
- `DirectBuyBuilder`, `DirectSellBuilder`, `LiveTxSender`
- IWIM policy functions

### Acceptance

- Test pokazuje, ze `GatekeeperBuffer::evaluate_from_features()` zwraca ten sam
  active verdict z i bez V3 fields.
- V3 output jest widoczny w JSONL.
- V3 nie odpala triggera, live sendera, IWIM ani shadow transportu.
- V3 `Pending` jest tylko polem logowym.

---

## 11. Workstream 6 - Raport V3

### Cel

Dac operatorowi i audytowi porownywalna telemetryke current-vs-V3 bez robienia
z niej promotion gate.

### Pliki

- nowy `scripts/v3_shadow_report.py`
- nowy `scripts/test_v3_shadow_report.py`
- import helperow z `scripts/shadow_run_report.py`, gdzie to ma sens

### Dlaczego osobny skrypt

`scripts/shadow_run_report.py` jest obecnie go/no-go burn-in reconciliation.
`gatekeeper_v25_repair_validation.py` jest fail-closed walidatorem V2.5 repair.
V3 P0 potrzebuje raportu analitycznego current-vs-V3, nie kolejnego promotion
gate.

### Minimalny raport P0

JSON output:

```text
schema_version
input_paths
record_counts
current_vs_v3_matrix
v25_vs_v3_matrix
v3_verdict_distribution
v3_reason_code_distribution
v3_confidence_buckets
v3_evidence_distribution
v3_buy_candidates
v3_timeout_taxonomy
degraded_unavailable_summary
warnings
```

### CLI

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --json
```

### Acceptance

- Raport dziala, gdy nie ma V3 fields: zwraca jasny status `no_v3_fields`.
- Raport nie failuje no-dispatch session jak lifecycle/economics gate.
- Raport rozdziela decision quality od execution/shadow lifecycle quality.
- Raport nie traktuje unknown status jako success.

---

## 12. Test Plan

### Rust - minimalna walidacja P0

```bash
cargo test -p ghost-core feature_builder
cargo test -p ghost-core materialized
cargo test -p ghost-brain reason_code
cargo test -p ghost-brain decision_logger
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher --test gatekeeper_v25_regression v3
```

Jesli nazwy testow beda inne po implementacji, implementer ma uruchomic
najwezszze odpowiedniki.

### Python

```bash
python3 -m unittest scripts/test_shadow_run_report.py
python3 -m unittest scripts/test_v3_shadow_report.py
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

### Regresje obowiazkowe

1. Stary `MaterializedFeatureSet` deserializuje sie z V3 defaults.
2. Stary `GatekeeperBuyLog` schema v19 deserializuje sie po dodaniu v20 fields.
3. Default evidence status nie jest `Clean`.
4. Missing critical evidence daje `Pending` przed deadline.
5. Missing critical evidence daje `Timeout` po deadline.
6. Hard risk blokuje V3 BUY.
7. Manipulation contradiction blokuje fake momentum.
8. Low organic broadening blokuje high pump-like activity.
9. V3 evaluator nie zmienia aktywnego Gatekeeper verdictu.
10. Reason codes V3 roundtripuja przez serde i `from_log_str()`.
11. Logger serializuje V3 fields addytywnie.
12. `ghost-brain` nie importuje typow z `ghost-launcher`.

### Opcjonalna walidacja koncowa

```bash
cargo test --workspace
cargo fmt --check
```

Uruchomic tylko jesli czas i stan repo na to pozwalaja. Dla PR etapowych
wystarcza najpierw targeted crate tests.

---

## 13. Kolejnosc Commitow / PR-ow

Rekomendowany podzial:

1. **Commit 1 - types only**
   - `ghost-core` typy V3,
   - serde defaults,
   - feature builder defaults,
   - core tests.

2. **Commit 2 - materialization only**
   - helpery w `PoolObservationSession`,
   - evidence status,
   - organic broadening,
   - manipulation contradiction,
   - materialization tests.

3. **Commit 3 - reason/log schema only**
   - V3 reason codes,
   - `GatekeeperBuyLog` v20 fields,
   - backward compatibility tests.

4. **Commit 4 - shadow evaluator only**
   - `gatekeeper_v3.rs`,
   - pure evaluator tests.

5. **Commit 5 - runtime log integration**
   - sidecar wywolanie V3,
   - log enrichment,
   - active-verdict invariance tests.

6. **Commit 6 - report script**
   - `scripts/v3_shadow_report.py`,
   - `scripts/test_v3_shadow_report.py`,
   - docs/usage.

Ten podzial minimalizuje ryzyko, ze blad w loggerze albo runtime integration
zostanie pomylony z bledem w typach/materializacji.

---

## 14. Acceptance Criteria P0

P0 jest gotowe, gdy:

- wszystkie nowe pola sa addytywne i backward-compatible,
- V3 evaluator dziala tylko na `MaterializedFeatureSet`,
- V3 shadow verdict jest logowany,
- raport current-vs-V3 dziala,
- aktywny Gatekeeper verdict nie zmienia sie,
- IWIM nie zmienia pozycji ani polityki,
- execution path nie jest dotkniety,
- V3 Pending jest tylko shadow/log semantics,
- missing/degraded evidence nie jest clean pass,
- testy targeted przechodza,
- dokumentacja/PR jasno mowi, ze V3 P0 nie jest live promotion.

---

## 15. Rollback

Rollback P0:

1. Wylaczyc V3 call site w runtime/log enrichment.
2. Ignorowac `v3_shadow_*` fields w raportach.
3. Zostawic addytywne pola serde, bo sa backward-compatible.
4. W razie potrzeby usunac `gatekeeper_v3.rs`.

Aktywny Gatekeeper i execution path pozostaja bez zmian, wiec rollback nie
powinien zmieniac zachowania runtime.

---

## 16. Najwieksze Ryzyka

1. Niechciana zmiana aktywnego verdictu przez wpiecie V3 w zlym miejscu.
2. Przeniesienie raw tx computation do `gatekeeper_v3.rs`.
3. Cykliczna zaleznosc `ghost-brain -> ghost-launcher`.
4. Potraktowanie `V3 Pending` jako aktywnego `PendingCurve`.
5. Brak `#[serde(default)]` na nowych polach.
6. Domyslny `EvidenceStatus::Clean`.
7. Mieszanie shadow simulation outcome z pre-execution evaluator confidence.
8. Zbyt wczesne dodanie config thresholds bez danych shadow/replay.
9. Reuzycie generic reject zamiast typed reason codes.
10. Reaktywacja legacy score path pod etykieta V3.

---

## 17. Roadmap P1/P2

### P1 - Richer Shadow Staged Funnel

Cel: przeniesc P0 z minimalnego shadow modelu do kalibrowalnego V3 shadow
funnel.

Zakres:

- `GatekeeperV3Config` z `#[serde(default)]`,
- config-driven thresholds i caps,
- per-feature-group actionability,
- component scores,
- feature snapshot hash,
- V3 policy/config hash,
- ablation runner,
- replay parity reporting.

Warunek startu P1:

- P0 generuje stabilne logi V3 shadow,
- raport ma wystarczajaca liczbe rekordow,
- brak regresji active verdictu.

### P2 - Selective Promotion

Cel: promowac tylko zwalidowane V3 gates.

Zakres:

- osobny ADR promotion,
- najpierw Normal-window hard risk gates,
- Early BUY na koncu,
- live canary tylko po shadow/replay evidence,
- rollback config dla kazdego promowanego gate.

Zakazy P2 bez danych:

- brak 68-70% quality claim bez shadow labels,
- brak ML/calibrator live use,
- brak sybil/PDD promotion bez false BUY/false REJECT analizy,
- brak execution infeasibility hard gate bez review execution path.

---

## 18. Delegation Trace

```yaml
delegation_trace:
  task_classification: "cross-cutting V3 P0 execution plan"
  routing_performed: true
  primary_specialist: "Ghost Runtime Coordinator"
  supporting_specialists_considered:
    - "SSOT Feature Materialization Guardian"
    - "Gatekeeper Policy Auditor"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/ssot-feature-materialization-guardian.md"
    - "docs/agents/gatekeeper-policy-auditor.md"
    - "docs/agents/decision-logging-replay-analyst.md"
  specialist_docs_not_loaded:
    - name: "Config Rollout Safety Reviewer"
      reason: "P0 nie zmienia config defaults ani aktywnych thresholdow; config sprawdzony read-only."
    - name: "Solana Execution Path Engineer"
      reason: "P0 nie dotyka DirectBuyBuilder, sendera, blockhash, retry ani confirmation."
    - name: "Seer Ingest Event Integrity Specialist"
      reason: "P0 nie zmienia ingest/parsing/event identity; korzysta z juz zmaterializowanych features."
  skills_used:
    - "ghost-execution"
    - "abstract-reasoning"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet SSOT"
    - "PoolObservationSession materialization boundary"
    - "active Gatekeeper verdict unchanged"
    - "typed reason codes"
    - "additive JSONL schema"
    - "shadow/live separation"
    - "ghost-brain must not depend on launcher"
    - "no legacy scoring revival"
  subagents_used:
    - "materialization/types audit"
    - "Gatekeeper/logging audit"
    - "config/reporting audit"
  unresolved_routing_uncertainty: []
  risk_level: "medium"
```

---

## 19. Final Recommendation

Implementowac V3 jako P0 shadow/evidence plane, nie jako nowy aktywny
Gatekeeper. Obecny repo ma dobre fundamenty: SSOT przez `MaterializedFeatureSet`,
materializacje w `PoolObservationSession`, typed reason codes, V2.5 shadow
telemetry i DecisionLogger. V3 powinien uporzadkowac semantyke dowodow,
rozdzielic risk od opportunity i dodac confidence caps, ale najpierw tylko jako
rownolegla, audytowalna warstwa dowodowa.

Promocja do active policy jest osobnym etapem po shadow/replay validation i ADR.
