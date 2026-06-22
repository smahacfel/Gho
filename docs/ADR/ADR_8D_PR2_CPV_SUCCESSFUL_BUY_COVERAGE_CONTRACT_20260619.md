# ADR-8D: PR2 CPV Successful-Buy Coverage Contract

Status: IMPLEMENTED / STATIC_VALIDATION_COMPLETED
Typ: ADR-8D / CPV evidence coverage and materialization contract
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: PR2 z planu evidence coverage contract; CPV successful-buy signer denominator, low-sample evidence status, `cpv_other_pool_activity` evidence parity i materialization do `MaterializedFeatureSet`
Poziom ryzyka: HIGH

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/tx_intelligence/types.rs`
- `ghost-core/tests/feature_builder_tests.rs`
- `ghost-core/tests/pr1_contracts_foundations.rs`
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`
- `ghost-launcher/tests/cpv_successful_buy_contract_tests.rs`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty w `ADR_8D_PR1_CONFIG_AND_EVIDENCE_FOUNDATION_20260619.md`.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR2 z planu, czyli zwiekszyc pokrycie CPV bez zmiany semantyki metryki. CPV nadal ma mierzyc zachowanie successful-buy signerow w rolling cross-pool indexie, a nie wszystkich signerow, failed tx, sell-only walletow lub ogolnego ruchu adresow.

Rzeczywisty przebieg:
- Potwierdzono, ze PR2 jest aktywnym runtime/materialization path, nie legacy path.
- Potwierdzono, ze PR1 dostarczyl config surface:
  - `cpv_min_successful_buy_signers_clean`
  - `cpv_min_successful_buy_signers_degraded`
  - `cpv_emit_degraded_low_sample`
  - `cpv_allow_degraded_in_strict_policy`
- PR2 podpial tylko sample-policy i evidence materialization. Nie podpial degraded CPV do strict policy.
- Z powodu istniejacego, niezaleznego compile problemu w `ghost-launcher/tests/session_lifecycle_tests.rs` dodano osobny test integracyjny PR2 zamiast mieszac walidacje CPV z uszkodzonym test binary.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT, `MaterializedFeatureSet`, DecisionLogger/replay boundary i shadow/live separation.
- `rust-master`: lokalna implementacja Rust, serde compatibility, minimalizacja regresji w hot path.
- `trading-systems`: rozdzielenie wartosci rynkowej od statusu evidence i ochrona policy przed degraded data.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/config-rollout-safety-reviewer.md`
- `docs/agents/decision-logging-replay-analyst.md`

Powod:
PR2 dotyka `MaterializedFeatureSet`, evidence status/source, config-driven thresholds i JSONL replay evidence. Nie zmienia live execution path ani Solana transaction construction.

## 3. Opis problemu - 3W2H

What:
CPV mial zbyt slaby kontrakt coverage:
- prog successful-buy signer sample byl efektywnie ukryty w kodzie,
- brakowalo jawnego statusu `clean` vs `degraded_low_sample` vs `insufficient_sample` vs `unavailable_source`,
- `cpv_other_pool_activity` nie mial tej samej evidence semantyki co `signer_cross_pool_velocity`,
- konsument nie mogl odroznic "nie ma danych" od "jest niski sample, ale policzony jawnie jako degraded".

Where:
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- `PoolObservationSession::materialize_features()`
- `SybilResistanceFeatures`
- embedded `v3_materialized_feature_snapshot`
- policy helper rozpoznajacy CPV jako actionable/non-actionable

Why it matters:
Jesli CPV coverage jest podbijane przez zmiane denominatora albo przez zamiane missing na zero, metryka przestaje znaczyc "successful-buy cross-pool behavior". To niszczy wiarygodnosc danych i moze nauczyc selector/Gatekeeper artefaktow runtime zamiast zachowania rynku.

How observed:
W runtime artefaktach CPV coverage bylo ograniczone, a uzytkownik wskazal wymog: nie liczyc wszystkich signerow, tylko zostac przy successful-buy signerach i dodac jawny status jakosci probki.

How many / scale:
Zmiana obejmuje CPV-family metrics w aktywnej materializacji. Nie obejmuje PR3 temporal carry-forward, PR4 burst-ratio SSOT ani PR5 policy use of degraded evidence.

## 4. Przyczyna zrodlowa

Root cause:
CPV computation zwracal tylko wartosc albo brak wartosci oraz liste degraded reasons. Brakowalo typed evidence context z:
- sample count,
- required clean/degraded sample count,
- source,
- rolling-state availability,
- wartosciami CPV-family metric,
- statusem jakosci evidence.

Dodatkowo:
Stary field `signer_cross_pool_velocity` byl jednoczesnie convenience fieldem dla policy i jedyna widoczna wartoscia CPV. To wymuszalo niebezpieczny wybor: albo brak coverage, albo ciche traktowanie low-sample jako clean. PR2 rozdziela te prawdy.

## 5. Strategia naprawy

Przyjeta strategia:
- Zachowac denominator jako `unique_successful_signers(transactions)`.
- Zmapowac progi z `GatekeeperV2Config` do `CrossPoolVelocityConfig`.
- Emitowac low-sample value tylko wtedy, gdy `cpv_emit_degraded_low_sample = true` oraz sample count spelnia prog degraded.
- Umiescic degraded low-sample wartosc w `cpv_evidence`, ale nie w starych clean-only polach policy.
- Dac `cpv_other_pool_activity` ten sam sample/status/source contract co `signer_cross_pool_velocity`.
- Dac `evidence_status.cpv` status wynikajacy z typed `cpv_evidence.quality`.
- Dodac defensywny policy guard: `CPV_LOW_SAMPLE_DEGRADED` nie jest actionable w obecnym PR2.

Granice:
- Brak liczenia all signers.
- Brak liczenia failed tx.
- Brak liczenia sell-only signerow.
- Brak zmiany BUY/REJECT/TIMEOUT semantics.
- Brak uzycia degraded CPV w strict policy.
- Brak PR3 carry-forward anchor semantics.
- Brak live/shadow execution changes.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: rozszerzenie CPV evidence types
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano `CpvMetricSource`.
- Rozszerzono `MetricEvidenceQuality` o `unavailable_source` i `not_configured`.
- Rozszerzono `CpvEvidenceContext` o:
  - `source`
  - `signer_cross_pool_velocity`
  - `cpv_other_pool_activity`
  - `degraded_reasons`

Zmiana 2: rozszerzenie `SybilResistanceFeatures`
- Plik: `ghost-core/src/tx_intelligence/types.rs`
- Dodano:
  - `cpv_other_pool_activity`
  - `cpv_evidence`
- Dodano jawne reason constants:
  - `CPV_INSUFFICIENT_SUCCESSFUL_BUY_SIGNERS`
  - `CPV_LOW_SAMPLE_DEGRADED`
  - `CPV_DISABLED_BY_CONFIG`
- Zachowano alias `CPV_INSUFFICIENT_SIGNERS_REASON` dla kompatybilnosci obecnego kodu.

Zmiana 3: config-driven CPV sample contract
- Plik: `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- `CrossPoolVelocityConfig::from_gatekeeper_config()` mapuje progi z PR1.
- `min_successful_buy_signers_degraded` jest clampowane do `<= clean`.
- Domyslnie `cpv_emit_degraded_low_sample = false`, wiec historyczne zachowanie pozostaje fail-closed.

