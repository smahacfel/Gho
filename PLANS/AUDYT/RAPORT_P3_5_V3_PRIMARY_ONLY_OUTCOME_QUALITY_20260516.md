# Raport P3.5 V3 Primary-Only Outcome Quality

Data: 2026-05-16
Status: `OUTCOME-QUALITY-READY / P2-NO-GO`

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
- `v3_neutral_no_target` - label jest poprawny, ale pool nie byl ani +40% targetem, ani rug/early-death,
- `inconclusive` - brak labela/outcome albo brak jednoznacznego mappingu.

Jesli outcome/lifecycle labels nie istnieja, raport zwraca `p3_5_status=insufficient_outcome_data`.
To jest oczekiwane fail-closed zachowanie analityczne, a nie blad skryptu.

Jawnie przekazane sciezki CLI, np. `--outcome-labels logs/...`, sa rozwiazywane wzgledem repo root,
a nie wzgledem katalogu rollout configu. To redukuje ryzyko cichego raportowania `missing=all` przy
poprawnie wygenerowanym pliku labeli.

## Walidacja na r9 primary-only

Komenda bez labeli:

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

Bez labeli R9 jest dobrym dowodem full replay / real ablation, ale nie odpowiada jeszcze ekonomicznie,
czy V3 poprawia jakosc selekcji. To byl stan poczatkowy przed dolaczeniem labeli.

## Walidacja na r9 z outcome labels

Outcome labels wygenerowano istniejaca sciezka `fetch_pool_price_at_30s.py` oraz
`scripts/gatekeeper_outcome_labeler.py` dla decision logu r9.

Wynik labelowania:

- `threshold_rows=28`
- `written=28`
- `label_valid=24`
- `hit_40_before_stop=2`
- `rug_or_early_death=5`
- `threshold_verdict.OK=2`
- `threshold_verdict.NOK=5`
- `threshold_verdict.NONTARGET=17`
- `unresolved=4` z powodu braku/nieprawidlowej entry price

Komenda raportu P3.5:

```bash
python3 scripts/v3_outcome_quality_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml \
  --outcome-labels logs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl \
  --json
```

Wynik:

- `status=ok`
- `p3_5_status=outcome_quality_ready`
- `v3_rows=28`
- `known_outcome_rows=24`
- `outcome_label_coverage=0.857143`
- `outcome_label_counts.bad_entry=5`
- `outcome_label_counts.good_entry=2`
- `outcome_label_counts.neutral_entry=17`
- `outcome_label_counts.unknown=4`
- `effect_counts.v3_helped_avoided_bad_entry=5`
- `effect_counts.v3_hurt_blocked_good_entry=2`
- `effect_counts.v3_neutral_no_target=17`
- `effect_counts.inconclusive=4`
- `selected_good_entries=0`
- `selected_bad_entries=0`

Rozbicie po V3 reason:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: 2 avoided bad, 1 blocked good, 14 neutral, 4 inconclusive.
- `PENDING_V3_WAIT_EVIDENCE`: 3 avoided bad, 1 blocked good, 3 neutral.

Interpretacja:

Na probce r9 V3 jest nadal konserwatywny i blokujacy. Dostarcza dodatnia wartosc ochronna na 5
zlych wejsciach, ale rownoczesnie blokuje 2 wejscia, ktore label +40% oznacza jako dobre. To nie
jest podstawa do P2. To jest natomiast pierwsza konkretna, sponsor-readable odpowiedz typu
pomogl/zaszkodzil/neutralne po de-scope FSC i po full replay.

`scripts/gatekeeper_40pct_validation.py` na tych samych labelach pokazuje `selected=0`, poniewaz
aktywny baseline V2/V2.5 w tej probce nie wykonal zadnego BUY. Dlatego raport P3.5 jest
porownaniem kontrfaktycznym V3 wobec outcome labeli, a nie potwierdzeniem precision aktywnych wejsc.

## Co zostalo osiagniete

- Powstal sponsor-readable quality layer: wynik liczy pomogl/zaszkodzil/neutralne/niekonkluzywne, a
  nie tylko hashe i replay status.
- Brak outcome labels jest jawnie mierzony jako blocker P3.5, a obecne r9 labels podnosza coverage
  do 24/28 rows.
- FSC nie jest wymagane do tej oceny; zgodnie z `ADR-0130` pozostaje de-scoped.
- Raport moze przyjac output z `scripts/gatekeeper_outcome_labeler.py` przez `--outcome-labels`.
- Raport moze przyjac `shadow_lifecycle.jsonl` przez `--shadow-lifecycle` albo odczytac sciezke z
  rollout configu.

## Werdykt

`NO-GO` dla P2.

`GO` dla kolejnego kroku P3.5: zwiekszyc probke primary-only i utrzymac ten sam pomiar jakościowy.
Obecne 28 rows / 24 znane outcome rows to za malo na ekonomiczna decyzje o promocji, ale wystarcza,
zeby uzasadnic dalsza iteracje jako mierzaca rzeczywista wartosc selekcji, a nie tylko plumbing.

## Weryfikacja

```bash
python3 -m unittest scripts/test_v3_outcome_quality_report.py -v
python3 -m py_compile scripts/v3_outcome_quality_report.py scripts/test_v3_outcome_quality_report.py
python3 scripts/v3_outcome_quality_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml --json
python3 scripts/v3_outcome_quality_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml --outcome-labels logs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl --json
python3 scripts/gatekeeper_40pct_validation.py --labels logs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl --output logs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only/decisions/p3_5_gatekeeper_plus40_validation.json --bootstrap 200 --permutations 200
python3 scripts/v3_full_replay_report.py --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml --strict --json
```
