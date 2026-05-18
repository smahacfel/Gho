# Raport P3.6 Decision Gate Review R10/R11/R13

Data: 2026-05-18

Status: **R12-CANDIDATE-BLOCKED / P3.7-TRIGGERED**

Ten dokument domyka Etap I planu P3.6 po:

- combined calibration R10/R11/R13,
- recent calibration R11/R13,
- feature separation audit R10/R11/R13.

Nie jest to rekomendacja P2, live ani tuning progow. To jest decyzja governance po evidence expansion.

## Inputs

Raporty zrodlowe:

- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json`
- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_recent_r11_r13_calibration_report.json`
- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json`
- `PLANS/AUDYT/RAPORT_P3_6_COMBINED_R10_R11_R13_CALIBRATION_20260518.md`
- `PLANS/AUDYT/RAPORT_P3_6_FEATURE_SEPARATION_AUDIT_R10_R11_R13_20260518.md`

## Governance lock

R13 pozostaje sample expansion i evidence generation, nie candidate promotion run.

Rozdzielenie oceny:

- replay correctness: OK na poprzednim checkpointcie combined calibration;
- operational integrity: OK jako sample/evidence closure;
- predictive quality: weak;
- economic viability: not proven.

`status=ok`, full replay i wysoka coverage nie sa traktowane jako dowod edge.

## Combined calibration gate

### R10/R11/R13

Headline:

- known rows: `2962`
- bad entry: `682`
- good entry: `652`
- neutral: `1628`
- unknown: `368`
- avoided bad: `682`
- blocked good: `652`
- protective ratio: `1.046012`
- protective precision: `0.511244`

Candidate `V3-P36-ORGANIC-RELAXED`:

- variant protective ratio: `1.053055`
- variant blocked bad: `655`
- variant blocked good: `622`
- good unblocked: `30`
- bad unblocked: `27`
- neutral unblocked: `11`
- unknown unblocked: `8`

Gate blockers:

- `candidate_protective_ratio_below_1_30`
- `bad_unblocked_exceeds_half_good_unblocked`

### R11/R13 recent-only

Headline:

- known rows: `2826`
- bad entry: `640`
- good entry: `627`
- neutral: `1559`
- unknown: `354`
- avoided bad: `640`
- blocked good: `627`
- protective ratio: `1.020734`
- protective precision: `0.505130`

Candidate `V3-P36-ORGANIC-RELAXED`:

- variant protective ratio: `1.028428`
- variant blocked bad: `615`
- variant blocked good: `598`
- good unblocked: `29`
- bad unblocked: `25`
- neutral unblocked: `11`
- unknown unblocked: `8`

Gate blockers:

- `candidate_protective_ratio_below_1_30`
- `bad_unblocked_exceeds_half_good_unblocked`

## Variant decision

| variant | all result | recent result | decision |
| --- | --- | --- | --- |
| `fsc_not_required` | no material delta | no material delta | blocked |
| `no_pending_wait_evidence_for_noncritical_degraded` | no material delta | no material delta | blocked |
| `no_manipulation_contradiction` | no material delta | no material delta | blocked |
| `manip_split_dev_top3_hhi` | no material delta | no material delta | blocked |
| `relaxed_sample_gate` | no material delta | no material delta | blocked |
| `p36_candidate_organic_relaxed` | ratio `1.053055`, bad_unblocked `27`, good_unblocked `30` | ratio `1.028428`, bad_unblocked `25`, good_unblocked `29` | blocked |

Interpretacja:

- zaden wariant nie zbliza sie do wymaganego `protective_ratio >= 1.30`;
- jedyny wariant z realnym unblockingiem odzyskuje prawie tyle samo zlych co dobrych rows;
- warunek `bad_unblocked <= 0.5 * good_unblocked` nie jest spelniony;
- brak candidate shadow profile do R12 calibrated run.

## Feature separation gate

Glowny all-set:

- `good_vs_bad_all`: A=652, B=682;
- najlepsza separacja AUC okolo `0.098`;
- overlap okolo `0.847`.

Recent-only:

- `good_vs_bad_recent_no_r10`: A=627, B=640;
- najlepsza separacja AUC okolo `0.098`;
- overlap okolo `0.855`.

R13 standalone:

- A=536, B=556;
- najlepsza separacja AUC okolo `0.094`;
- overlap okolo `0.854`.

Problem buckets:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: A=367, B=381, top AUC separation okolo `0.102`, overlap okolo `0.791`;
- `PENDING_V3_WAIT_EVIDENCE`: A=257, B=279, top AUC separation okolo `0.094`, overlap okolo `0.765`;
- organic failure groups: A=651, B=682, top AUC separation okolo `0.098`.

Candidate unblocked:

- `p36_candidate_organic_relaxed_good_unblocked_vs_bad_unblocked`: A=30, B=27;
- status `hypothesis_only`;
- zbyt mala probka do decyzji progowej lub runtime profile.

## Strongest evidence for V3

1. Pipeline replay/outcome/feature audit jest juz technicznie zdolny do duzych, audytowalnych porownan.
2. V3 nadal pelni funkcje szerokiej tarczy ryzyka: blokuje wszystkie znane bad rows w obecnym baseline.
3. Niektore cechy maja powtarzalny kierunek statystyczny miedzy R11 i R13, ale efekt jest zbyt maly.

## Strongest evidence against V3

1. Ekonomiczna separacja jest praktycznie symetryczna:
   - all protective ratio `1.046012`;
   - recent protective ratio `1.020734`.
2. Candidate unblocking jest niebezpiecznie blisko coin-flip:
   - all: 30 good vs 27 bad;
   - recent: 29 good vs 25 bad.
3. Feature separation pokazuje wysokie nakladanie rozkladow i brak mocnych AUC.
4. Manipulation, PENDING i organic families nie wskazuja stabilnego subobszaru, ktory mozna bezpiecznie poluzowac.

## Self-deception risks

- Mylenie full replay OK z dowodem edge.
- Interpretowanie duzej liczby rows jako jakosci sygnalu.
- Wyciaganie progow z legacy analyzera, mimo ze overlap jest wysoki.
- Uznanie neutral rows za ukryte success/failure bez dodatkowego labela.
- Overfitting candidate organic-relaxed na probce 30/27.
- Pominiecie kosztu `bad_unblocked`, gdy wariant odzyskuje troche dobrych rows.

## Alternative explanations

- Obecna rodzina V3 gate'ow jest dobra jako risk shield, ale nie ma wystarczajacej informacji do selekcji dobrych wejsc.
- Label `+40 before stop` moze byc zbyt jednowymiarowy i nie ujmuje MFE/MAE, latency ani execution feasibility.
- Potrzebna separacja moze znajdowac sie w trajektorii/lifecycle, nie w aktualnych agregatach V3.
- Obecne feature families moga mierzyc aktywnosc i koncentracje, ale nie przewage ekonomiczna.

## Decision

Formalny Etap I:

- **R12 candidate:** BLOCKED
- **P2:** BLOCKED
- **live:** BLOCKED
- **threshold-level calibration:** BLOCKED
- **P3.7 feature redesign:** OPEN

Nie nalezy uruchamiac kolejnego blind sample runu bez nowej hipotezy feature-level.

## P3.7 opening scope

P3.7 powinien szukac nowego evidence, nie luzowac obecnych gate'ow.

Zakres rekomendowany:

1. lifecycle-aware labels: MFE/MAE, time-to-MFE, max adverse excursion, stop-path;
2. execution feasibility joins: czy good label byl realnie wykonalny w shadow/live constraints;
3. trajectory features dostepne decision-time: shape, monotonicity, pullback/recovery, burst decay;
4. rozdzielenie neutral/no-target od failed opportunity;
5. przyczynowa dekompozycja PDD/organic/manipulation zamiast pojedynczych broad buckets;
6. tylko features obecne albo mozliwe do dodania do `MaterializedFeatureSet` w replay-safe formie.

Out of scope:

- P2;
- live;
- tuning progow V3 na podstawie obecnego analyzer output;
- reaktywacja HyperPrediction/Chaos jako active path;
- FSC jako active ranking/hard gate pod single-stream constraint.

## Verification

```bash
python3 -m py_compile scripts/v3_p36_feature_separation_audit.py
git diff --check
```
