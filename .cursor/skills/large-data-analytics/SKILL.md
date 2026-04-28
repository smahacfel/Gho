---
name: large-data-analytics
description: Large-scale data analysis, hidden pattern discovery, correlation mining, sequence and time-series analytics, anomaly detection, scalable feature engineering, and robust exploration of massive, noisy datasets. Use when tasks involve high-volume data processing, latent structure discovery, temporal dependence analysis, or promoting candidate patterns into validated feature specifications.
allowed-tools: Read, Edit, Grep, Bash, Python
---

# Large Data Analytics - Pattern Discovery & Correlation Intelligence

Use this skill when the task involves:
- processing large datasets at high volume and high velocity
- discovering non-obvious structure, latent regimes, or recurring market templates
- mining correlations, co-movements, temporal dependencies, and event-conditioned behavior
- analyzing time series, event streams, transaction sequences, or order-flow dynamics
- detecting anomalies, outliers, bursts, and structural breaks
- building feature sets for downstream scoring, filtering, or predictive models
- compressing raw data into interpretable, stable, reusable representations

## Operating Doctrine

This skill treats large datasets as a discovery surface, not as proof.

The agent must assume that:
- most apparent correlations are fragile
- many discovered patterns are sampling artifacts
- scale amplifies noise as well as signal
- a pattern that appears often may still be useless for decisioning
- temporal dependence matters more than naive aggregate statistics

The objective is not to find "interesting" structure. The objective is to find structure that is stable, measurable, and operationally useful.

---

## Core Analytical Domains

### 1) Correlation mining
The agent must understand and distinguish:
- Pearson correlation for linear dependence
- Spearman correlation for monotonic dependence
- Kendall-style rank dependence where robustness matters
- partial correlation when confounding variables exist
- mutual information for non-linear dependence
- lagged correlation for temporal delay structure
- conditional correlation within regimes or subpopulations

The agent must never treat correlation as causation. High correlation is only a candidate signal, not a conclusion.

### 2) Temporal structure
The agent must be fluent in:
- autocorrelation and partial autocorrelation
- rolling-window statistics
- seasonality and periodic effects
- lag selection
- change-point and regime-transition behavior
- event clustering and burst dynamics
- lead/lag asymmetry between variables

The agent must explicitly test whether a signal survives time shifting, window shifting, and out-of-sample evaluation.

### 3) Pattern discovery
The agent must understand:
- association rules for co-occurrence structure
- sequence mining for ordered event patterns
- frequent itemset discovery
- clustering of behavior states
- motif discovery in time series
- latent template extraction from repeated outcomes

Pattern discovery must always be separated from validation. Discovery alone is not evidence.

### 4) Anomaly and deviation analysis
The agent must understand:
- point anomalies
- contextual anomalies
- collective anomalies
- burst anomalies
- structural anomalies
- distributional shift

The agent must distinguish a rare event from a meaningful anomaly. Rarity alone is not significance.

### 5) Dimensionality and representation learning
The agent must understand:
- PCA for variance compression and orthogonal structure
- UMAP and t-SNE for exploratory visualization
- autoencoders and learned embeddings
- sparse versus dense feature representations
- redundancy reduction and manifold structure

Reduced-dimensional views are exploratory tools. They are not substitutes for validation.

---

## Data Quality Rules

Before any analysis, the agent must assess:

- missingness patterns
- duplicate records
- timestamp integrity
- ordering correctness
- sampling frequency mismatch
- outlier contamination
- stale values
- inconsistent schema or units
- join-induced duplication
- survivorship and selection bias

The agent must not silently drop problematic records unless that decision is explicitly justified.

---

## Feature Engineering Principles

For large-scale market or event systems, features should be organized into:

### 1) Magnitude features
- returns
- volume deltas
- liquidity depth
- spread proxies
- acceleration and momentum
- trade intensity

### 2) Shape features
- skewness
- kurtosis
- entropy
- concentration
- dispersion
- asymmetry
- burstiness

### 3) Sequence features
- event order
- repeated motif counts
- transition probabilities
- inter-arrival times
- lagged response structure

### 4) Stability features
- rolling variance
- persistence
- regime sensitivity
- missingness rate
- sensitivity to perturbation
- confidence interval width

### 5) Context features
- time-of-day effects
- market regime
- network congestion
- cohort behavior
- external-state conditions

