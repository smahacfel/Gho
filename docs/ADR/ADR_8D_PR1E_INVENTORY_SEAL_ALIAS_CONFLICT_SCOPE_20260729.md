# ADR-8D — PR1E: Candidate-local scope for inventory-seal alias conflicts

**Date:** 2026-07-29
**Status:** Implemented; qualifying smoke remains required
**Scope:** CandidateIntegrity inventory proof sealing in `ghost-launcher/src/components/seer.rs`

## 1. Decision

When `seal_complete_transaction_inventory` returns
`CandidateIntegrityErrorV1::CandidateAliasConflict`, the affected canonical
mutation is blocked and its staged receipt is reclaimed. The conflict is
candidate-local and does not close new-candidate admission.

All other inventory-seal errors retain the prior global fail-closed treatment:
the receipt is failed, coverage-incomplete evidence is recorded, and admission
is closed.

## 2. Root cause evidence

The smoke `ace-core-one-day-probe-r1-qualifying-smoke-20260729t122500z`
recorded the first close precursor at `2026-07-29T12:24:43.225Z`:

```text
Seer: complete transaction inventory could not seal apply fence
signature=2VfL... candidate pool=DEzR... mint=Saros...
```

The immediately following integrity-signal record for that same candidate
reported `candidate identity aliases disagree`. The appropriate root-cause
class is `INVENTORY_SEAL_FAILURE`, with the observed local error surface
`CandidateAliasConflict`; it is not evidence of a poisoned mutex.

## 3. Safety invariants

- The conflicting canonical mutation receives no runtime permit and reaches no
  Event Bus, MFS, Gatekeeper, Trigger, or execution path.
- The staged receipt/proof fence is reclaimed. A reclamation failure still
  closes admission globally.
- No poisoned mutex recovery, registry reset, or admission reopening is used.
- Capacity, receipt identity, inventory proof, registry availability, and
  other inventory-seal failures remain globally fail-closed.

## 4. Focused proof

`inventory_seal_alias_conflict_blocks_only_conflicting_candidate` constructs a
Ready inventory proof whose pool alias collides with an existing identity. It
requires `Blocked`, no remaining fence, invalidated existing alias evidence,
and an open unrelated-admission gate.

## 5. Out of scope

No ACE selector/outcome/quote/capacity change; no Gatekeeper, MFS, Position
Manager, Trigger, PR2, health-helper redesign, PR, or review is part of this
change. Day 1 remains prohibited until a fresh qualifying smoke passes.
