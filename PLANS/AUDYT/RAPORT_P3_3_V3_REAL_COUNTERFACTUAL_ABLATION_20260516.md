# RAPORT P3.3 V3 REAL COUNTERFACTUAL ABLATION - 2026-05-16

## Status

**P3.3 narzedziowo uruchomione. Wynik: insufficient data, bez P2 promotion.**

Po P3.2 r7 repo ma replay-stable full payload. P3.3 wykorzystuje ten payload do
prawdziwego counterfactual recompute w Rust, zamiast dotychczasowego
`reason_group_proxy`.

## Zakres

Dodany tryb offline:

```bash
cargo run -q -p ghost-launcher --bin v3_replay -- \
  --input <gatekeeper_v2_decisions.jsonl> \
  --ablation-json \
  --strict
```

`scripts/v3_replay_ablation_report.py` uzywa tego trybu automatycznie, gdy
wykryje `replay.status=full`.

Non-goals zachowane:

- brak P2 promotion
- brak zmian scoringu V3
- brak zmian progow
- brak zmian active V2/V2.5
- brak zmian IWIM
- brak zmian execution/live sender
- brak runtime activation V3

## Kontrakt P3.3

P3.3 wykonuje:

1. walidacje full replay parity przez Rust validator,
2. deserializacje `v3_materialized_feature_snapshot` jako `MaterializedFeatureSet`,
3. dekodowanie kanonicznego `v3_policy_config_payload`,
4. odtworzenie baseline V3 przez `evaluate_v3_from_features`,
5. wykonanie wariantow kontrfaktycznych na tej samej migawce,
6. policzenie zmian verdict/stage/reason/confidence.

Jesli full replay nie przechodzi, P3.3 fail-closed i nie wykonuje ablation.

## Warianty ablation

Obecny zakres wariantow:

- `no_manipulation_contradiction`
- `no_organic_broadening`
- `no_sybil_fsc_cpv_caps`
- `no_alpha_cap`
- `no_execution_cap`

Interpretacja:

- `no_manipulation_contradiction` neutralizuje materialized manipulation
  contradiction evidence i sprawdza, czy hard-risk REJECT byl decydujacy.
- `no_organic_broadening` neutralizuje organic broadening signal.
- `no_sybil_fsc_cpv_caps` usuwa wymaganie evidence dla sybil/fsc/cpv.
- `no_alpha_cap` usuwa wymaganie evidence dla alpha.
- `no_execution_cap` podnosi cap `execution_not_run` do `1.0`.

To sa warianty offline. Nie zmieniaja runtime configu ani aktywnej polityki.

## Wynik na P3.2 r7

Polecenie:

```bash
python3 scripts/v3_replay_ablation_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r7.toml \
  --json
```

Wynik replay:

- `status=ok`
- `replay.status=full`
- `ablation.mode=full_replay_counterfactual`
- `ablation.replay_status=full_replay_ok`
- `ablation.baseline_status_counts.full_replay_ok=20`
- `v3_rows=20`

Certification:

- `p3_status=insufficient_data`
- `no_p2_promotion=true`
- `blocked_gates=[]`
- `insufficient_evidence_gates`:
  - `dominant_manipulation_contradiction_requires_more_evidence`
  - `low_outcome_label_coverage`

## Counterfactual result

Baseline V3 distribution:

- `REJECT`: `14`
- `PENDING`: `6`

Najwazniejszy wariant:

```text
no_manipulation_contradiction:
  changed_verdict_rows=14/20
  changed_stage_rows=14/20
  changed_reason_rows=14/20
  verdict_distribution:
    PENDING=20
  reason_distribution:
    PENDING_V3_WAIT_EVIDENCE=12
    PENDING_V3_WAIT_SAMPLE=8
```

Pozostale warianty na probce r7:

```text
no_alpha_cap:
  changed_verdict_rows=0

no_execution_cap:
  changed_verdict_rows=0

no_organic_broadening:
  changed_verdict_rows=0

no_sybil_fsc_cpv_caps:
  changed_verdict_rows=0
```

## Interpretacja

P3.3 potwierdza, ze w probce r7 dominujacy bucket
`REJECT_V3_MANIPULATION_CONTRADICTION` jest decyzyjnie sprawczy: jego usuniecie
zmienia 14/20 V3 rows z `REJECT` na `PENDING`.

To nie oznacza jeszcze, ze bucket jest ekonomicznie poprawny. Oznacza, ze:

- V3 nie odrzuca tych 14 rows przez przypadkowy efekt raportowania,
- rejection zalezy realnie od materialized manipulation contradiction evidence,
- bez tego komponentu V3 nie promuje wierszy do BUY, tylko cofa je do stanu
  oczekiwania na evidence/sample.

## Ograniczenia

P3.3 r7 nadal nie wystarcza do P2:

- probka ma tylko `20` V3 rows,
- `outcome_label_coverage=0.0`,
- wszystkie confidence buckets sa w praktyce `0`,
- brak multi-run stability dla real ablation,
- brak porownania z outcome economics,
- manipulation contradiction nadal wymaga rozbicia subtypow i szerszej probki.

## Decyzja operacyjna

**Nie uruchamiac P2.**

P3.3 przesuwa projekt z pytania:

```text
czy potrafimy odtworzyc V3 z logu?
```

na pytanie:

```text
czy konkretny komponent V3 rzeczywiscie zmienia decyzje i czy te zmiany sa
ekonomicznie uzasadnione?
```

Na r7 odpowiedz techniczna brzmi: tak, manipulation contradiction realnie zmienia
decyzje. Odpowiedz ekonomiczna pozostaje pending, bo brak outcome labels.

## Nastepny krok

P3.4 powinien wykonac multi-run real ablation:

1. uzyc co najmniej r7 oraz kolejnego swiezego namespace,
2. zebrac wieksza probke full replay rows,
3. porownac stabilnosc `no_manipulation_contradiction`,
4. dolaczyc outcome/lifecycle labels, jesli dostepne,
5. dopiero potem przygotowac ADR o dalszym losie manipulation contradiction.
