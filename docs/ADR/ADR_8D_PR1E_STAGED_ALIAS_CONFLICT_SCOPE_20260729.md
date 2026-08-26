# ADR-8D — PR1E: Candidate-local scope for a staged alias conflict

**Date:** 2026-07-29
**Status:** Implemented; qualifying smoke remains required
**Scope:** `ghost-launcher/src/components/seer.rs` CandidateIntegrity admission boundary

## 1. Decision

`CandidateIntegrityErrorV1::CandidateAliasConflict` is treated as an
identity-local integrity failure even if the associated canonical mutation has
already staged its apply receipt. The current mutation is blocked, emits no
runtime permit, and its staged receipt/proof fence is reclaimed. The registry
does **not** close new-candidate admission solely for that error.

All other errors in the canonical-lifecycle signal path retain the existing
fail-closed global admission closure.

## 2. Evidence

The fresh smoke
`ace-core-one-day-probe-r1-qualifying-smoke-20260729t120100z` recorded the
first global-close precursor at `2026-07-29T12:07:36.147Z`:

```text
Seer: CandidateIntegrity canonical-lifecycle update failed;
candidate_pool=CJDpioWK2BXV4yFVYVi5RpauHSPmy4WDrc5enovPu3VH
candidate_mint=CT7GDh5V8jR1vF83HDacMvdMF3KwuH4WXyaD2F6ppump
outcome=PrimaryRawCoverageIncomplete
error=candidate identity aliases disagree
```

The later `RegistryUnavailable` / legacy display text mentioning mutex poison
was downstream of this close. No preceding panic, poison marker, registry
unavailability, ledger failure, inventory-seal failure, capacity failure, or
coverage-gap marker was found. The root-cause class is therefore
`OTHER_EXACTLY_IDENTIFIED` — a staged `CandidateAliasConflict`, not a proven
mutex poison.

## 3. Safety contract

- No `PoisonError::into_inner()` recovery is introduced.
- No registry reset/reopen occurs.
- A conflicting canonical mutation remains `Blocked` and cannot enter the
  Event Bus, MFS, Gatekeeper, Trigger, or execution.
- `fail_staged_canonical_runtime_admission` still reclaims the receipt fence.
- Actual registry failures, inventory failures, capacity failures, and
  admission closure retain global fail-closed behavior.

## 4. Verification

The focused test covers the exact stage → alias conflict → block/no permit →
reclaimed fence sequence and now additionally requires that unrelated
admission stays open. The existing before-receipt alias-conflict test covers
the same local scope without a staged fence.

Release build and a new isolated 120–300 second qualifying smoke are required
after this source change. Day 1 remains prohibited unless that smoke has zero
required PR1E counters, a valid health receipt, and `verify-probe` passes.

## 5. Out of scope

No ACE selector/outcome/quote/capacity change; no Gatekeeper, MFS, Position
Manager, Trigger, PR2, health-helper redesign, PR, or review is part of this
decision.
