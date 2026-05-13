## `abstract-reasoning/references.md`

# Abstract Reasoning Reference

This file expands the `abstract-reasoning` skill. Read it only when deeper first-principles analysis, contradiction testing, assumption validation, or long-horizon decision support is needed.

Use this reference for:

* deeply ambiguous problems
* high-stakes architecture decisions
* conflicting requirements
* hidden assumption discovery
* cross-domain trade-off analysis
* long-horizon design choices
* multi-step reasoning with uncertainty
* deciding whether to split work across specialist skills

Do not load this file for small localized edits or clearly specialist tasks.

---

# 1. Operating Assumptions

For complex system-design problems:

* most problems are underspecified
* most first impressions are incomplete
* most apparent simplicity hides unstated assumptions
* most reasoning shortcuts produce local optima
* most failures come from hidden constraints
* most agents overcommit to the first plausible framing
* most “obvious” solutions work only in the happy path
* most abstractions fail when forced into runtime details

Therefore, reasoning must be:

* explicit
* structured
* falsifiable
* bounded
* aware of uncertainty
* willing to hand off

The goal is not to sound clever.

The goal is to reach the most defensible conclusion under known constraints.

---

# 2. Core Reasoning Rules

Always separate:

* facts from assumptions
* problem statement from problem type
* possible answers from validated answers
* useful heuristics from proven conclusions
* local correctness from system-level correctness
* uncertainty about data from uncertainty about logic
* implementation details from architectural principles
* conceptual elegance from operational viability

Never allow:

* style of reasoning to substitute for reasoning
* confident language to mask uncertainty
* a preferred conclusion to shape the analysis
* abstraction to hide missing mechanisms
* decomposition to become infinite
* handoff avoidance to create shallow generalism

---

# 3. Intake and Scope Control

Before reasoning, classify the task.

## Specification Level

The problem may be:

* fully specified
* partially specified
* underspecified
* structurally ambiguous
* too broad for one pass

## Response Mode

Choose one:

* proceed directly
* ask a clarifying question
* provide best-effort answer with explicit assumptions
* narrow the scope
* hand off to a specialist skill
* split into subproblems

If the task cannot be framed without inventing critical facts, stop and ask for clarification.

If the task is clear but specialist-heavy, hand off immediately.

---

# 4. Problem Deconstruction

Break the problem into explicit components.

## Given

What is known?

Examples:

* existing code
* documented constraints
* user requirements
* current architecture
* observed failures
* hard limits

## Sought

What must be decided, designed, explained, or changed?

Examples:

* choose architecture
* resolve contradiction
* identify risk
* select approach
* define boundary
* explain mechanism

## Constrained

What limits the solution?

Examples:

* latency
* correctness
* replayability
* compatibility
* risk
* memory
* compute
* existing state ownership
* runtime invariants

## Controllable

What can be changed?

Examples:

* module boundaries
* data flow
* config
* thresholds
* state model
* tests
* implementation details

## External

What cannot be controlled?

Examples:

* provider behavior
* market behavior
* network latency
* protocol constraints
* chain state
* third-party APIs

Good decomposition reduces ambiguity without inventing facts.

---

# 5. Hidden Assumption Discovery

Identify assumptions that matter most.

Common hidden assumptions:

* stationarity
* independence
* deterministic ordering
* complete observability
* trustworthy external data
* stable latency
* sufficient compute
* no adversarial behavior
* no duplicate delivery
* no stale state
* no hidden coupling
* no race condition
* no post-hoc leakage
* one state source is authoritative
* retry behavior is harmless
* local optimization improves global behavior

Ask:

* What must be true for this to work?
* Which assumption would invalidate the conclusion if false?
* Can failure be detected early?
* Can the system degrade safely?
* Is the assumption operationally realistic?

Fragile assumptions must be exposed.

---

# 6. Boundary Conditions

Check:

* edge cases
* degenerate inputs
* extreme values
* missing data
* stale data
* duplicate events
* delayed information
* partial failure
* adversarial behavior
* inconsistent dependencies
* recovery after crash
* invalid state transitions

A design that works only in the ideal path is not robust.

Boundary testing often reveals the real problem.

---

# 7. Problem Type Classification

Classify the problem into one or more categories:

* optimization
* search
* inference
* prediction
* diagnosis
* design
* control
* verification
* selection
* explanation
* routing / handoff
* contradiction resolution

Examples:

