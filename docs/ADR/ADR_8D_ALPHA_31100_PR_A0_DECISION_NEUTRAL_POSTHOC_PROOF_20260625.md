# ADR-8D: alpha_31100 PR-A0 decision-neutral post-hoc proof inventory

Data: 2026-06-25

Status: accepted

Zakres: dokumentacyjny PR-A0 inventory i plan minimalnego decision-neutral post-hoc proofu dla `alpha_31100_score_pr_a0_diagnostic`.

Branch: `research/alpha-31100-validation-harness-v1`

## Problem

Po zamknieciu Gatekeeper PR1-PR4 `alpha_31100_candidate_v1` pozostaje kandydatem research/shadow-only. Najblizszy etap nie powinien budowac pelnego validation harnessu ani integrowac modelu z runtime.

Ryzyko: bez minimalnego inventory mozna przedwczesnie policzyc proof na niepelnej kohorcie, na discovery HTML, na danych bez outcome joinu albo na score, ktory nie ma realnej wartosci marginalnej ponad selector/Gatekeeper baseline.

## Decyzja

Utworzono dwa dokumenty PR-A0:

```text
PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_INVENTORY_20260625.md
PLANS/PLAN_ALPHA_31100_PR_A0_DECISION_NEUTRAL_POSTHOC_PROOF_20260625.md
```

Decyzja zakresowa:

- PR-A0 jest decision-neutral,
- score nazywa sie `alpha_31100_score_pr_a0_diagnostic`,
- score jest deterministic diagnostic reranker, nie finalny model,
- nie ma runtime alpha,
- nie ma sidecara,
- nie ma schema freeze/master ledger,
- nie ma treningu ML/XGBoost,
- nie ma zmian Gatekeeper/V2.5/V3.

## Uzasadnienie

R47 inventory potwierdzilo:

- konfiguracje `max_wait_time_ms = 31100`,
- `observation_end_ts_ms - observation_start_ts_ms = 31100` w `probe_selection` i `probe_skips`,
- dobra outcome coverage dla shadow BUY i probe shadow,
- istnienie event streamu R47.

Po korekcie source path inventory potwierdzilo feature-bearing R47/31100 ledger:

```text
logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/gatekeeper_v2_decisions.jsonl
```

Scalenie przez:

```text
scripts/zbiory.py 0.01 -0.01
```

potwierdzilo:

- `3948` merged rows,
- `A=387`, `B=3561`, `N=0`,
- `3948/3948` z `observation_window_ms=31100`,
- `3948/3948` z `final_pnl_pct`,
- `selector_soft_score` w `3948/3948`,
- joinowalny `selector_shadow_score` w `3948/3948`.

Dlatego finalny PR-A0 utility audit nie jest juz zablokowany brakiem danych. Pozostaje zablokowany zakresowo do osobnego kroku offline proofu, bez runtime zmian.

## Zmiany

- Dodano inventory report PR-A0 i skorygowano go o wlasciwy R47 feature-bearing source.
- Dodano plan/spec PR-A0 decision-neutral post-hoc proof i skorygowano go o potwierdzone wejscia.
- Nie zmieniono kodu runtime.
- Nie dodano schema JSON.
- Nie dodano modelu ani scoring hooka.
- Nie zmieniono konfiguracji.

## Non-goals

- Brak live enablement.
- Brak BUY/REJECT triggera.
- Brak integracji `alpha_31100` z `v25_confidence`.
- Brak sidecar JSONL.
- Brak DecisionLogger schema change.
- Brak kopiowania progow z HTML do runtime.
- Brak generowania `features_31100_v1_*.json`.

## Walidacja

Do wykonania po dodaniu dokumentow:

```text
git diff --check -- PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_INVENTORY_20260625.md PLANS/PLAN_ALPHA_31100_PR_A0_DECISION_NEUTRAL_POSTHOC_PROOF_20260625.md docs/ADR/ADR_8D_ALPHA_31100_PR_A0_DECISION_NEUTRAL_POSTHOC_PROOF_20260625.md
git status --short -- PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_INVENTORY_20260625.md PLANS/PLAN_ALPHA_31100_PR_A0_DECISION_NEUTRAL_POSTHOC_PROOF_20260625.md docs/ADR/ADR_8D_ALPHA_31100_PR_A0_DECISION_NEUTRAL_POSTHOC_PROOF_20260625.md
```

Uwaga: `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w aktualnym checkoutcie, wiec dokument zachowuje format zgodny z istniejacymi ADR-8D w `docs/ADR/`.

## Ryzyka

- Worktree zawiera unrelated dirty files; nie wolno ich stagingowac razem z PR-A0.
- `analiza/*` moze wygladac jak gotowy dowod, ale to discovery material, nie wystarczajacy joinowalny raw ledger.
- Odtwarzanie features z `datasets/events/R47` moze byc poprawna dalsza droga, ale byloby osobnym etapem i wymaga osobnego proofu replay-safety.