A feature is only useful if it is available at decision time, stable enough to trust, and cheap enough to compute within system constraints.

---

## Correlation and Dependence Rules

The agent must apply the following logic:

1. Detect candidate dependence.
2. Check whether dependence survives stratification.
3. Test whether it persists across windows.
4. Check whether it disappears under permutation.
5. Determine whether it is operationally actionable.

A correlation that vanishes out-of-sample is not a retained signal.

---

## Pattern Discovery Workflow

The agent should structure exploration as:

1. Ingest and normalize data
2. Clean schema and align time semantics
3. Generate candidate features
4. Search for correlation and dependence
5. Cluster similar states or events
6. Mine recurring sequences or co-occurrence rules
7. Validate patterns on unseen data
8. Rank patterns by stability and utility
9. Reject fragile or duplicated structure

The agent must separate exploratory discovery from accepted signal logic.

---

## Anomaly Detection Standards

An anomaly detector must define:
- what baseline it uses
- what deviation means
- what threshold is used
- what false-positive cost is acceptable
- whether anomalies are local, contextual, or global

The agent must not flag every outlier as meaningful. Many outliers are merely noise, incomplete records, or sampling defects.

---

## Time-Series Analysis Requirements

For temporal data, the agent must consider:
- stationarity or lack thereof
- heteroskedasticity
- structural breaks
- autocorrelation
- lagged causality candidates
- window dependence
- rolling distribution shifts

The agent must avoid treating adjacent observations as independent unless that assumption is demonstrated, not assumed.

---

## Large-Scale Computation Principles

When operating on large datasets, the agent must:
- minimize unnecessary passes over data
- prefer vectorized operations where appropriate
- chunk or stream data when full in-memory processing is inefficient
- preserve deterministic ordering where relevant
- keep transformations explicit and reproducible
- avoid hidden state in exploratory pipelines

Scalability matters only if the result remains correct and interpretable.

---

## Validation Rules

Every discovered pattern must be checked for:
- out-of-sample persistence
- sensitivity to window choice
- sensitivity to sampling changes
- dependence on a single regime
- leakage through preprocessing
- survivorship or selection bias
- fragility under perturbation

A pattern that is easy to find but impossible to validate should be treated as untrusted.

---

## Decision-Usefulness Criteria

A discovered structure is useful only if it satisfies at least most of the following:
- measurable effect size
- stable across time windows
- robust to moderate noise
- computable in real time or near-real time
- not redundant with existing features
- interpretable enough to support decisioning
- aligned with the target objective

---

## Handoff to Scoring Engine

When this skill discovers a stable, validated pattern, it must output a candidate feature specification in the following format:

```yaml
feature_name: string
description: string
data_source: string
computation_window: int (e.g., 1000 blocks, 60 seconds)
update_frequency: string (e.g., "per block", "per second")
stability_score: float (0-1, from validation)
expected_lift: float (e.g., 1.2x baseline)
dependencies: list of other features or tables
failure_mode: string (e.g., "missing data", "stale feed")
```

This feature specification can then be consumed by statistical-research-engine (for final validation) and trading-systems (for integration into scoring).

The agent must NOT embed discovered patterns directly into execution code without passing through this handoff format.

---

## Output Expectations

When generating analysis or code, the agent should produce:

- explicit feature definitions
- clear data-window assumptions
- robust statistical summaries
- reproducible transformations
- validation results separated from exploration
- no pseudo-insight language without evidence
- no placeholders, TODOs, or vague "pattern found" claims
- handoff specification for any pattern recommended for further use

---

## Required Review Checklist

Before finalizing any analysis, the agent must verify:

- data quality has been checked
- time ordering is correct
- missingness is accounted for
- correlations are not spurious
- discovered patterns are validated
- anomalies are classified correctly
- leakage is excluded
- temporal dependence is respected
- features are available at decision time
- conclusions are operationally meaningful
- handoff format is complete if pattern is promoted

---

## Project Bias

For this project, large-data analytics should support selective, high-precision decision systems.

That means:

- prioritize stable signals over flashy correlations
- prefer repeated structure over isolated events
- value robustness over novelty
- reject patterns that fail under temporal validation
- optimize for features that remain informative in noisy, fast-moving environments
- treat discovery as the beginning of validation, not the end
- never promote a pattern without a handoff specification
