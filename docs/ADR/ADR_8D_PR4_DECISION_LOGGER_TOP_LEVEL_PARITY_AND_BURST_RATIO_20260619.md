# ADR-8D: PR4 Decision Logger Top-Level Parity and Canonical Burst Ratio

Status: IMPLEMENTED / STATIC_VALIDATION_COMPLETED
Typ: ADR-8D / decision logging, top-level evidence parity, schema hygiene
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: PR4 z planu evidence coverage contract; DecisionLogger top-level parity, canonical `burst_ratio`, nullable top-level price vectors, temporal delta/rate parity checks, bez policy usage
Poziom ryzyka: HIGH

Dotkniete moduly/pliki:
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `docs/ADR/ADR_8D_PR4_DECISION_LOGGER_TOP_LEVEL_PARITY_AND_BURST_RATIO_20260619.md`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ADR-ach PR1/PR2/PR3.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR4, czyli domknac top-level convenience fields w `GatekeeperBuyLog` tak, aby nie rozjezdzaly sie z embedded `v3_materialized_feature_snapshot`, bez zmiany decyzji Gatekeepera i bez cichej imputacji danych.

Rzeczywisty przebieg:
- Potwierdzono, ze worktree zawiera juz szeroki dirty state z PR1-PR3 oraz wczesniejszymi runtime fixami. Nie cofano cudzych ani wczesniejszych zmian.
- Potwierdzono, ze aktywny path mapowania decyzji idzie przez `GatekeeperAssessment::to_buy_log()`.
- Potwierdzono, ze top-level `burst_ratio` powinien oznaczac canonical tx-intel feature z `MaterializedFeatureSet`, a nie phase2-only velocity value.
- Domknieto jawna separacje `burst_ratio` vs `phase2_burst_ratio`.
- Podbito schema version loga, bo top-level JSON shape i semantyka `burst_ratio` sa istotne dla replay/offline consumers.
- Rozszerzono/utwardzono testy tak, aby sprawdzaly canonical burst parity, phase2 diagnostic separation, temporal delta/rate parity, nullable `vectors_prices` i JSON shape.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT, `MaterializedFeatureSet`, DecisionLogger/replay boundary, shadow/live separation.

Zaladowane dokumenty/specjalistyczne instrukcje:
- Repo-local `AGENTS.md`
- `.agents/skills/ghost-execution/SKILL.md`

Nie ladowano dodatkowych dokumentow specjalistycznych:
- `solana-execution-path-engineer`: PR4 nie dotyka sendera, builderow transakcji ani potwierdzen.
- `seer-ingest-event-integrity-specialist`: PR4 nie dotyka ingestu, parserow ani stream ordering.
- `gatekeeper-policy-auditor`: PR4 nie zmienia policy/verdictow; testy potwierdzaja logger mapping.

## 3. Opis problemu - 3W2H

What:
Po PR1-PR3 pozostaly residual issues w warstwie trwawego JSONL:
- top-level fields mogly miec inna kompletosc niz embedded snapshot,
- `burst_ratio` top-level mogl byc rozumiany jako phase2-only, mimo ze embedded SSOT ma canonical tx-intel `burst_ratio`,
- `rate_mcap_sol_per_s_2s_to_3s` bylo krytycznym polem do sprawdzenia, bo w runtime review wczesniej widziano je jako embedded-only,
- nullable price vector musial zachowac dlugosc i `null` alignment, a nie znikac przy pierwszym missing price.

Where:
- `GatekeeperBuyLog`
- `GatekeeperAssessment::to_buy_log()`
- top-level JSONL decision records
- embedded `v3_materialized_feature_snapshot`
- downstream selector/replay/dataset consumers

Why it matters:
Jesli top-level JSONL nie jest zgodny z embedded SSOT, to offline selector, DTW consumer albo replay tooling moze trenowac na innym obrazie decyzji niz ten, ktory faktycznie byl materializowany w runtime. To niszczy auditability i moze zamienic artifact loggingowy w pozorny sygnal rynkowy.

How observed:
W runtime review po wczesniejszych zmianach wskazano:
- core delty byly naprawione, ale rate field wymagalo domkniecia/parity,
- `vectors_prices` musialo zachowywac nullable shape,
- top-level `burst_ratio` powinien byc zgodny z embedded canonical value.

How many / scale:
Zmiana dotyczy kazdego emitowanego `GatekeeperBuyLog`. Nie zmienia decyzji BUY/REJECT/TIMEOUT, nie zmienia thresholdow, nie zmienia policy path.

## 4. Przyczyna zrodlowa