* “Which module should own this state?” → design + verification
* “Why did this fail?” → diagnosis
* “Which signal should be used?” → selection + validation
* “How do we preserve latency and correctness?” → trade-off optimization
* “Is this architecture contradictory?” → verification + contradiction resolution

If the problem type is unclear, do not proceed as if it is obvious.

---

# 8. First-Principles Reframing

Restate the problem in minimal form.

Strip away:

* naming conventions
* historical baggage
* implementation accidents
* preferred solutions
* domain decoration

Reduce to:

* state
* constraints
* objective
* information available
* transformation required
* failure consequences

Ask:

* What is the actual decision?
* What information is necessary?
* What is merely context?
* What is truly unknown?
* What mechanism must exist for success?

A good reframing makes the decision clearer.

A bad reframing merely rephrases the original question.

---

# 9. Alternative Generation

Before choosing a path, generate qualitatively distinct approaches.

For each candidate, state:

* core idea
* key assumption
* main strength
* main weakness
* likely failure mode

Approaches should differ by mechanism.

Examples:

* centralized ownership vs distributed ownership
* eager computation vs lazy materialization
* deterministic rule vs adaptive score
* immutable snapshot vs live reads
* direct execution vs staged commit
* strict rejection vs degraded mode
* simple threshold vs multi-layer gate
* specialized module vs generic abstraction

Do not deeply evaluate before generating enough breadth.

Premature convergence is one of the most common reasoning failures.

---

# 10. Assumption Testing

For each candidate approach, ask:

* What must be true for this to work?
* What would make this fail?
* Can we detect failure early?
* Can we degrade gracefully?
* Is failure catastrophic or merely suboptimal?
* Does this require hidden coordination?
* Does this assume perfect data?
* Does this assume stable timing?
* Does this assume no adversary?
* Does this assume a single source of truth?

Test assumptions against:

* adversarial behavior
* data drift
* measurement error
* delayed information
* incomplete observability
* computational limits
* operational constraints
* concurrency effects
* recovery behavior

Approaches depending on fragile assumptions without mitigation are low quality.

---

# 11. Trade-Off Mapping

Every meaningful design choice sacrifices something.

Common trade-offs:

* latency vs correctness
* precision vs recall
* generality vs specificity
* robustness vs peak performance
* interpretability vs flexibility
* simplicity vs expressive power
* completeness vs tractability
* determinism vs adaptability
* modularity vs coordination cost
* hot-path efficiency vs rich diagnostics
* snapshot consistency vs live freshness
* local optimization vs system integrity

For each option, state:

* what it gains
* what it sacrifices
* whether the sacrifice is acceptable
* what risk it introduces
* how that risk can be monitored or mitigated

No trade-off analysis means no serious decision.

---

# 12. Contradiction Detection

Actively search for contradictions.

Check whether:

* one assumption is treated as both true and false
* one requirement conflicts with another
* the solution cannot satisfy all constraints simultaneously
* the solution is “fast” only because it skips checks
* the system is “simple” only because it hides state
* the design is “robust” only because failure cases are ignored
* the decision is “deterministic” while relying on nondeterministic inputs
* the state is “canonical” while multiple writers exist
* execution is “safe” while reconciliation is absent

When contradiction appears:

1. name it
2. identify its origin
3. resolve it by relaxing, splitting, or rejecting an assumption

Unresolved contradictions invalidate the reasoning.

---

# 13. Abstraction Laddering

Move between three levels.

## Why

Why does the problem matter?

Examples:

* preserve correctness
* reduce false positives
* prevent state drift
* improve replayability
* reduce execution risk
* protect hot-path latency

## What

What exactly is being solved?

Examples:

* state ownership
* decision policy
* execution validity
* feature materialization
* module boundary
* failure classification

## How

How does it work concretely?

Examples:

* data structure
* state transition
* queue boundary
* snapshot boundary
* retry logic
* typed API
* metric/logging hook

The Why must justify the What.

The What must be implementable by the How.

The How must not violate the Why.

If any level is unclear, the reasoning is incomplete.

---

# 14. Refutation Attempt

Try to prove the conclusion wrong.

Ask:

* Can I build a counterexample?
* What happens if a key assumption is false?
* What if the environment is adversarial?
* What if the problem is simpler than I think?
* What if the best solution is doing less?
* Would a skeptical engineer accept this reasoning?
* Does the conclusion survive stale data?
* Does it survive duplicate events?
* Does it survive partial failure?
* Does it survive replay requirements?

A conclusion that survives refutation is stronger than one that was never challenged.

If refutation succeeds, update or reject the conclusion.

