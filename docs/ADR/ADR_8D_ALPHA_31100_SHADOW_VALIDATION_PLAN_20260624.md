# ADR-8D: alpha_31100_candidate_v1 shadow validation plan

Data: 2026-06-24

Status: accepted

Zakres: utworzenie formalnego planu validation harness dla `alpha_31100_candidate_v1`

Branch: `research/alpha-31100-validation-harness-v1`

Start commit: `4d6208e gatekeeper: tighten policy availability and confidence contracts`

## Problem

Po zamknieciu Gatekeeper PR1-PR4 fundamenty runtime contracts sa uporzadkowane, ale kandydacki sygnal `alpha_31100_candidate_v1` nadal ma status badawczy.

Istnieje ryzyko, ze bez osobnego validation harness sygnal zostanie przedwczesnie potraktowany jako runtime input albo bedzie oceniany na zbyt miekkich metrykach, takich jak random split, balanced PR-AUC lub F1.

## Decyzja

Utworzyc osobny branch:

```text
research/alpha-31100-validation-harness-v1
```

oraz formalny plan:

```text
PLANS/PLAN_ALPHA_31100_CANDIDATE_V1_SHADOW_VALIDATION_20260624.md
```

Plan rozwija istniejacy backlog:

```text
PLANS/BACKLOG_ALPHA_31100_CANDIDATE_V1_VALIDATION_HARNESS_20260624.md
```

Plan utrzymuje status:

- `RESEARCH`,
- `SHADOW-ONLY`,
- `NO RUNTIME DECISION`,
- `NO LIVE DEPLOY`,
- `NO BUY TRIGGER`.

## Uzasadnienie

Kandydat `alpha_31100_candidate_v1` wymaga falsyfikacji, nie integracji.

Najpierw trzeba zamrozic schema, zbudowac master ledger, sprawdzic chronological OOS, ablation, leakage/missingness sentinels i top-k EV po kosztach. Dopiero po przejsciu tych gates mozna przygotowac shadow-only logging spec.

## Zmiany

- Dodano plan:
  - `PLANS/PLAN_ALPHA_31100_CANDIDATE_V1_SHADOW_VALIDATION_20260624.md`
- Utworzono branch z commita `4d6208e`.
- Nie dodano zadnych zmian runtime.
- Nie utworzono jeszcze schema JSON ani master ledger.

## Non-goals

- Brak zmian w Gatekeeper policy.
- Brak zmian progow.
- Brak live enablement.
- Brak BUY/REJECT hooka.
- Brak modelu XGBoost/LightGBM w runtime.
- Brak zmian `v25_confidence`.
- Brak zmian DecisionLogger runtime schema.

## Walidacja

Do wykonania po dodaniu planu:

- `git diff --check -- PLANS/PLAN_ALPHA_31100_CANDIDATE_V1_SHADOW_VALIDATION_20260624.md docs/ADR/ADR_8D_ALPHA_31100_SHADOW_VALIDATION_PLAN_20260624.md`
- scoped status musi pokazac tylko plan i ten ADR jako nowe pliki w zakresie aktualnego zadania.

## Ryzyka

- Worktree ma unrelated dirty files z poprzednich lokalnych artefaktow. Nie wolno ich stagingowac razem z tym planem.
- Plan dotyczy research harness. Kazda pozniejsza integracja z runtime wymaga osobnego planu i osobnego review.
