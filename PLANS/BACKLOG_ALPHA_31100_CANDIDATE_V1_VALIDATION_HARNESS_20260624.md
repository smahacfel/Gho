# Backlog: alpha_31100_candidate_v1 validation harness

Status: RESEARCH / SHADOW-ONLY / NO RUNTIME DECISION
Data: 2026-06-24
Powiazany plan: `PLANS/PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`
Powiazany etap: po PR4, osobny validation harness; nie PR3 runtime integration

## 1. Kontrakt

`alpha_31100_candidate_v1` jest kandydackim sygnalem alpha dla nowych pooli pump.fun obserwowanych w oknie `T0 + 31100 ms`.

Na dzien tego backlogu sygnal ma status:
- research-only,
- shadow-only,
- bez deploya live,
- bez BUY/REJECT hooka,
- bez thresholda produkcyjnego.

PR3 Confidence Semantics traktuje ten sygnal wylacznie jako przyszly kandydat na alpha component. PR3 nie dodaje go do runtime decision.

## 2. Aktualny stan wiedzy

Informacje przekazane przez operatora:
- natural ROC-AUC dla `31.1s`: okolo `0.807-0.813`;
- natural PR-AUC: okolo `0.292-0.319`;
- balanced PR-AUC: okolo `0.80`, diagnostycznie, nie jako metryka produkcyjna;
- hard-negative ROC: okolo `0.64-0.73`;
- sygnal laczy early traction, buy pressure, organicity, concentration/toxicity, dev toxicity, burstiness, Jito/tip intensity oraz cross-pool/sybil-ish activity.

Interpretacja:
- To nie jest gotowa strategia BUY.
- To jest kandydat na ranker/filtr wejsc.
- Wyniki wymagaja formalnego OOS, ablation, missingness/leakage audit i top-k EV po kosztach.

## 3. Zakazy

Nie wolno:
- deployowac live,
- dodawac nowego BUY triggera,
- zmieniac progow Gatekeepera,
- optymalizowac decyzji tradingowej pod F1,
- kopiowac regul z raportu HTML do bota,
- uzywac Segment Lab jako zrodla regul runtime,
- uzywac `alpha_31100_candidate_v1` jako inputu BUY/REJECT przed formalnym harness,
- uzywac pol outcome/leakage:
  - `exit_*`,
  - `final_*`,
  - `eval_*`,
  - `entry_price`,
  - `exit_price`,
  - `exit_value_sol`,
  - `sample_age_ms`,
  - absolutne timestampy,
  - sloty/finality,
  - token id,
  - join key,
  - record id,
  - pola symulacji po decyzji.

## 4. Future validation harness scope

Po PR4 przygotowac osobny harness, reprodukowalny jednym poleceniem, obejmujacy:

1. Schema freeze:
   - `features_31100_v1_all.json`,
   - `features_31100_v1_safe_core.json`,
   - `features_31100_v1_blacklist.json`.

2. Safe-core categories:
   - `traction/momentum`,
   - `buy_pressure`,
   - `organicity`,
   - `concentration_toxicity`,
   - `dev_toxicity`,
   - `execution_toxicity`,
   - `cross_pool_sybil`,
   - `temporal`,
   - `other`.

3. Master ledger:
   - `run_id`,
   - `mint/token_id`,
   - `created_ts`,
   - `observation_cutoff_ms = 31100`,
   - `decision_ts`,
   - `entry_ts`,
   - `exit_ts`,
   - `exit_reason`,
   - `final_pnl_pct`,
   - `label`,
   - `feature_schema_version`,
   - `feature_vector_hash`,
   - `source_file`,
   - `is_train/is_val/is_test`.

4. Chronological OOS:
   - train: starszy run `31100`,
   - validation: chronologicznie pozniejsza czesc starego runa albo osobny kawalek,
   - test_oos: swiezy run `31100`,
   - final_holdout: kolejny przyszly run, nietykany.

5. Report splits:
   - natural imbalance,
   - balanced random B,
   - hard-negative active B,
   - target-hit A vs stop-hit B.

6. Metrics:
   - ROC-AUC,
   - PR-AUC,
   - PR baseline,
   - PR lift = PR-AUC / positive_rate,
   - precision@top_0.5%,
   - precision@top_1%,
   - precision@top_2%,
   - precision@top_5%,
   - avg_pnl@top_k,
   - median_pnl@top_k,
   - target_rate@top_k,
   - stop_rate@top_k,
   - EV_after_costs@top_k,
   - liczba trade candidates,
   - max consecutive losses w sekwencji chronologicznej.

7. Ablation:
   - A: full safe core,
   - B: bez momentum/traction,
   - C: bez toxicity/concentration,
   - D: missingness-only,
   - E: safe-core-only bez cech o slabym coverage,
   - F: hard-negative-only train/test.

8. Leakage audit:
   - automatyczne blacklistowanie nazw `exit`, `final`, `pnl`, `profit`, `loss`, `target`, `stop`, `eval`, `simulation`, `result`, `future`, `after`,
   - identyfikatory i join keys,
   - absolutne timestampy,
   - slot/finality,
   - entry/exit price/value,
   - pola niedostepne przed `T0 + 31100 ms`,
   - model na blacklist features,
   - model na missingness-only.

9. Score buckets:
   - bucket `0.0-0.1`, `0.1-0.2`, ..., `0.9-1.0`,
   - count,
   - A_rate,
   - avg_pnl,
   - median_pnl,
   - target_rate,
   - stop_rate,
   - EV_after_costs,
   - cumulative top-k EV.

10. Shadow mode spec:
    - token,
    - timestamp,
    - `score_15s`, jesli dostepny,
    - `score_31s`,
    - `toxicity_veto_flags`,
    - `final_decision: WATCH / REJECT / WOULD_BUY`,
    - `reason_codes`,
    - top contributing feature families,
    - simulated outcome,
    - pnl,
    - exit_reason.

## 5. Acceptance criteria dla przyszlego harness

Minimalne kryteria przed jakakolwiek rozmowa o runtime integration:
- pelny naturalny OOS `31100 ms`: ROC-AUC okolo `>= 0.78`;
- PR lift wzgledem baseline `>= 2.5x`, preferowane blizej `3x`;
- hard-negative ROC-AUC `>= 0.62`;
- target-vs-stop balanced PR-AUC `>= 0.70`;
- top-k EV po kosztach dodatni na przynajmniej jednym konserwatywnym progu;
- missingness-only i blacklist-only nie wyjasniaja wyniku;
- raport reprodukowalny jednym poleceniem.

## 6. Handoff po PR4

Po domknieciu PR4:
1. Wrocic do tego backlogu jako osobnego zadania.
2. Nie mieszac validation harness z Gatekeeper PR1-PR4.
3. Najpierw zbudowac schema freeze i ledger.
4. Dopiero potem uruchomic chronological OOS, hard-negative suite, ablation, leakage audit i top-k EV.
5. Kazda propozycja runtime integration wymaga osobnego planu, review i shadow-only specification.
