# ADR-8D: analiza_porownawcza support for new decision vectors, AB tx and temporal deltas

Status: IMPLEMENTED / TARGETED_SMOKE_VERIFIED
Typ: ADR-8D / offline analysis script compatibility
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: dostosowanie offline skryptu `analiza_porownawcza.py` do nowych logow DecisionLogger z `decision_time_series`, top-level/embedded vectors, AB tx fields, temporal deltas/rates oraz evidence context
Poziom ryzyka: LOW-MEDIUM

Dotkniete moduly/pliki:
- `analiza_porownawcza.py`
- `docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_NEW_LOG_VECTORS_DELTAS_20260619.md`

Uwaga o sciezce skryptu:
Uzytkownik wskazal `scripts/analiza_porownawcza.py`, ale w tym checkoutcie taki plik nie istnieje. Istniejacy aktywny skrypt to root-level `analiza_porownawcza.py`, dlatego zmiana zostala wykonana tam.

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w istniejacych ADR-ach PR1-PR6 i runtime repair ADR-ach.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Skrypt porownawczy mial zostac dostosowany do nowych logow po zmianach evidence/DTW/temporal deltas, tak aby nie liczyl wylacznie starego top-level shape i nie ignorowal embedded `v3_materialized_feature_snapshot`.

Rzeczywisty przebieg:
- Potwierdzono, ze `scripts/analiza_porownawcza.py` nie istnieje.
- Zidentyfikowano aktywny plik: `analiza_porownawcza.py`.
- Przejrzano sample nowego R37 JSONL od rekordu 612 wzwyz.
- Potwierdzono nowe pola:
  - top-level `vectors_prices`, `vectors_ts_offsets_ms`, `vectors_sol_amounts`, `vectors_d_price`, `vectors_interval_ms`;
  - embedded `v3_materialized_feature_snapshot.decision_time_series`;
  - embedded `v3_materialized_feature_snapshot.temporal_deltas`;
  - AB fields `ab_record_id`, `join_key`, `ab_tx_count_window`, `ab_unique_signers_window`, `ab_fail_count_window`;
  - delta/rate fields oraz evidence/source/status companion fields.
- Nie zmieniano runtime, Gatekeeper policy, DecisionLoggera, selector dataset buildera ani shadow/live behavior.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: DecisionLogger/replay boundary, SSOT materialized snapshot i ochrona runtime kontraktow.
- `large-data-analytics`: offline analiza event-stream/decision-window, brak cichej imputacji i zachowanie statusu evidence.

Nie ladowano dokumentow specjalistycznych:
- `gatekeeper-policy-auditor`: zmiana nie dotyka verdictow, hard gates, reason policy ani kolejki policy evaluation.
- `oracle-session-runtime-engineer`: zmiana nie dotyka sesji, deadline, event routing ani lifecycle.
- `solana-execution-path-engineer`: zmiana nie dotyka TX buildera, sendera, blockhash, simulation ani confirmation.
- `config-rollout-safety-reviewer`: zmiana nie dodaje config fields ani progow runtime.

## 3. Opis problemu - 3W2H

What:
`analiza_porownawcza.py` byl pierwotnie pisany pod starsze logi, gdzie istotne pola byly glownie flat top-level. Po nowszych zmianach czesc prawdy decyzyjnej jest w embedded snapshot:
- pelna seria tickow w `decision_time_series`,
- delty/rates w `temporal_deltas`,
- alpha/sybil values w `alpha_fingerprint` i `sybil_resistance`,
- nullable `vectors_prices` dla przypadkow missing/degraded.

Where:
- offline porownanie zbiorow A/B,
- sekcje DTW, MI, Hill, summary, Sybil Interference,
- nowe logi Gatekeeper/DecisionLogger v26+.

Why it matters:
Bez adaptacji skrypt mogl:
- uznawac nowe rekordy za pozbawione wektorow,
- gubic ceny nullable,
- nie liczyc delt Jito/flipper/CPV/burst/mcap,
- zle deduplikowac rekordy bez `ab_record_id`,
- trenowac albo raportowac na innym obrazie danych niz embedded SSOT.

