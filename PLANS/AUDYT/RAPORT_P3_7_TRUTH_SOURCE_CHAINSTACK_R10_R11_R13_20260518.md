# Raport P3.7 Truth-Source Acquisition Chainstack R10/R11/R13

Data: 2026-05-18

Status: **TRUTH-SOURCE ACQUISITION COMPLETE / PHASE B BLOCKED**

## Executive Summary

P3.7 truth-source acquisition zostalo wykonane dla historycznych runow R10,
R11 i R13 przy uzyciu Chainstack Solana RPC jako zrodla post-decision
price-path samples.

Etap rozwiązal poprzedni blocker `no_good_clean_rows`: Outcome Label v2 ma
teraz realne MFE/MAE rows oraz `good_clean` w R10, R11 i R13.

Etap nie odblokowuje P3.7 Phase B jako candidate work, poniewaz nadal nie ma
`good_executable`: historyczne runy pozostaja shadow/reject/pending evidence i
nie zawieraja dispatch/shadow lifecycle dowodu egzekwowalnosci wejscia.

Nie wykonano P2, live, threshold tuningu ani zmian aktywnej polityki.

## Artefakty Operacyjne

Artefakty price-path, label-v2, execution feasibility oraz raporty
chainstack sa addytywne i zapisane w `logs/rollout/...`. Katalog `logs/` jest
operacyjnym evidence store i pozostaje poza commitem repo.

Najwazniejsze sciezki:

- R10 price path:
  `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_7_price_path_samples_chainstack_20260518.jsonl`
- R11 price path:
  `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_7_price_path_samples_chainstack_20260518.jsonl`
- R13 price path:
  `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_7_price_path_samples_chainstack_20260518.jsonl`
- Temporal split report:
  `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_temporal_split_chainstack_20260518.json`
- Evidence availability report:
  `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_evidence_availability_chainstack_20260518.json`

## Price-Path Coverage

| Run | Rows | Rows With Samples | Coverage | Total Samples | OK | Partial | RPC Error | Unavailable | Entry Invalid |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| R10 | 150 | 122 | 81.33% | 10,483 | 36 | 86 | 1 | 13 | 14 |
| R11 | 447 | 367 | 82.10% | 30,495 | 118 | 249 | 5 | 15 | 60 |
| R13 | 2,733 | 2,320 | 84.89% | 146,128 | 856 | 1,464 | 38 | 81 | 294 |

`partial` oznacza, ze row ma realne price-path samples, ale nie wszystkie
transakcje w oknie daly sie wycenic. Nie jest to brak sciezki.

## Outcome Label v2 Po Chainstack Price Path

| Run | Rows | Good Clean | Good Dirty | Bad Clean | Neutral Clean | Unknown | Price Path Available |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| R10 | 150 | 19 | 6 | 42 | 69 | 14 | 81.33% |
| R11 | 447 | 75 | 16 | 84 | 212 | 60 | 82.10% |
| R13 | 2,733 | 421 | 115 | 556 | 1,347 | 294 | 84.89% |
| Recent R11/R13 | 3,180 | 496 | 131 | 640 | 1,559 | 354 | 84.50% |
| Combined R10/R11/R13 | 3,330 | 515 | 137 | 682 | 1,628 | 368 | 84.35% |

## Execution Feasibility

Execution feasibility join pozostaje fail-closed:

- dispatch expected rows: 0,
- shadow dispatch observed rows: 0,
- `good_executable`: 0 dla R10/R11/R13.

To oznacza, ze P3.7 moze teraz analizowac market-quality target
(`good_clean`), ale nie moze jeszcze twierdzic, ze te okazje byly realnie
egzekwowalne przez Ghost.

## Gate Status

Temporal split:

```text
p3_7_5_status=blocked
blockers:
- r11_has_no_good_executable_rows
- r13_has_no_good_executable_rows
```

Evidence availability:

```text
p3_7_evidence_status=blocked
blockers:
- no_good_executable_rows
```

Poprzedni blocker `no_post_decision_price_path_rows` zostal usuniety przez
nowe Chainstack price-path samples.

## Governance

Ten etap:

- nie promuje V3 do P2,
- nie wlacza live,
- nie zmienia aktywnej polityki V2/V2.5,
- nie zmienia IWIM ani execution/live sender,
- nie tuninguje thresholdow,
- nie traktuje decision-time vectors jako outcome truth,
- nie traktuje threshold summary jako pelnej sciezki MFE/MAE.

## Wniosek

P3.7 Phase A zrobila istotny krok do przodu: mamy post-decision price-path
truth-source i niezerowe `good_clean` w obu wymaganych splitach R11/R13.

P3.7 Phase B feature prototype nadal pozostaje zablokowane jako candidate work,
bo brakuje executable-quality targetu. Najblizszy sensowny krok to decyzja, czy
P3.7 ma najpierw zbudowac execution-feasibility proxy/join dla historycznych
blocked opportunities, czy ograniczyc kolejny etap do market-quality feature
audit z bardzo wyraznym zastrzezeniem, ze nie jest to BUY-executable evidence.