Root cause:
Logger mial historycznie mieszane role:
- czesc pol top-level byla phase/gate diagnostic,
- czesc pol miala byc convenience projection z canonical `MaterializedFeatureSet`,
- embedded V3 snapshot stal sie SSOT, ale top-level compatibility layer nie mial do konca jawnie rozdzielonej semantyki.

Konkretnie:
- `burst_ratio` jako nazwa bez prefiksu powinna oznaczac canonical feature, nie phase2-only value.
- Phase2-only value musi miec osobna nazwe, zeby nie zmieniac znaczenia pola w zaleznosci od konsumenta.
- Nullable vectors musza serializowac brak ceny jako `null` w osi tickow, a nie usuwac wektor.
- Additive top-level fields wymagaja schema hygiene, bo downstream nie powinien mylic starego i nowego shape loga.

## 5. Strategia naprawy

Przyjeta strategia:
- Utrzymac `MaterializedFeatureSet` jako SSOT.
- Ustawic top-level `burst_ratio` z `feature_snapshot.tx_intel_features.burst_ratio`.
- Zachowac phase2-only value jako additive diagnostic `phase2_burst_ratio`.
- Nie zmieniac polityki Gatekeepera ani strict metric usage.
- Zachowac `None` jako brak evidence; nie zamieniac nulli na zera.
- Zachowac nullable `vectors_prices: Vec<Option<f64>>` jako aligned tick vector.
- Potwierdzic parity testami na warstwie `GatekeeperAssessment::to_buy_log()`.
- Podbic `GATEKEEPER_BUY_LOG_SCHEMA_VERSION`, bo top-level shape/semantyka sa addytywnie inne.

Granice:
- Brak policy activation.
- Brak zmiany verdictow.
- Brak zmian w Solana execution path.
- Brak zmian w Seer/ingest.
- Brak runtime rollout proof w tym ADR; long proof ma byc osobnym etapem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: schema version
- Plik: `ghost-brain/src/oracle/decision_logger.rs`
- Podbito `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` do `27`.
- Uzasadnienie: top-level schema/semantyka loggera zostala rozszerzona i nie powinna byc nierozroznialna od starszych rekordow.

Zmiana 2: phase2 burst separation
- Plik: `ghost-brain/src/oracle/decision_logger.rs`
- Dodano additive field:
  - `phase2_burst_ratio: Option<f64>`
- Pole ma `#[serde(default, skip_serializing_if = "Option::is_none")]`, wiec stare rekordy/configi zachowuja kompatybilnosc deserializacji.

Zmiana 3: canonical burst mapping
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `burst_ratio` top-level pozostaje mapowany z:
  - `feature_snapshot.tx_intel_features.burst_ratio`
- `phase2_burst_ratio` jest mapowany z:
  - `phase2_velocity.burst_ratio`
- To realizuje wybrana opcje A: top-level `burst_ratio == embedded tx_intel_features.burst_ratio`.

Zmiana 4: temporal delta/rate parity coverage
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Test `test_fingerprint_metrics_map_to_buy_log_and_summary` wypelnia embedded `temporal_deltas` i sprawdza top-level projection dla core delta/rate fields, w tym:
  - `delta_mcap_*`
  - `delta_price_pct_*`
  - `delta_burstratio_*`
  - `delta_buy_count_*`
  - `delta_unique_signers_*`
  - `delta_tx_count_*`
  - `delta_net_quote_sol_*`
  - `delta_jito_tip_intensity_*`
  - `delta_signer_cross_pool_velocity_*`
  - `delta_flipper_presence_ratio_*`
  - `rate_mcap_sol_per_s_2s_to_3s`

Zmiana 5: nullable vector parity coverage
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Test potwierdza, ze `vectors_prices` moze zawierac `None` i serializuje sie jako JSON `null`, zachowujac dlugosc osi tickow.
- To chroni DTW/offline consumerow przed utrata alignmentu.

Zmiana 6: test identity-field assertion
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Poprawiono test `test_buy_log_serialization_without_identity_fields_absent`.
- Wczesniejszy test uzywal `json.contains("\"base_mint\"")`, co lapalo zagniezdzone embedded evidence fields.
- Nowy test sprawdza brak kluczy tylko na top-level object, czyli faktyczna semantyke `skip_serializing_if` dla identity fields.
- Runtime behavior nie zostalo zmienione.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Format touched Rust files | `rustfmt --edition 2021 ghost-brain/src/oracle/decision_logger.rs ghost-launcher/src/components/gatekeeper.rs` | passed | PASS |
| Ghost brain buy log tests | `cargo test -q -p ghost-brain gatekeeper_buy_log --lib` | 3 passed | PASS |
| Ghost launcher buy log mapping tests | `cargo test -q -p ghost-launcher buy_log --lib` | 15 passed | PASS |

