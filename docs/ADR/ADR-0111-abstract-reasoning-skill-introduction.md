# ADR-0111: Abstract Reasoning Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested a new Cursor skill for deep abstract reasoning under uncertainty, with explicit phase-based decomposition, contradiction testing, assumption handling, refutation, and bounded-confidence synthesis.

The requested behavior targets ill-defined and open-ended problems where procedural templates are insufficient and where long-horizon reasoning quality is more important than fast, shallow pattern matching.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/abstract-reasoning/SKILL.md`

Implemented decisions:
- keep requested skill identity (`name: abstract-reasoning`);
- preserve requested frontmatter style including `allowed-tools`;
- keep ten-phase reasoning flow with mandatory decomposition, alternatives, trade-offs, contradiction checks, refutation, and confidence reporting;
- include explicit handoff rules to specialist skills to avoid misuse of generic reasoning for domain-heavy tasks.

## Architectural Impact
- Adds a reusable repository-level reasoning framework for underspecified, high-stakes decisions.
- Standardizes how uncertainty, assumptions, and contradictions are exposed before implementation choices.
- Introduces no runtime changes in production binaries or Solana execution paths.

## Risk Assessment
**Rate:** Low

Primary risks:
- strict phase sequencing may increase response length and latency for simple tasks;
- broad activation terms ("think", "reason") may trigger for problems that do not need deep decomposition.

Risk mitigation:
- skill includes scope control and allows clarification/handoff before over-committing;
- specialist escalation rules reduce risk of replacing technical skills with generic reasoning.

## Consequences
- reasoning outputs become more auditable and falsifiable;
- assumptions and uncertainty are less likely to remain implicit;
- solution quality under novelty/adversarial conditions should improve at the cost of additional reasoning overhead.

## Alternatives Considered
1. **Store as personal skill (`~/.cursor/skills`)**
   - Rejected because project-level sharing and consistency are preferred for this repository.
2. **Create a shorter heuristic-only version**
   - Rejected because user explicitly requested deep, phase-driven, contradiction-aware reasoning.
3. **Merge into an existing technical skill**
   - Rejected because abstract reasoning is cross-cutting and should remain reusable across domains with clear handoff boundaries.

## Validation Steps
1. Confirm file exists at `.cursor/skills/abstract-reasoning/SKILL.md`.
2. Verify frontmatter includes `name`, `description`, and `allowed-tools`.
3. Verify phase-based operating model and review checklist are present.
4. Verify escalation rules reference specialist skills.
