# ADR-8D: R37 DTW decision-series residual repair

Status: IMPLEMENTED / TARGETED_TESTS_PASS / RUNTIME_REPROOF_REQUIRED
Typ: ADR-8D / SSOT evidence repair / DecisionLogger top-level compatibility
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, local working tree
Commit/PR: not committed at report time
Zakres: R37 residual defects in DTW decision-time series, top-level JSONL vectors/deltas, AccountStateCore price evidence
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `scripts/build_selector_gatekeeper_feature_context.py`
- `docs/ADR/ADR_8D_R37_DTW_DECISION_SERIES_RESIDUAL_REPAIR_20260618.md`

Powiazane runy/logi/raporty:
- R37 evidence wskazane przez uzytkownika:
  `logs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/e50df85f7880fd9115b8d1c09a6fc1c71777328988346ff456fccec5802ca955/gatekeeper_v2_decisions.jsonl`
- Poprzedni ADR bazowy:
  `docs/ADR/ADR_8D_DTW_DECISION_SERIES_AND_TEMPORAL_DELTAS_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Punkt startowy:
Uzytkownik zweryfikowal R37 produkcyjnym artefaktem JSONL i wskazal, ze pierwszy etap naprawy nie moze byc zamkniety w najmocniejszym sensie.

Potwierdzone symptomy z R37:
- `v3_materialized_feature_snapshot.decision_time_series` byl obecny dla wszystkich decyzji, ale 9 decyzji z tickami mialo missing prices, razem 28 probek bez ceny.
- Top-level `vectors_*` reprezentowal AB-window (`ab_window_ms=2000`), a nie pelna serie decyzyjna.
- Top-level delty poza CPV byly nieobecne mimo pelnych embedded `temporal_deltas`.
- `ts_offsets_ms` w embedded decision series bywal niemonotoniczny.

Cel tego etapu:
- Usunac przyczyne niemonotonicznych offsetow.
- Poszerzyc bezpieczne zrodla ceny dla decision-time series bez zmiany Gatekeeper policy.
- Zatrzymac nadpisywanie top-level `vectors_*` i delt przez AB-window enrichment.
- Zachowac SSOT: `MaterializedFeatureSet` pozostaje kanonicznym snapshotem decyzyjnym.

Non-goals:
- Brak zmian strict threshold policy, reason-code taxonomy i BUY/REJECT/TIMEOUT semantics.
- Brak zmian TX buildera, Helius Sendera, live execution, blockhash/retry/confirmation.
- Brak syntetycznego future-backfill cen wstecz w czasie.
- Brak progu decyzyjnego opartego o nowe zrodla ceny.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT, decision materialization, DecisionLogger/replay boundary.
- `rust-master`: deterministyczne sortowanie event-time, bounded state, brak policy side effects.
- `trading-systems`: rozdzielenie evidence od aktywnej decyzji i zachowanie auditability.

Specjalisci logiczni:
- Primary: SSOT Feature Materialization Guardian.
- Supporting: Decision Logging Replay Analyst, Oracle Session Runtime Engineer.

Zaladowane dokumenty:
- Skill docs: `ghost-execution`, `rust-master`, `trading-systems`.
- Repo `AGENTS.md` w zakresie zasad SSOT, DecisionLogger i ADR-8D.

Nie ladowano:
- Solana Execution Path Engineer, bo zmiana nie dotyka TX buildera/sendera/live path.
- Gatekeeper Policy Auditor jako osobnego doc, bo policy/verdict thresholds nie byly zmieniane.

## 3. Opis problemu - 3W2H

What:
R37 dowiodl, ze embedded V3 evidence istnieje, ale jakosc i top-level exposure nadal sa niepelne:
- brak cen dla czesci tickow,
- ujemne interwaly w `ts_offsets_ms`,
- top-level `vectors_*` i delty nie odpowiadaja pelnemu embedded snapshotowi.

Where:
- `GatekeeperBuffer::collect_decision_series_samples()`
- `PoolObservationSession::on_account_state_core_updated*()`
- `GatekeeperAssessment::to_buy_log()`
- `enrich_buy_log_with_vectors()`
- `GatekeeperBuyLog`
- selector raw feature list

Why it matters:
DTW i audyt temporalny nie moga bezpiecznie porownywac ksztaltu ceny, jezeli os czasu jest niemonotoniczna albo top-level JSONL pokazuje tylko AB-window zamiast pelnej decision series. Model selectorowy traci tez praktyczne top-level kolumny delt, mimo ze embedded MFS je posiada.

How observed:
Artefakt R37 mial embedded delty, ale top-level delty poza CPV byly zerowo obecne. Dodatkowo niemonotoniczne przyklady mialy sekwencje offsetow typu `[1, 1, 1, 0, ...]` i `[0, 4, 4, 3, ...]`.

How many / scale:
Dotyczy kazdej decyzji Gatekeeper logowanej z V3 replay payload, szczegolnie analiz selector/DTW czytajacych top-level JSONL.

## 4. Przyczyna zrodlowa

Root cause 1: order-by-arrival zamiast order-by-event-time.
`collect_decision_series_samples()` iterowal `decision_series_txs` w kolejnosci przyjscia. Przy out-of-order delivery event-time offsety mogly isc wstecz, mimo ze `TxKey` ma juz deterministyczny porzadek event-time.

Root cause 2: price resolver nie korzystal z bezpiecznych zrodel ceny dostepnych w runtime.
Resolver uzywal `reserve -> price_quote -> price_history`. R37 pokazal decyzje z AccountStateCore price evidence (`account_features.price_sol`, `market_cap_sol`, update_count > 0), ale decision series nie mial osobnego kanalu price observations. Dodatkowo `market_cap_sol` bez `price_quote` nie byl wykorzystywany do ceny.

Root cause 3: AB-window enrichment nadpisywal MFS evidence.
`GatekeeperAssessment::to_buy_log()` ustawial top-level temporal deltas z `feature_snapshot.temporal_deltas`, ale pozniejszy `enrich_buy_log_with_vectors()` liczyl serie/delty z AB-window i bezwarunkowo nadpisywal te pola. Dla okna AB delty czesto byly `None`, wiec top-level JSONL tracil embedded wartosci.

Root cause 4: top-level `vectors_*` nie bylo plaskim widokiem pelnej MFS decision series.
Pola byly wypelniane dopiero w `oracle_runtime` z AB-window. Embedded `decision_time_series` bylo pelniejsze, ale konsumenci top-level widzieli tylko adapter okna AB.

## 5. Strategia naprawy

Przyjeta strategia:
- Sortowac probki decision series deterministycznie po `TxKey` przed wyliczeniem offsetow/interwalow.
- Dodac osobny `decision_series_price_observations` sidecar w `GatekeeperBuffer` dla AccountStateCore price evidence.
- Nie dopisywac AccountStateCore probek do Phase6 `price_history`, aby nie zmienic policy inputs.
- Rozszerzyc price resolver o kolejnosc:
  `reserve -> price_quote -> market_cap -> price_history -> account_state -> carry_forward`.
- `carry_forward` jest dozwolony tylko z poprzedniej probki, nigdy z przyszlosci, i oznacza `DecisionTimeSeriesPriceCarriedForward` / degraded evidence.
- Wypelniac top-level `vectors_*` z `MaterializedFeatureSet.decision_time_series` w `to_buy_log()`.
- Zmienic `enrich_buy_log_with_vectors()` na fill-if-missing dla wektorow i delt, aby AB-window nie nadpisywal MFS.

Odrzucone alternatywy:
- Future-backfill z finalnej ceny account state na wczesniejsze ticki: odrzucone jako temporal leakage.
- Wspolne dopisanie AccountStateCore do `price_history`: odrzucone, bo zmieniloby Phase6/policy surface.
- Zmiana semantyki Gatekeeper policy albo thresholds: poza zakresem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: deterministyczna chronologia.
- `collect_decision_series_samples()` buduje kopie tickow w oknie i sortuje je po `TxKey`.
- Interwaly sa liczone dopiero po sortowaniu.
- Zero interwaly pozostaja mozliwe dla wielu tickow z tym samym timestampem ms; ujemne interwaly nie powinny juz powstawac.

Zmiana 2: AccountStateCore price sidecar tylko dla DTW evidence.
- Dodano `DecisionSeriesPriceObservation`.
- Dodano `GatekeeperBuffer::record_decision_series_account_price_sample()`.
- `PoolObservationSession::on_account_update()` i runtime session refresh przekazuja `receive_ts_ms` accepted account update.
- Sidecar jest retencjonowany tym samym event-time observation window co `decision_series_txs`.

Zmiana 3: rozszerzone zrodla ceny.
- Dodano `DecisionTimeSeriesPriceSource::{MarketCap, AccountState, CarryForward}`.
- Dodano source-count fields:
  - `market_cap`
  - `account_state`
  - `carry_forward`
- Dodano degraded reason `DecisionTimeSeriesPriceCarriedForward`.
- `market_cap_sol` moze dac cene przez `market_cap / PUMP_TOKEN_TOTAL_SUPPLY`.
- `carry_forward` uzywa tylko poprzednio znanej ceny i pozostawia evidence degraded.

Zmiana 4: top-level vectors z MFS.
- `GatekeeperAssessment::to_buy_log()` wypelnia:
  - `vectors_max_len`
  - `vectors_ts_offsets_ms`
  - `vectors_sol_amounts`
  - `vectors_prices`
  - `vectors_interval_ms`
  - `vectors_d_price`
  - coverage/source-count fields
  bezposrednio z `feature_snapshot.decision_time_series`.

Zmiana 5: AB-window enrichment nie niszczy MFS.
- `enrich_buy_log_with_vectors()` nadal liczy selector flow features z AB-window.
- Wektory i delty sa ustawiane tylko jezeli top-level log nie ma juz MFS wartosci.
- To zachowuje legacy fallback dla starych logow/testow bez MFS.

Zmiana 6: DecisionLogger i selector exposure.
- `GatekeeperBuyLog` dostal opcjonalne pola:
  - `vectors_price_source_market_cap_count`
  - `vectors_price_source_account_state_count`
  - `vectors_price_source_carry_forward_count`
- `scripts/build_selector_gatekeeper_feature_context.py` dostal te pola w RAW_FEATURES.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda | Wynik |
|---|---|---|
| decision series source/order tests | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher decision_time_series -- --nocapture` | PASS, 6 tests |
| top-level MFS overwrite guard | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher test_enrich_buy_log_with_vectors_preserves_materialized_series_and_deltas -- --nocapture` | PASS |
| session MFS materialization | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_decision_series_and_temporal_deltas_from_session_buffer -- --nocapture` | PASS |
| DecisionLogger GatekeeperBuyLog | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain gatekeeper_buy_log -- --nocapture` | PASS, 3 tests |
| selector/report Python syntax | `python3 -m py_compile scripts/build_selector_gatekeeper_feature_context.py analiza_porownawcza.py` | PASS |
| formatting | `rustfmt --edition 2021 ...` on edited Rust files | PASS |

## 8. Wynik i aktualny status

Naprawione w kodzie:
- Niemonotoniczne offsety wynikajace z arrival-order iteration.
- Brak price resolvera dla `market_cap_sol`-only i AccountStateCore evidence.
- Brak jawnej klasyfikacji carry-forward.
- Nadpisywanie top-level delt przez AB-window.
- Top-level `vectors_*` dla V3 decyzji jako plaski widok pelnej MFS decision series.

Pozostaje do potwierdzenia runtime:
- Nowy shadow/burn-in repro dla R37/R38 powinien potwierdzic:
  - `series_negative_interval_records == 0`,
  - top-level delta counts zgodne z embedded counts dla tych samych pol,
  - wzrost `vectors_price_source_account_state_count` tam, gdzie TX-y nie niosly reserve/quote,
  - realny spadek `missing_price_count`.

Uczciwe ograniczenie:
Kod nie robi future-backfill. Jezeli pierwszy tick nie ma reserve/quote/mcap i pierwsza AccountStateCore probka przychodzi pozniej, pierwszy tick moze pozostac bez ceny. To jest celowo bezpieczniejsze niz cofanie pozniejszej ceny w czasie.

## 9. Ryzyka i zabezpieczenia

Ryzyko 1: zmiana top-level `vectors_*` z AB-window na MFS full-series moze zmienic zalozenia czesci offline analiz.
- Mitigacja: embedded MFS byl juz SSOT pelnej serii; AB-window metadata nadal istnieje w `ab_*`, a legacy fallback dziala tylko gdy MFS vectors brak.

Ryzyko 2: AccountStateCore receive-time nie jest dokladnym timestampem transakcji.
- Mitigacja: uzywamy tylko probek `<= tx_ts_ms`; nie backfillujemy z przyszlosci.

Ryzyko 3: carry-forward moze wygladzic krotka serie.
- Mitigacja: osobny source count i degraded reason.

Ryzyko 4: dodatkowa pamiec per pool.
- Mitigacja: sidecar price observations sa retencjonowane tylko w observation window.

## 10. Decyzja

Akceptujemy ten etap jako kodowa naprawe residual issues R37 w warstwie evidence/materialization/logging.

Nie zamykamy formalnie runtime acceptance bez nowego artefaktu po tej zmianie. Nastepny run powinien byc oceniony tymi kryteriami:
- embedded `decision_time_series` monotonic non-decreasing offsets,
- brak top-level delta loss wzgledem embedded `temporal_deltas`,
- top-level `vectors_*` odpowiada full decision series dla V3 MFS,
- missing prices sa ograniczone do przypadkow bez decision-time-safe zrodla ceny.