How observed:
Przykladowy nowy R37 JSONL zawieral komplet embedded `decision_time_series` i `temporal_deltas`, a czesc top-level fields byla addytywna lub aliasowana. Skrypt potrzebowal extractorow, ktore rozumieja oba shape.

How many / scale:
Zmiana dotyczy kazdego offline uruchomienia `analiza_porownawcza.py` na nowych logach. Nie dotyka aktywnego runtime ani decyzji produkcyjnych.

## 4. Przyczyna zrodlowa

Root cause:
Skrypt uzywal w wielu miejscach bezposredniego `r.get(field)` albo zakladal, ze wektory/delty sa tylko top-level. Nowe logi sa hybryda:
- top-level convenience fields,
- embedded SSOT snapshot,
- aliasy typu `delta_burstratio_*` vs `delta_burst_ratio_*`,
- nullable vectors,
- decision series retention fields,
- AB tx fields zwiazane z join/export context.

Dodatkowo stary filtr A/B deduplikowal glownie po `ab_record_id`; nowe logi moga miec bardziej stabilny `join_key` lub `decision_id`.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac wspolne extractory `get_val`, `get_bool`, `get_str`, `get_vector_raw`, `get_vector`.
- Najpierw czytac top-level, potem aliasy, potem embedded `v3_materialized_feature_snapshot`.
- Nie zamieniac `null` na `0`.
- Dla `vectors_prices` zachowac `None` w raw vector, a numerycznie filtrowac dopiero w `get_vector`.
- Dla DTW i Hill dopuscic `vectors_prices`, `vectors_d_price` oraz `vectors_interval_ms`.
- Dla delt obslugiwac alias `delta_burstratio_*` <-> `delta_burst_ratio_*`.
- Dodac sekcje 0B, ktora jawnie raportuje:
  - decision time series coverage,
  - AB tx fields coverage,
  - temporal deltas/rates coverage,
  - alpha/sybil new metrics coverage,
  - evidence status counters.

Granice:
- Brak runtime changes.
- Brak imputacji missing jako zero.
- Brak future-fill.
- Brak zmiany semantyki CPV, Jito, flipper, burst czy DTW.
- Brak progu decyzyjnego lub rekomendacji tradingowej w tym skrypcie.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: embedded-aware extractors
- `get_val` czyta top-level, aliasy, `evidence_policy_context`, embedded `temporal_deltas`, `decision_time_series`, `alpha_fingerprint`, `tx_intel_features`, `sybil_resistance`.
- `get_vector_raw` czyta top-level albo embedded `decision_time_series` i zachowuje `None`.
- `get_vector` filtruje tylko wartosci numeryczne.

Zmiana 2: nowe pola metryk
- Dodano `TEMPORAL_DELTA_FIELDS` dla m.in.:
  - `delta_mcap_*`,
  - `delta_price_pct_*`,
  - `delta_burstratio_*`,
  - `delta_buy_count_*`,
  - `delta_unique_signers_*`,
  - `delta_tx_count_*`,
  - `delta_net_quote_sol_*`,
  - `delta_jito_tip_intensity_*`,
  - `delta_signer_cross_pool_velocity_*`,
  - `delta_flipper_presence_ratio_*`.
- Dodano `TEMPORAL_RATE_FIELDS`, w tym `rate_mcap_sol_per_s_2s_to_3s`.
- Dodano vector/source/evidence numeric fields i policy bool fields.

Zmiana 3: nowe logi i AB tx
- Autodetect wektorow uzywa `vectors_d_price`, a gdy go brak, `vectors_prices`.
- Filtr AB potrafi deduplikowac po `ab_record_id`, `join_key`, `decision_id`, `candidate_id`, a dopiero potem fallback pool/mint/timestamp.
- `AB_MIN_TX` uzywa `ab_tx_count_window` albo `total_tx_evaluated` jako fallback.

Zmiana 4: sekcja 0B
- Dodano `SEKCJA 0B: NOWE LOGI - DECISION SERIES / AB TX / DELTY / EVIDENCE`.
- Sekcja raportuje coverage, missing price samples, interval quality, AB field presence, delta/rate coverage oraz evidence status counters.

