# ADR-0148: R21 Runtime Score Candidate Rejection

Status: Accepted
Typ: Model candidate rejection / forward-shadow gate
Data: 2026-06-09
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: `PR-P3M: analyze runtime score cross-run stability` (`6ce3527`)
Zakres: selector shadow score validation and candidate promotion decision only
Dotkniete moduly/pliki:
- `scripts/audit_selector_shadow_score_topk_drift.py`
- `scripts/analyze_selector_candidate_crossrun_stability.py`
- `scripts/test_selector_pipeline.py`
Powiazane runy/logi/raporty:
- `shadow-burnin-v3-selector-dataset-r21-shadow-score-flow-map-smoke`
- `selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final`
- `reports/selector/selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final/model_candidate_crossrun_stability_v1.json`
Poziom ryzyka: Low for runtime, because this ADR changes no runtime behavior; Medium for project planning, because it rejects the current promoted score candidate.

## Decyzja

`combined:simple_feature_score_v1` is rejected for forward-shadow promotion.

The runtime score plumbing is accepted as technically correct, but the model edge is not confirmed on the final R21 labeled validation scope. Therefore the score candidate must not be promoted to forward shadow burn-in, Gatekeeper tuning, threshold activation, or production use.

Canonical candidate status:

```text
candidate_id = combined:simple_feature_score_v1
status = REJECTED_FOR_FORWARD_SHADOW
reason = MODEL_EDGE_NOT_CONFIRMED_ON_R21
business_decision = DO_NOT_FORWARD_SHADOW_BURN_IN
```
## Evidence Summary

R21 technical runtime score validation passed:

```text
score coverage = 100%
runtime mapped features = 70 / 70
missing runtime mappings = 0
parity = PASS
leakage = PASS
unmatched score rows = 0
claim boundary violations = 0
```

R21 model edge did not pass:

```text
r2_resolved_rows = 2106
r2_positive_rows = 943
r2_negative_rows = 1163
base positive rate = 943 / 2106 = 44.78%

Runtime Top10 = 1 / 3 = 33.33%
Runtime Top25 = 7 / 15 = 46.67%
Runtime Top50 = 13 / 32 = 40.63%
```

The Top25 result is effectively base-rate, and Top50 is below base-rate. This is not enough evidence for a selector promotion path.

## Interpretation

This is not a runtime failure. The P3L/P3M pipeline did what it was designed to do:

1. Move an offline score candidate into runtime as shadow-only evidence.
2. Prove score emission coverage and runtime parity.
3. Validate the candidate on an independent labeled runtime scope.
4. Stop the candidate before burn-in when edge was not confirmed.

The correct conclusion is:

```text
Runtime score plumbing: PASS
Runtime score correctness/parity: PASS
R21 labeled validation: PASS as a test
Model edge: FAIL / NOT CONFIRMED
Forward shadow burn-in: NO-GO
```

## Non-Goals

This ADR does not:

- tune Gatekeeper thresholds
- change BUY / REJECT / TIMEOUT behavior
- activate selector score in runtime decisions
- change execution or send path
- change slippage, provider retry, or Custom error handling
- claim that all selector modeling is invalid

## Required Follow-Up

The next phase is offline redesign, not runtime burn-in.

Recommended next phase:

```text
P4A: Selector Model Redesign From Cross-Run Evidence
```

P4A should focus on:

- autopsy of high-score failed rows
- evidence sufficiency / score eligibility gates
- missing handling and feature direction checks
- cross-run candidate comparison on R19 and R21
- simple candidate redesign before any further runtime work

Any new candidate designed using R19 and R21 must be validated on a fresh independent run or a held-out split that was not used for candidate selection.

## Guardrail

Future reports must not describe `combined:simple_feature_score_v1` as forward-shadow ready based only on technical runtime readiness.

Reports should separate:

```text
technical_verdict
model_edge_verdict
business_decision
```

For R21, the correct split is:

```text
technical_verdict = FORWARD_SHADOW_TECHNICALLY_READY_WITH_FULL_RUNTIME_SCORE
model_edge_verdict = MODEL_EDGE_NOT_CONFIRMED_ON_RUNTIME_SCOPE
business_decision = DO_NOT_FORWARD_SHADOW_BURN_IN
```
