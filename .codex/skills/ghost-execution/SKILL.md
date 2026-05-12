---
name: ghost-execution
description: "Ghost-specific execution and decision architecture: Gatekeeper V2/V2.5, SSOT feature materialization, observation sessions, shadow-only execution, decision logging, replay safety, and Ghost runtime contracts for the selective pump.fun sniper."
allowed-tools: "Read, Edit, Grep, Bash"
---

# Ghost Execution

Use this skill when the task involves:

* Ghost-specific architecture or runtime behavior
* Gatekeeper V2 / V2.5 decision pipeline
* `MaterializedFeatureSet` / SSOT contracts
* observation sessions and per-pool lifecycle
* PDD / DOW / TAS / APS / Alpha / Prosperity / IWIM interactions
* decision verdicts, reason codes, or rejection paths
* shadow-only execution flow
* decision logging / JSONL schema evolution
* Ghost config changes
* Seer / Yellowstone event flow as used by Ghost
* AccountStateCore, ShadowLedger, WAL, or replay contracts
* diagnosing wrong BUY / REJECT / TIMEOUT decisions

Optimized for:

* Ghost repository work
* selective pump.fun sniper runtime
* bounded observation-window decisions
* feature-driven Gatekeeper evaluation
* low-latency Rust/Solana orchestration
* decision auditability and replay safety

Not optimized for:

* generic trading-system design
* generic Rust refactoring
* generic Solana transaction engineering
* statistical validation of new signals
* large-scale data mining
* abstract architecture debate without Ghost context

Use specialist skills for those domains.

---

# Quick Start

When activated:

> Work within the current Ghost runtime: selective sniper, not HFT/MEV; shadow-only execution unless explicitly changed; feature-based Gatekeeper; immutable decision snapshots; SSOT contracts preserved; typed verdicts and reason codes required.

Preferred Ghost flow:

```text
Yellowstone / Seer
→ Event Bus
→ OracleRuntime
→ PoolObservationSession
→ materialize MaterializedFeatureSet
→ Gatekeeper policy evaluation
→ IWIM veto if BUY path requires it
→ shadow execution / simulation
→ post-buy lifecycle
→ decision logging
→ replay / reconciliation evidence
````

For deeper file maps, module contracts, Gatekeeper V2/V2.5 details, failure modes, or config rules, read `references.md`.

---

# Current Runtime Truth

Assume the current active Ghost system is:

* selective pump.fun sniper
* Rust low-latency runtime
* event-driven
* bounded observation-window based
* feature-driven
* shadow-only unless explicitly changed
* replay/audit oriented
* conservative under uncertainty

Do not revive legacy paths unless the task explicitly asks for legacy analysis.

Do not assume HyperPrediction/Chaos are active production decision paths.

Do not assume live execution is enabled unless verified in config.

---

# Core Doctrine

Ghost does not try to predict all winners.

Ghost rejects obvious traps and enters only when the evidence is strong enough inside the observation window.

Therefore:

* selectivity > activity
* rejection quality matters
* decision-time truth matters
* stale state is unsafe
* every verdict needs a reason code
* every decision must be auditable
* every feature must come from the correct authority
* shadow results are evidence, not live guarantees

---

# SSOT & Materialization Discipline

The canonical decision snapshot is `MaterializedFeatureSet`.

Rules:

* decision logic must use materialized feature snapshots
* feature ownership must be explicit
* no feature should be recomputed from an unauthorized secondary source
* no live-state reads during policy evaluation unless already part of materialization
* runtime mutation during evaluation is forbidden
* post-verdict updates must not rewrite historical decisions

Preferred model:

```text
session state
→ PoolObservationSession::materialize_features()
→ immutable MaterializedFeatureSet
→ Gatekeeper assessment
→ policy verdict
```

If a change bypasses `MaterializedFeatureSet`, treat it as a potential SSOT violation.

---

# Observation Session Discipline

Per-pool observation must preserve:

* session lifecycle
* observation deadline
* tx accumulation
* account state refresh
* feature materialization
* terminal verdict semantics
* cleanup and logging

Valid terminal outcomes include:

* BUY / EARLY_BUY where supported
* REJECT with typed reason
* TIMEOUT
* PENDING / CURVE-related outcome only when explicitly modeled

Rules:

* terminal verdict must be explicit
* timeout is not a generic failure
* late events must not rewrite the terminal decision
* observation duration must not mix timestamp domains
* duplicates must not inflate metrics

---

# Gatekeeper Policy Discipline

Gatekeeper decisions must remain:

* feature-based
* deterministic for the same snapshot and config
* reason-code driven
* auditable from logs
* compatible with existing verdict taxonomy

Rules:

* hard fails precede soft scoring
* PDD live veto behavior must remain explicit when enabled
* Alpha / Prosperity / Sybil / Curve gates must preserve their configured order
* TAS must not be silently promoted from modulator to hard gate unless policy explicitly says so
* DOW early/normal/extended behavior must respect config
* IWIM operates after Gatekeeper BUY path according to its configured policy

Do not collapse multiple rejection classes into generic `REJECT`.

---

# Config Safety

When changing Ghost config structs:

* preserve backward compatibility
* use `#[serde(default)]` for new config fields
* avoid changing defaults silently
* avoid hardcoding thresholds in runtime logic
* keep thresholds config-driven
* verify active config path before changing policy behavior
* do not assume README values are current if code/config differs

