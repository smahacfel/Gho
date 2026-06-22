# ADR-8D: Runtime gaps in top-level evidence, CPV temporal anchors and DTW vectors

Status: IMPLEMENTED / TARGETED_TESTS_PASS / RUNTIME_REPROOF_REQUIRED
Typ: ADR-8D / SSOT evidence repair / DecisionLogger and selector dataset compatibility
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, local working tree
Commit/PR: not committed at report time
Zakres: residual runtime gaps after PR1-PR3 evidence coverage work
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `scripts/build_selector_gatekeeper_feature_context.py`
- `ghost-launcher/tests/session_lifecycle_tests.rs`
- `docs/ADR/ADR_8D_RUNTIME_GAPS_TOP_LEVEL_EVIDENCE_CPV_DTW_20260619.md`

Powiazane runtime symptomy:
- R37/R36 JSONL showed embedded `temporal_deltas`, but selected top-level fields were incomplete.
- `vectors_prices` disappeared from top-level when embedded `decision_time_series.prices` contained `null`.
- `rate_mcap_sol_per_s_2s_to_3s` existed embedded but was absent top-level.
- Fresh runtime evidence still required `decision_time_series` as full decision tick axis, not AB-window-only vectors.
- Temporal CPV/flipper deltas were weak because anchor raw values did not derive CPV/flipper from anchor-prefix evidence.
- `cpv_other_pool_activity` existed in SSOT/evidence but was missing from selector RAW feature exposure.

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Punkt startowy:
Uzytkownik wskazal, ze po PR1-PR3 i runtime smoke nadal nie mozna zamknac tematu jako kompletny, bo artefakty JSONL ujawnily residual gaps w top-level evidence, price vectors i temporal metrics.

Cel:
- Zachowac `MaterializedFeatureSet` jako SSOT.
- Wypelnic top-level JSONL z embedded SSOT bez cichej imputacji.
- Zachowac `null` jako brak evidence, nie zamieniac go na `0.0`.
- Poprawic CPV/flipper temporal anchor materialization bez zmiany definicji CPV.
- Utrzymac Gatekeeper policy, strict thresholds i shadow/live separation bez regresji.

Non-goals:
- Brak zmian TX buildera, Helius Sendera, DirectBuy/DirectSell path albo live execution.
- Brak zmian BUY/REJECT/TIMEOUT semantics.
- Brak zmiany CPV z successful-buy signer semantics na "wszyscy signerzy".
- Brak future-backfill cen albo future-fill anchorow.
- Brak aktywowania FSC jako policy signal.

## 2. Routing i skills

Uzyte skills:
- `ghost-execution`: SSOT, Gatekeeper, DecisionLogger/replay, shadow/live separation.
- `rust-master`: deterministyczna kolejnosc eventow, Rust type safety, testy regresyjne.
- `trading-systems`: evidence truth, brak cichej imputacji, selector/runtime boundary.

Specjalisci logiczni:
- Primary: Decision Logging Replay Analyst.
- Supporting: SSOT Feature Materialization Guardian, Config Rollout Safety Reviewer, Gatekeeper Policy Auditor.

