# ADR-8D: DTW decision-time series coverage and temporal delta materialization

Status: IMPLEMENTED / TARGETED_TESTS_PASS
Typ: ADR-8D / SSOT feature materialization and decision-log evidence hardening
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: DTW price vector materialization, temporal delta features, Gatekeeper buy log / selector exposure
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/tests/session_lifecycle_tests.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `scripts/build_selector_gatekeeper_feature_context.py`
- `analiza_porownawcza.py`

Powiazane runy/logi/raporty:
- Brak nowego live/shadow runtime proof w ramach tej zmiany.
- Walidacja wykonana lokalnie testami jednostkowymi/integracyjnymi i kompilacja skryptow.

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
- Naprawic DTW coverage, ktore w raportach widzialo tylko niewielka czesc wektorow cenowych.
- Zmaterializowac temporalne delty "szybkosci dojscia" do stanu obserwacji: mcap, burst ratio, buy count, unique signers i dodatkowe metryki wskazane przez uzytkownika.
- Zachowac `MaterializedFeatureSet` jako SSOT i `PoolObservationSession::materialize_features()` jako granice materializacji.
- Nie zmieniac aktywnych verdictow, reason-code taxonomy, TX buildera, live execution ani shadow/live separation.

Rzeczywisty przebieg:
- Potwierdzono aktywna sciezke: `PoolObservationSession::materialize_features()` -> `MaterializedFeatureSet` -> Gatekeeper V2/V2.5 -> DecisionLogger/selector.
- Ustalono root cause DTW starvation: ekstrakcja wektorow czytala z `buffered_txs`, ktory jest terminalnym/relay buforem i po przejsciu do `Approved` nie reprezentowal juz pelnej historii obserwacyjnej.
- Dodano read-only-for-materialization historie `decision_series_txs` w `GatekeeperBuffer`, zasilana po dedupe i po `update_tracking()`, retencjonowana po event-time observation window zamiast po liczbie probek.
- Dodano strukturalne `DecisionTimeSeriesFeatures` oraz `TemporalDeltaFeatures` do `MaterializedFeatureSet`.
- Rozszerzono `GatekeeperBuyLog`, runtime enrichment i selector feature context o nowe pola, bez wymuszania ich w policy scoring.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: ochrona SSOT, materialization boundary, DecisionLogger/replay i shadow/live separation.
- `rust-master`: bounded Rust state, brak niekontrolowanego shared mutable state, testy i kompatybilnosc serde.
- `trading-systems`: oddzielenie feature evidence od aktywnej decyzji i zachowanie fail-closed przy brakach probki.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/oracle-session-runtime-engineer.md`

Powod:
Zmiana dotyka aktywnej materializacji `MaterializedFeatureSet`, DecisionLogger schema i runtime session path. Nie dotyka Solana execution path, TX buildera ani live sendera.

## 3. Opis problemu - 3W2H

What:
Raporty DTW mialy niska kompletność wektorow cenowych; dodatkowo selector/Gatekeeper log widzial glownie wartosci stanu w T, bez informacji o predkosci zmian miedzy 1s, 2s i 3s.

Where:
- `GatekeeperBuffer::extract_window_vectors()`
- `GatekeeperBuffer` runtime ingest paths
- `PoolObservationSession::materialize_features()`
- `MaterializedFeatureSet`
- `GatekeeperBuyLog`
- selector feature context builder

Why it matters:
Kształt wykresu ceny w pierwszych sekundach jest istotnym sygnalem separujacym front-loaded bot capital injection od organicznego wzrostu. DTW i selektor nie powinny tracic tickow po wczesnym approve ani sprowadzac obserwacji do suchego snapshotu w T.

How observed:
Uzytkownik wskazal raportowe coverage ok. 6% oraz brak delt takich jak `delta_mcap_1s_to_2s`, `delta_burstratio_1s_to_3s`, `delta_jito_tip_intensity_1s_to_2s`, `delta_signer_cross_pool_velocity_1s_to_3s` i `delta_flipper_presence_ratio_1s_to_3s`.

How many / scale:
Zmiana dotyczy kazdej runtime materializacji Gatekeeper V2/V2.5 snapshotu, ale tylko jako evidence/log/selector feature. Nie zmienia aktywnych progow ani verdictow.

## 4. Przyczyna zrodlowa

Root cause 1:
DTW extraction uzywala `buffered_txs`, czyli bufora terminalnego/relay. Ten bufor moze zostac przeniesiony przez terminalny BUY albo nie byc zasilany po `PoolState::Approved`, gdy kolejne transakcje ida jako `ApprovedTx`.

Mechanizm:
- Przed approve ticki trafialy do `buffered_txs`.
- Po approve unikalne ticki aktualizowaly tracking, ale nie tworzyly pelnej historii dla ekstrakcji DTW.
- `extract_window_vectors()` i nowe serie cenowe widzialy tylko fragment okna, mimo ze runtime widzial transakcje.

Root cause 2:
DTW mial legacy flat vectors bez jawnej informacji o coverage i zrodlach ceny. Brak ceny byl trudny do odroznienia od braku ticka albo od internal NaN legacy fallback.

Root cause 3:
Model decyzyjny/logging mial metryki stanu obserwacji, ale nie mial materializowanych delt anchor-to-anchor dla 1s->2s i 1s->3s.

Odrzucone hipotezy:
- Rekonstruowac wektory z DecisionLoggera pozniej: odrzucone, bo SSOT musi powstac w runtime materialization boundary.
- Czytac live mutable state w Gatekeeper policy: odrzucone, naruszaloby SSOT.
- Uzyc tylko `price_history`: odrzucone, bo quote/reserve price moze istniec dla ticka mimo braku Phase6 price point.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac osobna historie `decision_series_txs`, ktora przechowuje wszystkie unikalne ticki zaakceptowane przez Gatekeeper ingest/tracking w obserwacyjnym `[curve_t0, curve_t0 + max_wait_time_ms]`.
- Materializowac strukturalne `DecisionTimeSeriesFeatures` z aligned offsets, SOL amounts, optional prices, market caps, deltas ceny i price-source counts.
- Dla `MaterializedFeatureSet` uzywac full in-memory series (`max_len=0`, bez downsamplingu); zachowac legacy `WindowVectors` / buy-log arrays jako limitowany adapter dla istniejacych analiz.
- Materializowac `TemporalDeltaFeatures` z anchorami 1s/2s/3s/T i deltami.
- CPV delty materializowac dopiero w session boundary, bo autorytetem CPV jest shared `CrossPoolVelocityIndex`, nie sam `GatekeeperBuffer`.
- Eksponowac nowe pola w JSONL i selector feature context jako decision-neutral evidence.

Granice:
- Brak zmian w aktywnym Gatekeeper scoring/policy.
- Brak zmian verdictow, reason code taxonomy i hard-fail order.
- Brak zmian TX buildera, Helius Sender, blockhash/retry/confirmation.
- Brak promocji shadow evidence do live inclusion.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: observation-window decision series memory
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Dodano `decision_series_txs: VecDeque<GatekeeperBufferedTx>`.
- Bufor jest zasilany po dedupe i po `update_tracking()` w long tracking, normal tracking, post-approval relay i legacy test path.
- Bufor nie jest count-capped dla DTW; retencja jest ograniczona do obserwacyjnego event-time window `[curve_t0, curve_t0 + max_wait_time_ms]`, zeby zachowac kazdy tick w oknie.
- Bufor sluzy tylko do materializacji DTW/delt, nie do decyzji.

Zmiana 2: DTW/decision-time series features
- Pliki: `ghost-core/src/checkpoint/types.rs`, `ghost-core/src/checkpoint/mod.rs`, `ghost-core/src/checkpoint/feature_builder.rs`, `ghost-launcher/src/components/gatekeeper.rs`
- Dodano `DecisionTimeSeriesFeatures`, `DecisionTimeSeriesPriceSource`, `DecisionTimeSeriesSourceCounts`.
- Dodano statusy degraded/unavailable dla missing/partial price i sparse series.
- Ekstrakcja ceny uzywa kolejnosci: reserves -> `price_quote` -> ostatni finite `price_history`.
- `MaterializedFeatureSet` dostal `decision_time_series` z `#[serde(default)]`.
- `PoolObservationSession::materialize_features()` materializuje full decision-time series przez `current_decision_time_series(0)`.

