# ADR-0107: Solana Pump.fun Architect Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested creation of a Cursor skill for advanced Solana and pump.fun engineering focused on low-latency, selective trading systems. The requested scope included on-chain program design (Anchor, PDAs, CPI), token program handling (SPL Token and Token-2022), transaction shaping (compute budget, priority fees), real-time event ingestion (Geyser/WebSocket/RPC), and resilient off-chain orchestration for sniper-style execution without HFT/MEV orientation.

The task required preserving the provided structure and non-negotiable engineering rules while making the skill immediately usable as a project-level capability.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/solana-pumpfun-architect/SKILL.md`

Implemented decisions:
- keep the exact requested skill identity (`name: solana-pumpfun-architect`) and high-specificity domain description;
- preserve `allowed-tools` metadata as provided;
- retain the full rule set for invariants, stale-data handling, deduplication, and idempotent execution;
- keep explicit separation between observation, scoring, execution, and reconciliation stages.

## Architectural Impact
- Adds a reusable project-level AI skill specialized for Solana + pump.fun selective trading engineering.
- Standardizes safety and correctness guardrails for both on-chain and off-chain implementation/review tasks.
- Introduces no runtime changes to application binaries or protocol behavior.

## Risk Assessment
**Rate:** Low

Primary risks:
- the broad domain scope may cause skill activation in tasks that only partially match Solana/pump.fun context;
- preserving non-required frontmatter (`allowed-tools`) depends on parser tolerance in current Cursor behavior.

Risk mitigation:
- description is tightly scoped to Solana + pump.fun selective trading workflows;
- the body emphasizes deterministic, explainable, conservative execution and explicit validation.

## Consequences
- Future Solana/pump.fun tasks gain consistent engineering standards and checklist-driven reviews.
- Improves repeatability for low-latency event ingestion and execution-path hardening.
- Expands repository governance surface by one additional project skill and ADR.

## Alternatives Considered
1. **Create as personal skill (`~/.cursor/skills`)**
   - Rejected to keep team-shared behavior in repository scope.
2. **Trim guidance to a minimal short skill**
   - Rejected because the requested domain requires explicit operational constraints and failure taxonomy.
3. **Drop `allowed-tools` field**
   - Rejected to preserve user-provided contract unless incompatibility is observed.

## Validation Steps
1. Verify file exists at `.cursor/skills/solana-pumpfun-architect/SKILL.md`.
2. Confirm frontmatter includes valid normalized `name` and detailed `description`.
3. Confirm the skill includes explicit Solana runtime, token, and off-chain orchestration guardrails.
4. Confirm non-HFT/non-MEV boundary is explicitly retained.