Uwaga:
Pierwsze uruchomienie `rustfmt` bez `--edition 2021` nie bylo miarodajne, bo domyslnie probowalo parsowac pliki jako Rust 2015 i trafilo na istniejacy `async fn` syntax. Po podaniu edycji 2021 formatowanie przeszlo.

### Runtime/log proof status

Code path proof:
- `GatekeeperAssessment::to_buy_log()` jest aktywnym mapperem do `GatekeeperBuyLog`.
- Top-level `burst_ratio` pochodzi z `feature_snapshot.tx_intel_features.burst_ratio`.
- `phase2_burst_ratio` jest osobnym diagnostic field.
- Temporal top-level fields sa testowane jako projection z embedded `TemporalDeltaFeatures`.
- Nullable `vectors_prices` jest testowane jako aligned `Vec<Option<f64>>`.

Not executed in this ADR:
- Fresh shadow rollout artifact validation.
- Longer proof na realnym R37/R38 JSONL po restarcie runtime.

## 8. Ryzyka regresji i jak zostaly ograniczone

Ryzyko 1: zmiana znaczenia `burst_ratio`.
- Kiedy: top-level `burst_ratio` raz znaczy phase2-only, raz canonical tx-intel.
- Mitigacja: `burst_ratio` zostaje canonical SSOT projection; phase2-only trafia do `phase2_burst_ratio`.

Ryzyko 2: downstream zbudowany na starym schema nie rozpoznaje nowego shape.
- Kiedy: nowe pola sa emitowane przy starym `log_schema_version`.
- Mitigacja: schema podbita do `27`.

Ryzyko 3: `null` price niszczy caly top-level vector.
- Kiedy: serializer wymaga `Vec<f64>` i usuwa pole przy missing price.
- Mitigacja: `vectors_prices` ma nullable shape `Vec<Option<f64>>`; test sprawdza JSON `null`.

Ryzyko 4: rate field zostaje embedded-only.
- Kiedy: `rate_mcap_sol_per_s_2s_to_3s` istnieje w embedded temporal deltas, ale nie na top-level.
- Mitigacja: test parity sprawdza top-level `rate_mcap_sol_per_s_2s_to_3s`.

Ryzyko 5: top-level testy lapia zagniezdzone embedded keys i falszywie blokuja loggera.
- Kiedy: assertion uzywa substring search po calym JSON.
- Mitigacja: test identity fields sprawdza tylko top-level object keys.

Ryzyko 6: policy zaczyna uzywac degraded/carried values przez przypadek.
- Kiedy: PR4 logger fields zostalyby podlaczone do strict policy.
- Mitigacja: PR4 nie zmienia Gatekeeper policy path, verdictow ani thresholdow.

Ryzyko 7: shadow/live separation.
- Kiedy: logger change pociaga zmiane execution lub shadow dispatch.
- Mitigacja: dotkniete zmiany sa w DecisionLogger/mapping/testach; brak zmian w execution sender/build path w PR4 scope.

## 9. Stan po zmianie

Zamkniete w PR4 statycznie/testowo:
- canonical top-level `burst_ratio` zgodny z embedded tx-intel SSOT,
- separate `phase2_burst_ratio` diagnostic field,
- top-level temporal delta/rate projection test, w tym `rate_mcap_sol_per_s_2s_to_3s`,
- nullable top-level `vectors_prices` alignment test,
- schema version bump dla nowego logger shape,
- targeted tests przechodza.

Nie zamkniete w tym ADR:
- runtime proof na swiezym shadow rollout JSONL,
- formalne potwierdzenie coverage procentow po dluzszym runie,
- PR5 policy usage/evidence policy enforcement.

## 10. Rekomendacja

PR4 mozna traktowac jako domkniety na poziomie kodu i targeted tests. Przed formalnym runtime closure nalezy wykonac longer proof na swiezym runie i sprawdzic:
- `log_schema_version == 27`,
- top-level `burst_ratio == v3_materialized_feature_snapshot.tx_intel_features.burst_ratio`,
- `phase2_burst_ratio` istnieje tylko jako diagnostic, gdy `phase2_velocity` jest obecne,
- top-level temporal delta/rate fields sa zgodne z embedded `temporal_deltas`,
- `vectors_prices` nie znika przy embedded `null`, tylko zachowuje aligned nullable vector.