---

# 15. Long-Horizon Reasoning

For sustained work over multiple steps, maintain working state.

Track:

* established facts
* active assumptions
* unresolved uncertainty
* rejected options
* current hypothesis
* known constraints
* next verification step

At major checkpoints, ask:

* is the original framing still valid?
* did new constraints appear?
* should the approach change?
* is the current path still worth pursuing?
* should this be handed off?
* has scope creep occurred?

Confidence should update as evidence accumulates:

* initial: low
* after decomposition: low to medium
* after assumption checks: medium
* after contradiction testing: medium
* after refutation: medium to high
* after external validation: high

Long-horizon reasoning without re-evaluation becomes inertia.

---

# 16. Decision and Output Synthesis

Before finalizing a non-trivial conclusion, produce a compact synthesis.

Include:

## Restated Problem

What was actually solved.

## Core Insight

The key understanding that drives the recommendation.

## Alternatives Considered

Best options and why they were chosen or rejected.

## Trade-Offs

What was gained and sacrificed.

## Remaining Assumptions

What is still unproven.

## Confidence

Use:

* low
* medium
* high

Explain why.

## Actionable Outcome

Examples:

* design decision
* architectural direction
* implementation structure
* proof sketch
* next research step
* specialist handoff

The output should be auditable but not bloated.

---

# 17. Specialist Handoff Rules

Use this skill to route complex work, not to absorb all domains.

Hand off when the task is primarily:

* Rust runtime implementation → `rust-master`
* Solana / pump.fun execution semantics → `solana-pumpfun-architect`
* trading architecture / execution orchestration → `trading-systems`
* signal discovery / dataset mining → `large-data-analytics`
* statistical validation / calibration → `statistical-research-engine`

If the task spans multiple domains:

1. use abstract reasoning to define boundaries
2. decide skill order
3. hand off each subproblem
4. avoid solving specialist details generically

Do not force a generic reasoning skill to do specialist work.

---

# 18. Reasoning Failure Modes

Detect and name:

* pattern matching masquerading as reasoning
* premature convergence
* assumption blindness
* false precision
* local optimization harming the system
* confirmation bias
* scope creep
* overgeneralization
* vague abstraction without mechanism
* overfitting conclusion to preferred narrative
* handoff avoidance
* infinite decomposition
* abstraction hiding implementation impossibility
* elegant theory ignoring runtime reality

If detected:

* stop
* name the failure mode
* return to the relevant phase
* narrow or hand off if needed

---

# 19. Uncertainty Policy

Never:

* present guesses as facts
* inflate confidence to sound decisive
* hide unsupported assumptions
* collapse multiple uncertain claims into one confident claim
* infer beyond available evidence without labeling it
* make architectural recommendations without stating trade-offs

Always:

* state key assumptions
* mark uncertainty clearly
* distinguish provisional from validated
* identify what evidence would increase confidence
* prefer bounded conclusions over false certainty

A conclusion can be useful and uncertain at the same time.

---

# 20. Output Patterns

## For architecture decisions

Use:

```yaml
problem: string
core_constraint: string
assumptions: list
options:
  - name: string
    strength: string
    weakness: string
    failure_mode: string
tradeoff_summary: string
recommendation: string
confidence: low | medium | high
remaining_uncertainty: list
handoff: string | none
````

## For contradiction analysis

Use:

```yaml
contradiction: string
where_it_appears: string
why_it_matters: string
resolution_options: list
recommended_resolution: string
confidence: low | medium | high
```

## For handoff routing

Use:

```yaml
task_type: string
primary_skill: string
supporting_skills: list
reason: string
boundary: string
```

---

# 21. Final Review Checklist

Before finalizing deep reasoning:

* problem framed correctly
* facts separated from assumptions
* key assumptions listed
* problem type classified
* alternatives generated when needed
* assumptions tested
* trade-offs mapped
* contradictions checked
* abstraction levels aligned
* refutation attempted
* uncertainty stated
* handoff used where appropriate
* no active reasoning failure mode remains
* conclusion is bounded and actionable

---

# 22. Final Principle

Deep reasoning is valuable only when it improves the decision.

A good reasoning pass:

* clarifies what is unknown
* reduces false certainty
* exposes hidden constraints
* compares real alternatives
* rejects fragile conclusions
* identifies the right specialist path
* produces an actionable bounded recommendation

Clarify before inventing.
Decompose before deciding.
Refute before trusting.
Hand off before overgeneralizing.
Bound confidence before acting.