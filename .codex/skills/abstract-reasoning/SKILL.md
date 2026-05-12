---
name: abstract-reasoning
description: "Deep design reasoning, first-principles decomposition, contradiction testing, assumption validation, and bounded decision support for ambiguous architecture, runtime, or system-design problems."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Abstract Reasoning

Use this skill when the task involves:

* ambiguous or underspecified problems
* first-principles system decomposition
* contradiction detection
* trade-off analysis
* hidden assumption discovery
* architecture boundary decisions
* unclear ownership or responsibility split
* long-horizon design choices
* deciding which specialist skill should handle the task

Optimized for:

* complex design reasoning
* architecture decisions
* uncertain requirements
* cross-module trade-offs
* high-stakes system choices
* contradiction and blind-spot detection

Not optimized for:

* direct Rust implementation
* direct Solana execution work
* statistical validation
* large data exploration
* localized code edits
* routine refactors

Use specialist skills instead when the task is clearly domain-specific.

---

# Quick Start

When activated:

> Decompose the problem, separate facts from assumptions, identify the key uncertainty, test contradictions, compare viable approaches, and produce a bounded conclusion with confidence and remaining uncertainty.

Preferred reasoning flow:

frame problem
→ list assumptions
→ identify constraints
→ generate alternatives
→ test contradictions
→ compare trade-offs
→ recommend or hand off


For deeper methodology, refutation, abstraction laddering, or long-horizon reasoning, read `references.md`.

---

# Core Doctrine

Assume:

* most problems are underspecified
* first impressions are incomplete
* apparent simplicity hides assumptions
* local optimizations can harm the system
* hidden constraints cause most failures
* agents often overcommit to the first plausible framing

Therefore:

* separate facts from assumptions
* prefer clarification over invention
* prefer alternatives over premature commitment
* prefer refutation over self-confirmation
* prefer bounded confidence over false certainty
* prefer handoff over forced generalization

---

# FAST PATH RULE

If the task is:

* localized
* clearly domain-specific
* implementation-oriented
* already well-scoped
* better handled by another skill

Then:

* do not run full abstract reasoning
* do not generate unnecessary alternatives
* do not broaden the scope
* hand off or answer minimally

This skill must not create overthinking loops.

---

# When to Hand Off Immediately

Delegate instead of solving when the task is primarily about:

* Rust runtime implementation → `rust-master`
* Solana / pump.fun execution semantics → `solana-pumpfun-architect`
* trading architecture / execution orchestration → `trading-systems`
* signal discovery / dataset mining → `large-data-analytics`
* statistical validation / calibration → `statistical-research-engine`

If multiple domains apply, use this skill only to decide the boundary and order of handoff.

---

# Reasoning Rules

Always distinguish:

* facts vs assumptions
* known constraints vs inferred constraints
* local correctness vs system-level correctness
* useful heuristics vs validated conclusions
* uncertainty about data vs uncertainty about logic
* implementation detail vs architectural decision

Do not:

* invent missing critical facts
* hide uncertainty
* collapse multiple uncertain claims into one confident claim
* decide before considering at least one alternative
* use abstraction without mechanism
* turn a specialist problem into generic reasoning

---

# Problem Framing

Before proposing a solution, identify:

* what is given
* what is sought
* what is constrained
* what is controllable
* what is external
* what would invalidate the conclusion

If critical facts are missing:

* ask a clarifying question
  or
* proceed with clearly labeled assumptions

Do not silently fill gaps.

---

# Alternative Generation

For non-trivial decisions, generate 2–3 meaningfully different approaches.

Each approach should state:

* core idea
* key assumption
* strength
* weakness
* likely failure mode

Approaches must differ in mechanism, not just wording.

---

# Contradiction Testing

Actively check whether:

* one assumption conflicts with another
* a requirement makes another requirement impossible
* the proposed solution is fast only because it skips necessary checks
* the solution is simple only because it ignores failure states
* local optimization harms system integrity

If a contradiction appears:

1. name it
2. locate its source
3. resolve it by relaxing, splitting, or rejecting an assumption

Unresolved contradictions invalidate the recommendation.

---

# Trade-Off Mapping

For each serious option, identify what it gains and sacrifices.

Common trade-offs:

* latency vs correctness
* precision vs recall
* robustness vs peak performance
* simplicity vs expressiveness
* determinism vs adaptability
* completeness vs tractability
* modularity vs coordination cost

No trade-off analysis means no serious decision.

---

# Refutation Requirement

Before finalizing a non-trivial conclusion, attempt to refute it.

Ask:

* what would make this fail?
* what is the strongest counterargument?
* what assumption is most fragile?
* what happens in adversarial conditions?
* is doing less actually better?

If the conclusion cannot survive a basic refutation attempt, downgrade confidence or choose another path.

---

# Output Requirements

Outputs should include:

* restated problem
* key assumptions
* viable alternatives
* main trade-offs
* contradiction/refutation notes
* recommendation
* confidence level
* remaining uncertainty
* handoff notes if applicable

Keep output as short as the task allows.

Do not produce a long reasoning report for a small localized problem.

---

# Failure Modes

Detect and avoid:

* pattern matching masquerading as reasoning
* premature convergence
* assumption blindness
* false precision
* local optimization harming the system
* confirmation bias
* scope creep
* vague abstraction without mechanism
* overgeneralization
* overfitting a conclusion to a preferred narrative

If detected:

* stop
* name the failure mode
* restart from the relevant reasoning step

---

# Final Review Checklist

Before completion verify:

* problem framed correctly
* assumptions explicit
* key constraints identified
* alternatives considered when needed
* contradictions checked
* trade-offs mapped
* refutation attempted for non-trivial conclusions
* uncertainty stated
* handoff used when specialist skill is better
* no hidden critical assumption remains

---

# Final Principle

Reasoning exists to improve decisions, not to produce long explanations.

Clarify before inventing.
Decompose before deciding.
Refute before trusting.
Hand off before overgeneralizing.
Bound confidence before acting.