# ADR-0112: Trading Systems Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested a new Cursor skill focused on selective autonomous trading-system architecture, including decision engines, score interpretation, order routing, risk and sizing controls, execution orchestration, and mandatory post-trade reconciliation.

Requested doctrine explicitly prioritizes selectivity, explainability, hard safety bounds, explicit state transitions, and graceful failure handling over raw execution speed.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/trading-systems/SKILL.md`

Implemented decisions:
- keep requested skill identity (`name: trading-systems`);
- preserve requested `description` and `allowed-tools` metadata;
- encode a strict, phase-ordered lifecycle (0-9) from system boundary to observability;
- enforce explicit handoff boundaries for specialized domains instead of embedding specialist logic;
- require hard risk gates, idempotency, reconciliation, and failure classification as first-class constraints.

## Architectural Impact
- Adds a reusable repository-level operating doctrine for selective execution systems.
- Standardizes the decision-to-execution pipeline with explicit risk precedence and reconciliation closure.
- Improves auditability by requiring reason codes, decision journaling, and explicit uncertainty handling.
- Introduces no direct runtime code-path changes in production binaries.

## Risk Assessment
**Rate:** Low

Primary risks:
- broad skill scope may be activated for tasks that only need narrow implementation guidance;
- strict governance may increase design and review effort for fast experimentation loops.

Risk mitigation:
- phase model supports incremental execution while preserving hard safety boundaries;
- explicit handoff rules reduce cross-domain coupling and hidden assumptions.

## Consequences
- trading changes become more consistent in structure and review quality.
- unsafe shortcuts (risk bypass, stale-signal execution, implicit state mutation) are harder to introduce.
- reconciliation-first behavior is documented as a non-optional completion condition.

## Alternatives Considered
1. **Store as personal skill (`~/.cursor/skills`)**
   - Rejected because this doctrine should be shared at repository level.
2. **Create a short checklist-only skill**
   - Rejected because requested scope requires phase ordering, failure taxonomy, and explicit handoffs.
3. **Merge rules into existing Solana-focused skill**
   - Rejected because trading-system governance spans domains beyond Solana execution and needs a dedicated boundary.

## Validation Steps
1. Confirm file exists at `.cursor/skills/trading-systems/SKILL.md`.
2. Verify frontmatter includes `name`, `description`, and `allowed-tools`.
3. Verify phases 0-9, risk gates, execution discipline, and reconciliation sections are present.
4. Verify specialist handoff rules and failure-mode taxonomy are explicitly defined.
