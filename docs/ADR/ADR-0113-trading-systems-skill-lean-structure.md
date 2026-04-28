# ADR-0113: Trading Systems Skill Lean Structure

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
Initial `trading-systems` skill was delivered as a single, very detailed `SKILL.md` document. While complete, the main file exceeded preferred size guidance for fast skill loading and context efficiency.

## Decision
Refactor the skill into a lean main file with progressive disclosure:
- keep concise operational contract in `.cursor/skills/trading-systems/SKILL.md`
- move full phase-by-phase specification to `.cursor/skills/trading-systems/reference.md`
- link detailed content from `SKILL.md` via one-level-deep reference

## Architectural Impact
- Improves context efficiency and activation speed for routine tasks.
- Preserves the full governance model without dropping risk or reconciliation requirements.
- Keeps discovery behavior stable through unchanged metadata (`name`, `description`, `allowed-tools`).

## Consequences
- Default interactions use a shorter, high-signal control plane.
- Deep implementations still have full reference coverage when needed.
- Lower prompt bloat risk during multi-skill composition.

## Alternatives Considered
1. **Keep monolithic SKILL.md**
   - Rejected due to token overhead and reduced agility.
2. **Aggressively delete details**
   - Rejected because it would weaken risk and integrity doctrine.
3. **Split into many nested docs**
   - Rejected to avoid deep reference chains and partial-read risk.

## Validation Steps
1. Verify `.cursor/skills/trading-systems/SKILL.md` remains under 500 lines.
2. Verify `reference.md` contains detailed Phases 0-9 and failure/uncertainty policy.
3. Verify `SKILL.md` links to `reference.md` directly (one level deep).
4. Verify metadata in `SKILL.md` remains unchanged.
