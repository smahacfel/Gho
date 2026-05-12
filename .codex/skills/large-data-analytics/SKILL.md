---
name: large-data-analytics
description: "Event-stream intelligence and runtime-feasible feature discovery for selective low-latency trading systems: temporal pattern mining, sequence analytics, anomaly detection, replay-safe dataset analysis, and promotion of candidate patterns into validated feature specifications."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Large Data Analytics

Use this skill when the task involves:

* discovering candidate features from large event streams
* analyzing transaction sequences or temporal behavior
* mining recurring patterns in noisy market data
* detecting bursts, anomalies, regime shifts, or structural breaks
* compressing raw event data into runtime-feasible features
* analyzing bounded observation windows
* preparing candidate features for statistical validation
* building replay-safe analytical pipelines

Optimized for:

* event-stream analytics
* selective trading systems
* bounded observation-window systems
* early-window feature discovery
* noisy/adversarial market data
* runtime-feasible feature generation

Not optimized for:

* generic BI dashboards
* offline-only data science workflows
* black-box embedding research
* academic clustering experiments
* execution-code integration without validation

---

# Quick Start

When activated:

> Discover candidate structure in noisy event streams, but treat every pattern as untrusted until it is stable, replayable, decision-time-safe, and feasible to compute online.

Preferred workflow:

```text
verify data integrity
→ normalize event streams
→ define observation window
→ generate candidate features
→ test temporal stability
→ check runtime feasibility
→ produce feature handoff specification
````

For deeper feature families, advanced tests, or ambiguous event-stream reasoning, read `references.md`.

---

# Core Doctrine

Large datasets are discovery surfaces, not proof.

Assume:

* most correlations are fragile
* scale amplifies noise as well as signal
* many patterns are sampling artifacts
* rare events are not automatically meaningful
* hindsight-enriched patterns are dangerous
* adversarial markets adapt to exposed structure

Therefore:

* discovery must be separated from validation
* operational usefulness matters more than novelty
* temporal stability matters more than aggregate strength
* runtime feasibility matters more than analytical elegance
* weak or non-replayable patterns must be rejected

---

# Event-Stream First Principle

For this project, data is primarily:

* event-driven
* timestamp-sensitive
* bursty
* duplicate-prone
* incomplete
* non-stationary
* partially ordered

Always reason in terms of:

* event order
* inter-arrival time
* slot-relative behavior
* sequence transitions
* burst formation
* early-window dynamics
* late-arriving events
* replay reconstruction

Do not treat event streams as independent tabular rows unless that assumption is explicitly justified.

---

# Data Quality Gate

Before analysis, verify:

* duplicate records
* missingness patterns
* timestamp integrity
* event ordering correctness
* schema consistency
* unit consistency
* stale values
* outlier contamination
* join-induced duplication
* survivorship or selection bias

Rules:

* do not silently drop problematic records
* do not repair ordering without documenting the rule
* do not merge datasets if time semantics are unclear
* do not infer missing values in a way that creates leakage

If time/order integrity fails, stop or mark results as provisional.

---

# Decision-Time Safety

Every discovered pattern must define:

* what data it uses
* when the data is available
* whether it exists at decision time
* whether it requires future/outcome information
* whether it can be computed inside the observation window

Reject features that:

* use future information
* depend on post-outcome enrichment
* require unavailable runtime data
* mix observation and outcome
* require expensive recomputation in hot paths

No hindsight-enriched feature may be promoted.

---

# Observation Window Analytics

Analyze features inside explicit windows:

* first N transactions
* first N milliseconds
* first N slots
* pre-decision window
* early / normal / extended observation phases

Measure:

* early discriminative power
* stability across window sizes
* sensitivity to delayed events
* degradation under incomplete inputs
* usefulness before terminal decision time

Signals useful only after the decision window are not operationally useful.

---

# Runtime-Feasible Feature Rules

Prefer features that are:

* available at decision time
* incrementally computable
* bounded in memory
* cheap in latency
* deterministic under replay
* interpretable enough for reason codes

Prefer:

* one-pass features
* counters
* ratios
* rolling windows
* bounded sequence summaries
* deterministic aggregations

Avoid:

* expensive global recomputation
* large unbounded joins
* non-deterministic clustering
* runtime-heavy embeddings
* features requiring full historical context

---

# Pattern Promotion Rules

A discovered pattern may be promoted only if it has:

* measurable effect size
* temporal persistence
* runtime availability
* low enough compute cost
* low redundancy with existing features
* interpretable semantics
* defined failure modes
* handoff specification

Do not embed discovered patterns directly into execution logic.

Discovery output must pass through:

```text
large-data-analytics
→ statistical-research-engine
→ trading-systems / domain integration
```

---

# Feature Handoff Specification

When promoting a candidate feature, output:

```yaml
feature_name: string
description: string
hypothesis: string
data_source: string
event_scope: string
observation_window: string
decision_time_available: true/false
online_computable: true/false
incremental_update: true/false
estimated_latency_cost: O(1) | O(log n) | O(window) | O(history) | external
estimated_memory_cost: constant | bounded_window | bounded_pool_count | unbounded | external
sample_size: integer | unknown
effect_size: float | unknown
expected_lift: float | unknown
stability_score: float | unknown
known_regimes: list
dependencies: list
failure_modes: list
leakage_risks: list
validation_status: exploratory | provisional | validated
recommended_next_step: string
```

If any field is unknown, mark it explicitly. Do not invent confidence.

---

# FAST PATH RULE

If task is:

* localized
* feature-specific
* metric-specific
* exploratory only
* non-architectural

Then:

* avoid full analytical expansion
* check only relevant assumptions
* preserve runtime feasibility constraints
* produce a focused result

Do not over-engineer simple exploration tasks.

---

# Handoff Boundaries

Delegate instead of solving:

* statistical validation → `statistical-research-engine`
* trading architecture → `trading-systems`
* Solana runtime constraints → `solana-pumpfun-architect`
* low-level Rust optimization → `rust-master`
* deep decomposition → `abstract-reasoning`

If boundaries are unclear → stop and request clarification.

---

# Output Requirements

Outputs must include:

* explicit feature definitions
* data-window assumptions
* time-ordering assumptions
* reproducible transformations
* separation of exploration vs validation
* runtime feasibility notes
* handoff specification for promoted patterns

Never use:

* vague “interesting pattern” language
* unquantified “strong correlation” claims
* hidden preprocessing
* hindsight-enriched features
* execution-ready recommendations without validation

---

# Failure Modes

Explicitly detect:

* spurious correlation
* data leakage
* lookahead bias
* survivorship bias
* join-induced duplication
* timestamp corruption
* order corruption
* non-stationarity blindness
* regime collapse
* alpha decay
* feature redundancy
* runtime-infeasible features
* replay-inconsistent features

If detected:

* reject the pattern
  or
* mark it provisional with explicit limitations

---

# Final Review Checklist

Before completion verify:

* data quality checked
* time ordering verified
* observation window defined
* decision-time availability verified
* leakage excluded
* temporal dependence respected
* runtime feasibility checked
* replayability considered
* pattern stability assessed
* failure modes named
* handoff specification complete if promoted
* no execution logic modified directly

---

# Final Principle

Discovery is not validation.
Correlation is not usefulness.
Scale is not truth.
Early-window utility matters most.
Runtime-feasible features beat elegant offline features.
Reject fragile patterns before they become expensive mistakes.