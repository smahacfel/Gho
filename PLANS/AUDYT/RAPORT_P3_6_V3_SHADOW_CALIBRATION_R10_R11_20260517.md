# Raport P3.6 V3 Shadow Calibration R10+R11

Data: 2026-05-17
Status: `P3.6-OFFLINE-CALIBRATION-READY / R12-BLOCKED / P2-NO-GO`

## Cel

Celem P3.6 bylo sprawdzenie, czy po ADR-0130 warianty offline rozdzielajace
FSC/evidence i manipulation subtriggery poprawiaja jakosc decyzji V3 na polaczonym
zbiorze R10+R11.

To nie byl rollout runtime i nie byl to P2 promotion. Analiza byla offline na full replay payloadach.

## Dane wejsciowe

- R10: `configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml`
- R10 labels: `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl`
- R11: `configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml`
- R11 labels: `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl`

Baseline replay dla obu runow przechodzi `full_replay_ok`.

## Combined headline

Komenda:

```bash
python3 scripts/v3_p36_calibration_report.py \
  --run r10:configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --run r11:configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml:logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
```

Wynik:

- `status=ok`
- `known_rows=523`
- `bad_entry=126`
- `good_entry=116`
- `neutral_entry=281`
- `unknown=74`
- `avoided_bad=126`
- `blocked_good=116`
- `protective_ratio=1.086207`
- `protective_precision=0.520661`

Interpretacja: current V3 ma realny efekt ochronny, ale combined ratio jest za slabe
na R12 candidate i daleko od P2.

## Candidate V3-P36-EVIDENCE-MANIP-SPLIT

Wariant: `p36_evidence_soft_manip_split`

Wynik na R10+R11:

- `variant_blocked_bad=126`
- `variant_blocked_good=116`
- `variant_protective_ratio=1.086207`
- `variant_protective_precision=0.520661`
- `good_unblocked=0`
- `bad_unblocked=0`
- `unknown_unblocked=0`
- `net_good_recovered=0`

Wariant nie poprawia baseline i nie odzyskuje zadnych dobrych wejsc.

Zmienia jednak przyczyny decyzji:

- R10: changed verdict `47`, changed reason `47`
- R11: changed verdict `131`, changed reason `133`

Po zdjeciu targetowanych blockerow kandydat nie przechodzi do BUY. Glownym efektem
jest przejscie wielu rows do `REJECT_V3_LOW_ORGANIC_BROADENING`, czyli kolejna
bariera jest juz opportunity/organic, nie FSC.

R12 gate:

- `r12_gate_status=blocked`
- `blocked_gates=["candidate_protective_ratio_below_1_30", "blocked_good_not_reduced"]`

Werdykt: nie uruchamiac R12 na `V3-P36-EVIDENCE-MANIP-SPLIT`.

## Evidence decomposition

`PENDING_V3_WAIT_EVIDENCE`:

- rows: `181`
- strict effective block: `181`
- terminal-only: `181 pending_separate`

Required non-clean groups:

- `fsc.degraded=181`
- `sybil.degraded=181`
- `manipulation_contradiction.degraded=181`
- `alpha.degraded=6`

Required non-clean reasons:

- `fsc_evidence_partial=181`
- `sybil_evidence_partial=181`
- `manipulation_contradiction_partial=181`
- `alpha_evidence_partial=6`

Interpretacja: FSC jest obecny w degraded evidence, ale samo `fsc=false` nie wystarcza,
bo te same rows maja rowniez degraded sybil i manipulation_contradiction. To obala prosta
hipoteze, ze caly R11 byl zanizony tylko przez FSC jako required evidence.

## Manipulation decomposition

`REJECT_V3_MANIPULATION_CONTRADICTION` rows: `385`

Najwazniejsze subtrigger outcome splits:

- `same_ms_bundle`: bad `54`, good `46`, neutral `203`, unknown `60`
- `dev_volume_ratio`: bad `41`, good `31`, neutral `177`, unknown `47`
- `hhi`: bad `6`, good `8`, neutral `111`, unknown `37`
- `top3_volume_pct`: bad `3`, good `12`, neutral `88`, unknown `35`

Najwieksze kombinacje:

- `dev_volume_ratio+same_ms_bundle=145`
- `dev_volume_ratio+hhi+same_ms_bundle+top3_volume_pct=69`
- `same_ms_bundle=43`
- `dev_volume_ratio+hhi+same_ms_bundle=39`

