# Plan P3.6 V3 Shadow-Only Calibration

Data: 2026-05-17
Status: `IMPLEMENTATION-IN-PROGRESS / R12-BLOCKED`

## Cel

P3.6 ma przejsc z samego zbierania kolejnych runow do kontrolowanej kalibracji V3
na podstawie pelnego replayu i outcome labels z R10 oraz R11.

Nie jest to etap P2, live ani zmiana aktywnej polityki V2/V2.5.

## Kontrakty

- R10/R11 pozostaja historyczne i immutable; ich policy hashes nie sa przepisywane.
- P3.6 tworzy forward-only profil primary-only z nowym policy hash.
- Baseline replay parity uzywa policy payload zapisany w row.
- Warianty ablation sa counterfactual replay, nie dowod row policy parity.
- FSC jest zde-scope'owany zgodnie z ADR-0130 pod single-stream provider constraint.
- Unknown degraded reason pozostaje fail-closed jako non-funding.
- PENDING jest raportowany jako effective block, ale osobno od terminalnego REJECT.
- Brak P2 promotion.
- Brak zmian active V2/V2.5, IWIM, live sender i execution.

## Zakres implementacyjny

1. Utworzyc izolowany brain config P3.6:
   - `configs/rollout/ghost_brain_v3_p36_primary_only.toml`
   - `gatekeeper_v3.enabled=false`
   - `shadow_emit_enabled=true`
   - `replay_payload_enabled=true`
   - `promotion.enabled=false`
   - `gatekeeper_v3.evidence_requirements.fsc=false`

2. Utworzyc rollout candidate R12, ale nie uruchamiac go przed offline gate:
   - `configs/rollout/shadow-burnin-v3-p36-calibrated-r12-primary-only.toml`
   - `seer.funding_lane_mode="disabled"`
   - `entry_mode="shadow_only"`
   - `execution_mode="shadow"`

3. Rozszerzyc Rustowy full replay validator:
   - row-level deltas dla ablation,
   - `variant_policy_config_hash`,
   - `fsc_not_required`,
   - `no_pending_wait_evidence_for_noncritical_degraded`,
   - `no_manipulation_contradiction`,
   - `manip_split_dev_top3_hhi`,
   - `p36_evidence_soft_manip_split`,
   - `p36_candidate_no_organic_hhi`,
   - `p36_candidate_no_organic_growth`,
   - `p36_candidate_no_buy_ratio_min`,
   - `p36_candidate_organic_relaxed`,
   - `relaxed_sample_gate`.

4. Dodac zbiorczy raport P3.6:
   - `scripts/v3_p36_calibration_report.py`
   - R10+R11 combined headline,
   - PENDING evidence-group breakdown,
   - manipulation subtrigger breakdown,
   - organic/opportunity failure decomposition,
   - variant quality deltas,
   - R12 gate.

## Acceptance dla R12

R12 moze zostac uruchomiony dopiero gdy offline P3.6 gate spelnia:

- baseline full replay OK dla kazdego runu wejsciowego,
- candidate variant ma row-level deltas,
- candidate protective_ratio >= 1.30,
- candidate blocked_good spada wzgledem current,
- candidate bad_unblocked <= good_unblocked * 0.50,
- unknown_unblocked nie dominuje,
- neutral_unblocked raportowane osobno,
- brak active policy change,
- brak P2 promotion.

## Weryfikacja

Planowane komendy:

```bash
cargo test -p ghost-launcher --bin v3_replay
cargo test -p ghost-brain --test ghost_brain_config_load_test
python3 -m unittest scripts/test_v3_p36_calibration_report.py -v
python3 -m unittest scripts/test_v3_replay_ablation_report.py -v
python3 -m unittest scripts/test_v3_outcome_quality_report.py -v
python3 scripts/v3_p36_calibration_report.py \
  --run r10:configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run r11:configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
git diff --check
```

## Decyzja operacyjna

Jezeli `p36_evidence_soft_manip_split` nie poprawi combined R10+R11, R12 pozostaje
zablokowany. Wtedy nastepny krok to nie blind run, tylko organic-only offline
ablation dla `max_segment_hhi`, growth ratios i `buy_ratio_min`.
