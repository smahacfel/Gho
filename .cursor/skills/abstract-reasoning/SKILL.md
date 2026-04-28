---
name: abstract-reasoning
description: Deep abstract reasoning, first-principles analysis, problem decomposition, contradiction testing, uncertainty management, and long-horizon decision support for ill-defined, novel, or open-ended problems.
allowed-tools: Read, Edit, Grep, Bash, Python
---

# Abstract Reasoning - Deep Thinking & Complex Problem Solving

Use this skill when the task involves:
- solving problems without a clear procedural path
- reasoning from first principles instead of memorized patterns
- analyzing trade-offs with multiple non-commensurable objectives
- designing systems under uncertainty, novelty, or incomplete information
- validating assumptions before committing to implementation
- modeling complex, adaptive, or non-linear systems
- sustained multi-step reasoning over long horizons
- detecting hidden contradictions, blind spots, or implicit bias in a design
- deciding whether a problem should be solved here or handed off to a specialist skill
- when the user explicitly says "think", "reason", "analyze deeply", or asks for abstract reasoning

## Operating Doctrine

This skill assumes that:
- most problems are underspecified
- most first impressions are incomplete
- most apparent simplicity hides unstated assumptions
- most reasoning shortcuts produce local optima
- most failures come from hidden constraints, not from arithmetic mistakes
- most agents overcommit to the first plausible framing

Therefore: thinking must be explicit, structured, and falsifiable.

The agent must prefer:
- clarification over invention
- decomposition over compression
- alternatives over premature commitment
- refutation over self-confirmation
- bounded confidence over false certainty
- handoff over forced generalization

## Core Reasoning Rules

1. Separate facts from assumptions.
2. Separate the problem statement from the problem type.
3. Separate possible answers from validated answers.
4. Separate a useful heuristic from a proven conclusion.
5. Separate local correctness from system-level correctness.
6. Separate uncertainty about data from uncertainty about logic.
7. Never let style of reasoning substitute for actual reasoning.

## Phase 0 - Intake and Scope Control

Before reasoning, determine whether the problem is:
- fully specified
- partially specified
- underspecified
- structurally ambiguous
- too broad for one pass

The agent must then decide one of the following:
- proceed
- ask a clarifying question
- provide a best-effort answer with explicit assumptions
- hand off to a specialist skill

If the task cannot be framed without inventing critical facts, the agent must stop and ask for clarification.

## Phase 1 - Problem Deconstruction

Before proposing any solution, the agent must break the problem into:

### 1.1 Explicit components
- what is given
- what is sought
- what is constrained
- what is controllable
- what is external and uncontrollable

### 1.2 Implicit assumptions
The agent must list the hidden assumptions that matter most, such as:
- stationarity
- independence
- observability
- determinism
- availability of data
- absence of adversarial behavior
- sufficient compute or time
- correctness of external dependencies

At minimum, identify the assumptions that would most likely invalidate the conclusion if false.

### 1.3 Boundary conditions
- edge cases
- degenerate inputs
- extreme values
- failure states
- what happens when key assumptions break

### 1.4 Problem type classification
Classify the problem into one or more categories:
- optimization
- search
- inference
- prediction
- diagnosis
- design
- control
- verification
- selection
- explanation

If the problem type cannot be classified, the agent must not proceed as if it were obvious.

## Phase 2 - First-Principles Reframing

The agent must restate the problem in minimal irreducible form.

Rules:
- strip away domain decoration
- ignore existing conventions temporarily
- identify the smallest mechanism that must be true
- reduce the problem to state, constraints, and objective

A valid reframing should answer:
- what is the actual decision or transformation?
- what information is necessary?
- what is merely context?
- what is truly unknown?

If the reframed problem is clearer than the original, the reasoning is likely improving. If not, continue decomposing.

## Phase 3 - Alternative Generation

Before choosing a path, generate at least 3 qualitatively distinct approaches.

Each approach must differ in mechanism, not just wording. Examples:
- brute-force versus structured search
- deterministic versus probabilistic
- analytical versus empirical
- centralized versus modular
- explicit rules versus learned scoring
- conservative versus aggressive

For each candidate approach, note:
- core idea
- key assumption
- main strength
- main weakness
- likely failure mode

Do not evaluate deeply yet. First generate breadth.

## Phase 4 - Assumption Testing

For each candidate approach, test the assumptions that support it.

The agent must ask:
- what must be true for this to work?
- what would make this fail?
- can we detect failure early?
- can we degrade gracefully?
- is the failure catastrophic or merely suboptimal?

Assumptions must be checked against:
- adversarial behavior
- data drift
- measurement error
- delayed information
- incomplete observability
- computational limits
- operational constraints

Any approach that depends on a fragile assumption without a mitigation path is low quality.

## Phase 5 - Trade-Off Mapping

The agent must map the design space explicitly.

Typical trade-offs include:
- latency vs correctness
- precision vs recall
- generality vs specificity
- robustness vs peak performance
- interpretability vs flexibility
- simplicity vs expressive power
- completeness vs tractability
- determinism vs adaptability

For each candidate approach, state:
- what it gains
- what it sacrifices
- whether the sacrifice is acceptable in this context

No trade-off analysis means no serious decision.

## Phase 6 - Contradiction Detection

The agent must actively search for contradictions.

Check whether:
- one assumption is treated as both true and false
- one requirement conflicts with another
- the proposed mechanism cannot satisfy all constraints simultaneously
- the solution is "fast" only because it ignores necessary checks
- the system is "optimal" only under unrealistic assumptions

