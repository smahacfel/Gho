# PR-RUG-MARKUP-A0: Offline proof for synthetic/rug/scambot markup pools

Data: `2026-06-28`

Status: `SPEC_ONLY / OFFLINE_ONLY / NO_RUNTIME`

## Cel

Sprawdzic offline, czy przewidywalne syntetyczne, rugowe albo scambot markup pools sa bardziej exploitable niz normalne organic pools.

Hipoteza nie brzmi: "lepiej wybierac organic pools".

Hipoteza brzmi:

> Jezeli czesc pooli ma rozpoznawalna, sztucznie podbijana faze markup przed dumpem, to prosty, decision-time-safe classifier tej fazy moze dac krotki, kosztowo dodatni exit/hold window lepszy niz organic baseline.

## Twarde ograniczenia

- offline-only,
- no runtime,
- no BUY/REJECT change,
- no Gatekeeper policy change,
- no selector runtime change,
- no live close,
- no `shadow_close_only`,
- no active close,
- no TX/Jito/live path change,
- no new run unless this spec is separately approved,
- no use of future outcome as feature,
- no final PnL / target / stop / timeout as input feature,
- no R48/R49/R50 retuning,
- no alpha_31100,
- no XGBoost,
- no promotion do runtime z tego A0.

## Zakres pytania

RUG-MARKUP-A0 odpowiada tylko na pytanie offline:

Czy da sie zidentyfikowac pre-entry albo early-window faze synthetic/rug/scambot markup, ktora:

1. jest rozpoznawalna bez lookahead,
2. ma wystarczajaco wysoka precision,
3. pozostaje dodatnia po kosztach 100/200 bps,
4. ma nieujemna mediane,
5. nie zalezy od top 5% prawego ogona,
6. jest stabilna na co najmniej dwoch niezaleznych scope.

## Dane wymagane

### Decision-time / pre-entry inputs

Wymagane pola musza byc znane przed decyzja lub w early-window horyzoncie testowym:

- pre-entry dev concentration,
- pre-entry signer concentration,
- dev wallet / creator behavior, jesli dostepne,
- signer cross-pool velocity, jesli dostepne,
- cpv other pool activity, jesli dostepne,
- holder/signature concentration,
- top signer / top holder concentration,
- hhi / top3 ratio / unique ratio,
- early buy burst features,
- same-ms transaction burst,
- buy_count / total_tx / total_volume_sol,
- sol_buy_ratio / buy_ratio,
- dev/sybil funding or behavior, jesli dostepne i decision-time-safe.

### Replay / outcome evaluation only

Te pola sa do ewaluacji, nie jako feature:

- `shadow_exit_replay_v1.path_bps`,
- MFE,
- MAE,
- time-to-dump,
- first hit target/stop,
- terminal PnL,
- target/stop/timeout labels.

### Exit grid

Predeclared grid:

- max hold: `20000`, `30000`, `40000` ms,
- target grid: `1000`, `1500`, `2000`, `2500` bps,
- stop grid: `-300`, `-500`, `-700`, `-1000` bps,
- costs: `100`, `200` bps.

No broad grid search. No additional target/stop/hold values in A0.

## Proponowany classifier family

A0 moze testowac tylko mala, jawnie opisana rodzine classifierow. Przyklad rodzin do specyfikacji implementacyjnej:

1. `R0_BROAD`: broad acted/replay cohort baseline.
2. `R1_DEV_SIGNER_CONCENTRATION`: high dev/signer/holder concentration.
3. `R2_BUY_BURST_MARKUP`: early buy burst + high buy ratio + same-ms burst.
4. `R3_SCAMBOT_COORDINATION`: concentration + cross-pool velocity / repeated signer patterns.
5. `R4_MARKUP_WITH_DUMP_RISK`: R2/R3 plus early overextension / liquidity-to-volume imbalance.

Progi nie moga byc strojone na holdout. Jesli progi sa potrzebne, musza pochodzic z:

- train-only distribution cuts,
- albo malego predeclared zestawu `loose / medium / strict`,
- albo juz istniejacych decision-time feature thresholds opisanych w repo.

## Metryki

Raport musi pokazywac per scope, per segment chronologiczny i per fixed classifier/grid cell:

- count,
- retained percentage,
- precision,
- target rate,
- stop rate,
- timeout rate,
- negative timeout rate,
- avg PnL bps,
- median PnL bps,
- sum PnL bps,
- cost100 PnL,
- cost200 PnL,
- MFE / MAE distribution,
- time-to-dump distribution,
- max consecutive losses,
- top 5% / top 10% tail contribution,
- result after removing top 5% positive records,
- result after removing top 10% positive records,
- stability across chronological terciles,
- stability across at least two independent scopes.

## Acceptance gates

RUG-MARKUP-A0 moze byc `PROMISING_OFFLINE_ONLY` tylko jesli ta sama fixed classifier/grid rule przechodzi na co najmniej dwoch niezaleznych scope:

- precision `>= 65%`,
- cost100 positive,
- cost200 positive,
- median PnL nonnegative,
- result not dependent on top 5% only,
- stable across chronological terciles,
- stable across at least two independent scopes,
- sufficient sample size,
- feature set jest decision-time-safe,
- no future outcome used as feature,
- replay/evaluation join quality acceptable.

## Kill criteria

Zamknac jako rejected/inconclusive, jesli:

- no clean classifier for rug-markup phase,
- positive avg only from right tail,
- median negative after cost,
- cannot distinguish from organic/noise,
- no fresh evidence logging path,
- sample size too small,
- result exists only in one scope,
- result depends on unavailable post-outcome fields,
- result requires broad grid search or new thresholds tuned on holdout.

## Evidence retention requirement

Przed jakimkolwiek przyszlym runem musi istniec jawna evidence-retention guard:

- no active log symlinks to archive volume,
- local active logs write to real local dirs or explicit non-archive active path,
- archive is read-only by default,
- mandatory manifest before cleanup,
- raw JSONL not committed,
- no cleanup without scope allowlist and sha256/size/rows manifest.

## Runtime decision

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

RUG-MARKUP-A0 jest spec-only. Nie startuje runu i nie zmienia runtime.

## Expected output if implemented later

Jesli spec zostanie osobno zaakceptowany, przyszly offline proof powinien wygenerowac co najmniej:

- `scripts/rug_markup_a0_offline_proof.py`
- `reports/selector/rug_markup_a0_summary.csv`
- `reports/selector/rug_markup_a0_cost_sensitivity.csv`
- `reports/selector/rug_markup_a0_stability.csv`
- `reports/selector/rug_markup_a0_tail_audit.csv`
- `reports/selector/rug_markup_a0_threshold_manifest.csv`
- `PLANS/AUDYT/RAPORT_RUG_MARKUP_A0_OFFLINE_PROOF_<date>.md`
- `docs/ADR/ADR_8D_RUG_MARKUP_A0_RESULT_<date>.md`

Te pliki nie sa tworzone w tym kroku.