Zmiana 5: sekcje analityczne
- DTW uwzglednia `vectors_prices`.
- MI dostaje scalar features z price vector: return, range pct, coverage.
- Hill moze analizowac `vectors_prices`.
- Causal/TDA/Hill metric lists zawieraja reprezentatywne delty/rates.
- Sybil Interference coverage uzywa `get_val`, a nie tylko top-level `r.get`.

## 7. Walidacja

Wykonane komendy:
- `python3 -m py_compile analiza_porownawcza.py`
- `git diff --check -- analiza_porownawcza.py`
- `python3 analiza_porownawcza.py /tmp/gho_analiza_sample/a.jsonl /tmp/gho_analiza_sample/b.jsonl > /tmp/gho_analiza_sample/run_after_sybil_patch.txt`

Wynik:
- Skrypt kompiluje sie poprawnie.
- `git diff --check` nie wykazal bledow whitespace.
- Smoke run zakonczyl sie exit code 0.
- Wygenerowano raport:
  - `/tmp/gho_analiza_sample/analiza_20260619_200829.html`

Dowody ze smoke run czyta nowe pola:
- `decision_time_series present`: 35/35 dla A i 35/35 dla B.
- `negative interval records`: 0 dla A i 0 dla B.
- `delta_jito_tip_intensity_1s_to_2s`: present 47/70.
- `delta_flipper_presence_ratio_1s_to_2s`: present 47/70.
- `signer_cross_pool_velocity`: present 36/70.
- `cpv_other_pool_activity`: present 36/70.
- `vectors_prices` zachowuje nullable shape: B mial `null=1/35`, bez wysypania skryptu.

Uwaga walidacyjna:
Czesc opcjonalnych sekcji zostala pominieta z powodu brakujacych zaleznosci Pythona w srodowisku (`fastdtw/scipy`, `causal-learn/networkx`, `ripser/persim`, `scikit-learn`, `numpy`). Nie blokuje to walidacji parsera/log-shape, ale ogranicza pelna analize statystyczna na tej maszynie.

## 8. Ryzyka i ochrona przed regresja

Ryzyko 1: Pomieszanie `null` z `0`.
Mitigacja:
- `get_vector_raw` zachowuje `None`.
- `get_vector` filtruje numerycznie tylko tam, gdzie algorytm wymaga liczb.
- Sekcja 0B pokazuje missing/finites oddzielnie.

Ryzyko 2: Ukryta zmiana semantyki embedded vs top-level.
Mitigacja:
- Embedded fallback jest tylko odczytem SSOT dla raportowania offline.
- Nie jest to runtime source ani policy input.

Ryzyko 3: Alias `burst_ratio`/`burstratio` i delta naming drift.
Mitigacja:
- Dodano jawne aliasy `delta_burstratio_*` <-> `delta_burst_ratio_*`.

Ryzyko 4: Nowe pola evidence trafiaja do rankingow bez kontekstu.
Mitigacja:
- Sekcja 0B raportuje presence/nonzero i status counters.
- Skrypt nie imputuje brakujacych wartosci.

Ryzyko 5: Brak pelnych opcjonalnych zaleznosci.
Mitigacja:
- Sekcje opcjonalne nadal fail-soft z instrukcja instalacji.
- Parser i podstawowy smoke zostaly sprawdzone bez tych zaleznosci.

## 9. Status koncowy

Status: implemented.

`analiza_porownawcza.py` zostal dostosowany do nowych logow z:
- top-level i embedded decision vectors,
- nullable `vectors_prices`,
- AB tx fields,
- temporal deltas/rates,
- Jito/flipper/CPV/burst/mcap/new runtime metrics,
- embedded SSOT fallback bez silent imputation.

Pozostale ograniczenia:
- To jest narzedzie offline, nie dowod kompletnosci runtime PR5/PR6.
- Pelne sekcje DTW/causal/TDA/MI wymagaja opcjonalnych pakietow Pythona.
- Long proof na pelnym real export powinien byc wykonany oddzielnie po zebraniu docelowego datasetu.
