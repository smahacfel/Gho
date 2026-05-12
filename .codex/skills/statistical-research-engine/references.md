## `statistical-research-engine/references.md`

# Statistical Research Engine Reference

This file expands the `statistical-research-engine` skill. Read it only when deeper statistical validation, robustness testing, calibration, leakage analysis, regime analysis, or signal approval/rejection reasoning is needed.

Use this reference for:

* signal validation design
* separability testing
* leakage diagnostics
* out-of-sample validation
* walk-forward analysis
* robustness testing
* calibration analysis
* regime-dependent performance analysis
* operational decision-filter validation

Do not load this file for small localized metric checks unless needed.

---

# 1. Operating Assumptions

For selective low-latency trading systems:

* most apparent signals are noise
* most correlations are unstable
* most separability degrades out-of-sample
* most high-confidence outputs are miscalibrated
* most alpha decays after deployment
* adversarial markets adapt to exposed edges
* offline usefulness does not imply runtime usefulness
* statistical validity does not imply operational viability
* a signal unavailable at decision time is invalid for live decisions

The validation objective is not to prove that a signal works.

The objective is to determine whether the signal survives serious attempts to disprove it.

A rejected weak signal is a successful research outcome.

---

# 2. Signal Definition

A valid signal must explicitly define:

* input space `X`
* target `Y`
* prediction horizon
* observation window
* computation rule
* decision-time availability
* runtime cost
* expected monotonicity or relationship direction
* intended decision use

The signal must answer:

