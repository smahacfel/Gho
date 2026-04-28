# ADR-0110: Large Data Analytics Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested a new Cursor skill focused on large-scale analytics, hidden pattern discovery, correlation mining, time-series and sequence analysis, anomaly detection, and feature engineering for noisy high-volume datasets.

Requested behavior is explicitly falsification-oriented: discovery is not proof, correlation is not causation, and candidate structures must be validated for stability and operational usefulness before downstream use.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/large-data-analytics/SKILL.md`

Implemented decisions:
- keep requested skill identity (`name: large-data-analytics`);
- preserve `allowed-tools` metadata as provided;
- encode strict separation between exploratory discovery and accepted signal validation;
- include mandatory handoff format for promoted patterns to prevent direct unvalidated execution coupling.

## Architectural Impact
- Adds a reusable, repository-scoped doctrine for data-heavy discovery tasks.
- Standardizes correlation/pattern/anomaly workflows before candidate features enter scoring research.
- Introduces no runtime changes to production binaries, Solana transaction flows, or execution pipelines.

## Risk Assessment
**Rate:** Low

Primary risks:
- broad activation scope may be used for tasks that only require lightweight descriptive statistics;
- comprehensive checklist may increase analysis cycle time during exploration.

Risk mitigation:
- skill language emphasizes validation gates and rejection of fragile structure;
- existing statistical orchestration rules can still constrain work to quick checks where appropriate.

## Consequences
- discovered structures are less likely to be promoted without temporal and out-of-sample validation;
- feature candidates are documented in a consistent handoff schema for downstream consumers;
- analytical outputs become more reproducible, auditable, and decision-oriented.

## Alternatives Considered
1. **Store as personal skill (`~/.cursor/skills`)**
   - Rejected because repository-level sharing is preferred for team consistency.
2. **Create a shorter heuristic-only skill**
   - Rejected because user requested robust, validation-centric governance over large-data discovery.
3. **Allow direct pattern-to-execution promotion**
   - Rejected because it increases leakage, fragility, and false-positive operational risk.

## Validation Steps
1. Confirm file exists at `.cursor/skills/large-data-analytics/SKILL.md`.
2. Verify frontmatter includes `name`, `description`, and `allowed-tools`.
3. Verify discovery and validation are explicitly separated.
4. Verify candidate feature handoff schema is present and complete.