Zaladowane dokumenty:
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/config-rollout-safety-reviewer.md`
- `docs/agents/gatekeeper-policy-auditor.md`
- Repo `AGENTS.md`

## 3. Opis problemu - 3W2H

What:
Runtime artifacts pokazaly, ze embedded MFS zaczyna zawierac poprawne evidence, ale top-level JSONL i temporal anchor computation nadal nie byly w pelni zgodne z tym evidence.

Where:
- `PoolObservationSession::materialize_features()`
- `PoolObservationSession::materialize_v3_temporal_deltas()`
- `CrossPoolVelocityIndex::compute_for_transactions()`
- `GatekeeperAssessment::to_buy_log()`
- `enrich_buy_log_with_vectors()`
- `GatekeeperBuyLog`
- selector dataset builder RAW feature list

Why it matters:
Selector/DTW/offline consumers czesto czytaja top-level JSONL. Jezeli embedded SSOT ma wartosc, a top-level jej nie ma albo traci os tickow przy `null`, offline model uczy sie artefaktu loggingowego zamiast rynku. Jezeli anchor CPV liczy z przyszlosci albo nie liczy wcale, delty CPV/flipper sa niewiarygodne.

How observed:
Uzytkownik wskazal runtime snapshots, w ktorych:
- core top-level deltas zostaly czesciowo naprawione, ale rate field byl embedded-only,
- `vectors_prices` znikalo w degraded/missing price cases,
- `cpv_other_pool_activity` i niektore ratio/delta fields mialy slabe top-level coverage,
- CPV/flipper temporal values mialy `null` mimo istniejacych prefix events.

How many / scale:
Problem dotyka kazdego runu, w ktorym downstream uzywa top-level JSONL jako dataset/replay surface, oraz kazdej decyzji z missing price sample albo temporal CPV/flipper anchor.

## 4. Przyczyny zrodlowe

Root cause 1: top-level vector type nie mogl reprezentowac nullable per-tick price.
`GatekeeperBuyLog.vectors_prices` bylo `Vec<f64>`, wiec rekord z pojedynczym missing price nie mogl zachowac pelnej osi tickow jako `[null, price, ...]`.

Root cause 2: top-level convenience fields byly niekompletne wzgledem embedded `temporal_deltas`.
`rate_mcap_sol_per_s_2s_to_3s` i wybrane delty/rates nie mialy pelnego mirroru w `GatekeeperBuyLog`/`to_buy_log()`.

Root cause 3: temporal anchor raw values nie materializowaly CPV/flipper z prefix evidence.
`TemporalAnchorRawValues` zostawial `signer_cross_pool_velocity` i `flipper_presence_ratio` jako `None`, wiec delta computation nie mogla powstac nawet wtedy, gdy session/index mialy wystarczajace dane.

Root cause 4: CPV index nie filtrowal historii gornym boundem `anchor_ts_ms`.
Dla temporal anchorow samo usuwanie starych wpisow nie wystarcza; trzeba rowniez ignorowac aktywnosc po anchorze, inaczej powstaje future leakage.

Root cause 5: selector RAW feature list nie znala nowego `cpv_other_pool_activity` i missing source count.
Nawet po dodaniu top-level fielda dataset builder nie wystawial go jako RAW feature.

## 5. Strategia naprawy

Przyjeta strategia:
- W MFS przechowywac pelna decision-time series z nullable price vector.
- W top-level JSONL zachowac nullable vectors jako `Vec<Option<f64>>`.
- Mirrorowac top-level temporal deltas/rates bez zmiany embedded SSOT.
- CPV anchor liczyc tylko przy clean successful-buy sample, zgodnie z obecna semantyka CPV.
- Flipper anchor liczyc z prefix events bez future-fill; gdy brak evidence, zostaje `None`.
- CPV index liczy tylko aktywnosci `<= anchor_ts_ms`.
- Dataset builder dostaje tylko addytywne RAW fields.

Odrzucone alternatywy:
- Zamiana `null` price na `0.0`: odrzucone jako falszowanie evidence.
- Future-backfill ceny z pozniejszego AccountStateCore ticka: odrzucone jako temporal leakage.
- CPV na wszystkich signerach zamiast successful-buy signers: odrzucone, bo zmienia znaczenie metryki.
- Ciche uzywanie degraded CPV jako clean top-level field: odrzucone; degraded zostaje w embedded evidence.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: nullable top-level vectors.
- `GatekeeperBuyLog.vectors_prices` i `vectors_d_price` sa teraz `Option<Vec<Option<f64>>>`.
- `to_buy_log()` przepisuje wektory bezposrednio z `feature_snapshot.decision_time_series`.
- `enrich_buy_log_with_vectors()` jest fill-if-missing i nie nadpisuje MFS vectors.

Zmiana 2: top-level temporal parity.
- Dodano top-level mirror dla delt i rate fields z `TemporalDeltaFeatures`, w tym `rate_mcap_sol_per_s_2s_to_3s`.
- `burst_ratio` top-level idzie z embedded canonical `tx_intel_features.burst_ratio`, a nie z phase2-only adaptera.

Zmiana 3: decision-time series price evidence.
- Dodano typed decision-series fields/source counts w `ghost-core`.
- Materializacja session buduje full tick axis po deterministycznym event-time order.
- Price resolver uzywa bezpiecznych zrodel: reserve, quote, market cap, account state at-or-before tick, missing.
- Brak ceny pozostaje `None`; no future-backfill.

Zmiana 4: temporal CPV/flipper anchors.
- `temporal_anchor_raw_values()` zbiera prefix transakcji do anchor cutoff.
- CPV anchor korzysta z `CrossPoolVelocityIndex::compute_for_transactions()` dla prefixu i emituje wartosc tylko przy `MetricEvidenceQuality::Clean`.
- Flipper ratio liczy owner-token-delta evidence, a gdy owner deltas sa puste, uzywa signer fallback z prefixu.
- Brak buyer/evidence zostaje `None`.

Zmiana 5: CPV anti-future-leakage.
- `CrossPoolVelocityIndex::compute_for_transactions()` ignoruje history activities z `observed_at_ms > anchor_ts_ms`.
- Dodano unit test, ktory potwierdza, ze future other-pool activity nie podnosi CPV dla wczesniejszego anchoru.

Zmiana 6: selector dataset RAW exposure.
- `scripts/build_selector_gatekeeper_feature_context.py` dostal:
  - `cpv_other_pool_activity`
  - `vectors_price_source_missing_count`

Zmiana 7: schema/version hygiene.
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` podniesiono do `26`, bo top-level JSONL shape zmienil sie addytywnie.

