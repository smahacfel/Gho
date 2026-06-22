# ADR-8D: Decision Time Series Retention Contract and PR6 Real Export Proof

Status: IMPLEMENTED / TARGETED_TESTS_AND_REAL_EXPORT_PROOF_VERIFIED
Typ: ADR-8D / runtime evidence retention + selector dataset export proof
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: domkniecie dwoch residuali po PR1-PR6: limit 128 probek w `decision_time_series` oraz brak long proofu PR6 na realnym dataset export
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/ghost_brain_config.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r37_threshold_probe_maxwait3789_fsc_off.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/tests/tx_intelligence_tests.rs`
- `scripts/build_selector_gatekeeper_feature_context.py`
- `scripts/build_selector_training_view.py`
- `scripts/test_selector_pipeline.py`

Powiazane plany i ADR:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`
- `docs/ADR/ADR_8D_PR6_SELECTOR_EVIDENCE_CONTEXT_EXPORT_20260619.md`
- `docs/ADR/ADR_8D_R37_CPV_DEGRADED_TOP_LEVEL_EMISSION_20260619.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w istniejacych raportach.

## 1. Przygotowanie i dzialania wstepne

Zadanie:
- Usunac residual `decision_time_series` hard cap 128 jako ukryty limit runtime.
- Zachowac bounded memory, ale uczynic truncation jawna prawda evidence.
- Dac R37/R38 profilom mozliwosc utrzymania pelnej serii tickow w typowym oknie obserwacji.
- Udowodnic PR6 na realnych artefaktach dataset/export, nie tylko testem kontraktowym.

Wykonano:
- Nie zatrzymywano i nie restartowano aktywnego runtime.
- Ograniczono zmiany do konfiguracji, retention evidence, DecisionLogger top-level projection, observation session buffer i selector export.
- Nie zmieniano Gatekeeper verdict policy, shadow/live behavior, TX builderow, sendera, ingestu ani CPV denominatora.
- Uruchomiono proof-scope datasetowy na realnych R37 decision logs.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT `MaterializedFeatureSet`, DecisionLogger/replay boundary, shadow/live separation.
- `rust-master`: bounded buffer, replay-safe retention semantics, brak unbounded runtime allocation.
- `large-data-analytics`: real export proof, evidence/model-column separation, brak cichej imputacji.

Nie ladowano dokumentow:
- `solana-execution-path-engineer`: zadanie nie dotyka execution path.
- `seer-ingest-event-integrity-specialist`: zadanie nie dotyka ingestu ani parserow.
- `gatekeeper-policy-auditor`: nie zmieniano verdictow, hard gates ani strict policy order.

## 3. Opis problemu - 3W2H

What:
W artefaktach wykryto residual: `decision_time_series.sample_count` moglo zatrzymac sie na 128, mimo ze `total_tx_evaluated` / tx-intel `tx_count` byly wieksze, np. 140. To oznaczalo, ze runtime dalej mial ukryty cap i gubil najstarsze ticki bez jawnego statusu truncation.

Where:
- `PoolObservationSession.tx_buffer`
- `PoolObservationSession::materialize_decision_time_series()`
- `v3_materialized_feature_snapshot.decision_time_series`
- top-level DecisionLogger JSONL
- selector PR6 export evidence columns

Why it matters:
DTW i offline selector musza wiedziec, czy seria jest kompletna, czy obcieta. Ciche obciecie jest gorsze niz jawny degraded status, bo downstream moze uznac krotsza serie za rzeczywisty ksztalt rynku.

How observed:
W runtime review wskazano przypadki, gdzie `decision_time_series` zachowywal tylko 128 probek przy wiekszej liczbie tx. Dodatkowo PR6 mial proof kontraktowy, ale brakowalo long proofu na realnym exportowanym dataset context.

How many / scale:
Zmiana dotyczy kazdej observation session, ale domyslnie zachowuje stary bounded cap 128 dla kompatybilnosci. Profile R37/R38 dostaly jawny wyzszy cap 4096, aby typowe probe runy nie tracily tickow w oknie obserwacji.

## 4. Przyczyna zrodlowa

Root cause 1:
`tx_buffer` byl bounded na stalym limicie 128, odziedziczonym z `DEFAULT_SESSION_TX_RING_CAPACITY`. Limit byl technicznie bezpieczny pamieciowo, ale nie byl czescia evidence contractu `decision_time_series`.

Root cause 2:
`DecisionTimeSeriesFeatures` nie mialo pol retention/truncation, wiec downstream nie mogl odroznic:
- kompletnej serii,
- pustej/unavailable serii,
- serii jawnie obcietej przez capacity.

Root cause 3:
PR6 exporter nie mial jeszcze long proofu na realnych R37 decision logs po zmianach evidence context. Istnialy testy/smoke, ale brakowalo artefaktu datasetowego z tysiacami realnych rekordow.

## 5. Strategia naprawy

Przyjeta strategia:
- Nie robic unbounded buffer.
- Dodac config-driven capacity:
  - default: `decision_time_series_tx_capacity = 128`
  - R37/R38 profile: `decision_time_series_tx_capacity = 4096`
- Dodac jawny retention contract:
  - `retention_status = clean | truncated | unavailable`
  - `retention_policy = truncate_with_status`
  - `retention_capacity`
  - `retained_sample_count`
  - `total_tx_count`
  - `dropped_oldest_count`
- Jesli `dropped_oldest_count > 0`, ustawic `decision_time_series.status = degraded` i dodac degraded reason `decision_time_series_truncated`.
- Wyplaszczyc retention fields do top-level JSONL i do PR6 evidence export.
- Zachowac price missing semantics bez future-fill i bez `null -> 0`.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: typed retention evidence
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano:
  - `DecisionTimeSeriesRetentionStatus`
  - `DecisionTimeSeriesRetentionPolicy`
  - `EvidenceDegradedReason::DecisionTimeSeriesTruncated`
  - retention fields w `DecisionTimeSeriesFeatures`

Zmiana 2: config-driven capacity
- Plik: `ghost-brain/src/config/ghost_brain_config.rs`
- Dodano pola `GatekeeperV2Config`:
  - `decision_time_series_tx_capacity`
  - `decision_time_series_retention_policy`
- Pola maja `serde(default)`.
- Walidacja odrzuca capacity `0`.
- Base config zostaje konserwatywny: `128`.
- R37/R38 rollout profile dostaly `4096`.

Zmiana 3: observation session retention behavior
- Plik: `ghost-launcher/src/session/observation.rs`
- `PoolObservationSession` czyta capacity z configu.
- `tx_buffer` zachowuje bounded FIFO.
- Przy overflow usuwa najstarsze probki.
- Materializacja liczy `retained_sample_count`, `total_tx_count` i `dropped_oldest_count`.
- Obcieta seria jest `degraded`, nie udaje `clean`.

Zmiana 4: DecisionLogger schema and top-level fields
- Pliki:
  - `ghost-brain/src/oracle/decision_logger.rs`
  - `ghost-launcher/src/components/gatekeeper.rs`
- Podniesiono schema do v29.
- Dodano top-level:
  - `decision_time_series_retention_status`
  - `decision_time_series_retention_policy`
  - `decision_time_series_retention_capacity`
  - `decision_time_series_retained_sample_count`
  - `decision_time_series_total_tx_count`
  - `decision_time_series_dropped_oldest_count`
- Dodano capacity/policy do `evidence_policy_context`.

Zmiana 5: selector PR6 export
- Pliki:
  - `scripts/build_selector_gatekeeper_feature_context.py`
  - `scripts/build_selector_training_view.py`
- Dodano evidence columns:
  - `gk_decision_time_series_present`
  - `gk_decision_time_series_source`
  - `gk_decision_time_series_evidence_status`
  - `gk_decision_time_series_retention_status`
  - `gk_decision_time_series_retention_policy`
  - `gk_decision_time_series_retention_capacity`
  - `gk_decision_time_series_retained_sample_count`
  - `gk_decision_time_series_total_tx_count`
  - `gk_decision_time_series_dropped_oldest_sample_count`
  - `gk_decision_time_series_price_finite_sample_count`
  - `gk_decision_time_series_price_missing_sample_count`
- Evidence fields sa klasyfikowane poza `model_feature_columns`.

Zmiana 6: tests
- Plik: `ghost-launcher/tests/tx_intelligence_tests.rs`
- Rozszerzono bounded-buffer test o truncation evidence.
- Dodano test config-driven capacity wiekszej niz 128, potwierdzajacy brak truncation przy 133 probkach.

## 7. Walidacja dzialan naprawczych

### Rust / Python checks

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Rustfmt | `cargo fmt --package ghost-core --package ghost-brain --package ghost-launcher` | passed | PASS |
| Config enum serde | `cargo test -q -p ghost-brain test_gatekeeper_v2_evidence_policy_enums_deserialize_all_values --lib` | 1 passed | PASS |
| R37/R38 config profile fields | `cargo test -q -p ghost-brain gatekeeper_v2_r37_and_r38_profiles_define_pr1_evidence_foundation_fields --lib` | 1 passed | PASS |
| Default bounded buffer + truncation evidence | `cargo test -q -p ghost-launcher session_tx_buffer_is_bounded --test tx_intelligence_tests` | 1 passed | PASS |
| Configurable capacity > 128 | `cargo test -q -p ghost-launcher session_decision_time_series_capacity_is_configurable --test tx_intelligence_tests` | 1 passed | PASS |
| Buy log flattening smoke | `cargo test -q -p ghost-launcher test_fingerprint_metrics_map_to_buy_log_and_summary --lib` | 1 passed | PASS |
| Evidence policy context smoke | `cargo test -q -p ghost-launcher evidence_policy_context_is_emitted_in_buy_log_when_enabled --lib` | 1 passed | PASS |
| Python syntax compile | `python3 -m py_compile scripts/build_selector_gatekeeper_feature_context.py scripts/build_selector_training_view.py scripts/test_selector_pipeline.py` | passed | PASS |
| Diff whitespace | `git diff --check` | clean | PASS |

Repo-wide warnings from Rust tests were pre-existing warnings/deprecations in unrelated modules.

### PR6 real export proof

Utworzono izolowany scope:
- `selector-pr6-r37-real-decision-proof-20260619`

Wejscie:
- R37 v25_shadow decision log:
  - `logs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/v2.5/v25_shadow/45137ae410c1ab231b457abed6a34f99b4086136f912e6de64c7dd703d6850d8/gatekeeper_v2_decisions.jsonl`

Wygenerowano minimalny real proof candidate universe:
- `datasets/selector/selector-pr6-r37-real-decision-proof-20260619/candidate_universe_v1.jsonl`

Exporter:
- `python3 scripts/build_selector_gatekeeper_feature_context.py --root /root/Gho --scope selector-pr6-r37-real-decision-proof-20260619 --source-scope shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1 --decision-plane v25_shadow --observation-profile all --json`

Wynik:
- `status = PASS`
- `gatekeeper_feature_context_status = PASS`
- `context_rows_written = 7411`
- `context_status_counts = {"ok": 7411}`
- `join_method_counts.join_key = 7411`
- `forbidden_fields_detected = []`
- `gk_decision_time_series_present` present rate = `1.0`
- `gk_decision_time_series_source` present rate = `1.0`
- `gk_decision_time_series_evidence_status` present rate = `1.0`
- `gk_decision_time_series_price_finite_sample_count` present rate = `1.0`
- `gk_decision_time_series_price_missing_sample_count` present rate = `1.0`
- `decision_time_series` evidence columns do not leak into `model_feature_columns`

Artefakty:
- `datasets/selector/selector-pr6-r37-real-decision-proof-20260619/gatekeeper_feature_context_v1.jsonl`
- `reports/selector/selector-pr6-r37-real-decision-proof-20260619/gatekeeper_feature_context_manifest_v1.json`

Uwaga:
Pierwsza proba z `--observation-profile observation_8s_10s` dala `no_profile_match`, bo R37 w tym proofie ma krotszy profil niz preset 8-10s. To nie byl blad eksportera. Wlasciwy proof dla R37 wykonano z `--observation-profile all`.

### Test harness limitation

Proba uruchomienia pojedynczego unittest:

`python3 -m unittest scripts.test_selector_pipeline.SelectorPipelineTests.test_gatekeeper_feature_context_exports_evidence_without_imputation`

zatrzymuje sie na imporcie calego modulu:

```text
ModuleNotFoundError: No module named 'build_selector_route_manifest_reuse_projection'
```

Plik `build_selector_route_manifest_reuse_projection.py` nie istnieje w `scripts/` w tym checkoutcie. To blokuje test harness `test_selector_pipeline.py`, ale nie blokuje bezposredniego export proofu PR6, ktory przeszedl end-to-end na realnych artefaktach.

## 8. Ryzyka resztkowe / czego ten ADR nie zamyka

- Aktualny real export proof opiera sie na R37 logs schema 26/28. Nie moze potwierdzic nowych runtime top-level retention fields schema v29, bo te wymagaja rebuild/restart i swiezych rows.
- Proof potwierdza exporter PR6 i evidence separation na realnych danych, ale nie zastepuje runtime proofu nowej schema v29.
- Base default `128` zostal zachowany dla kompatybilnosci i pamieci. Profile wymagajace pelnej serii musza jawnie ustawic wieksza wartosc, jak R37/R38 `4096`.
- Truncation nie zostala ukryta. Jezeli capacity nadal bedzie za male dla dluzszego runa, seria bedzie degraded z `decision_time_series_truncated`, a nie cicho udawana jako kompletna.
- Ten ADR nie zmienia price-missing behavior, market-cap fallback, carry-forward price source ani no-future-fill semantics.

## 9. Scope out

Poza zakresem pozostaly:
- unbounded decision-series buffer;
- future-fill cen;
- zamiana missing price lub missing delta na `0`;
- zmiana Gatekeeper policy;
- zmiana strict metric threshold behavior;
- zmiana CPV denominatora;
- restart aktywnego runa;
- porzadkowanie starego dirty worktree;
- naprawa brakujacego modulu `build_selector_route_manifest_reuse_projection.py`.

## 10. Wniosek

Residual z limitem 128 zostal naprawiony w sposob konserwatywny:
- memory pozostaje bounded,
- capacity jest konfigurowalne,
- R37/R38 maja `4096`,
- truncation jest jawnie statusowane i logowane,
- downstream dostaje evidence zamiast ukrytej utraty tickow.

PR6 dostal realny long proof na R37 export:
- 7411 realnych context rows,
- status `PASS`,
- evidence fields obecne,
- brak forbidden fields,
- brak przecieku `decision_time_series` evidence do model feature columns.

Formalny runtime proof nowych v29 retention fields wymaga swiezych decyzji po rebuild/restart. Do tego czasu code/test/export proof jest zamkniety, ale runtime artefakt v29 pozostaje do potwierdzenia w kolejnym swiezym runie.
