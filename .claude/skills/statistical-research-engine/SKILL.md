---
name: statistical-research-engine
description: Research-grade statistical reasoning for signal discovery, separability validation, regime detection, causal sanity checks, and robust decision modeling under uncertainty and non-stationarity. Use when validating predictive signals, testing robustness out-of-sample, and designing high-risk decision filters.
allowed-tools: Read, Edit, Grep, Bash, Python
---

# Statistical Research Engine - Signal Validation and Decision Integrity

Use this skill when the task involves:
- discovering or validating predictive signals
- testing whether a feature has real separability power
- building scoring models under uncertainty
- analyzing noisy, non-stationary, or adversarial data
- designing robust filters for high-risk decision systems
- verifying whether a signal is causal, spurious, or unstable
- validating performance under strict out-of-sample conditions

## Operating Doctrine

This system assumes:
- most signals are noise
- most apparent patterns are unstable
- most separability disappears out-of-sample
- most high-confidence predictions are miscalibrated

Therefore:
Every signal must be **earned through falsification, not assumed through correlation**.

The agent must actively try to disprove usefulness before accepting it.

## Phase 1 - Signal Definition

A valid signal must be explicitly defined as:
- input space `X` (features)
- target `Y` (what is being predicted)
- mapping `f: X -> Y` or `X -> score`
- time relationship (`t` vs `t + delta`)
- data availability constraints (what is known at decision time)

The agent must reject any signal that:
- uses future information implicitly
- mixes observation and outcome
- is not computable in real-time constraints
- lacks a clearly defined target

## Phase 2 - Signal Detection

The agent must test whether signal exists at all.

Allowed techniques:
- correlation analysis (with lag structure)
- mutual information
- likelihood ratio tests
- simple baseline classifiers
- permutation tests (critical for sanity check)

Key rule:
If a shuffled version of the data produces similar performance, the signal is invalid.

## Phase 3 - Separability Analysis

The agent must determine whether classes or outcomes are separable.

Test levels:
1. linear separability
2. non-linear separability
3. probabilistic separability
4. temporal separability (sequence-dependent)

The agent must explicitly answer:
- Is separation stable?
- Is it consistent across time?
- Is it dependent on a narrow regime?

If separability exists only in a narrow slice, mark as fragile.

## Phase 4 - Robustness Testing

The agent must stress-test the signal:
- subsampling (remove random chunks)
- noise injection
- feature perturbation
- time-shift testing
- cross-window validation

If performance collapses under small perturbations, reject or downgrade the signal.

## Phase 5 - Regime Detection

The agent must identify whether performance depends on hidden regimes.

Techniques:
- clustering (feature space or outcome space)
- volatility segmentation
- change-point detection
- hidden state modeling

The agent must determine:
- does the signal work globally or only in regimes?
- can regime be detected in real time?

If regime cannot be detected, the signal is operationally unsafe.

## Phase 6 - Causal Sanity Check

The agent must test whether the signal is:
- causal
- proxy
- artifact
- leakage

Checks:
- temporal precedence
- invariance across environments
- removal test (does signal still work without correlated features?)
- alternative explanation testing

The agent must never assume causality from correlation.

## Phase 7 - Calibration

The agent must ensure outputs are calibrated:
- predicted probabilities vs actual outcomes
- reliability curves
- Brier score
- log-loss

A model with high accuracy but poor calibration is unsafe for decision systems.

## Phase 8 - Decision Thresholding

The agent must define:
- decision boundary
- expected value of decisions
- cost of false positives vs false negatives
- acceptable risk level

Threshold must be:
- data-driven
- stable
- justified under cost model

## Phase 9 - Walk-Forward Validation

Mandatory for temporal systems.

Procedure:
- train on past window
- test on next unseen window
- roll forward
- repeat

The agent must:
- report performance drift
- detect degradation
- identify instability zones

Single split validation is invalid.

## Phase 10 - Deployment Constraints

Before approving a signal, verify:
- latency feasibility
- data availability at decision time
- computational cost
- reproducibility
- determinism of output

If a signal cannot be computed within system constraints, it is rejected.

## Statistical Failure Modes (Must Detect)

- data leakage
- survivorship bias
- lookahead bias
- overfitting
- multiple testing without correction
- non-stationarity blindness
- spurious correlation
- hidden confounders
- regime collapse

## Minimal Research Pipeline (Reference Implementation)

```python
import numpy as np
from sklearn.metrics import roc_auc_score
from sklearn.model_selection import train_test_split


def evaluate_signal(X, y, model, seed=42):
    rng = np.random.default_rng(seed)
    X_train, X_test, y_train, y_test = train_test_split(
        X, y, shuffle=False
    )

    model.fit(X_train, y_train)
    preds = model.predict_proba(X_test)[:, 1]

    auc = roc_auc_score(y_test, preds)

    # permutation sanity check on labels
    y_perm = rng.permutation(y_test)
    auc_perm = roc_auc_score(y_perm, preds)

    return {
        "auc": float(auc),
        "auc_permuted": float(auc_perm),
        "signal_valid": bool(auc > auc_perm + 0.02),
    }
```

## Decision Engine Integration

The agent must output:
- score
- confidence
- uncertainty
- regime classification
- validity flag

Final decision must depend on:
- `score > threshold`
- `confidence > minimum`
- `signal_validity == true`
- `regime in supported_regimes`

## Output Standards

The agent must:
- clearly separate hypothesis, test, and conclusion
- quantify uncertainty explicitly
- avoid vague statistical language
- report both success and failure cases
- explain why a signal works or fails

## Project Bias

For this system:
- precision > recall
- stability > peak performance
- robustness > complexity
- explainability > black-box performance
- rejection of weak signals > forced utilization

A signal that is rejected correctly is more valuable than a signal that is incorrectly used.

## Output Expectations

When generating statistical code or analysis, the agent must produce:
- reproducible pipeline: seeded randomness, fixed splits, documented preprocessing
- explicit failure reporting: if a signal fails any phase, state why and at which phase
- no "promising results" language: use quantified statements (`AUC`, lift, p-value, effect size)
- no hiding behind p-values alone: report effect size, stability, and practical significance
- no over-interpretation of small samples: flag when sample size is insufficient
- confidence intervals for reported metrics
- regime breakdown if performance varies by regime

The agent must prefer:
- rejecting a weak signal over recommending it
- stating uncertainty over pretending confidence
- failing transparently over succeeding silently.