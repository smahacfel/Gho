# ADR-8D: Organic Pool Candidate Policy A0 Offline Proof

Status: IMPLEMENTED / OFFLINE_REPORT_GENERATED / RUNTIME_GATE_CLOSED
Typ: ADR-8D / offline research tooling / candidate policy proof
Data: 2026-06-26
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `research/alpha-31100-validation-harness-v1`
HEAD podczas pracy: `da5873eca63ff1bf9f69f7039a1f6b50804d628b`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: PR-ORG-A0 offline proof dla polityki organicznych pooli
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Dotkniete moduly/pliki:
- `scripts/organic_candidate_policy_proof.py`
- `PLANS/AUDYT/RAPORT_ORGANIC_POOL_CANDIDATE_POLICY_A0_20260626.md`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_summary.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_exit_matrix.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_cost_sensitivity.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_stability.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_inventory.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv`
- `docs/ADR/ADR_8D_ORGANIC_POOL_CANDIDATE_POLICY_A0_OFFLINE_PROOF_20260626.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Sprawdzic offline, czy na normalnych / organicznych poolach da sie znalezc realny edge uzywajac wylacznie istniejacych pol pre-entry / materialized features.

Twarde ograniczenia:
- offline-only,
- bez zmian Gatekeeper BUY/REJECT,
- bez zmian `v25_confidence`,
- bez promocji V3,
- bez zmian runtime selectora,
- bez zmian TX buildera, sendera, Jito path i live execution,
- bez zmian istniejacych logow,
- bez uzycia `alpha_31100`,
- bez XGBoost,
- bez uzycia `selector_shadow_score` jako inputu polityki,
- bez uzycia `combined_score_tail_v1` jako inputu runtime.

Glowny scope danych:
- R48/R2, bo ten scope ma `shadow_exit_replay_v1.jsonl` i pelna macierz Target/Stop/max_hold bez koniecznosci nowego rollout.
- R47/R48/R49 raporty i ADR-y zostaly potraktowane jako kontekst/inventory, nie jako runtime evidence do zmiany polityki.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powod uzycia: ochrona SSOT, shadow/live boundary, brak zmian Gatekeeper/runtime oraz klasyfikacja artefaktow jako offline-only.
Zakres uzycia: utrzymanie `MaterializedFeatureSet`/decision row jako zrodla pre-entry features i odciecie post-decision fields od kandydackiej polityki.
Wynik: nie zmieniono Rust runtime, Gatekeepera, selectora, TX buildera ani logow.

Nazwa: `large-data-analytics`
Powod uzycia: event/replay data quality, duplicate/join controls, chronological split, field coverage.
Zakres uzycia: inventory pol, train-only distribution cuts, matrix artifacts, stability across chronological terciles.

Nazwa: `statistical-research-engine`
Powod uzycia: anti-leakage validation, no-lookahead thresholding, falsyfikacja kandydata wzgledem F5.
Zakres uzycia: porownanie S1/F5 vs C1-C5, cost sensitivity, holdout/stability, explicit failure criteria.

## 3. Opis problemu - 3W2H

What:
Dotychczasowe wyniki wskazywaly, ze standalone `alpha_31100`, equal-family reranker, `selector_shadow_score` oraz szeroka macierz exitow nie dostarczyly czystego edge. Hipoteza PR-ORG-A0 wymagala sprawdzenia prostej drabinki organicznej bez nowej warstwy runtime.

Where:
- `gatekeeper_v2_decisions.jsonl` jako feature-bearing decision rows,
- `selector_shadow_score_v1.jsonl` jako diagnostic baseline,
- `shadow_lifecycle.jsonl` / `probe_shadow_lifecycle.jsonl` jako lifecycle context,
- `shadow_exit_replay_v1.jsonl` jako post-filter exit replay surface,
- raporty pod `reports/selector/...r48...`.

Why it matters:
Jesli edge istnieje, powinien byc wyrazalny przez proste decision-time guards: minimal traction, anti-overextension, organic broadening, low toxicity i dev/cross-pool guard. Wynik nie moze zalezec od sidecar score, outcome leakage, token id, absolutnych timestampow ani future fields.