Interpretacja: dominujacy bucket nie jest pojedynczym prostym progiem. `same_ms_bundle`
i `dev_volume_ratio` sa najwiekszymi skladowymi, a `top3_volume_pct` sam w sobie wyglada
slabo jako twardy reject na obecnej probce.

## Ablation summary

R10:

- `fsc_not_required`: changed verdict `0`
- `no_pending_wait_evidence_for_noncritical_degraded`: changed verdict `0`
- `no_manipulation_contradiction`: changed verdict `94`
- `manip_split_dev_top3_hhi`: changed verdict `1`
- `p36_evidence_soft_manip_split`: changed verdict `47`
- `relaxed_sample_gate`: changed verdict `0`

R11:

- `fsc_not_required`: changed verdict `0`
- `no_pending_wait_evidence_for_noncritical_degraded`: changed verdict `0`
- `no_manipulation_contradiction`: changed verdict `291`
- `manip_split_dev_top3_hhi`: changed verdict `4`
- `p36_evidence_soft_manip_split`: changed verdict `131`
- `relaxed_sample_gate`: changed verdict `0`

Zaden wariant nie odblokowal ekonomicznie good entries w trybie strict effective block.
`no_manipulation_contradiction` i `p36_evidence_soft_manip_split` zmieniaja klase decyzji,
ale nadal nie dowoza entry-ready V3.

## Organic / opportunity decomposition

Po kandydacie `p36_evidence_soft_manip_split` do `REJECT_V3_LOW_ORGANIC_BROADENING`
wpada `177` rows:

- `bad_entry=64`
- `good_entry=59`
- `neutral_entry=48`
- `unknown=6`

Najczestsze failure predicates:

- `buy_ratio_min_below_min=171`
- `tx_count_growth_ratio_below_min=166`
- `unique_signer_growth_ratio_below_min=162`
- `t1_unique_signer_delta_negative=109`
- `t2_unique_signer_delta_negative=93`
- `max_segment_hhi_above_max=60`

Outcome split dla najwazniejszych predykatow:

- `buy_ratio_min_below_min`: bad `64`, good `54`, neutral `48`, unknown `5`
- `tx_count_growth_ratio_below_min`: bad `57`, good `55`, neutral `48`, unknown `6`
- `unique_signer_growth_ratio_below_min`: bad `57`, good `51`, neutral `48`, unknown `6`
- `t1_unique_signer_delta_negative`: bad `36`, good `37`, neutral `33`, unknown `3`
- `max_segment_hhi_above_max`: bad `12`, good `19`, neutral `29`

Interpretacja:

Po zdjeciu targetowanych evidence/manip blockerow problem przenosi sie na organic gate.
To nie wyglada na pojedynczy prosty prog do poluzowania. `buy_ratio_min` i growth ratios
blokuje zarowno zle, jak i dobre entries prawie symetrycznie. `max_segment_hhi` wyglada
szczegolnie podejrzanie jako twarda bariera, bo w tej probce blokuje wiecej good niz bad.
Wymaga to dalszej offline ablation, ale nadal nie uzasadnia R12 ani P2.

## Werdykt

P3.6 dostarcza narzedzia i raport do kalibracji, ale kandydat
`V3-P36-EVIDENCE-MANIP-SPLIT` nie przechodzi offline gate.

Nie uruchamiac R12 jeszcze.
Nie przechodzic do P2.
Nie zmieniac active V2/V2.5, IWIM ani live sender.

Nastepny sensowny krok: zrobic organic-only offline ablation dla `max_segment_hhi`,
growth ratios i `buy_ratio_min`, z tym samym wymogiem: mierzyc `good_unblocked`,
`bad_unblocked`, neutral i unknown osobno. Nie dokladac kolejnego runu, dopoki organic
candidate nie poprawi combined R10+R11.

## Organic-only ablation

Wykonano dodatkowe warianty organic-only na bazie `p36_evidence_soft_manip_split`:

- `p36_candidate_no_organic_hhi`
- `p36_candidate_no_organic_growth`
- `p36_candidate_no_buy_ratio_min`
- `p36_candidate_organic_relaxed`

Wynik combined R10+R11:

| Wariant | good_unblocked | bad_unblocked | neutral_unblocked | variant_ratio |
| --- | ---: | ---: | ---: | ---: |
| `p36_candidate_no_organic_hhi` | 0 | 0 | 0 | 1.086207 |
| `p36_candidate_no_organic_growth` | 1 | 0 | 0 | 1.095652 |
| `p36_candidate_no_buy_ratio_min` | 1 | 2 | 0 | 1.078261 |
| `p36_candidate_organic_relaxed` | 9 | 12 | 2 | 1.065421 |