When a contradiction appears:
1. name it
2. identify its origin
3. resolve it by relaxing, splitting, or rejecting an assumption

Unresolved contradictions invalidate the reasoning.

## Phase 7 - Abstraction Laddering

The agent must move between abstraction levels.

### Why
Why does the problem matter? What larger objective does it serve?

### What
What exactly is being solved or decided? What are the boundaries?

### How
How does it work concretely? What mechanism produces the result?

The agent must ensure:
- the Why justifies the What
- the What is implementable by the How
- the How does not violate the Why

If any level cannot be explained clearly, the reasoning is incomplete.

## Phase 8 - Refutation Attempt

The agent must attempt to prove its own conclusion wrong.

Refutation checks:
- can I build a counterexample?
- what happens if I negate a key assumption?
- what if the environment is adversarial?
- what if the problem is simpler than I think?
- what if the best solution is to do less, not more?
- would a skeptic accept the chain of logic?

A conclusion that survives serious refutation is stronger than one that was never challenged.

## Phase 9 - Long-Horizon Agentic Work

For tasks that require sustained reasoning over multiple steps:

### 9.1 Subgoal decomposition
Break the problem into verifiable subgoals.

### 9.2 Working state
Track:
- what is established
- what is uncertain
- what is currently being tested
- what has been rejected

### 9.3 Periodic re-evaluation
At major checkpoints, ask:
- is the original framing still valid?
- did new constraints appear?
- should the approach change?
- is the current line of reasoning still worth pursuing?

### 9.4 Confidence updating
Update confidence as evidence accumulates:
- initial: low
- after decomposition: low to medium
- after assumption checks: medium
- after refutation attempts: medium to high
- after external validation: high

Long-horizon reasoning without re-evaluation becomes inertia.

## Phase 10 - Decision and Output Synthesis

Before finalizing, the agent must produce a structured synthesis:

### 10.1 Restated problem
What was actually solved, which may differ from the original wording.

### 10.2 Core insight
The key non-obvious understanding that drives the answer.

### 10.3 Candidate approaches considered
A compact summary of the best alternatives and why they were not chosen.

### 10.4 Trade-offs made
What was sacrificed and why.

### 10.5 Assumptions that remain
What is still unproven or uncertain.

### 10.6 Confidence assessment
- confidence level: low / medium / high
- primary sources of uncertainty
- what evidence would increase confidence

### 10.7 Actionable outcome
- design decision
- architectural direction
- proof sketch
- implementation structure
- next research step

## Escalation and Handoff Rules

The agent must hand off to a specialist skill when the task becomes primarily:

- statistical validation -> `statistics-probability`
- signal discovery or dataset mining -> `large-data-analytics`
- research-grade validation of a candidate signal -> `statistical-research-engine`
- Solana or Rust architecture -> `solana-pumpfun-architect`
- scoring, calibration, or filter construction -> `statistics-probability` or project-specific scoring skill

The agent must not force a generic reasoning skill to do the work of a specialized technical skill.

## Reasoning Failure Modes

The agent must detect and name these failure modes:

- pattern matching masquerading as reasoning
- premature convergence on the first plausible answer
- assumption blindness
- false precision
- local optimization that hurts the system
- confirmation bias
- scope creep
- overgeneralization
- vague abstraction without mechanism
- overfitting a conclusion to a preferred narrative

If a failure mode is detected, the agent must stop, state it explicitly, and restart from the relevant phase.

## Uncertainty Policy

The agent must never hide uncertainty.

Rules:
- do not present guesses as facts
- do not inflate confidence to sound decisive
- do not collapse several uncertain claims into one confident claim
- do not infer beyond available evidence without labeling it as inference

If the answer depends on unverified assumptions, state that clearly.

## Output Expectations

When producing reasoning output, the agent should provide:
- explicit phase markers
- separate sections for facts, assumptions, alternatives, trade-offs, and conclusions
- concise but real refutation attempts
- confidence statements
- handoff notes when applicable
- no hidden leaps
- no vague filler language

The output should be structured enough to audit, but not so verbose that it obscures the conclusion.

## Required Review Checklist

Before finalizing, verify:
- [ ] the problem was deconstructed
- [ ] assumptions were explicitly listed
- [ ] the problem type was classified
- [ ] at least 3 alternatives were generated
- [ ] key assumptions were tested
- [ ] trade-offs were mapped
- [ ] contradictions were checked
- [ ] abstraction laddering was applied
- [ ] refutation was attempted
- [ ] confidence and uncertainty were stated
- [ ] no active reasoning failure mode remains
- [ ] handoff was specified if a specialist skill is needed

## Project Bias

For this project, abstract reasoning should support high-stakes autonomous systems operating in uncertain, non-stationary, or adversarial environments.

That means:
- prefer conservative conclusions over optimistic ones
- require evidence rather than intuition
- expose uncertainty rather than hiding it
- design for resilience instead of peak cleverness
- reject reasoning that cannot survive its strongest counterargument
- treat every conclusion as provisional until validated

A reasoning pass that ends with "I cannot fully prove this, but here is the best-supported conclusion" is acceptable.
A reasoning pass that ends with "trust me" is not.

## Quick Start

When this skill is activated, begin with:

> [Abstract Reasoning] I will decompose the problem, make assumptions explicit, generate alternatives, test contradictions, attempt refutation, and then synthesize a bounded conclusion with confidence and remaining uncertainty.

Then proceed through the phases in order.
Do not skip phases.
Do not compress everything into one paragraph.
Do not decide before alternatives and refutation have been considered.
