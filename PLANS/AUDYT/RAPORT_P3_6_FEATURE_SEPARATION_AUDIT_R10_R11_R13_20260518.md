# Raport P3.6 Feature Separation Audit R10/R11/R13

Data: 2026-05-18

Status: **FEATURE-SEPARATION-WEAK / R12-CANDIDATE-BLOCKED**

Zakres:

- R10: `configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml`
- R11: `configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml`
- R13: `configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml`
- labels R10/R11: `p3_5_gatekeeper_plus40_labels.jsonl`
- labels R13: `p3_6_r13_gatekeeper_plus40_labels.jsonl`

Artefakty:

- wrapper: `scripts/v3_p36_feature_separation_audit.py`
- output root: `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13`
- index: `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json`

## Governance

Ten audyt jest etapem P3.6 shadow-only. Nie zmienia active policy, V2/V2.5, IWIM, live sendera, progow ani configow runtime.

Interpretacja:

- wynik `status=ok` w wrapperze oznacza tylko, ze porownania A/B i legacy appendix zostaly wygenerowane;
- wynik legacy analyzera nie jest source of truth;
- sekcje legacy `Youden J`, `Optymalne Progi`, `L1 logistic regression`, `Scoring Rule` i podobne sa appendixem hipotez;
- `threshold_recommendation_allowed=false` dla kazdego porownania;
- FSC fields sa wykluczone z rankingu feature zgodnie z ADR-0130:
  - `funding_source_concentration`
  - `funding_source_diagnostics`

## Wykonane porownania

| comparison | status | A | B | neutral excluded | unknown excluded |
| --- | ---: | ---: | ---: | ---: | ---: |
| `good_vs_bad_all` | ok | 652 | 682 | 1628 | 368 |
| `good_vs_bad_recent_no_r10` | ok | 627 | 640 | 1559 | 354 |
| `p36_candidate_organic_relaxed_good_unblocked_vs_bad_unblocked` | hypothesis_only | 30 | 27 | 11 | 8 |
| `reject_manipulation_contradiction_good_vs_bad` | ok | 367 | 381 | 1264 | 301 |
| `pending_wait_evidence_good_vs_bad` | ok | 257 | 279 | 272 | 46 |
| `organic_failure_groups_good_vs_bad` | ok | 651 | 682 | 1628 | 368 |
| `good_vs_bad_r10` | hypothesis_only | 25 | 42 | 69 | 14 |
| `good_vs_bad_r11` | ok | 91 | 84 | 212 | 60 |
| `good_vs_bad_r13` | ok | 536 | 556 | 1347 | 294 |

## Najmocniejsze evidence za V3

1. Mechaniczna integralnosc evidence jest dobra: poprzedni combined checkpoint mial full replay OK dla R10/R11/R13, a Etap H mogl joinowac i deduplikowac zbiory po `ab_record_id`.
2. Skala R13 usuwa problem bardzo malej probki dla glownego good-vs-bad: R13 ma 536 good vs 556 bad, combined ma 652 good vs 682 bad.
3. Kierunki niektorych feature sa powtarzalne statystycznie w bootstrapie, np. `total_volume_sol`, `buy_count`, `tx_count`, ale efekt jest maly i nie daje samodzielnej separacji ekonomicznej.

## Najmocniejsze evidence przeciw V3

1. Glowna separacja good vs bad jest slaba.
   - `good_vs_bad_all`: najlepsze AUC separation okolo `0.098`, overlap `0.847`.
   - `good_vs_bad_recent_no_r10`: najlepsze AUC separation okolo `0.098`, overlap `0.855`.
   - `good_vs_bad_r13`: najlepsze AUC separation okolo `0.094`, overlap `0.854`.
2. Candidate `p36_candidate_organic_relaxed` nadal ma zbyt mala probke:
   - A=30 good_unblocked
   - B=27 bad_unblocked
   - status=`hypothesis_only`
   - zatem brak podstaw do runtime profile albo tuning thresholds.
3. Buckety, ktore mialy byc najbardziej informacyjne, tez nie separuja mocno:
   - `REJECT_V3_MANIPULATION_CONTRADICTION`: top AUC separation okolo `0.102`, overlap `0.791`.
   - `PENDING_V3_WAIT_EVIDENCE`: top AUC separation okolo `0.094`, overlap `0.765`.
4. Organic failure groups wygladaja prawie jak glowny all-set:
   - A=651, B=682
   - top AUC separation okolo `0.098`
   - brak sygnalu, ze sama rodzina organic failures oddziela dobre okazje od zlych wejsc.

## Red-team notes

Najbardziej prawdopodobne alternatywne wyjasnienia obserwowanych efektow:

- V3 obecnie mierzy glownie szeroka ekspozycje risk/liquidity/volume, a nie predykcyjna separacje entry quality.
- Good i bad labels moga byc blisko symetryczne w tym feature space, a widoczne roznice sa efektem market regime albo wolumenu, nie stabilnego edge.
- `PENDING` jako effective block moze mieszac kilka semantycznie roznych stanow: real missing evidence, konserwatywny wait state i opportunity loss.
- Neutral rows stanowia duza czesc materialu i pozostaja osobnym stanem; nie wolno ich uzywac do wygenerowania optymistycznego ratio.
- Candidate organic-relaxed wyglada interesujaco lokalnie tylko dlatego, ze probka jest mala; traktowanie 30/27 jako podstawa progu byloby overfittingiem.

## Nierozstrzygniete niepewnosci

- Czy separacja poprawia sie po segmentacji regime/time-of-day zamiast laczenia wszystkich runow.
- Czy label `+40 before stop` jest zbyt gruby i maskuje MFE/MAE albo execution feasibility.
- Czy V3 potrzebuje nowych decision-time-safe feature families, zamiast dalszego luzowania obecnych gate'ow.
- Czy `PENDING_V3_WAIT_EVIDENCE` da sie rozdzielic na faktyczny blocker krytyczny vs non-critical degraded evidence bez przepuszczania zlych wejsc.

## Werdykt

Etap H nie dostarcza stabilnej separacji feature, ktora uzasadnialaby R12 candidate runtime profile.

Decyzje:

- **P2: NO-GO**
- **live: NO-GO**
- **R12 calibrated candidate: BLOCKED**
- **threshold tuning: BLOCKED**
- **P3.7 feature redesign: RECOMMENDED, jezeli nastepny przeglad nie znajdzie stabilnego regime split**

Najbardziej racjonalny nastepny krok to nie luzowanie progow, tylko P3.6-I decision gate review:

1. potwierdzic, ze zaden wariant nie spelnia warunkow `protective_ratio >= 1.30` i `bad_unblocked <= 0.5 * good_unblocked`;
2. sprawdzic stabilnosc top feature directions miedzy R11 i R13;
3. jesli brak stabilnosci/separacji sie potwierdzi, otworzyc P3.7 feature redesign zamiast kolejnego blind runu.

## Komendy walidacyjne

```bash
python3 -m py_compile scripts/v3_p36_feature_separation_audit.py

python3 scripts/v3_p36_feature_separation_audit.py \
  --run r10:configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run r11:configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run r13:configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml:logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_6_r13_gatekeeper_plus40_labels.jsonl \
  --comparison all \
  --output-dir logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13 \
  --json \
  --markdown

git diff --check
```
