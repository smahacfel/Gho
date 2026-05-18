# Raport P3.6 Final Closure - V3 Current Family Negative Calibration

Data: 2026-05-18

Status: **P3.6 CLOSED / CURRENT V3 SELECTOR FAMILY NEGATIVE / P3.7 OPEN**

Commit odniesienia: `372a980 Add P3.6 feature separation gate review`

## Executive summary

P3.6 zostaje formalnie zamkniete jako zakonczony etap negatywnej kalibracji
obecnej rodziny V3.

Wniosek jest jednoznaczny:

- pipeline pomiarowy V3 jest wartosciowy i dziala,
- obecny V3 pozostaje uzyteczny jako risk shield, audit layer, telemetry i
  negative control,
- obecna rodzina feature families V3 nie dowozi stabilnego selekcyjnego edge,
- dalszy threshold tuning na tych samych feature families bylby blind tuning /
  overfitting risk.

Nie przechodzimy do:

- P2,
- live,
- R12 calibrated candidate,
- kolejnego blind sample runu,
- threshold tuningu obecnych V3 gate'ow,
- progow z `analiza_porownawcza.py`.

Otwieramy:

- P3.7 Feature Redesign + Lifecycle-Aware Outcome Model.

## Why this is closure, not pause

P3.6 mialo odpowiedziec, czy wieksza probka, full replay, outcome labels,
ablation i feature separation potrafia znalezc bezpieczny shadow-only candidate
w obecnej rodzinie V3.

Odpowiedz: **nie**.

To nie jest juz problem malej probki ani replay mechanics:

- R13 dostarczyl `2733` V3 rows,
- R13 ma full replay payload dla wszystkich rows,
- strict full replay przeszedl dla `2733/2733`,
- R10/R11/R13 sa replay-stable i labelowane,
- combined analysis oraz feature separation zostaly wykonane.

P3.6 spelnilo swoja funkcje: pozwolilo falsyfikowac aktualny kierunek
threshold-level calibration.

## Dataset manifest

### R10

- config: `configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml`
- decision log: `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- labels: `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl`
- rows: `150`
- labels loaded: `150`
- strict replay: `full_replay_ok`
- bad replay rows: `0`
- known rows: `136`
- bad entry: `42`
- good entry: `25`
- neutral: `69`
- unknown: `14`
- protective ratio: `1.680000`
- protective precision: `0.626866`

### R11

- config: `configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml`
- decision log: `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- labels: `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl`
- rows: `447`
- labels loaded: `447`
- strict replay: `full_replay_ok`
- bad replay rows: `0`
- known rows: `387`
- bad entry: `84`
- good entry: `91`
- neutral: `212`
- unknown: `60`
- protective ratio: `0.923077`
- protective precision: `0.480000`

### R13

- config: `configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml`
- decision log: `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/shadow-burnin-v3-p36-sample-r13-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- labels: `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_6_r13_gatekeeper_plus40_labels.jsonl`
- rows: `2733`
- labels loaded: `2733`
- strict replay: `full_replay_ok`
- bad replay rows: `0`
- known rows: `2439`
- bad entry: `556`
- good entry: `536`
- neutral: `1347`
- unknown: `294`
- protective ratio: `1.037313`
- protective precision: `0.509158`

## Supporting documents

ADR:

- `docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md`
- `docs/ADR/ADR-0131-v3-p36-sample-expansion-runtime-governance.md`
- `docs/ADR/ADR-0132-v3-p36-r13-sample-threshold-closure.md`
- `docs/ADR/ADR-0133-v3-p37-feature-redesign-lifecycle-labels.md`

P3.6 reports:

- `PLANS/AUDYT/RAPORT_P3_6_COMBINED_R10_R11_R13_CALIBRATION_20260518.md`
- `PLANS/AUDYT/RAPORT_P3_6_FEATURE_SEPARATION_AUDIT_R10_R11_R13_20260518.md`
- `PLANS/AUDYT/RAPORT_P3_6_DECISION_GATE_REVIEW_R10_R11_R13_20260518.md`

JSON artifacts:

- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json`
- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_recent_r11_r13_calibration_report.json`
- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json`

## Combined evidence

### R10 + R11 + R13

- known rows: `2962`
- bad entry: `682`
- good entry: `652`
- neutral: `1628`
- unknown: `368`
- avoided bad: `682`
- blocked good: `652`
- protective ratio: `1.046012`
- protective precision: `0.511244`
- neutral share: `0.488889`
- unknown share: `0.110511`

