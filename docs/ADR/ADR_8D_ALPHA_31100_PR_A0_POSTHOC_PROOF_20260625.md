# ADR-8D: alpha_31100 PR-A0 offline post-hoc proof

Data: 2026-06-25

Status: accepted - `REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0`

Zakres: wykonanie offline-only PR-A0 proofu dla `alpha_31100_score_pr_a0_diagnostic` na scalonym R47/31100 ledgerze.

## Problem

Po korekcie inventory R47 ma feature-bearing decision log, lifecycle outcomes i primary baseline `selector_shadow_score`. Nalezy sprawdzic, czy deterministic, allowlist-only, threshold-free alpha wnosi marginalna wartosc ponad obecny selector baseline bez zmian runtime.

## Decyzja

Wykonano offline-only proof i zapisano raport:

```text
PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_POSTHOC_PROOF_20260625.md
```

Proof:

- uzywa `scripts/zbiory.py 0.01 -0.01`,
- laczy `gatekeeper_v2_decisions.jsonl`, `shadow_lifecycle.jsonl`, `probe_shadow_lifecycle.jsonl`,
- uzywa `selector_shadow_score_v1.jsonl` jako primary baseline,
- liczy `alpha_31100_score_pr_a0_diagnostic` z jawnej allowlisty decision-time features,
- nie uzywa outcome/config/id/verdict/selector/confidence fields jako alpha inputu,
- nie zmienia runtime.

## Wynik

Final decision: `REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0`.

Standalone alpha nie pokazal wystarczajacej incremental value ponad `selector_shadow_score`.

Alpha zmniejsza StopLoss rate, ale glownie przez przesuniecie w TimeStop, przy nizszym Target rate i slabszym top-K PnL na konserwatywnych progach.

Combined diagnostic jest badawczo ciekawy, ale niestabilny. Nie tworzy runtime claim.

## Zmiany

- Dodano raport PR-A0 post-hoc proof.
- Dodano backlog follow-up dla TimeStop bucket analysis, combined diagnostic anomaly review i ewentualnego przyszlego constrained reranker research.
- Nie zmieniono kodu runtime.
- Nie dodano sidecara.
- Nie dodano schema JSON.
- Nie dodano ML/XGBoost.
- Nie zmieniono Gatekeeper/V2.5/V3.

## Non-goals

- Brak runtime alpha.
- Brak BUY/REJECT triggera.
- Brak zmian `v25_confidence`.
- Brak kopiowania progow z HTML.
- Brak schema freeze/master ledger.

## Walidacja

Do wykonania po zapisie:

```text
git diff --check -- PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_POSTHOC_PROOF_20260625.md docs/ADR/ADR_8D_ALPHA_31100_PR_A0_POSTHOC_PROOF_20260625.md
```

## Ryzyka

- Proof jest jednym runem R47; nie jest OOS validation.
- Rank normalization jest cohort-level, wiec nie jest runtime-ready scoring.
- Alpha polarity jest zdefiniowana konserwatywnie, ale wymaga family ablation.
- Combined diagnostic nie jest runtime proposal.
