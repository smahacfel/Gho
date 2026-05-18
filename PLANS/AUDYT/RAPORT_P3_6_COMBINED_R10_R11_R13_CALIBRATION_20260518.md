# Raport P3.6 Combined Calibration R10/R11/R13 - 2026-05-18

Status: `R12-GATE-BLOCKED / P2-NO-GO / LIVE-NO-GO`

## Zakres

Ten raport podsumowuje Etap G planu `PLAN_P3_6_SAMPLE_EXPANSION_R12_GOVERNANCE_20260517.md` po domknieciu R13 jako duzej probki `sample expansion`.

R13 pozostaje:

- probka evidence,
- nie kandydat promocji,
- nie R12 calibrated candidate,
- nie dowod edge tylko dlatego, ze replay i label coverage sa poprawne.

## Artefakty

Raporty JSON:

```text
logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json
logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_recent_r11_r13_calibration_report.json
logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_summary.json
```

Label files:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl
logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_6_r13_gatekeeper_plus40_labels.jsonl
```

## Replay / operational integrity

Wszystkie wlaczone runy przeszly strict replay:

| Run | Rows | Labels | Replay | Bad rows |
|---|---:|---:|---|---:|
| R10 | 150 | 150 | `full_replay_ok` | 0 |
| R11 | 447 | 447 | `full_replay_ok` | 0 |
| R13 | 2733 | 2733 | `full_replay_ok` | 0 |

To potwierdza integralnosc pomiaru, ale nie jest dowodem istnienia edge.

## Standalone headline

| Run | Known | Bad | Good | Neutral | Unknown | Avoided bad | Blocked good | Ratio | Precision |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| R10 | 136 | 42 | 25 | 69 | 14 | 42 | 25 | 1.680000 | 0.626866 |
| R11 | 387 | 84 | 91 | 212 | 60 | 84 | 91 | 0.923077 | 0.480000 |
| R13 | 2439 | 556 | 536 | 1347 | 294 | 556 | 536 | 1.037313 | 0.509158 |

Interpretacja red-team:

- R10 wyglada dobrze, ale jest mala probka i zostaje oslabiony przez R11/R13.
- R11 i R13 sa znacznie bardziej informacyjne dla obecnego regime.
- R13 pokazuje prawie symetryczne `bad_entry` vs `good_entry`, wiec obecny V3 bardziej przypomina szeroka tarcze blokujaca niz selektywny filtr ekonomiczny.

## Combined headline

### R10 + R11 + R13

```text
known_rows=2962
bad_entry=682
good_entry=652
neutral_entry=1628
unknown=368
avoided_bad=682
blocked_good=652
protective_ratio=1.046012
protective_precision=0.511244
neutral_share=0.488889
unknown_share=0.110511
```

### R11 + R13

```text
known_rows=2826
bad_entry=640
good_entry=627
neutral_entry=1559
unknown=354
avoided_bad=640
blocked_good=627
protective_ratio=1.020734
protective_precision=0.505130
neutral_share=0.490252
unknown_share=0.111321
```

Recent aggregate jest slabszy niz aggregate z R10, wiec R10 nie powinien maskowac obecnego wyniku.

## Gate status

`r12_gate_status=blocked` dla:

- `R10+R11+R13`,
- `R11+R13`.

Blokery:

```text
candidate_protective_ratio_below_1_30
bad_unblocked_exceeds_half_good_unblocked
```

Kandydat `V3-P36-ORGANIC-RELAXED` nie przechodzi gate.

### Candidate metrics - R10+R11+R13

```text
bad_unblocked=27
good_unblocked=30
unknown_unblocked=8
variant_blocked_bad=655
variant_blocked_good=622
variant_protective_ratio=1.053055
variant_protective_precision=0.512921
```

### Candidate metrics - R11+R13

```text
bad_unblocked=25
good_unblocked=29
unknown_unblocked=8
variant_blocked_bad=615
variant_blocked_good=598
variant_protective_ratio=1.028428
variant_protective_precision=0.507007
```

Red-team interpretation:

- Kandydat odzyskuje prawie tyle samo zlych co dobrych.
- Net improvement jest minimalny.
- Safety cost jest za wysoki wzgledem odzyskanych dobrych wejsc.
- Nie ma podstaw do runtime candidate ani P2.

## Variant summary

Najwazniejsze warianty `R10+R11+R13`:

| Variant | Bad unblocked | Good unblocked | Variant ratio | Net good recovered | Safety cost |
|---|---:|---:|---:|---:|---:|
| `fsc_not_required` | 0 | 0 | 1.046012 | 0 | 0 |
| `no_pending_wait_evidence_for_noncritical_degraded` | 0 | 0 | 1.046012 | 0 | 0 |
| `no_manipulation_contradiction` | 0 | 0 | 1.046012 | 0 | 0 |
| `manip_split_dev_top3_hhi` | 0 | 0 | 1.046012 | 0 | 0 |
| `p36_evidence_soft_manip_split` | 0 | 1 | 1.047619 | 1 | 0 |
| `p36_candidate_no_organic_hhi` | 0 | 3 | 1.050847 | 3 | 0 |
| `p36_candidate_no_organic_growth` | 0 | 2 | 1.049231 | 2 | 0 |
| `p36_candidate_no_buy_ratio_min` | 3 | 5 | 1.049459 | 2 | 3 |
| `p36_candidate_organic_relaxed` | 27 | 30 | 1.053055 | 3 | 27 |
| `relaxed_sample_gate` | 0 | 0 | 1.046012 | 0 | 0 |

Warianty nie dostarczaja materialnej poprawy. Najwiekszy kandydat ma zbyt wysoki koszt bezpieczenstwa.

## Evidence decomposition

`PENDING_V3_WAIT_EVIDENCE`:

```text
rows=854
strict_effect.block=854
terminal_only_effect.pending_separate=854
```

Non-clean required groups:

```text
manipulation_contradiction.degraded=854
sybil.degraded=854
fsc.degraded=181
alpha.degraded=48
```

ADR-0130 nadal blokuje traktowanie FSC jako authoritative negative dependency. Obecnosc `fsc.degraded` w decomposition jest raportowa/diagnostyczna, nie promotion evidence.

Outcome split dla `manipulation_contradiction` i `sybil` w `PENDING_WAIT_EVIDENCE`:

```text
bad_entry=279
good_entry=257
neutral_entry=272
unknown=46
```

To nie pokazuje ostrej separacji dobrych i zlych wejsc.

## Manipulation decomposition

`REJECT_V3_MANIPULATION_CONTRADICTION`:

```text
rows=2313
```

Subtrigger outcome split:

```text
dev_volume_ratio: bad=297 good=288 neutral=991 unknown=216
hhi: bad=38 good=68 neutral=601 unknown=162
same_ms_bundle: bad=367 good=339 neutral=1150 unknown=291
top3_volume_pct: bad=96 good=124 neutral=546 unknown=170
```

Red-team interpretation:

- `same_ms_bundle` i `dev_volume_ratio` maja lekka przewage bad nad good, ale dominuje neutral.
- `hhi` i `top3_volume_pct` wygladaja gorzej: blokuja wiecej good niz bad w subtrigger split.
- Te subtriggery nie sa gotowe jako proste hard gates.
- Bucket nadal wyglada szeroko i malo selektywnie ekonomicznie.

## Organic decomposition

Dla `p36_candidate_organic_relaxed`:

```text
rows=744
bad_entry=239
good_entry=216
neutral_entry=253
unknown=36
```

Najwieksze failure counts:

```text
tx_count_growth_ratio_below_min=731
buy_ratio_min_below_min=728
unique_signer_growth_ratio_below_min=727
t1_unique_signer_delta_negative=546
t2_unique_signer_delta_negative=458
max_segment_hhi_above_max=336
```

Interpretacja: organic failures sa czeste, ale ich outcome split nadal nie daje wystarczajacej separacji ekonomicznej do kandydata runtime.

## Governance lock checkpoint

### Najmocniejsze evidence za V3

- Full replay i outcome join dzialaja na duzej probce.
- V3 rzeczywiscie blokuje wiele zlych wejsc: `682` bad avoided w aggregate.
- `same_ms_bundle` i `dev_volume_ratio` maja slaby, ale realny kierunek protective.

### Najmocniejsze evidence przeciw V3

- V3 blokuje prawie tyle samo dobrych wejsc: `652` blocked good w aggregate.
- Protective ratio `1.046` jest ekonomicznie slabe.
- Recent ratio `1.021` jest jeszcze slabsze.
- Candidate organic relaxed nie przechodzi gate i ma koszt `bad_unblocked=27` wobec tylko `good_unblocked=30`.
- Brak selected BUY oznacza, ze nadal mierzymy blokowanie, nie precision wejsc.

### Nierozstrzygniete niepewnosci

- Czy neutral dominuje z powodu realnego braku okazji, czy konstrukcji labela.
- Czy `entry_price_unavailable` i match-quality bias zmieniaja proporcje OK/NOK.
- Czy okres R13 reprezentuje stabilny regime, czy tylko jeden market slice.
- Czy istnieje kombinacja subtriggerow dajaca separacje bez nadmiernego `bad_unblocked`.

### Mozliwe zrodla self-deception

- Traktowanie `status=ok`, `full_replay_ok` albo label coverage jako dowodu edge.
- Patrzenie tylko na `avoided_bad` bez `blocked_good`.
- Uznanie R10 za reprezentatywny mimo sprzecznego R11/R13.
- Mieszanie `neutral` z sukcesem albo porazka.
- Traktowanie PENDING jako semantycznie rownego REJECT bez osobnego raportowania.

### Alternatywne wyjasnienia

- Obecny V3 moze byc ogolnym risk shield, nie selektywnym filtrem.
- Subtriggery manipulation moga korelowac z aktywnoscia rynku, a nie z negatywnym outcome.
- Balanced OK/NOK moze wynikac z niezbalansowanego regime albo sposobu hipotetycznego entry.
- Obecne organic thresholds moga wycinac zarowno momentum dobre, jak i zle.

## Decyzja

```text
R12-GATE-BLOCKED
P2-NO-GO
LIVE-NO-GO
NO RUNTIME CANDIDATE
```

Nastepny krok: Etap H - feature separation audit z uzyciem offline analyzer jako appendix, bez generowania runtime progow i bez promowania V3.
