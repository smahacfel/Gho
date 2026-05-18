# Raport P3.7 Label v2 Baseline R10/R11/R13

Data: 20260518

Status: **TRUTH-LAYER READY / NO FEATURE CLAIMS / NO P2 / NO LIVE**

## Executive summary

P3.7.4 wygenerowal addytywne `Outcome Label v2` oraz `Execution Feasibility Join` dla R10/R11/R13. Ten etap nie stroi progow i nie ocenia jeszcze nowych feature families. Jego celem jest oczyszczenie prawdy o wyniku przed temporal split i feature prototype.

Najwazniejszy wynik: obecne label v1 `+40 before stop` nie daje jeszcze `good_clean`. Wszystkie historyczne `good_entry` przechodza do `good_dirty`, bo artefakty nie zawieraja pelnej price path/lifecycle series wymaganej do MFE/MAE/time-path i execution-feasible BUY-quality claims.

- Total rows: `3330`
- `good_clean`: `0`
- `good_dirty`: `652`
- `bad_clean`: `682`
- `neutral_clean`: `1628`
- `unknown`: `368`
- Price path source counts: `{'none': 3330}`

Interpretacja: P3.7 potwierdza, ze P3.6 nie powinno bylo przechodzic do feature redesign na bazie samego v1 good/bad. Najpierw trzeba odseparowac market outcome od executable opportunity.

## Artefakty wygenerowane

| Run | Label v2 | Label summary | Feasibility join | Feasibility summary |
| --- | --- | --- | --- | --- |
| R10 | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_7_label_v2.jsonl` | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/reports/p3_7_label_v2_summary.json` | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/reports/p3_7_execution_feasibility_join.jsonl` | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/reports/p3_7_execution_feasibility_summary.json` |
| R11 | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_7_label_v2.jsonl` | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/reports/p3_7_label_v2_summary.json` | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/reports/p3_7_execution_feasibility_join.jsonl` | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/reports/p3_7_execution_feasibility_summary.json` |
| R13 | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_7_label_v2.jsonl` | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_label_v2_summary.json` | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_execution_feasibility_join.jsonl` | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_execution_feasibility_summary.json` |

## Label v2 per run

| Run | Rows | Good clean | Good dirty | Bad clean | Neutral clean | Unknown | Price path none |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| R10 | 150 | 0 | 25 | 42 | 69 | 14 | 150 |
| R11 | 447 | 0 | 91 | 84 | 212 | 60 | 447 |
| R13 | 2733 | 0 | 536 | 556 | 1347 | 294 | 2733 |

## V1 -> V2 transition matrix

| Transition | Count |
| --- | ---: |
| `bad_entry->bad_clean` | 682 |
| `good_entry->good_dirty` | 652 |
| `neutral_entry->neutral_clean` | 1628 |
| `unknown->unknown` | 368 |

## Execution feasibility per run

| Run | Dispatch expected | Shadow observed | Observed without expected | Unknown exec | No dispatch expected | Good executable | Good not executable | Bad avoidable |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| R10 | 0 | 0 | 0 | 0 | 150 | 0 | 25 | 42 |
| R11 | 0 | 0 | 0 | 0 | 447 | 0 | 91 | 84 |
| R13 | 0 | 1 | 1 | 0 | 2733 | 0 | 536 | 556 |

## Combined execution interpretation

| Class | Count |
| --- | ---: |
| `bad_avoidable` | 682 |
| `good_not_executable` | 652 |
| `neutral` | 1628 |
| `unknown` | 368 |

Wniosek wykonawczy: na R10/R11/R13 nie ma jeszcze `good_executable`. `good_not_executable` oznacza tutaj glownie v1/v2 market-good bez price-path/lifecycle/dispatch proof. To nie jest dowod, ze okazje byly niewykonalne ekonomicznie; to dowod, ze obecne artefakty nie uprawniaja do clean executable-good claim.

## Unknown / degraded reasons

| Reason | Count |
| --- | ---: |
| `missing_or_invalid_entry_price` | 367 |
| `missing_price_path_for_good_clean` | 652 |
| `post_entry_tx_unpriced` | 1 |

## Governance checks

- Label v1 pozostaje nietkniety; label v2 jest rownoleglym artefaktem.
- Decision logs pozostaja immutable; raporty sa addytywne.
- `good_clean` wymaga realnej price path/lifecycle evidence; nie powstaje z samego `+40 before stop`.
- `unknown_execution_status` nie jest sukcesem.
- REJECT/PENDING bez dispatchu jest `no_dispatch_expected`, nie execution failure.
- R13 ma `dispatch_observed_without_expected=1`; to jest sygnal diagnostyczny, nie evidence BUY-quality.
- Brak P2, live, threshold tuningu i runtime feature extension.
- Combined-only wynik nie jest podstawa do candidate. Kolejny etap musi raportowac R11/R13 osobno.

## Evidence for / against V3 at this checkpoint

Najmocniejsze evidence za V3: pipeline replay/outcome dziala deterministycznie na 3330 rows, a P3.7 truth layer potrafi jawnie zdegradowac zbyt gruby label zamiast produkowac optymistyczne `good_clean`.

Najmocniejsze evidence przeciw V3 jako selector: po label v2 nie istnieje jeszcze zadna probka `good_executable`, a wszystkie v1 good sa `good_dirty`. Nie ma podstaw do BUY precision claims.

Nierozstrzygniete niepewnosci: czy brak price path/lifecycle wynika tylko z ograniczen artefaktow, czy z realnej niewykonalnosci; czy nowe outcome v2 po pozyskaniu pelniejszej sciezki zachowa rozklad good/bad; czy temporal split R11/R13 ujawni regime drift.

Mozliwe zrodla self-deception: liczenie v1 `+40` jako clean good, mieszanie neutral z bad/good, traktowanie braku dispatchu po REJECT/PENDING jako execution failure, uznanie full replay za edge, oraz wybieranie feature candidates na combined-only R10/R11/R13.

## Next step

P3.7.5 Temporal Split Baseline. Raport musi pokazywac R11 standalone, R13 standalone i recent-only R11/R13; combined moze byc tylko secondary. Nie przechodzic do feature prototype, jesli temporal split nie utrzyma semantycznej stabilnosci label v2/feasibility.