Zmiana 3: temporal delta features
- Pliki: `ghost-core/src/checkpoint/types.rs`, `ghost-launcher/src/components/gatekeeper.rs`, `ghost-launcher/src/session/observation.rs`
- Dodano `TemporalAnchorSnapshot` i `TemporalDeltaFeatures`.
- Materializowane sa delty:
  - `delta_mcap_1s_to_2s`, `delta_mcap_1s_to_3s`, `delta_mcap_2s_to_3s`
  - `delta_price_pct_*`
  - `delta_burstratio_*`
  - `delta_buy_count_*`
  - `delta_unique_signers_*`
  - `delta_tx_count_*`
  - `delta_net_quote_sol_*`
  - `delta_jito_tip_intensity_1s_to_2s`, `delta_jito_tip_intensity_1s_to_3s`
  - `delta_signer_cross_pool_velocity_1s_to_2s`, `delta_signer_cross_pool_velocity_1s_to_3s`
  - `delta_flipper_presence_ratio_1s_to_2s`, `delta_flipper_presence_ratio_1s_to_3s`
  - rate fields dla mcap, buy count, unique signers i net quote.
- `MaterializedFeatureSet` dostal `temporal_deltas` z `#[serde(default)]`.

Zmiana 4: CPV delta authority
- Plik: `ghost-launcher/src/session/observation.rs`
- Dodano `materialize_cpv_temporal_deltas()`.
- CPV na anchorach 1s/2s/3s jest liczony z `CrossPoolVelocityIndex::compute_for_transactions()` dla prefixu tx do danego cutoffu.
- Delt CPV nie wylicza sam `GatekeeperBuffer`, zeby nie tworzyc konkurencyjnego autorytetu.