How observed:
Skrypt `scripts/organic_candidate_policy_proof.py` zjoinowal clean exit replay rows z decision rows po `base_mint`, dolaczyl `selector_shadow_score` tylko diagnostycznie, przypisal tercyle chronologiczne po `entry_ts_ms`, wyprowadzil progi z train-only S1/F5 i policzyl identyczna macierz Target/Stop/max_hold dla S0/S1/C1-C5.

How many / scale:
- exit replay records: 4341,
- clean qualified records: 4317,
- decision rows scanned: 17194,
- joined decision records: 4317,
- joined selector scores: 4317,
- matrix combinations per candidate ladder policy: 1232,
- cost sensitivity rows: 43120 plus header.

## 4. Przyczyna zrodlowa

Root cause badawczy:
Wczesniejsze proby szukaly separacji przez score/reranker albo exit matrix na szerokim samplerze. PR-ORG-A0 wymagal odwrotnej kolejnosci: najpierw mala kohorta wejscia oparta o decision-time fields, dopiero potem exit replay.

Mechanizm ryzyka:
Bez jawnego allowlist-only feature surface latwo pomieszac:
- pre-entry fields z post-decision/outcome fields,
- diagnostic sidecar z runtime input,
- raw sampler exit optimum z exit optimum po wybranej kohorcie,
- wynik jednego tercyla z realna stabilnoscia.

## 5. Strategia naprawy

Przyjeta strategia:
Dodac jeden offline/read-only skrypt i wygenerowac raporty. Skrypt nie jest podlaczony do runtime i nie jest importowany przez Gatekeepera.

Candidate ladder:
- `S0`: clean joined `shadow_exit_replay_v1` acted/broad sampler cohort.
- `S1_F5`: `current_market_cap_sol >= 30.2`, `bonding_progress_pct >= 36.5`, `price_change_ratio >= 1.012`, `buy_count >= 8`, `sol_buy_ratio >= 0.520`.
- `C1`: S1 + anti-overextension caps.
- `C2`: C1 + low execution toxicity caps.
- `C3`: C2 + organic broadening floor.
- `C4`: C3 + concentration guard.
- `C5`: C4 + optional dev/cross-pool guard.

Threshold source:
- profile `medium`,
- train-only S1 distribution cuts,
- cap quantile `0.75`,
- floor quantile `0.25`,
- holdout nie jest uzyty do wyboru progow.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: offline proof script
- Dodano `scripts/organic_candidate_policy_proof.py`.
- Skrypt parsuje JSONL streamingowo.
- Skrypt ma jawna allowliste decision-time field specs.
- Skrypt odrzuca zakazane feature name classes dla pol uzytych w ladder.
- Skrypt tworzy S0/S1/C1-C5 i diagnostic selector equal-count baselines.

Zmiana 2: output CSV
- `organic_candidate_policy_summary.csv`: metryki wybranych train-only exitow per policy.
- `organic_candidate_policy_exit_matrix.csv`: pelna macierz Target/Stop/max_hold dla S0/S1/C1-C5.
- `organic_candidate_policy_cost_sensitivity.csv`: koszt 0/50/100/150/200 bps dla macierzy.
- `organic_candidate_policy_stability.csv`: train/validation/holdout dla train-selected exitow.
- `organic_candidate_policy_inventory.csv`: coverage i source path pol.
- `organic_candidate_policy_thresholds.csv`: progi i coverage train S1.

Zmiana 3: raport
- Dodano `PLANS/AUDYT/RAPORT_ORGANIC_POOL_CANDIDATE_POLICY_A0_20260626.md`.
- Raport zapisuje file inventory, data controls, field inventory, threshold source, metrics, stability, output paths i final verdict.

## 7. Wynik operacyjny

Final verdict:
`INCONCLUSIVE`

Najwazniejsze metryki dla kandydackiej drabinki:

