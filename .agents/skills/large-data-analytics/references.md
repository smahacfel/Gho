## `large-data-analytics/references.md`

# Large Data Analytics Reference

This file expands the `large-data-analytics` skill. Read it only when deeper analytical guidance is needed.

Use this reference for:

* feature-family selection
* event-stream reasoning
* observation-window design
* runtime-feasibility analysis
* sequence analytics
* anomaly classification
* feature handoff preparation

Do not load this file for small localized metric checks unless needed.

---

# 1. Operating Assumptions

For selective low-latency trading systems:

* most apparent correlations are fragile
* early market data is noisy and adversarial
* scale amplifies noise as well as signal
* rare events are not automatically useful
* repeated events are not automatically predictive
* visible alpha decays after exposure
* manipulators can mimic apparently good patterns
* features are useless if unavailable at decision time
* features are unsafe if they cannot be reconstructed under replay

The goal is to find candidate structure that is:

* stable
* measurable
* decision-time-safe
* online-computable
* replayable
* interpretable
* worthy of statistical validation

---

# 2. Event-Stream Analytics Model

Event-stream data should be analyzed as ordered or partially ordered sequences.

Important dimensions:

* event type
* event order
* event timestamp
* slot number
* transaction index
* signer identity
* direction
* amount
* price/reserve state
* account state freshness
* ingestion timestamp
* processing timestamp
* source/provider

Core questions:

* What happened first?
* What repeated?
* What accelerated?
* What clustered?
* What changed regime?
* What degraded under delay?
* What was known at decision time?
* Can this be reconstructed during replay?

---

# 3. Timestamp and Ordering Discipline

Separate timestamp domains:

* event time
* slot time
* chain time
* ingestion time
* processing time
* wall-clock time

Rules:

* never mix timestamp domains silently
* never infer causality from processing order alone
* never use post-event timestamps as decision-time facts
* document ordering repair rules
* classify late-arriving events
* preserve deterministic replay ordering

Common ordering failures:

* duplicate delivery
* delayed provider stream
* out-of-order account updates
* transactions arriving before pool metadata
* wall-clock skew
* event-time fallback hiding source inconsistency
* replay sorting that differs from live ordering

If ordering assumptions are unstable, results must be marked provisional.

---

# 4. Data Quality Checks

Before feature discovery, check:

## Completeness

* missing fields
* missing event types
* missing windows
* partial sessions
* dropped stream segments

## Duplication

* duplicate transaction signatures
* duplicate event IDs
* duplicate joins
* duplicate account updates
* repeated replay ingestion

## Consistency

* schema drift
* unit mismatch
* token decimal mismatch
* reserve normalization mismatch
* timestamp domain mismatch
* inconsistent signer identifiers

## Bias

* survivorship bias
* selection bias
* only analyzing successful pools
* excluding timeouts/rejects
* missing failed transactions
* post-hoc filtering by outcome

## Integrity

* corrupted ordering
* impossible values
* negative amounts
* invalid price movement
* inconsistent buy/sell labels
* stale account state

Do not silently drop records unless the exclusion rule is explicit and replayable.

---

# 5. Observation Window Types

## Transaction-count windows

Examples:

* first 5 tx
* first 10 tx
* first 20 tx
* first 50 tx

Useful for:

* early signer diversity
* buy/sell run behavior
* repeated signer activity
* early volume concentration

## Time windows

Examples:

* first 500 ms
* first 2 seconds
* first 5 seconds
* first 10 seconds

Useful for:

* burst intensity
* event velocity
* early demand pressure
* bot-like timing signatures

## Slot windows

Examples:

* block 0
* first 1 slot
* first 3 slots
* first 5 slots

Useful for:

* slot-local clustering
* early block dominance
* slot-relative execution behavior

## Decision windows

Examples:

* pre-Gatekeeper deadline
* early / normal / extended observation phases
* pre-execution snapshot window

Useful for:

* runtime-safe feature materialization
* terminal decision support
* replay-compatible feature extraction

A feature that only works outside the live decision window is not operationally useful.

---

# 6. Candidate Feature Families

## Magnitude Features

Measure size or intensity.

Examples:

* total volume
* buy volume
* sell volume
* volume delta
* average transaction size
* max transaction size
* market-cap movement
* reserve movement
* price movement
* liquidity depth proxy

