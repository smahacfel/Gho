# ADR-8D: R36 nullable top-level DTW vectors and mcap-rate schema closure

Status: IMPLEMENTED / TARGETED_TESTS_PASS / RUNTIME_REPROOF_REQUIRED
Typ: ADR-8D / DecisionLogger schema hygiene / DTW top-level vector repair
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, local working tree
Commit/PR: not committed at report time
Zakres: R36 residual closure for nullable top-level `vectors_prices`, `vectors_d_price`, `rate_mcap_sol_per_s_2s_to_3s`, schema version bump
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `scripts/build_selector_gatekeeper_feature_context.py`
- `docs/ADR/ADR_8D_R36_NULLABLE_TOP_LEVEL_VECTORS_AND_RATE_SCHEMA_20260618.md`

Powiazane runy/logi/raporty:
- R36 evidence wskazane przez uzytkownika:
  `logs/rollout/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/d3199ccbe39d4c101c6b6858d2e320c8bc3f5adf4ceff045c8be7a764c90b04e/gatekeeper_v2_decisions.jsonl`
- R36 v25 shadow evidence wskazane przez uzytkownika:
  `logs/rollout/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/v2.5/v25_shadow/d3199ccbe39d4c101c6b6858d2e320c8bc3f5adf4ceff045c8be7a764c90b04e/gatekeeper_v2_decisions.jsonl`
