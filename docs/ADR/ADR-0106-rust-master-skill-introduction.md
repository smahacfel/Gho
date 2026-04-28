# ADR-0106: Rust Master Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Context
User requested creation of a Cursor skill focused on advanced Rust engineering topics, including ownership/lifetimes, unsafe code, FFI, async internals, trait system depth (HRTB, GAT, TAIT), variance, pinning, macros, and robust error handling.

The provided draft frontmatter used `name: Rust-Master`, but Cursor skill naming conventions require lowercase kebab-case identifiers. The task also required preserving the user intent and practical trigger conditions for automatic skill selection.

## Decision
Create a project-scoped skill at:
- `.cursor/skills/rust-master/SKILL.md`

Implemented decisions:
- normalize skill name to `rust-master` for compatibility with skill metadata constraints;
- keep the requested advanced Rust scope and original principle set;
- preserve `allowed-tools` as requested in the draft;
- extend the skill with actionable sections (`Working Rules`, `Review Checklist`) to make it execution-ready, not only descriptive.

## Architectural Impact
- Adds a reusable project-level AI capability for advanced Rust tasks.
- Improves consistency of Rust implementation and review standards in this repository.
- Introduces no runtime behavior changes to application code.

## Risk Assessment
**Rate:** Low

Primary risks:
- overly broad trigger scope could apply the skill in tasks that do not need high-complexity Rust guidance;
- preserving non-required metadata (`allowed-tools`) depends on Cursor parser tolerance.

Risk mitigation:
- description explicitly targets complex and performance-critical Rust scenarios;
- skill body is concise and operational, limiting ambiguity.

## Consequences
- Future Rust-heavy tasks gain consistent guardrails for safety, performance, and error handling.
- Team members can reuse one SSOT-style skill instead of repeating advanced guidance in chat.
- Slight increase in repository policy surface (one additional skill and ADR document).

## Alternatives Considered
1. **Personal skill (`~/.cursor/skills`) instead of project skill**
   - Rejected to keep shared behavior in-repo and available to all collaborators.
2. **Keep original `Rust-Master` casing**
   - Rejected due to naming constraints in skill metadata conventions.
3. **Minimal skill body without checklist sections**
   - Rejected because practical execution quality would be lower in complex Rust tasks.

## Validation Steps
1. Verify file exists and is discoverable at `.cursor/skills/rust-master/SKILL.md`.
2. Confirm frontmatter uses a valid normalized `name`.
3. Confirm description includes both WHAT and WHEN triggers.
4. Confirm guidance explicitly enforces no-`unwrap()` and safe `unsafe` boundaries.