Useful for:

* demand estimation
* liquidity sanity checks
* early pressure detection

Risks:

* whale dominance
* manipulation by large single orders
* sensitivity to outliers
* poor normalization

---

## Shape Features

Measure distributional structure.

Examples:

* concentration
* entropy
* dispersion
* skew
* kurtosis
* Gini
* HHI
* top-N dominance
* burstiness
* asymmetry

Useful for:

* sybil/bundling detection
* organic vs concentrated behavior
* signer diversity analysis

Risks:

* small sample instability
* regime dependency
* high sensitivity to early outliers

---

## Sequence Features

Measure order-sensitive behavior.

Examples:

* buy/sell run length
* inter-arrival time
* transition counts
* motif recurrence
* signer repetition pattern
* repeated size pattern
* slot-local clustering
* early flipper presence
* dev-first-buyer relationship

Useful for:

* bot-like behavior
* synthetic demand detection
* organic sequence detection
* early manipulation signals

Risks:

* ordering corruption
* duplicate events
* provider delay
* late-arriving transactions

---

## Stability Features

Measure feature trustworthiness.

Examples:

* rolling variance
* feature persistence
* missingness rate
* perturbation sensitivity
* window sensitivity
* confidence interval width
* degradation under stale inputs
* replay variance

Useful for:

* confidence scoring
* feature rejection
* degraded-mode behavior

Risks:

* expensive computation
* unstable in tiny samples
* misleading if window definition changes

---

## Context Features

Measure environment or cohort state.

Examples:

* slot congestion
* provider lag
* funding-source behavior
* cross-pool signer recurrence
* cohort-level demand
* local regime class
* recent pool success/failure profile

Useful for:

* regime-aware scoring
* false-positive reduction
* context-sensitive filtering

Risks:

* leakage through future cohort data
* hidden dependencies
* expensive joins
* non-replayable external state

---

# 7. Correlation and Dependence Discovery

Candidate methods:

* Pearson correlation
* Spearman correlation
* Kendall rank dependence
* lagged correlation
* conditional correlation
* mutual information
* stratified dependence
* windowed dependence
* permutation tests

Use correlations only as discovery signals.

Required checks:

* Does dependence survive temporal split?
* Does dependence survive stratification?
* Does dependence survive permutation sanity checks?
* Does dependence survive window shifts?
* Does dependence remain useful in the live observation window?
* Is it redundant with an existing feature?

Reject dependence if:

* it vanishes out-of-sample
* it appears only after hindsight enrichment
* it requires unavailable data
* it depends on a hidden regime
* it is non-actionable inside runtime constraints

---

# 8. Sequence and Temporal Tests

For event streams, test:

## Inter-arrival dynamics

* mean interval
* median interval
* interval coefficient of variation
* burst ratio
* same-millisecond ratio
* slot-local clustering

## Run dynamics

* consecutive buys
* consecutive sells
* buy/sell alternation
* early sell pressure
* dev activity sequence

## Transition dynamics

* buy → buy
* buy → sell
* sell → buy
* signer repeat → buy
* whale buy → cluster response

## Motif dynamics

* repeated size pattern
* repeated signer pattern
* repeated timing pattern
* repeated funding-source pattern

Validation checks:

* shuffled order baseline
* time-shift baseline
* duplicate-suppressed baseline
* degraded-event baseline
* cross-window stability

If shuffled ordering performs similarly, the sequence feature is invalid.

---

# 9. Anomaly and Burst Detection

An anomaly detector must define:

* baseline population
* deviation metric
* threshold
* observation window
* false-positive cost
* replay behavior

Anomaly types:

* point anomaly
* contextual anomaly
* collective anomaly
* burst anomaly
* structural anomaly
* distributional shift

Rules:

* rarity alone is not significance
* incomplete data can imitate anomalies
* burst does not imply opportunity
* anomaly must be decision-useful
* detector output must be replay-stable

Useful anomaly candidates:

* abnormal same-slot concentration
* abnormal signer reuse
* abnormal buy-size uniformity
* abnormal fee topology similarity
* abnormal early sell pressure
* abnormal cross-pool signer velocity

---

# 10. Runtime Feasibility Scoring

