# BACKLOG: alpha_31100 PR-A0 follow-ups

Data: 2026-06-25

Status: `RESEARCH_BACKLOG_ONLY / NO_RUNTIME_ACTION`

Parent decision: `REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0`

## Context

PR-A0 offline proof na R47/31100 odrzucil `alpha_31100_score_pr_a0_diagnostic` jako standalone reranker ponad `selector_shadow_score`.

Wniosek do zachowania:

```text
The diagnostic alpha reduces StopLoss rate, but does so by sacrificing Target rate
and increasing TimeStop share. It does not improve conservative top-K PnL versus
selector_shadow_score.
```

## Non-goals

- brak runtime alpha,
- brak sidecara,
- brak zmian Gatekeeper/V2.5/V3,
- brak zmian `v25_confidence`,
- brak schema freeze,
- brak ML/XGBoost bez osobnej akceptacji,
- brak kopiowania progow z HTML do bota.

## Backlog items

### 1. TimeStop bucket analysis

Cel: wyjasnic, dlaczego alpha standalone zamienia czesc StopLoss na TimeStop i czy TimeStop bucket zawiera jakikolwiek uzyteczny sub-sygnal.

Zakres tylko offline:

- porownac alpha top-K TimeStop rows vs selector top-K TimeStop rows,
- rozbic TimeStop po `final_pnl_pct` quantiles,
- sprawdzic, czy alpha TimeStop rows sa blisko breakeven czy po prostu slabymi kandydatami bez target traction,
- oddzielic BUY/BUY_EXTENDED od TIMEOUT_PHASE1_INSUFFICIENT w merged ledgerze,
- raportowac target preservation vs stop reduction.

Acceptance:

- brak runtime recommendation,
- wynik musi stwierdzic, czy TimeStop shift jest potencjalnym downside filterem czy tylko utrata targetow.

### 2. Combined diagnostic anomaly review

Cel: sprawdzic, dlaczego combined diagnostic wyglada lepiej w `top_1%` i `top_10%`, ale nie w `top_2%`/`top_5%`.

Zakres tylko offline:

- zbadac overlap i swapped rows dla combined vs `selector_shadow_score`,
- wykonac segment stability review,
- sprawdzic, czy wynik wynika z kilku outlierow,
- porownac valid-only alpha vs degraded alpha,
- nie traktowac combined jako runtime score.

Acceptance:

- combined pozostaje `diagnostic_only`,
- jakakolwiek promocja wymaga osobnego planu i kolejnego OOS/chronological proofu.

### 3. Future constrained reranker research

Status: only if explicitly approved.

Mozliwy kierunek: ML-based constrained reranker albo monotonic constrained model, ale tylko po osobnej akceptacji.

Minimalne warunki przed startem:

- chronological OOS,
- leakage/missingness audit,
- hard-negative suite,
- equal-count top-K EV,
- target preservation constraint,
- StopLoss reduction nie moze byc osiagana przez masowe przesuniecie w TimeStop,
- no F1 threshold tuning,
- no runtime integration.

Acceptance:

- osobny plan,
- osobny branch,
- osobny raport,
- no runtime changes until explicitly approved.