Before changing decision thresholds:

* identify current source of truth
* check related diagnostics/logging
* consider shadow-only validation impact
* preserve rollback path

---

# Decision Logging & Audit

Every terminal decision should be reconstructable from logs.

Decision logging must preserve:

* pool identifiers
* verdict type
* reason code
* assessment/diagnostics
* timing metadata
* feature snapshot or enough references to reconstruct it
* IWIM veto reason if relevant
* schema compatibility

Rules:

* do not remove existing JSONL fields without explicit migration
* prefer additive schema changes
* verify current schema/version constants in code before editing
* preserve replay/audit usefulness
* no generic failure buckets for decision paths

---

# Shadow / Live Boundary

Current Ghost work should assume shadow-only unless verified otherwise.

Rules:

* shadow simulation is not live inclusion
* shadow BUY does not prove live landing
* live sender behavior must not be modified casually
* live execution changes require specialist review
* post-buy lifecycle evidence must remain separate from decision evidence
* do not mix shadow metrics with live safety claims

If the task touches live execution or transaction construction, hand off to `solana-pumpfun-architect`.

---

# FAST PATH RULE

If task is:

* localized
* single-file
* non-policy
* non-SSOT
* non-execution-critical

Then:

* avoid broad architecture analysis
* preserve existing runtime contracts
* make minimal safe changes
* do not rewrite decision flow
* do not introduce new global abstractions

Do not over-expand Ghost-specific work unnecessarily.

---

# Handoff Boundaries

Delegate instead of solving:

* Rust ownership/concurrency/performance → `rust-master`
* Solana transaction/execution semantics → `solana-pumpfun-architect`
* statistical validation/calibration → `statistical-research-engine`
* raw data mining / feature discovery → `large-data-analytics`
* system-level trading architecture → `trading-systems`
* ambiguous decomposition/trade-off reasoning → `abstract-reasoning`

Ghost-specific pipeline contracts, component boundaries, verdict semantics, and SSOT rules stay in this skill.

---

# Ghost-Specific Failure Modes

Detect and stop on:

* `MaterializedFeatureSet` bypassed
* feature computed from competing authority
* terminal verdict emitted without reason code
* generic `REJECT` replacing typed rejection
* legacy HyperPrediction/Chaos path treated as active production path
* config field added without `#[serde(default)]`
* active shadow/live boundary blurred
* decision logger schema changed destructively
* observation timestamp domains mixed
* duplicate events inflating decision features
* late events rewriting historical verdict
* DOW/TAS/PDD/APS behavior changed without config review
* IWIM coupling moved before its intended stage
* legacy modes or tests broken unintentionally

If detected:

* stop
* name the failure mode
* preserve current contract
* correct course or hand off

---

# Final Review Checklist

Before completion verify:

* current active runtime path understood
* SSOT / `MaterializedFeatureSet` preserved
* observation lifecycle preserved
* Gatekeeper policy order preserved
* typed verdicts and reason codes preserved
* config compatibility preserved
* `#[serde(default)]` used for new config fields
* decision logging remains replay/audit useful
* shadow/live boundary not blurred
* legacy code not revived accidentally
* specialist handoff used when appropriate
* no hidden state authority introduced

---

# Final Principle

Ghost is a selective execution system for noisy early pump.fun markets.

Do not optimize for activity.
Do not predict beyond evidence.
Do not bypass SSOT.
Do not weaken reason codes.
Do not confuse shadow evidence with live execution truth.
Do not revive legacy paths unless explicitly asked.