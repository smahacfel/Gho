---
name: statistical-research-engine
description: "Operational statistical validation for selective low-latency trading systems: signal separability, robustness testing, regime awareness, anti-leakage validation, online stability analysis, and decision-time-safe scoring under non-stationary conditions."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Statistical Research Engine

Use this skill when the task involves:

* validating predictive signals
* testing feature separability
* evaluating scoring usefulness
* detecting overfitting or leakage
* analyzing non-stationary/adversarial data
* validating runtime-safe decision filters
* testing robustness under online constraints
* designing selective decision systems

Optimized for:

* selective trading runtimes
* bounded observation-window systems
* online signal validation
* adversarial/non-stationary environments
* low-latency decision systems

Not optimized for:

* academic ML experimentation
* leaderboard optimization
* black-box prediction systems
* offline-only research pipelines

---

# Quick Start

When activated:

> Validate whether the signal is real, stable, decision-time-safe, operationally useful, and robust under online runtime constraints. Prefer falsification over confirmation.

Preferred workflow:


→ define signal
→ check decision-time purity
→ falsify
→ stress-test
→ validate regimes
→ verify runtime feasibility
→ approve or reject


For deeper validation methodology, statistical tests, calibration, regime analysis, or leakage diagnostics, read `references.md`.

---

# Core Doctrine

Assume:

* most signals are noise
* most separability disappears out-of-sample
* most high-confidence predictions are miscalibrated
* most alpha decays after deployment
* adversarial markets adapt to exposed edges

Therefore:

* every signal must survive falsification
* operational usefulness matters more than statistical beauty
* stability matters more than peak metrics
* rejection of weak signals is success

---

# Decision-Time Purity

Every signal must define:

* what was known
* when it was known
* whether it was available at decision time
* whether computation fits runtime latency constraints

Reject signals that:

* use future information implicitly
* leak post-event state
* mix observation with outcome
* require unavailable runtime data
* cannot be computed within runtime constraints

Never use hindsight-enriched features in live decision modeling.

---

# Signal Validation Rules

A valid signal must have:

* explicit target
* explicit time relationship
* reproducible computation
* measurable separability
* stable online behavior

Minimum checks:

* baseline comparison
* permutation sanity check
* out-of-sample validation
* temporal stability check
* regime sensitivity check

Key rule:

If shuffled or permuted data produces similar performance, reject the signal.

---

# Separability & Utility

Test whether the signal separates outcomes:

* early enough
* stably enough
* robustly enough
* inside the live observation window

Ask:

* does separation remain stable over time?
* does it collapse outside one narrow regime?
* does it work under noisy or incomplete information?
* does it retain usefulness before terminal decision time?

Signals useful only in hindsight or late-stage windows are operationally weak.

---

# Online Stability

Assume deployed signals degrade.

Evaluate:

* regime drift
* alpha decay
* feedback-loop contamination
* crowding effects
* adversarial adaptation
* sensitivity to latency and freshness

Reject or downgrade signals that:

* collapse under small perturbations
* require unrealistically stable distributions
* degrade immediately after deployment
* depend on fragile microstructure assumptions

---

# Calibration & Confidence

Outputs must include:

* score
* confidence
* uncertainty
* validity flag
* regime classification

Rules:

* confidence must be calibrated
* uncertainty must be explicit
* high accuracy with poor calibration is unsafe
* opaque confidence generation is forbidden

---

# Runtime Feasibility

A statistically valid signal is still rejected if it:

* exceeds runtime latency budgets
* requires unstable data sources
* cannot operate under observation-window deadlines
* violates determinism or replay requirements
* depends on expensive recomputation

Operational usefulness > theoretical predictive power.

---

# Replay & Reproducibility

Research must support:

* deterministic replay
* reproducible splits
* fixed preprocessing
* dataset reconstruction
* snapshot/version traceability

Avoid:

* hidden preprocessing
* non-reproducible sampling
* mutable research datasets
* inconsistent train/test boundaries

---

# Failure Modes

Explicitly detect:

* data leakage
* lookahead bias
* survivorship bias
* overfitting
* spurious correlation
* non-stationarity blindness
* regime collapse
* hidden confounders
* calibration drift
* deployment-induced degradation
* hindsight contamination

If detected:

* reject the signal
  or
* materially downgrade confidence

---

# Selective Runtime Bias

Prefer:

* precision over recall
* stability over peak metrics
* explainability over complexity
* monotonic relationships over opaque interactions
* operational robustness over model sophistication

Avoid:

* giant opaque ensemble scoring
* fragile nonlinear interactions
* feature explosions without interpretability
* black-box scoring without reason-code compatibility

Weak but stable signals are preferable to unstable high-performing signals.

---

# FAST PATH RULE

If task is:

* localized
* feature-specific
* metric-specific
* non-architectural

Then:

* avoid unnecessary research expansion
* avoid full modeling pipelines
* validate only relevant assumptions
* preserve runtime constraints

Do not over-engineer simple validation tasks.

---

# Handoff Boundaries

Delegate instead of solving:

* system/runtime architecture → `trading-systems`
* Solana execution/runtime constraints → `solana-pumpfun-architect`
* low-level optimization → `rust-master`
* deep decomposition → `abstract-reasoning`
* large-scale data mining → `large-data-analytics`

If boundaries are unclear → stop and request clarification.

---

# Output Requirements

Outputs must:

* separate hypothesis, test, and conclusion
* quantify uncertainty explicitly
* report both strengths and weaknesses
* explain operational limitations
* disclose instability/regime sensitivity
* state why a signal should be rejected if weak

Never:

* present backtests as guarantees
* hide uncertainty
* hide failure cases
* rely on p-values alone
* over-interpret small samples
* use vague “promising signal” language

Prefer quantified statements:

* AUC
* lift
* calibration error
* effect size
* confidence intervals
* regime-specific performance

---

# Final Review Checklist

Before completion verify:

* decision-time purity preserved
* no leakage detected
* separability validated
* robustness tested
* regime sensitivity evaluated
* calibration verified
* runtime feasibility verified
* replayability preserved
* operational usefulness confirmed
* no hindsight contamination
* no unresolved instability

---

# Final Principle

Reject weak signals aggressively.
Operational robustness > statistical elegance.
Stable discrimination > peak backtest metrics.
Decision-time truth > hindsight performance.