Zmiana 5: DecisionLogger / buy log exposure
- Pliki: `ghost-brain/src/oracle/decision_logger.rs`, `ghost-launcher/src/components/gatekeeper.rs`, `ghost-launcher/src/oracle_runtime.rs`
- Dodano coverage/source fields dla vector price:
  - `vectors_price_finite_count`
  - `vectors_price_missing_count`
  - `vectors_price_coverage_ratio`
  - `vectors_price_source_reserve_count`
  - `vectors_price_source_quote_count`
  - `vectors_price_source_history_count`
- Dodano flat delta/rate fields do `GatekeeperBuyLog`.
- Nowe pola sa `Option` + serde default/skip, wiec stare JSONL pozostaja kompatybilne.

Zmiana 6: selector/report exposure
- Pliki: `scripts/build_selector_gatekeeper_feature_context.py`, `analiza_porownawcza.py`
- Selector feature builder dostal nowe raw fields, wiec generuje `gk_*` kolumny dla nowych delt i coverage.
- Raport DTW pokazuje srednia/medianowa coverage oraz finite/missing counts, zeby nie ukrywac brakow cen.

Zmiana 7: tests
- Pliki: `ghost-launcher/src/components/gatekeeper.rs`, `ghost-launcher/tests/session_lifecycle_tests.rs`
- Dodano quote-only DTW test bez Phase6 `price_history`.
- Dodano test, ze `max_len=0` zachowuje full tick vector powyzej legacy limitu 200.
- Dodano temporal delta unit test.
- Rozszerzono post-approval relay test o assertion, ze DTW series zawiera ticki po BUY/ApprovedTx.
- Dodano session-level materialization test z decision series, deltas, Jito, CPV i flipper ratio.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status |
|---|---|---|---|
| Session materialization DTW+deltas | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_decision_series_and_temporal_deltas_from_session_buffer -- --nocapture` | 1 passed | PASS |
| Full in-memory series beyond legacy cap | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher test_decision_time_series_max_len_zero_keeps_full_tick_vector -- --nocapture` | 1 passed | PASS |
| Post-approval DTW history | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher test_full_flow_active_relay_after_buy -- --nocapture` | 1 passed | PASS |
| Legacy vector helpers | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher extract_window_vectors -- --nocapture` | 4 passed | PASS |
| Quote-only DTW fallback | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher test_decision_time_series_uses_price_quote_without_phase6_price_history -- --nocapture` | 1 passed | PASS |
| Temporal delta unit test | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher test_temporal_delta_features_materialize_speed_of_change -- --nocapture` | 1 passed | PASS |
| Existing CPV materialization | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_cpv_from_shared_session_index -- --nocapture` | 1 passed | PASS |
| GatekeeperBuyLog write | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain test_gatekeeper_buy_log_file_write -- --nocapture` | 1 passed | PASS |
| Legacy GatekeeperBuyLog serde | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain test_gatekeeper_buy_log_v19_without_v3_fields_deserializes -- --nocapture` | 1 passed | PASS |
| Python syntax | `python3 -m py_compile scripts/build_selector_gatekeeper_feature_context.py analiza_porownawcza.py` | passed | PASS |

## 8. Wynik i aktualny status

Naprawione:
- DTW extraction ma teraz materialization-safe historie wszystkich zaakceptowanych tickow w oknie, rowniez po `ApprovedTx`.
- `MaterializedFeatureSet` zawiera strukturalne decision-time series i temporal deltas.
- Selector i buy log dostaja nowe pola delta/coverage bez zmiany aktywnej decyzji.
- CPV delty sa liczone z wlasciwego shared session index, nie z duplikowanej lokalnej logiki.

Pozostaje do potwierdzenia runtime:
- Realny shadow/live-like run powinien pokazac wzrost `vectors_price_coverage_ratio` i `vectors_price_finite_count` w raportach.
- Ta implementacja nie uruchamiala nowego burn-in/runtime proof.

## 9. Ryzyka i zabezpieczenia

Ryzyko 1: dodatkowa pamiec per pool.
- Mitigacja: `decision_series_txs` jest retencjonowane po event-time observation window, a nie po calej zywnotnosci poola. Nie jest count-capped w samym oknie, bo to byloby sprzeczne z wymaganiem pelnego wektora tickow dla DTW.

Ryzyko 2: mieszanie evidence z policy.
- Mitigacja: nowe pola sa materializowane/logowane, ale nie podlaczone do Gatekeeper verdict scoring.

Ryzyko 3: CPV dual authority.
- Mitigacja: `GatekeeperBuffer` zostawia CPV delty jako `None`; session materialization uzupelnia je z `CrossPoolVelocityIndex`.

Ryzyko 4: kompatybilnosc JSONL.
- Mitigacja: nowe pola `GatekeeperBuyLog` sa opcjonalne i maja serde defaults; legacy deserialize test przeszedl.

## 10. Decyzja

Akceptujemy materializacje DTW decision-time series i temporal delta features jako additive SSOT evidence w `MaterializedFeatureSet`.

Nie akceptujemy w tej zmianie:
- aktywacji nowych delt jako Gatekeeper hard/soft policy,
- zmian progow,
- zmian execution path,
- runtime burn-in jako udowodnionego wyniku produkcyjnego.

Status koncowy:
Core code closed na poziomie lokalnych testow i static checks. Formal runtime evidence pozostaje do zebrania w osobnym shadow/burn-in runie.