### R11 + R13 recent-only

- known rows: `2826`
- bad entry: `640`
- good entry: `627`
- neutral: `1559`
- unknown: `354`
- avoided bad: `640`
- blocked good: `627`
- protective ratio: `1.020734`
- protective precision: `0.505130`
- neutral share: `0.490252`
- unknown share: `0.111321`

Interpretacja:

- R10 wygladal najbardziej optymistycznie, ale byl mniejszy i nie moze maskowac
  slabszych R11/R13.
- Recent-only aggregate jest bliski symetrii good/bad.
- Obecny V3 nie pokazuje selekcyjnej przewagi ekonomicznej.

## Candidate and variant closure

Candidate `V3-P36-ORGANIC-RELAXED` zostaje zamkniety.

All-set:

- variant protective ratio: `1.053055`
- good unblocked: `30`
- bad unblocked: `27`
- unknown unblocked: `8`
- blocked gates:
  - `candidate_protective_ratio_below_1_30`
  - `bad_unblocked_exceeds_half_good_unblocked`

Recent-only:

- variant protective ratio: `1.028428`
- good unblocked: `29`
- bad unblocked: `25`
- unknown unblocked: `8`
- blocked gates:
  - `candidate_protective_ratio_below_1_30`
  - `bad_unblocked_exceeds_half_good_unblocked`

Pozostale warianty nie dostarczyly materialnej poprawy:

- `fsc_not_required`
- `no_pending_wait_evidence_for_noncritical_degraded`
- `no_manipulation_contradiction`
- `manip_split_dev_top3_hhi`
- `relaxed_sample_gate`

## Feature separation closure

Feature separation audit nie pokazal stabilnej separacji good vs bad.

Glowny all-set:

- `good_vs_bad_all`: A=`652`, B=`682`
- top AUC separation okolo `0.098`
- overlap okolo `0.847`

Recent-only:

- A=`627`, B=`640`
- top AUC separation okolo `0.098`
- overlap okolo `0.855`

R13 standalone:

- A=`536`, B=`556`
- top AUC separation okolo `0.094`
- overlap okolo `0.854`

Problem buckets:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: top AUC separation okolo `0.102`
- `PENDING_V3_WAIT_EVIDENCE`: top AUC separation okolo `0.094`
- organic failure groups: top AUC separation okolo `0.098`

Wniosek:

- obecne manipulation/organic/PENDING families sa za szerokie i za slabo
  separuja ekonomicznie,
- dalsze strojenie progow na tych fields byloby self-deception risk.

## What is closed

Zamkniete:

- P3.6 threshold-level calibration,
- P3.6 organic-relaxed candidate,
- P3.6 manipulation split candidate,
- P3.6 FSC-descoped unlock hypothesis jako candidate path,
- R12 calibrated candidate,
- P2 promotion discussion for current V3 family,
- threshold tuning current feature families,
- blind sample expansion without a new feature-level hypothesis.

## What remains valuable

Pozostaje wartosciowe:

- full replay payload pipeline,
- Rust-first strict replay validator,
- V3 shadow sidecar telemetry,
- DecisionLogger V3 evidence enrichment,
- outcome labeler,
- P3.6 calibration report,
- P3.6 feature separation audit wrapper,
- ADR-0130 primary-only governance,
- R10/R11/R13 as negative calibration baseline,
- current V3 as risk shield / audit layer / negative control.

## Governance rules carried into P3.7

1. Outcome Label v2 before feature redesign conclusions.
2. Execution feasibility join before BUY-quality claims.
3. Temporal split required; combined-only evidence is insufficient.
4. Fresh holdout required before any candidate run.
5. Kill criterion required: close V3 selector line if no stable separation.
6. Current V3 remains risk shield/audit layer, not selector candidate.

## Final decision

```text
P3.6 status: CLOSED
Current V3 selector family: NEGATIVE CALIBRATION
R12 calibrated candidate: BLOCKED
P2: BLOCKED
Live: BLOCKED
Threshold tuning: BLOCKED
P3.7 Feature Redesign: OPEN
```

P3.7 is not an attempt to keep V3 alive at all costs. P3.7 is the final
evidence-driven attempt to determine whether V3 can become a selector. If
label-v2, execution-feasible, temporal-holdout evidence still does not show
stable separation, V3 selector line must be closed and V3 remains risk shield /
audit infrastructure only.