| Policy | Count | Retained | Selected exit | Gross avg bps | Cost100 avg bps | Cost100 sum bps | Cost100 median bps | Max consec losses |
|---|---:|---:|---|---:|---:|---:|---:|---:|
| S0 | 4317 | 100.00% | 10000/-200/120000 | 69.57 | -30.43 | -131373 | -300 | 99 |
| S1_F5 | 1154 | 26.73% | 7500/-100/30000 | 293.94 | 193.94 | 223810 | -200 | 41 |
| C1 | 768 | 17.79% | 7500/-100/30000 | 305.46 | 205.46 | 157797 | -200 | 31 |
| C2 | 323 | 7.48% | 10000/-200/40000 | 377.58 | 277.58 | 89657 | -300 | 19 |
| C3 | 273 | 6.32% | 10000/-200/40000 | 359.81 | 259.81 | 70929 | -300 | 16 |
| C4 | 203 | 4.70% | 10000/-200/30000 | 352.32 | 252.32 | 51221 | -300 | 13 |
| C5 | 60 | 1.39% | 10000/-200/30000 | 426.18 | 326.18 | 19571 | -300 | 8 |

Interpretacja:
- C1 poprawia avg po 100 bps wzgledem F5, ale nie poprawia sum PnL wzgledem F5.
- C2-C4 poprawiaja avg, ale sa mniejsze, maja slabszy full mix i nie bija F5 suma PnL.
- C5 jest mikroskopijny (`60` rows) i nie moze byc traktowany jako runtime kandydat.
- `selector_shadow_score` zostal policzony tylko jako equal-count diagnostic baseline; nie jest wymagany do wyniku.

Holdout cost100:
- S1_F5: count `458`, avg `188.66`, sum `86406`, median `-200`.
- C1: count `306`, avg `186.64`, sum `57111`, median `-200`.
- C2: count `118`, avg `230.19`, sum `27163`, median `-300`.
- C3: count `95`, avg `179.08`, sum `17013`, median `-300`.
- C4: count `73`, avg `261.29`, sum `19074`, median `-300`.
- C5: count `20`, avg `277.40`, sum `5548`, median `-300`.

## 8. Walidacja dzialan

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Python compile | `python3 -m py_compile scripts/organic_candidate_policy_proof.py` | passed | PASS |
| Smoke single-cell | `python3 scripts/organic_candidate_policy_proof.py --targets-bps 6000 --stops-bps=-6000 --max-hold-ms 120000 --output-dir /tmp/organic_a0_smoke --report-path /tmp/organic_a0_smoke_report.md` | generated smoke report, verdict INCONCLUSIVE | PASS |
| Full R48/R2 proof | `python3 scripts/organic_candidate_policy_proof.py` | generated report/CSV, verdict INCONCLUSIVE | PASS |

## 9. Ryzyka resztkowe

- To jest jeden run R48/R2 i tercyle chronologiczne w jednym runie; dowod jest slabszy niz multi-run holdout.
- C1-C4 poprawiaja wybrane srednie, ale nie spelniaja pelnego acceptance gate wzgledem F5.
- C5 retained sample jest za maly.
- Median cost100 pozostaje ujemny dla S1 i C1-C5.
- Exit selection jest train-only, ale nadal jest offline replay, nie live execution.
- `shadow_exit_replay_v1` uzywa replay path/horizon, nie dowodzi live landing, fee, Jito tip ani realnego sender behavior.

## 10. Scope out

Nie wykonano i nie wolno uznawac za wykonane w tym PR-ORG-A0:
- runtime Gatekeeper policy change,
- runtime selector policy change,
- `v25_confidence` change,
- V3 promotion,
- TX builder/sender/Jito/live execution change,
- alpha_31100 integration,
- XGBoost/model training,
- selector_shadow_score or combined_score_tail_v1 runtime input.

## 11. Decyzja

Decyzja:
`INCONCLUSIVE`, runtime gate remains closed.

Uzasadnienie:
Kandydaci C1-C4 nie bija S1/F5 jednoczesnie na mix, negative timeout, avg i sum PnL po 100 bps. C5 jest za maly. Wynik nie jest failure typu "dziala tylko na zakazanych polach", ale nie wystarcza do `PROMISING_OFFLINE_ONLY`.

## 12. Nastepne kroki

Przed jakakolwiek rozmowa o runtime wymagane sa:
- multi-run holdout lub przynajmniej niezalezny scope z takim samym proofem,
- nie-mikroskopijny retained cohort,
- nieujemna albo wyraznie poprawiona mediana po kosztach,
- stabilny holdout po 100 bps,
- conservative precision blisko 65-70% overall i nie mniej niz 60% w segmentach,
- proste typed availability guards mozliwe do wyrazenia w istniejacej polityce.
