# Raport P3.5 V3 Primary-Only Outcome Quality

Data: 2026-05-16
Status: `INSTRUMENTATION-READY / OUTCOME-DATA-PENDING`

## Zakres

Ten raport domyka pierwszy krok P3.5 po decyzji `ADR-0130`.

Dodano offline report:

```text
scripts/v3_outcome_quality_report.py
```

Raport nie zmienia runtime, scoringu, progow, V2/V2.5, IWIM, execution ani P2. Jego zadaniem jest
polaczyc V3 decision rows z outcome labels albo shadow lifecycle economics i odpowiedziec na pytanie:

```text
czy V3 pomogl, zaszkodzil, czy wynik jest nadal niekonkluzywny?
```

## Kontrakt P3.5

Raport klasyfikuje kazdy V3 row jako:

- `v3_helped_avoided_bad_entry` - V3 zablokowal wejscie, ktore label oznacza jako zle,
- `v3_hurt_blocked_good_entry` - V3 zablokowal wejscie, ktore label oznacza jako dobre,
- `v3_helped_selected_good_entry` - V3 dopuscil dobre wejscie,
- `v3_hurt_selected_bad_entry` - V3 dopuscil zle wejscie,
- `inconclusive` - brak labela/outcome albo brak jednoznacznego mappingu.

Jesli outcome/lifecycle labels nie istnieja, raport zwraca `p3_5_status=insufficient_outcome_data`.
To jest oczekiwane fail-closed zachowanie analityczne, a nie blad skryptu.

## Walidacja na r9 primary-only

Komenda:

```bash
python3 scripts/v3_outcome_quality_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml \
  --json
```

Wynik:

- `status=ok`
- `p3_5_status=insufficient_outcome_data`
- `v3_rows=28`
- `known_outcome_rows=0`
- `outcome_label_coverage=0.0`
- `effect_counts.inconclusive=28`
- `label_source_counts.missing=28`
- `avoided_bad_entries=0`
- `blocked_good_entries=0`
- `selected_good_entries=0`
- `selected_bad_entries=0`

Interpretacja:

R9 jest nadal dobrym dowodem full replay / real ablation, ale nie odpowiada jeszcze ekonomicznie, czy
V3 poprawia jakosc selekcji. Brakuje outcome labels albo lifecycle rows dla tych decyzji.

## Co zostalo osiagniete

- Powstal sponsor-readable quality layer: wynik ma liczyc pomogl/zaszkodzil/niekonkluzywne, a nie
  tylko hashe i replay status.
- Brak outcome labels jest teraz jawnie mierzony jako blocker P3.5.
- FSC nie jest wymagane do tej oceny; zgodnie z `ADR-0130` pozostaje de-scoped.
- Raport moze przyjac output z `scripts/gatekeeper_outcome_labeler.py` przez `--outcome-labels`.
- Raport moze przyjac `shadow_lifecycle.jsonl` przez `--shadow-lifecycle` albo odczytac sciezke z
  rollout configu.

## Werdykt

`NO-GO` dla P2.

`GO` dla kolejnego kroku P3.5: wygenerowac albo dolaczyc outcome labels dla r7/r9/r kolejnych
primary-only runow i ponownie uruchomic `v3_outcome_quality_report.py`.

## Weryfikacja

```bash
python3 -m unittest scripts/test_v3_outcome_quality_report.py -v
python3 -m py_compile scripts/v3_outcome_quality_report.py scripts/test_v3_outcome_quality_report.py
python3 scripts/v3_outcome_quality_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml --json
python3 scripts/v3_full_replay_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml --strict --json
```