```text
Given what was known at decision time, does this feature improve selection quality?
````

Reject signals that:

* lack a clear target
* mix observation and outcome
* use post-event information
* cannot be computed in the live window
* require hidden external state
* are not reproducible under replay

---

# 3. Decision-Time Purity

Decision-time purity is mandatory.

For every feature, define:

* when the value becomes known
* which event or snapshot exposes it
* whether it is available before decision
* whether replay can reconstruct it
* whether any enrichment occurs after outcome

Common leakage sources:

* using final pool outcome inside early-window features
* using post-decision volume
* using post-migration data
* using final max price as a label-adjacent feature
* computing cohort statistics with future pools included
* joining with labels before feature generation
* using account state that arrived after the decision window
* reconstructing features from finalized data that was not available live

If a feature cannot prove decision-time availability, reject it or mark it invalid for live use.

---

# 4. Baselines

Every signal must beat appropriate baselines.

Useful baselines:

* random ranking
* majority-class prediction
* existing production heuristic
* simple threshold rule
* simple monotonic score
* previous config/version
* shuffled-label baseline
* shuffled-time baseline
* regime-only baseline

A complex signal is not useful if it only matches a simple baseline.

Report improvement over baseline using:

* absolute lift
* relative lift
* AUC delta
* precision delta
* false-positive reduction
* decision-quality improvement
* regime-specific improvement

---

# 5. Separability Testing

Separability asks whether the signal helps distinguish desirable from undesirable outcomes.

Test:

## Linear separability

Useful for:

* monotonic thresholds
* simple gating rules
* interpretable policy design

Methods:

* logistic regression
* linear discriminant analysis
* simple threshold scans
* monotonic binning

## Non-linear separability

Useful for:

* interactions
* curved boundaries
* conditional effects

Methods:

* shallow trees
* random forests
* gradient boosted trees
* kernel methods
* interaction scans

Use carefully. Non-linear separation is easier to overfit.

## Temporal separability

Useful for:

* observation-window systems
* sequence-aware features
* early-window filtering

Methods:

* windowed validation
* first-N transaction analysis
* first-N millisecond analysis
* first-N slot analysis
* sequence-shuffled baselines

## Regime-dependent separability

Useful for:

* market state adaptation
* congestion-aware behavior
* cohort-sensitive filters

Methods:

* stratified validation
* cluster/regime splits
* local model comparison
* regime interaction tests

If separability exists only in a narrow regime, mark the signal fragile unless that regime is detectable live.

---

# 6. Metrics

Choose metrics according to decision use.

For selective high-precision systems, prefer:

* precision at top-k
* precision above threshold
* lift over baseline
* false-positive rate
* false-negative rate
* expected value proxy
* calibration error
* decision coverage
* rejection quality
* regime-specific performance

Use AUC carefully.

AUC can be useful for ranking, but it may hide:

* poor top-tail precision
* bad calibration
* poor threshold behavior
* unstable regime performance
* unacceptable false-positive cost

Always report:

* sample size
* class balance
* decision threshold
* confidence interval if feasible
* baseline comparison
* regime breakdown if relevant

---

# 7. Robustness Testing

Stress-test signals with:

* subsampling
* bootstrapping
* noise injection
* feature perturbation
* time-shift testing
* window-shift testing
* label permutation
* degraded-input simulation
* duplicate suppression changes
* missingness injection

A signal is fragile if:

* small perturbations destroy performance
* performance depends on one narrow window
* performance depends on one time period
* performance depends on one cohort
* performance disappears after deduplication
* performance depends on unavailable or stale data

Fragile signals should be rejected or materially downgraded.

---

# 8. Out-of-Sample Validation

Single split validation is insufficient for temporal systems.

Use:

* train-past / test-future
* rolling windows
* walk-forward validation
* anchored walk-forward validation
* holdout by time
* holdout by regime
* holdout by cohort
* holdout by deployment period

Avoid:

* random train/test split for temporal decision systems unless justified
* leakage through preprocessing before splitting
* tuning thresholds on final holdout
* reporting best split only
* ignoring failed windows

Report:

* mean performance
* median performance
* worst-window performance
* degradation trend
* instability zones
* supported and unsupported regimes

---

# 9. Walk-Forward Procedure

Recommended procedure:

1. Sort data by decision-time timestamp.
2. Define training window.
3. Define next unseen test window.
4. Fit or tune only on training data.
5. Evaluate on next window.
6. Roll forward.
7. Repeat.
8. Report all windows, including failures.

Required outputs:

* number of windows
* train/test period per window
* class balance per window
* metric per window
* threshold per window if dynamic
* drift across windows
* failure periods
* final supported regime statement

A signal that only works in one cherry-picked window is not validated.

---

# 10. Regime Analysis

Determine whether performance depends on regime.

Possible regime dimensions:

* market activity
* volatility
* liquidity
* congestion
* slot timing
* funding-source behavior
* cohort behavior
* pool age
* observation-window phase
* signer diversity
* early buy pressure
* dev behavior

For each regime:

* report sample size
* report performance
* report confidence/uncertainty
* report degradation behavior
* check whether regime is detectable at decision time

Signals dependent on hidden or undetectable regimes are operationally unsafe.

---

# 11. Calibration

A score used for decisioning must have interpretable confidence.

Check:

* reliability curves
* calibration bins
* Brier score
* log loss
* expected calibration error
* observed frequency vs predicted probability
* calibration by regime
* calibration drift over time

Poorly calibrated signals are unsafe when used for:

* position sizing
* confidence thresholds
* risk gating
* adaptive policy
* comparing candidates across regimes

High ranking quality does not imply calibrated probability.

If calibration is poor:

* recalibrate
* downgrade confidence
* avoid probability semantics
* use rank/score semantics instead
* restrict to supported regimes

---

# 12. Thresholding

Thresholds must be justified by decision cost.

Define:

* false-positive cost
* false-negative cost
* expected value proxy
* minimum precision
* acceptable coverage
* supported regimes
* stale-input behavior

Avoid:

* arbitrary thresholds
* thresholds tuned on final holdout
* thresholds that maximize aggregate metric while failing live use
* thresholds that ignore transaction/execution cost
* thresholds that ignore sample-size uncertainty

For selective systems:

* fewer trades with higher precision is often preferable
* rejection quality matters
* marginal signals should be rejected
* borderline regimes should degrade safely

---

# 13. Runtime Feasibility

A statistically valid signal is rejected operationally if it:

* exceeds latency budget
* requires unavailable data
* requires unstable external state
* cannot run inside observation-window deadline
* requires expensive recomputation
* cannot be reproduced under replay
* violates deterministic scoring requirements
* cannot produce reason-code-compatible output

Runtime feasibility checks:

* decision-time availability
* incremental computability
* memory footprint
* CPU cost
* dependency reliability
* degraded-input behavior
* replay determinism
* observability

Operational usefulness beats theoretical predictive power.

---

# 14. Online Stability and Alpha Decay

Assume deployed signals degrade.

Evaluate:

* alpha decay
* signal crowding
* adversarial adaptation
* feedback-loop contamination
* regime drift
* threshold drift
* calibration drift
* execution-pressure decay

Questions:

* does performance degrade over time?
* does performance collapse after exposure?
* does the signal depend on fragile behavior?
* can manipulators mimic it?
* does deployment change the data distribution?

Signals with high decay risk require:

* shadow monitoring
* conservative rollout
* degradation triggers
* rollback conditions
* regime-limited usage

---

# 15. Causal Sanity Check

Do not assume causality from correlation.

Check:

* temporal precedence
* alternative explanations
* confounders
* proxy behavior
* invariance across regimes
* removal of correlated features
* stability under stratification
* mechanism plausibility

Classify signal as:

* likely causal
* proxy
* artifact
* leakage
* unknown mechanism

Unknown mechanism does not automatically reject a signal, but it increases required robustness.

---

# 16. Scoring Integration

When a signal is approved for scoring, output:

* score range
* interpretation
* monotonicity expectation
* validity conditions
* uncertainty/confidence
* supported regimes
* degraded-input behavior
* reason-code mapping
* failure modes
* calibration status

Avoid:

* opaque scores
* mixing risk appetite into signal strength
* mixing execution feasibility into predictive signal
* giant ensemble scores without explanation
* feature explosions that cannot be audited

A score should answer one question:

```text
How strong is the evidence for action under current known conditions?
```

---

# 17. Signal Approval Levels

## Rejected

Use when:

* leakage detected
* no stable separability
* poor out-of-sample behavior
* regime not detectable
* runtime infeasible
* poor replayability
* effect too small or unstable

## Provisional

Use when:

* initial signal exists
* no obvious leakage
* limited sample size
* regime sensitivity unresolved
* runtime feasibility likely but unproven
* needs more validation

## Validated

Use when:

* decision-time purity confirmed
* out-of-sample persistence shown
* robustness tested
* calibration or score semantics defined
* regime behavior known
* runtime feasibility confirmed
* failure modes documented

Only validated signals should be considered for policy integration.

---

# 18. Recommended Output Format

For signal validation, output:

```yaml
signal_name: string
hypothesis: string
target: string
observation_window: string
decision_time_available: true/false
runtime_feasible: true/false
sample_size: integer
class_balance: string
baseline: string
metrics:
  auc: float | unknown
  precision_at_threshold: float | unknown
  lift: float | unknown
  calibration_error: float | unknown
  effect_size: float | unknown
