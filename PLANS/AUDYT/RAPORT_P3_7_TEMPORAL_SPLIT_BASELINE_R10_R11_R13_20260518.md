# Raport P3.7 Temporal Split Baseline R10/R11/R13

Status: **TEMPORAL SPLIT COMPLETE / FEATURE PROTOTYPE BLOCKED**

## Executive summary

P3.7.5 raportuje R11 standalone, R13 standalone, recent-only R11/R13 oraz combined all jako widok pomocniczy. Ten raport nie dowodzi edge i nie autoryzuje P2, live, threshold tuning ani feature prototype.

- Temporal gate status: `blocked`
- Blockers: `['r11_has_no_good_clean_rows', 'r13_has_no_good_clean_rows', 'r11_has_no_good_executable_rows', 'r13_has_no_good_executable_rows', 'mfe_mae_unavailable_all_runs']`
- `do_not_train_on_R13_then_validate_on_R13`: `true`

## Required Views

| View | Rows | Good clean | Good dirty | Bad clean | Good executable | Price path available | Dispatch expected | Shadow observed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| r10 | 150 | 0 | 25 | 42 | 0 | 0.000000 | 0 | 0 |
| r11 | 447 | 0 | 91 | 84 | 0 | 0.000000 | 0 | 0 |
| r13 | 2733 | 0 | 536 | 556 | 0 | 0.000000 | 0 | 1 |
| recent_r11_r13 | 3180 | 0 | 627 | 640 | 0 | 0.000000 | 0 | 1 |
| combined_all_secondary | 3330 | 0 | 652 | 682 | 0 | 0.000000 | 0 | 1 |

## R11 vs R13 Drift

| Metric | R11 rate | R13 rate | R13-R11 | CI95 low | CI95 high | Crosses zero |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `good_clean` | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | True |
| `good_dirty` | 0.203579 | 0.196121 | -0.007458 | -0.047645 | 0.032729 | True |
| `bad_clean` | 0.187919 | 0.203439 | 0.015520 | -0.023714 | 0.054754 | True |
| `good_executable` | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | True |
| `execution_feasible` | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | True |
| `price_path_available` | 0.000000 | 0.000000 | 0.000000 | 0.000000 | 0.000000 | True |

## Numeric Path Availability

| View | MFE 10s n | MAE 10s n | Time to MFE n | Time to MAE n |
| --- | ---: | ---: | ---: | ---: |
| r10 | 0 | 0 | 0 | 0 |
| r11 | 0 | 0 | 0 | 0 |
| r13 | 0 | 0 | 0 | 0 |
| recent_r11_r13 | 0 | 0 | 0 | 0 |
| combined_all_secondary | 0 | 0 | 0 | 0 |

## Governance Interpretation

- Combined all jest secondary; nie wolno wybrac candidate na podstawie samego combined.
- Candidate fails if direction differs between R11 and R13.
- Candidate fails if effect exists only in combined.
- Candidate fails if R13 standalone does not support it.
- Candidate fails if confidence interval crosses zero in either required split.
- Obecny truth-layer blokuje feature prototype, bo `good_clean=0`, `good_executable=0` i MFE/MAE sa niedostepne na wszystkich wymaganych widokach.

## Evidence Checkpoint

Najmocniejsze evidence za V3: replay/outcome pipeline zachowuje stabilna, audytowalna semantyke across R11/R13 i nie promuje grubego labela v1 do `good_clean`.

Najmocniejsze evidence przeciw V3 jako selector: nie ma ani jednego `good_clean` ani `good_executable` w R11/R13, wiec nie istnieje temporalnie walidowalny BUY-quality target.

Nierozstrzygniete niepewnosci: czy brak price path/lifecycle jest tylko brakiem artefaktu, czy realnie oznacza niewykonalnosc; czy pelniejszy outcome v2 po danych sciezkowych zmieni rozklad.

Mozliwe zrodla self-deception: uznanie v1 `+40` za clean good, traktowanie combined jako walidacji, ignorowanie CI crossing zero, ignorowanie braku dispatch proof, oraz przejscie do feature mining bez executable target.

## Next step

Nie przechodzic do Phase B feature prototype jako candidate work. Nastepny sensowny krok w Phase A to uzupelnienie price path/lifecycle evidence albo jawne udokumentowanie, ze obecne artefakty nie pozwalaja zbudowac `good_clean` / `good_executable` targetu dla P3.7.
