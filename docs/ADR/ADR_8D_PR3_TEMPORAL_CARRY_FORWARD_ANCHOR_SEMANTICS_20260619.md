# ADR-8D: PR3 Temporal Carry-Forward Anchor Semantics

Status: IMPLEMENTED / STATIC_VALIDATION_COMPLETED
Typ: ADR-8D / temporal anchor evidence and carry-forward contract
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: PR3 z planu evidence coverage contract; embedded temporal anchors, event-counter carry-forward, per-delta evidence status/source/staleness, bez top-level parity i bez policy usage
Poziom ryzyka: HIGH

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- `ghost-core/tests/feature_builder_tests.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/tests/temporal_carry_forward_contract_tests.rs`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ADR-ach PR1/PR2.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR3, czyli dodac jawna semantyke temporal carry-forward dla anchorow i delt bez cichej imputacji, bez future-fill i bez zmiany aktywnego Gatekeeper policy behavior.

Rzeczywisty przebieg:
- Potwierdzono, ze aktualny `MaterializedFeatureSet` w tym checkoutcie nie mial aktywnych pol `temporal_deltas`.
- Potwierdzono, ze istniejacy `ghost-launcher/tests/session_lifecycle_tests.rs` zawiera niezalezny broken test oczekujacy nieistniejacych pol `decision_time_series` i `temporal_deltas`.
- PR3 zostal zrealizowany jako additive embedded evidence surface w `MaterializedFeatureSet`, materializowany w `PoolObservationSession::materialize_features()`.
- Nie ruszono DecisionLogger top-level parity, `GatekeeperBuyLog` flat fields ani strict policy usage. To pozostaje PR4/PR5.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT, `MaterializedFeatureSet`, replay/audit boundary i shadow/live separation.
- `rust-master`: deterministic Rust implementation, no future-fill, bounded local materialization logic.
- `trading-systems`: rozdzielenie wartosci, braku evidence i carried-forward statusu.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/config-rollout-safety-reviewer.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/gatekeeper-policy-auditor.md`

Powod:
PR3 dotyka canonical feature snapshot, config-driven temporal evidence semantics i replay JSON shape. Nie dotyka Solana execution path, sendera, live execution ani transaction buildera.

## 3. Opis problemu - 3W2H

What:
Temporal deltas nie mialy jawnego kontraktu evidence w aktualnym kodzie. System nie mogl odroznic:
- anchor nieosiagniety,
- anchor osiagniety eventem,
- anchor osiagniety uplywem obserwacji bez nowego eventu,
- wartosc przeniesiona z poprzedniego anchoru,
- carry-forward niedozwolony lub przeterminowany.

Where:
- `MaterializedFeatureSet`
- `PoolObservationSession::materialize_features()`
- embedded `v3_materialized_feature_snapshot`
- downstream selector/replay consumers czytajacy snapshot

Why it matters:
Bez statusu/source/staleness delta `0` moze znaczyc zarowno realny brak zmiany, jak i ukryta imputacje albo brak danych. To prowadzi do uczenia artefaktow runtime zamiast rynku.

How observed:
Plan PR3 opisuje przypadek, w ktorym obserwacja dotarla do 3s, ale po 2s nie bylo eventu. Dla event counters delta powinna byc policzalna z jawnym statusem `carried_forward_no_event`; dla state/price/ratio domyslnie nie wolno udawac clean stability.

How many / scale:
Zmiana obejmuje wszystkie `MaterializedFeatureSet` emitowane przez session materialization. Nie zmienia terminalnych verdictow.

## 4. Przyczyna zrodlowa

Root cause:
Aktualny kod nie mial aktywnego embedded `TemporalDeltaFeatures`, wiec nie istnialo miejsce na:
- `reached_by`,
- observation elapsed,
- per-class anchor evidence,
- per-delta evidence,
- carried source/staleness,
- max-staleness reason.

Dodatkowy constraint:
Materializacja nie moze polegac na pozniejszym dataset fillna ani top-level logger rewrite. Evidence musi powstac w `MaterializedFeatureSet` jako SSOT snapshot.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac additive temporal typy do `ghost-core`.
- Materializowac `temporal_deltas` w `PoolObservationSession`, bez policy usage.
- Uzyc event-time t0 jako pierwszego zaakceptowanego ticka w `tx_buffer`.
- Anchor jest `reached_by=event`, jesli event stream przekroczyl anchor.
- Anchor jest `reached_by=observation_elapsed` albo `deadline`, jesli realny czas obserwacji doszedl do anchoru mimo braku nowego eventu.
- Carry-forward idzie tylko z poprzedniego anchoru do pozniejszego anchoru.
- Event counters moga byc carried tylko, gdy config to wlacza i staleness miesci sie w limicie.
- State/price i ratio carry-forward pozostaja domyslnie niedozwolone.
- Rate fields dziedzicza evidence status/source z delty bazowej.

Granice:
- Brak synthetic event na 3s.
- Brak zmiany timestampow tickow.
- Brak future-fill.
- Brak `null -> 0`.
- Brak strict policy usage of carried values.
- Brak top-level JSONL flat-field parity w PR3.
- Brak dodatkowej mutacji CPV rolling index dla anchorow temporal.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: temporal evidence types
- Plik: `ghost-core/src/checkpoint/types.rs`
- Rozszerzono `MetricEvidenceQuality` o:
  - `stale`
  - `not_allowed`
- Rozszerzono `TemporalMetricSource` o:
  - `partial_carried_forward`
  - `stale`
  - `not_allowed`
- Rozszerzono `TemporalMetricEvidenceContext` o `reason`.

Zmiana 2: embedded temporal snapshot
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano `TemporalAnchorSnapshot`.
- Dodano `TemporalDeltaFeatures` z anchorami `anchor_1s`, `anchor_2s`, `anchor_3s`, delta/rate fields oraz `delta_evidence: BTreeMap<String, TemporalMetricEvidenceContext>`.
- Dodano `MaterializedFeatureSet.temporal_deltas` z `#[serde(default)]`.