Zmiana 4: typed `CpvComputation`
- Plik: `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- `CpvComputation` zawiera teraz:
  - wartosci `signer_cross_pool_velocity` i `cpv_other_pool_activity`,
  - sample count,
  - required clean/degraded sample count,
  - status,
  - source,
  - rolling-state availability,
  - degraded reasons,
  - helper `evidence_context()`.

Zmiana 5: ochrona denominatora
- Plik: `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- `unique_successful_signers()` nadal filtruje:
  - `tx.is_buy == true`,
  - `tx.success == true`,
  - non-empty signer.
- Failed buy, failed sell i sell-only signer nie podbijaja sample count.

Zmiana 6: materializacja CPV evidence
- Plik: `ghost-launcher/src/session/observation.rs`
- `signer_cross_pool_velocity` i `cpv_other_pool_activity` sa wypelniane tylko dla `MetricEvidenceQuality::Clean`.
- Low-sample degraded wartosci sa zachowane w `sybil_resistance.cpv_evidence`, razem ze statusem/source/sample/reason.
- `evidence_status.cpv` wynika z `cpv_evidence.quality`.

Zmiana 7: defensywny policy guard
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- `CPV_LOW_SAMPLE_DEGRADED` jest traktowany jako nie-actionable w obecnym strict policy path.
- To zapobiega regresji typu "degraded value dziala jak clean value" przed PR5.

Zmiana 8: testy
- Plik: `ghost-launcher/tests/cpv_successful_buy_contract_tests.rs`
- Dodano test integracyjny clean CPV materialization.
- Dodano test integracyjny degraded low-sample materialization, gdzie:
  - clean-only policy fields pozostaja `None`,
  - `cpv_evidence` zawiera wartosci i metadata,
  - JSON snapshot zachowuje status/source/sample/reason.
- Rozszerzono testy unit CPV o przypadki 1/2/3 signerow, low-sample emit true/false oraz failed/sell-only transactions.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| CPV unit contract | `cargo test -q -p ghost-launcher cross_pool_velocity --lib` | 12 passed | PASS |
| CPV materialization contract | `cargo test -q -p ghost-launcher --test cpv_successful_buy_contract_tests -- --nocapture` | 2 passed | PASS |
| Policy degraded guard | `cargo test -q -p ghost-launcher --test gatekeeper_policy_tests degraded_sybil_metrics_do_not_score_even_with_active_penalties -- --nocapture` | 1 passed | PASS |
| Evidence shell serde | `cargo test -q -p ghost-core --test feature_builder_tests evidence_foundation_shell_types_serialize_in_snake_case -- --nocapture` | 1 passed | PASS |
| PR1 foundation compatibility | `cargo test -q -p ghost-core --test pr1_contracts_foundations -- --nocapture` | 4 passed | PASS |
| Config profile coverage | `cargo test -q -p ghost-brain gatekeeper_v2_r37_and_r38_profiles_define_pr1_evidence_foundation_fields --lib` | 1 passed | PASS |
| Partial TOML compatibility | `cargo test -q -p ghost-brain test_gatekeeper_v2_from_toml_file_partial_override --lib` | 1 passed | PASS |
| Evidence policy config tests | `cargo test -q -p ghost-brain evidence_policy --lib` | 2 passed | PASS |