Before promotion, score each feature on:

## Online computability

Good:

* counters
* bounded rolling windows
* simple ratios
* fixed-size histograms

Bad:

* full-dataset recomputation
* unbounded joins
* global clustering
* large embeddings

## Latency cost

Classify:

* O(1)
* O(log n)
* O(window)
* O(history)
* external dependency

## Memory cost

Classify:

* constant
* bounded by observation window
* bounded by pool count
* unbounded
* external storage required

## Replay determinism

Check:

* ordering rule
* timestamp source
* rounding behavior
* missing-data behavior
* dependency versioning

## Failure behavior

Define what happens when:

* data is missing
* data is stale
* sequence is partial
* duplicates appear
* provider state diverges

---

# 11. Adversarial Market Awareness

In pump.fun-style early token markets:

* manipulators can imitate organic buying
* bots can manufacture diversity
* visible patterns decay quickly
* early demand may be artificial
* liquidity state can change faster than analysis windows
* crowding can destroy exposed edges

Evaluate every pattern for:

* manipulation sensitivity
* sybil mimicry risk
* alpha decay risk
* regime fragility
* deployment-induced degradation
* false-positive cost

Prefer:

* robust repeated structure
* cheap early discriminators
* interpretable concentration/sequence features
* features that degrade safely

Avoid:

* fragile nonlinear interactions
* flashy but rare patterns
* unexplainable embeddings
* features requiring perfect data

---

# 12. Pattern Promotion Workflow

## Exploratory

Pattern observed but not validated.

Allowed output:

* candidate hypothesis
* supporting examples
* required tests
* known limitations

Not allowed:

* execution integration
* threshold recommendation
* production claim

## Provisional

Pattern survived basic sanity checks.

Required:

* effect size
* sample size
* temporal split
* leakage check
* runtime feasibility estimate

Not allowed:

* live gating without statistical validation

## Validated

Pattern passed statistical validation and runtime feasibility checks.

Required:

* out-of-sample persistence
* regime breakdown
* calibration or stability estimate
* failure modes
* integration plan

Only validated patterns may be considered for scoring or policy integration.

---

# 13. Feature Handoff Specification

Use this structure when promoting any candidate feature:

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
sample_size: integer
effect_size: float | unknown
expected_lift: float | unknown
stability_score: float | unknown
known_regimes: list
dependencies: list
failure_modes: list
leakage_risks: list
replay_requirements: list
validation_status: exploratory | provisional | validated
recommended_next_step: string
````

Unknown values must be marked explicitly.

Do not invent confidence.

---

# 14. Recommended Analytical Outputs

When reporting findings, include:

* dataset scope
* time range
* event count
* pool/token count
* missingness summary
* duplicate summary
* ordering assumptions
* observation window definition
* feature computation rule
* effect size
* stability notes
* runtime feasibility notes
* leakage risks
* recommended next step

Avoid:

* “interesting”
* “promising”
* “strong signal”
* “clearly predictive”
* “looks good”

Use quantified language instead.

---

# 15. Failure Modes

Explicitly detect and name:

* spurious correlation
* data leakage
* lookahead bias
* survivorship bias
* selection bias
* join-induced duplication
* timestamp corruption
* ordering corruption
* schema drift
* unit mismatch
* stale-state contamination
* non-stationarity blindness
* regime collapse
* feature redundancy
* alpha decay
* runtime infeasibility
* replay inconsistency
* hidden external dependency

If detected:

* reject the pattern
  or
* mark it provisional with explicit limitations

---

# 16. Final Review Checklist

Before finalizing analysis:

* data quality checked
* duplicates checked
* missingness checked
* timestamp domains separated
* ordering rule documented
* observation window defined
* decision-time availability verified
* leakage excluded
* temporal dependence respected
* runtime feasibility estimated
* replayability considered
* pattern stability assessed
* adversarial risk considered
* failure modes named
* handoff specification complete if promoted
* no execution logic modified directly

---

# 17. Final Principle

Discovery is not validation.
Correlation is not usefulness.
Scale is not truth.
Rarity is not signal.
Early-window utility matters most.
Replayability is mandatory.
Runtime-feasible features beat elegant offline features.
Reject fragile patterns before they become expensive mistakes.