robustness:
  permutation_test: pass/fail/unknown
  temporal_split: pass/fail/unknown
  perturbation_test: pass/fail/unknown
  regime_stability: pass/fail/unknown
leakage_risks: list
supported_regimes: list
failure_modes: list
validation_status: rejected | provisional | validated
recommendation: string
```

Unknown fields must be marked explicitly.

Do not invent confidence.

---

# 19. Minimal Python Validation Pattern

Use only as a simple sanity pattern, not as a complete validation suite.

```python
import numpy as np
from sklearn.metrics import roc_auc_score
from sklearn.model_selection import TimeSeriesSplit


def evaluate_signal_time_series(X, y, model, n_splits=5, seed=42):
    rng = np.random.default_rng(seed)
    tscv = TimeSeriesSplit(n_splits=n_splits)

    results = []

    for train_idx, test_idx in tscv.split(X):
        X_train, X_test = X[train_idx], X[test_idx]
        y_train, y_test = y[train_idx], y[test_idx]

        model.fit(X_train, y_train)
        preds = model.predict_proba(X_test)[:, 1]

        auc = roc_auc_score(y_test, preds)

        y_perm = rng.permutation(y_test)
        auc_perm = roc_auc_score(y_perm, preds)

        results.append({
            "auc": float(auc),
            "auc_permuted": float(auc_perm),
            "signal_valid_window": bool(auc > auc_perm + 0.02),
            "n_test": int(len(test_idx)),
        })

    return results
```

For production research, add:

* confidence intervals
* calibration checks
* regime breakdown
* threshold-specific precision
* effect size
* failure-window reporting
* leakage diagnostics

---

# 20. Failure Modes

Explicitly detect and name:

* data leakage
* lookahead bias
* survivorship bias
* selection bias
* overfitting
* multiple testing without correction
* spurious correlation
* hidden confounder
* non-stationarity blindness
* regime collapse
* calibration drift
* alpha decay
* feedback-loop contamination
* hindsight contamination
* runtime-infeasible signal
* replay-inconsistent signal
* black-box score without reason-code compatibility
* threshold overfitting
* sample-size overinterpretation

If detected:

* reject the signal
  or
* materially downgrade confidence with explicit limitation

---

# 21. Review Checklist

Before approving a signal:

* signal target is explicit
* prediction horizon is explicit
* observation window is explicit
* decision-time purity confirmed
* leakage excluded
* baseline comparison performed
* permutation sanity check performed
* out-of-sample validation performed
* temporal stability evaluated
* robustness tested
* regime sensitivity evaluated
* calibration checked if confidence is used
* runtime feasibility confirmed
* replayability confirmed
* operational usefulness shown
* failure modes named
* uncertainty quantified
* recommendation is explicit

---

# 22. Final Principle

The system is not looking for beautiful models.

It is looking for signals that remain useful under:

* incomplete information
* noisy early windows
* adversarial behavior
* non-stationarity
* latency constraints
* replay requirements
* strict false-positive costs

Reject weak signals aggressively.
Operational robustness beats statistical elegance.
Stable discrimination beats peak backtest metrics.
Decision-time truth beats hindsight performance.