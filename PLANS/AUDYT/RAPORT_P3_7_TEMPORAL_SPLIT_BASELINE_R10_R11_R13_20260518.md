# Raport P3.7 Temporal Split Baseline R10/R11/R13

Status: **TEMPORAL SPLIT COMPLETE / FEATURE PROTOTYPE BLOCKED**

## Executive summary

P3.7.5 raportuje R11 standalone, R13 standalone, recent-only R11/R13 oraz combined all jako widok pomocniczy. Ten raport nie dowodzi edge i nie autoryzuje P2, live, threshold tuning ani feature prototype.

- Temporal gate status: `blocked`
- Blockers: `['r11_has_no_good_executable_rows', 'r13_has_no_good_executable_rows']`
- `do_not_train_on_R13_then_validate_on_R13`: `true`

## Required Views

| View | Rows | Good clean | Good dirty | Bad clean | Good executable | Price path available | Dispatch expected | Shadow observed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| r10 | 150 | 19 | 6 | 42 | 0 | 0.813333 | 0 | 0 |
| r11 | 447 | 75 | 16 | 84 | 0 | 0.821029 | 0 | 0 |
| r13 | 2733 | 421 | 115 | 556 | 0 | 0.848884 | 0 | 1 |
| recent_r11_r13 | 3180 | 496 | 131 | 640 | 0 | 0.844969 | 0 | 1 |
| combined_all_secondary | 3330 | 515 | 137 | 682 | 0 | 0.843544 | 0 | 1 |

## R11 vs R13 Drift

| Metric | R11 rate | R13 rate | R13-R11 | CI95 low | CI95 high | Crosses zero |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `good_clean` | 0.167785 | 0.154043 | -0.013742 | -0.050933 | 0.023449 | True |
| `good_dirty` | 0.035794 | 0.042078 | 0.006284 | -0.012511 | 0.025079 | True |
| `bad_clean` | 0.187919 | 0.203439 | 0.015520 | -0.023714 | 0.054754 | True |
| `good_executable` | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | True |
| `execution_feasible` | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | True |
| `price_path_available` | 0.821029 | 0.848884 | 0.027855 | -0.010134 | 0.065844 | True |

## Numeric Path Availability

| View | MFE 10s n | MAE 10s n | Time to MFE n | Time to MAE n |
| --- | ---: | ---: | ---: | ---: |
| r10 | 114 | 114 | 122 | 122 |
| r11 | 338 | 338 | 367 | 367 |
| r13 | 2189 | 2189 | 2320 | 2320 |
| recent_r11_r13 | 2527 | 2527 | 2687 | 2687 |
| combined_all_secondary | 2641 | 2641 | 2809 | 2809 |

## Governance Interpretation

- Combined all jest secondary; nie wolno wybrac candidate na podstawie samego combined.
- Candidate fails if direction differs between R11 and R13.
- Candidate fails if effect exists only in combined.
- Candidate fails if R13 standalone does not support it.
- Candidate fails if confidence interval crosses zero in either required split.
- Obecny truth-layer blokuje feature prototype, bo price path i `good_clean` sa juz dostepne, ale `good_executable=0` i brakuje execution proof dla market-good rows.

## Evidence Checkpoint

Najmocniejsze evidence za V3: Chainstack price path daje niezerowe `good_clean` w R11/R13 oraz stabilny, audytowalny temporal split dla market-quality targetu.

Najmocniejsze evidence przeciw V3 jako selector: `good_clean` nie jest `good_executable`; nie ma ani jednego BUY-quality targetu z realnym shadow entry/lifecycle/simulation proof.

Nierozstrzygniete niepewnosci: czy historyczne market-good rows byly realnie egzekwowalne, czy tylko wygladaja dobrze na post-decision price path; R13 ma pojedynczy dispatch fail-closed.

Mozliwe zrodla self-deception: uznanie v1 `+40` za clean good, traktowanie combined jako walidacji, ignorowanie CI crossing zero, ignorowanie braku dispatch proof, oraz przejscie do feature mining bez executable target.

## Next step

Nie przechodzic do Phase B feature prototype jako candidate work. Nastepny sensowny krok to P3.7.6 Execution Feasibility Resolution: rozstrzygnac `good_clean` vs `good_executable` na podstawie shadow entry/lifecycle/simulation evidence.