Zmiana 3: feature builder default
- Plik: `ghost-core/src/checkpoint/feature_builder.rs`
- `ObservationFeatureBuilder` wypelnia `temporal_deltas: Default::default()`.

Zmiana 4: session-side carry-forward config snapshot
- Plik: `ghost-launcher/src/session/observation.rs`
- Dodano lokalny `TemporalCarryForwardRuntimeConfig`, mapowany z PR1 fields:
  - `temporal_carry_forward_enabled`
  - `temporal_carry_forward_max_staleness_ms`
  - `temporal_carry_forward_event_counters_enabled`
  - `temporal_carry_forward_state_metrics_enabled`
  - `temporal_carry_forward_ratio_metrics_enabled`

Zmiana 5: temporal materialization
- Plik: `ghost-launcher/src/session/observation.rs`
- Dodano `materialize_v3_temporal_deltas()`.
- Anchory sa budowane deterministycznie z posortowanych tx po event-time.
- `reached_by` rozdziela event-time od observation elapsed/deadline.
- Event counters sa carry-forwardowane tylko z poprzedniego anchoru, tylko do przyszlego anchoru i tylko pod limitem staleness.
- State/price i ratio bez eventu dostaja `not_allowed` przy default configu.
- Dla staleness violation wartosc zostaje `None`, a evidence ma `source=stale`, `reason=stale`.

Zmiana 6: testy PR3
- Plik: `ghost-launcher/tests/temporal_carry_forward_contract_tests.rs`
- Dodano testy:
  - no-event 2s->3s event counters emituja wartosc + `carried_forward_no_event`,
  - state/ratio no-event przy default configu zostaja unavailable/not_allowed,
  - future state value po anchorze nie backfilluje wczesniejszych anchorow,
  - max staleness blokuje carry-forward i daje reason `stale`.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| PR3 temporal contract | `cargo test -q -p ghost-launcher --test temporal_carry_forward_contract_tests -- --nocapture` | 4 passed | PASS |
| Ghost launcher compile | `cargo check -q -p ghost-launcher --lib` | passed | PASS |
| Evidence shell serde | `cargo test -q -p ghost-core --test feature_builder_tests evidence_foundation_shell_types_serialize_in_snake_case -- --nocapture` | 1 passed | PASS |
| PR1 foundation compatibility | `cargo test -q -p ghost-core --test pr1_contracts_foundations -- --nocapture` | 4 passed | PASS |
| PR2 CPV contract compatibility | `cargo test -q -p ghost-launcher --test cpv_successful_buy_contract_tests -- --nocapture` | 2 passed | PASS |
| Gatekeeper sybil degraded guard | `cargo test -q -p ghost-launcher --test gatekeeper_policy_tests degraded_sybil_metrics_do_not_score_even_with_active_penalties -- --nocapture` | 1 passed | PASS |
| Config profile coverage | `cargo test -q -p ghost-brain gatekeeper_v2_r37_and_r38_profiles_define_pr1_evidence_foundation_fields --lib` | 1 passed | PASS |
| Diff hygiene | `git diff --check` | passed | PASS |
| Staged diff hygiene | `git diff --cached --check` | passed | PASS |