### Runtime/log proof status

Code path proof:
- `PoolObservationSession::materialize_features()` populates `MaterializedFeatureSet.sybil_resistance.cpv_evidence`.
- `ghost-launcher/src/oracle_runtime.rs` serializes `assessment.feature_snapshot` into `v3_materialized_feature_snapshot`.
- The new integration test serializes `MaterializedFeatureSet` and verifies JSON shape for clean and degraded CPV evidence.

Not executed in this ADR:
- Fresh shadow rollout artifact verification. This PR2 ADR records static/unit/integration validation only.

### Known unrelated test limitation

`ghost-launcher/tests/session_lifecycle_tests.rs` currently contains an unrelated compile failure around fields `decision_time_series` and `temporal_deltas` on `MaterializedFeatureSet`. PR2 does not fix that file and does not use it as acceptance gate.

## 8. Ryzyka regresji i jak zostaly ograniczone

Ryzyko 1: false coverage przez zmiane denominatora.
- Kiedy: gdy CPV zacznie liczyc all signers, failed tx albo sell-only wallets.
- Mitigacja: denominator pozostaje `unique_successful_signers()`, a test `failed_and_sell_only_transactions_do_not_increase_cpv_sample` pilnuje tego kontraktu.

Ryzyko 2: low-sample degraded traktowany jak clean.
- Kiedy: gdy `cpv_emit_degraded_low_sample=true` i policy zacznie czytac degraded value ze starego fielda.
- Mitigacja: stare clean-only fields sa wypelniane tylko przy `MetricEvidenceQuality::Clean`; degraded value trafia do `cpv_evidence`.

Ryzyko 3: ciche zamienienie missing na 0.0.
- Kiedy: gdy insufficient/unavailable CPV zostanie serializowane jako liczba bez statusu.
- Mitigacja: insufficient/unavailable zwraca `None`, a evidence context niesie status/source/reason/sample.

Ryzyko 4: drift miedzy `signer_cross_pool_velocity` i `cpv_other_pool_activity`.
- Kiedy: gdy obie metryki mialyby rozne sample/status/source.
- Mitigacja: obie wartosci sa liczone w jednym `CpvComputation` i maja wspolny `CpvEvidenceContext`.

Ryzyko 5: hidden config semantics.
- Kiedy: gdy progi CPV sa zaszyte w runtime bez config/replay context.
- Mitigacja: PR2 uzywa pol PR1 z `GatekeeperV2Config`; brak nowych hidden thresholds.

Ryzyko 6: policy regression przed PR5.
- Kiedy: gdy degraded CPV bylby uzyty w strict scoring/hard gate bez jawnej polityki.
- Mitigacja: `CPV_LOW_SAMPLE_DEGRADED` jest non-actionable, a test policy guard potwierdza brak scoringu degraded sybil metric.

Ryzyko 7: shadow/runtime logging shape regression.
- Kiedy: gdy additive fields zlamia serialization albo snapshot.
- Mitigacja: pola sa serde-default/additive, a integration test sprawdza JSON `MaterializedFeatureSet` shape.

## 9. Ryzyka resztkowe / czego PR2 jeszcze nie robi

- PR2 nie uruchamia swiezego shadow rollout i nie potwierdza artefaktu JSONL na logach runtime.
- PR2 nie dodaje aktywnego config flag `cpv_enabled`, wiec `CPV_DISABLED_BY_CONFIG` jest przygotowanym reason constant, ale nie ma jeszcze aktywnej galezi disabled-by-config.
- PR2 nie pozwala strict policy uzywac degraded CPV. To pozostaje poza zakresem do PR5.
- PR2 nie implementuje temporal carry-forward anchor semantics. To pozostaje PR3.
- PR2 nie wyrownuje burst_ratio top-level vs embedded. To pozostaje PR4.
- PR2 nie naprawia niezaleznego compile problemu w `ghost-launcher/tests/session_lifecycle_tests.rs`.

## 10. Konkluzja

PR2 zostal zrealizowany w kierunku evidence truthfulness, nie cosmetic coverage.

Najwazniejsze gwarancje:
- CPV nadal mierzy successful-buy signerow.
- Coverage mozna zwiekszyc tylko jawnie przez config i status `degraded_low_sample`.
- Degraded low-sample value nie udaje clean value.
- `cpv_other_pool_activity` ma ten sam evidence contract co `signer_cross_pool_velocity`.
- `MaterializedFeatureSet` niesie typed `cpv_evidence`, ktory moze byc replayowany i analizowany offline.

Formalne zamkniecie runtime artefaktu wymaga jeszcze swiezego runa shadow i sprawdzenia `v3_materialized_feature_snapshot.sybil_resistance.cpv_evidence` w JSONL.