## 7. Walidacja

| Walidacja | Komenda | Wynik |
|---|---|---|
| DTW/session materialization | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_decision_series_and_temporal_deltas_from_session_buffer -- --nocapture` | PASS |
| PR2 CPV contract | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test cpv_successful_buy_contract_tests -- --nocapture` | PASS, 2 tests |
| PR3 temporal carry-forward contract | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test temporal_carry_forward_contract_tests -- --nocapture` | PASS, 4 tests |
| DecisionLogger buy log | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain gatekeeper_buy_log --lib -- --nocapture` | PASS, 3 tests |
| Launcher to_buy_log mirror | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --lib test_fingerprint_metrics_map_to_buy_log_and_summary -- --nocapture` | PASS |
| CPV index unit tests | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher cross_pool_velocity --lib -- --nocapture` | PASS, 13 tests |
| Ghost-core feature builder | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-core --test feature_builder_tests -- --nocapture` | PASS, 5 tests |
| Launcher compile | `RUSTFLAGS=-Awarnings cargo check -q -p ghost-launcher --lib` | PASS |
| Python syntax | `python3 -m py_compile scripts/build_selector_gatekeeper_feature_context.py scripts/test_selector_pipeline.py scripts/test_v3_p37_evidence_availability_report.py` | PASS |
| Formatting | `cargo fmt --package ghost-core --package ghost-brain --package ghost-launcher` | PASS |

Nie wykonano:
- `python3 -m pytest ...`, bo w srodowisku brak modulu `pytest`.
- Swiezy runtime proof R37/R38 po patchu; wymagany po restarcie/rebuildzie procesu.

## 8. Aktualny status

Naprawione kodowo:
- top-level `vectors_prices` zachowuje dlugosc osi tickow i `null` per missing sample,
- top-level rate/delta fields sa mirrorowane z embedded SSOT,
- `rate_mcap_sol_per_s_2s_to_3s` jest top-level,
- `cpv_other_pool_activity` jest top-level i w RAW features,
- temporal CPV/flipper deltas moga powstawac z anchor-prefix evidence,
- CPV index nie liczy future history entries dla anchoru,
- `burst_ratio` top-level jest zgodny z embedded canonical value,
- schema version bump odroznia nowy top-level shape.

Pozostaje do potwierdzenia runtime:
- `len(vectors_prices) == len(vectors_ts_offsets_ms) == len(v3_materialized_feature_snapshot.decision_time_series.prices)` rowniez w degraded records,
- top-level delta/rate presence zgodne z embedded presence,
- `series_negative_interval_records == 0`,
- CPV/flipper delta coverage wzrosnie tylko tam, gdzie istnieje clean evidence,
- missing price count nie jest maskowany zerami i pozostaje zgodny z realnym brakiem evidence.

## 9. Ryzyka i zabezpieczenia

Ryzyko 1: downstream oczekujacy `vectors_prices: list[number]` musi tolerowac `null`.
- Mitigacja: schema version 26 i zachowanie osi tickow sa wazniejsze niz ciche kasowanie calego pola.

Ryzyko 2: CPV temporal coverage wzrosnie, ale tylko dla clean sample.
- Mitigacja: degraded/low-sample CPV nadal nie trafia do clean top-level policy fields.

Ryzyko 3: signer fallback dla flipper ratio jest slabszy niz owner-token-delta evidence.
- Mitigacja: fallback dziala tylko w obrebie prefixu anchoru i nie future-filluje; brak buyer/evidence nadal daje `None`.

Ryzyko 4: CPV future-filter moze obnizyc historyczne CPV w sytuacjach, gdzie wczesniej przypadkowo liczono aktywnosc po anchorze.
- Mitigacja: to jest korekta leakage, nie regresja semantyczna.

Ryzyko 5: top-level fields moga zwiekszyc payload JSONL.
- Mitigacja: pola sa opcjonalne i `skip_serializing_if`, wiec brak evidence nie zwieksza rekordu.

## 10. Decyzja

Akceptujemy ten etap jako kodowe domkniecie runtime gaps ujawnionych po PR1-PR3 w warstwie evidence/materialization/logging/dataset exposure.

Formalne zamkniecie wymaga jeszcze swiezego runtime runu po rebuildzie oraz porownania top-level vs embedded evidence na realnym JSONL.