### Runtime/log proof status

Code path proof:
- `PoolObservationSession::materialize_features()` populates `MaterializedFeatureSet.temporal_deltas`.
- `oracle_runtime` serializes `assessment.feature_snapshot` to `v3_materialized_feature_snapshot`.
- Test PR3 serializes through normal `materialize_features()` path and validates values/status/source/staleness in embedded structures.

Not executed in this ADR:
- Fresh shadow rollout artifact validation.
- Top-level JSONL flat delta/rate parity. To pozostaje PR4.

## 8. Ryzyka regresji i jak zostaly ograniczone

Ryzyko 1: `null` staje sie `0.0`.
- Kiedy: gdy anchor missing albo not allowed jest traktowany jako zero.
- Mitigacja: brak wartosci daje `None`; status/source/reason sa w `delta_evidence`.

Ryzyko 2: future-fill.
- Kiedy: wartosc z eventu po anchorze wypelnia anchor wstecz.
- Mitigacja: prefix anchoru uzywa tylko tx `<= cutoff`; test future-fill potwierdza brak backfillu.

Ryzyko 3: false stability state/price.
- Kiedy: market cap albo price jest carry-forwardowany przez cisze bez config/source.
- Mitigacja: state metrics carry-forward sa default-off; no-event state dostaje `not_allowed`.

Ryzyko 4: false stability ratio.
- Kiedy: burst/Jito/flipper/CPV ratio sa przeniesione jako clean.
- Mitigacja: ratio carry-forward default-off; no-event ratio dostaje `not_allowed`.

Ryzyko 5: carried values uzyte w policy.
- Kiedy: Gatekeeper strict policy zaczyna czytac carried values jako clean.
- Mitigacja: PR3 nie podlacza temporal deltas do policy. `temporal_carried_forward_policy` pozostaje interpretacja dla kolejnych PR-ow.

Ryzyko 6: replay drift.
- Kiedy: carry-forward zalezy od niewidocznego runtime czasu.
- Mitigacja: anchor zapisuje `reached_by`, `anchor_observation_elapsed_ms`, `carried_from_anchor_ms`, `staleness_ms` i `reason`.

Ryzyko 7: CPV rolling index side-effect.
- Kiedy: temporal CPV deltas bylyby liczone przez dodatkowe zapytania do rolling indexu podczas materializacji.
- Mitigacja: PR3 nie liczy CPV anchorow i nie mutuje CPV indexu poza istniejaca PR2 materializacja. CPV ratio delty pozostaja jawnie unavailable/not_allowed.

## 9. Ryzyka resztkowe / czego PR3 jeszcze nie robi

- PR3 nie materializuje decision-time series/DTW. Aktualny broken test w `session_lifecycle_tests.rs` nadal oczekuje `decision_time_series`, ktorego ten PR nie dodaje.
- PR3 nie splaszcza top-level delta/rate evidence do JSONL fields.
- PR3 nie zmienia `GatekeeperBuyLog`.
- PR3 nie wyrownuje `burst_ratio` top-level z embedded SSOT. To pozostaje PR4.
- PR3 nie pozwala policy uzywac carried-forward evidence. To pozostaje PR5.
- PR3 nie potwierdza swiezego runtime artefaktu z rollout runa.

## 10. Konkluzja

PR3 wprowadza wiarygodny embedded temporal evidence contract:
- event counters moga uzyskac jawne `carried_forward_no_event`,
- state/price i ratio nie sa cicho carry-forwardowane domyslnie,
- kazda carried delta/rate ma source, carried-from i staleness,
- future-fill jest zabroniony i testowany,
- max staleness blokuje wartosc zamiast produkowac falszywe zero.

To poprawia prawde danych bez malowania coverage na zielono. Kolejny etap powinien zajac sie PR4: top-level parity i `burst_ratio` Option A.