`p36_candidate_organic_relaxed` wygenerowal `23` BUY candidates (`3` w R10, `20` w R11),
ale ekonomicznie pogorszyl wynik: odblokowal wiecej zlych niz dobrych entries.

R12 gate dla `p36_candidate_organic_relaxed`:

- `r12_gate_status=blocked`
- `blocked_gates=["candidate_protective_ratio_below_1_30", "bad_unblocked_exceeds_half_good_unblocked"]`

Interpretacja:

Organic-only loosening falsyfikuje prosta hipoteze, ze problemem jest jeden z progow
organic. `max_segment_hhi` samodzielnie nic nie odzyskuje, growth daje tylko `1` good
bez kosztu, a `buy_ratio_min` i wariant laczony pogarszaja safety/economics. To oznacza,
ze nie ma jeszcze bezpiecznego kandydata R12 na podstawie R10+R11.

Nastepny krok powinien byc analityczny, nie runtime: przeanalizowac te 23 BUY candidates
z wariantu organic-relaxed oraz 1 good recovered z growth-only, zeby sprawdzic, ktore
konkretne cechy odrozniaja je od `bad_unblocked`.

## Organic-relaxed BUY candidate analysis

Do `scripts/v3_p36_calibration_report.py` dodano sekcje `candidate_buy_analysis`
dla wariantu `p36_candidate_organic_relaxed`.

Wynik combined R10+R11:

- `rows=23`
- `sample_size_warning=true`
- `bad_entry=12`
- `good_entry=9`
- `neutral_entry=2`
- `R10=3`, `R11=20`

Interpretacja probki:

Ta probka jest za mala do estymacji precision, do tuningu progow i do decyzji
promocyjnej. Jest jednak wystarczajaca jako falsyfikacja tego konkretnego
kandydata, bo juz na tych 23 rows candidate lamie kierunek safety/economics:
uwalnia `12` zlych wejsc wobec `9` dobrych wejsc, przy wymaganiu P3.6, zeby
`bad_unblocked <= 0.5 * good_unblocked`.

Najwazniejsze cechy 23 candidate BUY:

- wszystkie `23/23` maja subtrigger `same_ms_bundle`
- organic failures dla `bad_entry`: `buy_ratio_min_below_min=12`,
  `tx_count_growth_ratio_below_min=8`, `unique_signer_growth_ratio_below_min=9`,
  `max_segment_hhi_above_max=1`
- organic failures dla `good_entry`: `buy_ratio_min_below_min=8`,
  `tx_count_growth_ratio_below_min=7`, `unique_signer_growth_ratio_below_min=4`,
  `max_segment_hhi_above_max=2`

Mediany wybranych cech:

| Cecha | bad_entry median | good_entry median |
| --- | ---: | ---: |
| `buy_count` | 51.0 | 39.0 |
| `total_tx` | 80.0 | 54.0 |
| `unique_signers` | 45.5 | 39.0 |
| `buy_ratio_min` | 0.560606 | 0.55 |
| `buy_ratio_mean` | 0.702756 | 0.667793 |
| `tx_count_growth_ratio` | 0.57 | 0.666667 |
| `unique_signer_growth_ratio` | 0.693182 | 1.0 |
| `max_segment_hhi` | 0.072862 | 0.091837 |
| `same_ms_tx_ratio` | 0.145857 | 0.114286 |
| `bundle_suspicion_ratio` | 0.497827 | 0.444444 |
| `dev_volume_ratio` | 0.101032 | 0.150546 |
| `top3_volume_pct` | 0.315057 | 0.387577 |

Wniosek:

Nie widac stabilnego, bezpiecznego separatora miedzy `good_entry` i `bad_entry`
w tej probce. `same_ms_bundle` trafia wszystkie candidate rows, a organic/growth
cechy sa blisko siebie. `max_segment_hhi` nie wyglada na samodzielny dobry gate,
bo w tej probce mediana jest wyzsza dla `good_entry` niz dla `bad_entry`.

Werdykt dla `p36_candidate_organic_relaxed`: nadal `R12-BLOCKED`.
Nie uzywac tych 23 rows do strojenia progow. Uzywac ich tylko jako dowodu, ze
obecny organic-relaxed candidate jest falsyfikowany i wymaga innego kierunku
analizy.
