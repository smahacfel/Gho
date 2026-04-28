# ADR-0108: Statistics Probability Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested creation of a Cursor skill for advanced statistical and probabilistic reasoning, focused on separability analysis, inference quality, uncertainty modeling, Bayesian updating, and high-precision decision-system foundations.

The requested scope explicitly covers probability theory, frequentist and Bayesian inference, multivariate structure, statistical learning concerns, and operational boundaries between statistical signal and deployable decision rules.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/statistics-probability/SKILL.md`

Implemented decisions:
- preserve the requested skill identity (`name: statistics-probability`) and high-specificity domain description;
- preserve user-provided `allowed-tools` metadata;
- keep explicit separability taxonomy (linear, non-linear, probabilistic, temporal);
- encode operational guardrails to avoid leakage, instability, and uncalibrated confidence in filters.

## Architectural Impact
- Adds a reusable project-level AI skill for rigorous statistics/probability tasks.
- Standardizes how separability, uncertainty, and decision thresholds are discussed in analytical and implementation work.
- Introduces no runtime changes to binaries, protocols, or deployment topology.

## Risk Assessment
**Rate:** Low

Primary risks:
- broad activation surface can trigger in partially relevant analytics tasks;
- compatibility of non-standard frontmatter field (`allowed-tools`) depends on current Cursor parser behavior.

Risk mitigation:
- detailed description remains tightly scoped to advanced statistical decision workflows;
- guidance emphasizes measurable separation, calibration, and explicit failure modes.

## Consequences
- future analytical and modeling tasks gain a shared statistical vocabulary and stricter inference discipline;
- decision/filter design discussions become more reproducible and auditable;
- repository governance expands by one skill plus one ADR.

## Alternatives Considered
1. **Create as personal skill (`~/.cursor/skills`)**
   - Rejected to keep the capability shared at repository scope.
2. **Reduce the skill to a short checklist**
   - Rejected because the requested domain needs explicit mathematical and operational detail.
3. **Drop `allowed-tools` from frontmatter**
   - Rejected to preserve the provided contract unless incompatibility appears.

## Validation Steps
1. Verify file exists at `.cursor/skills/statistics-probability/SKILL.md`.
2. Confirm frontmatter includes normalized `name` and detailed `description`.
3. Confirm separability taxonomy and anti-leakage/anti-overconfidence guidance are present.
4. Confirm filter-design section includes explicit threshold/failure-mode expectations.
