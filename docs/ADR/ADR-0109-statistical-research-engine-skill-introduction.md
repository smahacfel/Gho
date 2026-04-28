# ADR-0109: Statistical Research Engine Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested a new Cursor skill for research-grade statistical reasoning focused on discovering and validating predictive signals under uncertainty and non-stationarity.

The requested scope includes a full validation doctrine: signal definition, detection, separability, robustness, regime dependence, causal sanity checks, calibration, thresholding, walk-forward validation, and deployment feasibility.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/statistical-research-engine/SKILL.md`

Implemented decisions:
- keep the requested skill identity (`name: statistical-research-engine`);
- preserve `allowed-tools` metadata as provided;
- encode a strict 10-phase falsification-first pipeline;
- include explicit failure modes and output standards to prevent overconfident recommendations.

## Architectural Impact
- Adds a reusable, repository-scoped analytical standard for high-risk signal validation work.
- Unifies statistical reasoning in one operational checklist before decision-engine integration.
- Introduces no runtime changes to application binaries or production data paths.

## Risk Assessment
**Rate:** Low

Primary risks:
- broad trigger terms may activate this skill for exploratory tasks that only need lightweight analysis;
- strict doctrine can increase analysis time before a signal is considered usable.

Risk mitigation:
- the description is focused on predictive signal validation and robust decision modeling;
- the statistical research orchestration rule can still choose quick phases (1-4) when appropriate.

## Consequences
- future signal work is audited against explicit phases rather than ad hoc correlation checks;
- weak or unstable signals are more likely to be rejected early;
- team receives reproducible and transparent failure reporting for model research.

## Alternatives Considered
1. **Store as personal skill (`~/.cursor/skills`)**
   - Rejected because repository-level sharing is preferred for team consistency.
2. **Create a shorter checklist-only skill**
   - Rejected because user requested a research-grade framework with detailed phase criteria.
3. **Remove `allowed-tools` frontmatter field**
   - Rejected to preserve user-provided contract and keep parity with existing local skills.

## Validation Steps
1. Confirm file exists at `.cursor/skills/statistical-research-engine/SKILL.md`.
2. Verify frontmatter includes `name`, `description`, and `allowed-tools`.
3. Verify all 10 validation phases are present and explicitly ordered.
4. Verify failure modes and output constraints are included.