- Poprzedni ADR bazowy:
  `docs/ADR/ADR_8D_R37_DTW_DECISION_SERIES_RESIDUAL_REPAIR_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Punkt startowy:
Uzytkownik zweryfikowal najnowsze artefakty R36 po restarcie i potwierdzil, ze poprzednia naprawa zrobila istotny postep, ale nadal nie zamyka tematu formalnie.

Potwierdzone pozytywy z R36:
- `series_negative_interval_records == 0` dla legacy_live i v25_shadow.
- Top-level core deltas sa zgodne z embedded `temporal_deltas`.
- Top-level `vectors_*` nie sa juz AB-window adapterem w rekordach z pelna cena.
- AccountStateCore realnie zasila decision series jako price source.

Potwierdzone residual issues:
- Czesci tickow nadal nie da sie wycenic bez temporal leakage.
- W rekordach, gdzie embedded `decision_time_series.prices` zawiera `null`, top-level `vectors_prices` znika calkowicie zamiast zachowac dlugosc osi tickow z `null` na brakujacej pozycji.
- `rate_mcap_sol_per_s_2s_to_3s` jest obecne embedded, ale nie jest obecne top-level.
- `log_schema_version` pozostaje `25`, mimo addytywnych zmian shape JSONL.

Cel tego etapu:
- Zachowac pelny top-level price-vector shape rowniez w degraded/missing cases.
- Wyrownac top-level rate field z embedded temporal deltas dla `rate_mcap_sol_per_s_2s_to_3s`.
- Oznaczyc addytywna zmiane schema jako `GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 26`.
- Nie zmieniac Gatekeeper policy, verdictow, reason-code semantics ani execution path.

Non-goals:
- Brak future-backfill ceny z pozniejszej probki AccountStateCore na wczesniejszy tick.
- Brak proby sztucznego wyeliminowania wszystkich `missing_price_count` kosztem temporal correctness.
- Brak zmian TX buildera, sendera, live/shadow execution semantics.
- Brak zmian progow policy, strict thresholds albo reason-code taxonomy.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: DecisionLogger/replay boundary, SSOT evidence, shadow/live separation.
- `rust-master`: bezpieczna zmiana typu Rust, serde compatibility, brak hot-path side effects.
- `trading-systems`: rozdzielenie evidence/selector surface od aktywnej decyzji.

Specjalisci logiczni:
- Primary: Decision Logging Replay Analyst.
- Supporting: SSOT Feature Materialization Guardian, Gatekeeper Policy Auditor considered, Rust runtime discipline.

Zaladowane dokumenty:
- `AGENTS.md`
- `.agents/skills/ghost-execution/SKILL.md`
- `.agents/skills/rust-master/SKILL.md`
- `.agents/skills/trading-systems/SKILL.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/ssot-feature-materialization-guardian.md`

Nie ladowano:
- Solana Execution Path Engineer, bo zadanie nie dotyka transakcji, sendera, blockhash, retry ani reconciliation.
- Seer Ingest Event Integrity Specialist, bo residual dotyczy shape juz zmaterializowanego/logowanego evidence, nie parserow ani stream identity.
- Config Rollout Safety Reviewer, bo nie dodano config fields ani thresholdow.

## 3. Opis problemu - 3W2H

What:
Top-level `vectors_prices` nie zachowywalo pelnej osi tickow, gdy embedded decision series mialo jakakolwiek probke bez ceny. Zamiast listy typu `[null, price, price]`, pole bylo pomijane. Dodatkowo top-level brakowalo `rate_mcap_sol_per_s_2s_to_3s`.

Where:
- `GatekeeperBuyLog.vectors_prices`
- `GatekeeperBuyLog.vectors_d_price`
- `GatekeeperAssessment::to_buy_log()`
- `enrich_buy_log_with_vectors()`
- `scripts/build_selector_gatekeeper_feature_context.py::RAW_FEATURES`

Why it matters:
DTW/offline selector consumers czytajacy plaskie top-level JSONL nadal nie mialy pelnego ksztaltu wektora dla degraded/missing price cases. Embedded MFS byl poprawniejszy, ale top-level contract byl niespojny z celem "pelny wektor tickow".

How observed:
Uzytkownik wskazal, ze w R36 dla v25_shadow:
- `top_vectors_missing_prices_field_when_embedded_has_missing: 17`
- `vec_nonprice_len_bad: 0`
czyli non-price vectors mialy poprawna dlugosc, ale `vectors_prices` znikalo przy embedded null price.

How many / scale:
W wskazanym R36 dotyczylo 17 rekordow z missing price samples. Skala bedzie proporcjonalna do liczby decyzji, w ktorych pierwsze lub inne ticki nie maja decision-time price source.

## 4. Przyczyna zrodlowa

Root cause 1: typ top-level byl zbyt waski.
`GatekeeperBuyLog.vectors_prices` i `vectors_d_price` byly `Option<Vec<f64>>`. Ten typ nie moze reprezentowac `null` per element bez znikniecia calego pola albo utraty pozycji.

Root cause 2: helper zachowywal sie jako all-or-nothing.
`finite_option_values()` / `finite_option_vec()` probowaly zebrane `Option<f64>` zamienic na `Vec<f64>`. Jezeli jakikolwiek element byl `None`, caly wynik stawal sie `None`, wiec JSONL pomijal `vectors_prices`.

Root cause 3: top-level rate field nie mial pola docelowego.
`TemporalDeltaFeatures` materializowalo `rate_mcap_sol_per_s_2s_to_3s`, ale `GatekeeperBuyLog` nie mial top-level pola, wiec `to_buy_log()` i enrichment nie mialy gdzie go zapisac.

Root cause 4: schema version nie odrozniala shape po addytywnych zmianach.
`GATEKEEPER_BUY_LOG_SCHEMA_VERSION` pozostawal 25, mimo dodania top-level source-count fields i zmiany oczekiwanego shape wektorow cenowych.

## 5. Strategia naprawy

Przyjeta strategia:
- Zmienic top-level `vectors_prices` i `vectors_d_price` na `Option<Vec<Option<f64>>>`.
- Mapowac brakujace albo non-finite probki na `null` per element, bez kasowania calego wektora.
- Zachowac stare numeric arrays jako kompatybilne wejscie serde: `[1.0, 1.5]` deserializuje sie jako `[Some(1.0), Some(1.5)]`.
- Dodac `rate_mcap_sol_per_s_2s_to_3s` do `GatekeeperBuyLog`, `to_buy_log()`, `enrich_buy_log_with_vectors()` i selector RAW_FEATURES.
- Podbic `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` z 25 do 26.
- Dopasowac diagnostic selector shadow score do nullable vectors przez liczenie scalar vector features z finite subsequence, tak jak Python readers filtrujace `null`.

Odrzucone alternatywy:
- Zamiana `null` na `NaN`: odrzucone, bo JSON nie powinien wymagac non-standard float encoding.
- Future-backfill brakujacej ceny: odrzucone jako temporal leakage.
- Dodanie nowego pola obok `vectors_prices`: odrzucone jako niepotrzebne rozdwajanie top-level contractu.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: DecisionLogger schema v26.
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` ustawiono na `26`.
- Komentarz schema opisuje nullable top-level price vectors i mcap-rate 2s->3s mirror.

Zmiana 2: nullable top-level price vectors.
- `GatekeeperBuyLog.vectors_prices: Option<Vec<Option<f64>>>`.
- `GatekeeperBuyLog.vectors_d_price: Option<Vec<Option<f64>>>`.
- `GatekeeperAssessment::nullable_f64_values()` zachowuje dlugosc osi i mapuje brak/non-finite na `None`.
- `oracle_runtime::nullable_f64_vec()` robi to samo dla fallback enrichment.

Zmiana 3: rate field parity.
- Dodano `rate_mcap_sol_per_s_2s_to_3s` do top-level `GatekeeperBuyLog`.
- `GatekeeperAssessment::to_buy_log()` bierze wartosc z `feature_snapshot.temporal_deltas`.
- `enrich_buy_log_with_vectors()` wypelnia to pole fill-if-missing z buffer temporal deltas.
- `scripts/build_selector_gatekeeper_feature_context.py::RAW_FEATURES` zawiera nowe pole.

Zmiana 4: selector shadow score compatibility.
- Rust selector shadow score liczy price scalar features z finite subsequence nullable vector.
- Full vector shape w JSONL pozostaje nienaruszony; tylko scalar score adapter ignoruje `null`, zgodnie z Python readerami.

Zmiana 5: testy regresyjne.
- Test launcherowy sprawdza, ze `vectors_prices` serializuje sie jako `[null, 1.5, 2.0]`, `vectors_d_price` jako `[null, 0.5]`, a `rate_mcap_sol_per_s_2s_to_3s` jest top-level.
- Test `ghost-brain` sprawdza JSON shape i backward serde compatibility dla numeric arrays.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda | Wynik |
|---|---|---|
| GatekeeperBuyLog nullable/schema tests | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain gatekeeper_buy_log -- --nocapture` | PASS, 4 tests |
| MFS vectors/deltas overwrite guard + nullable JSON | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher test_enrich_buy_log_with_vectors_preserves_materialized_series_and_deltas -- --nocapture` | PASS |
| Decision time series source/order regression | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher decision_time_series -- --nocapture` | PASS, 6 tests |
| Selector shadow score nullable vector compatibility | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain selector_shadow_score -- --nocapture` | PASS, 9 tests |
| Python reader/context syntax | `python3 -m py_compile scripts/build_selector_gatekeeper_feature_context.py analiza_porownawcza.py scripts/audit_selector_shadow_score_parity.py scripts/v3_p37_evidence_availability_report.py` | PASS |

## 8. Wynik i aktualny status

Naprawione w kodzie:
- Top-level `vectors_prices` nie znika juz tylko dlatego, ze embedded series ma `null` price.
- Top-level `vectors_d_price` moze zachowac brakujace transition points jako `null`.
- Top-level `rate_mcap_sol_per_s_2s_to_3s` jest dostepne tak jak embedded temporal delta rate.
- Schema version odroznia nowy shape jako v26.
- Selector RAW_FEATURES obejmuje brakujacy rate field.

Pozostaje do potwierdzenia runtime:
- Nowy rollout/restart powinien pokazac `top_vectors_missing_prices_field_when_embedded_has_missing == 0`.
- Dla rekordow degraded/missing nalezy sprawdzic, ze `len(vectors_prices) == len(vectors_ts_offsets_ms) == len(decision_time_series.prices)`.
- `rate_mcap_sol_per_s_2s_to_3s` powinno miec top-level presence zgodne z embedded presence.
- `missing_price_count` moze nadal byc dodatnie, jezeli cena nie byla znana decision-time bez future-backfill; to jest akceptowany residual temporal correctness, nie blad shape JSONL.

## 9. Ryzyka i zabezpieczenia

Ryzyko 1: downstream oczekujacy `vectors_prices: list[number]` moze musiec filtrowac `null`.
- Mitigacja: schema bump do v26; Python readers w repo juz ignoruja non-numeric/null przy scalar extraction.

Ryzyko 2: zmiana typu w Rust mogla naruszyc selector shadow score.
- Mitigacja: `selector_shadow_score` tests passed; adapter filtruje finite subsequence.

Ryzyko 3: same missing prices nie znikna w 100%.
- Mitigacja: brak future-backfill jest jawny i chroni przed temporal leakage.

Ryzyko 4: runtime proof nie zostal jeszcze odswiezony po tym patchu.
- Mitigacja: status dokumentu pozostaje `RUNTIME_REPROOF_REQUIRED`.

## 10. Decyzja

Akceptujemy te poprawke jako kodowe domkniecie residual issues wskazanych po R36 w zakresie:
- nullable top-level DTW price vectors,
- top-level `rate_mcap_sol_per_s_2s_to_3s`,
- schema hygiene v26.

Nie deklarujemy jeszcze formalnego runtime closure bez nowego artefaktu rollout/restart po tej zmianie.
