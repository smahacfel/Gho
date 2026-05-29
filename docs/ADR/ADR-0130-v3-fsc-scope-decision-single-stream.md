# ADR-0130: V3 FSC scope decision under single-stream provider constraint

**Date:** 2026-05-16
**Status:** Accepted
**Author:** Ghost Father
**Amended by:** `ADR-0138-fsc-v2-nln-program-streams-capture-evidence.md` for NLN Program Streams
capture/evidence only; active Gatekeeper use remains out of scope.

## Context

`FSC` (`funding_source_concentration`) was designed as an authoritative funding-chain signal.
The existing FSC contract deliberately requires a real full-chain funding-transfer lane before
the signal can become policy-effective. This is consistent with earlier decisions:

- `ADR-0096`: do not derive FSC from pool-local or partial trade observations.
- `ADR-0101`: missing authoritative funding stream must remain fail-closed / degraded, not fake-clean.
- `ADR-0102`: authoritative FSC requires a separate funding lane and must not be unlocked by
  flipping `full_chain_coverage` on the filtered primary stream.

During V3 P3.4, this requirement became operationally incompatible with the current provider
constraint:

- the current Yellowstone endpoint allows only one concurrent stream,
- there is no second endpoint available for a dedicated full-chain funding lane,
- there is no planned provider-side change that would make a second stream available,
- r8 with `funding_lane_mode = "full_chain"` consumed/competed for stream capacity and did not
  produce usable primary decision rows,
- r9 with `funding_lane_mode = "disabled"` produced fresh primary-only rows, passed strict full
  replay, and enabled real counterfactual ablation.

Therefore FSC is not blocked by local scoring code at this point. It is blocked by an unavailable
data-plane prerequisite.

## Decision

For the current V3 validation and promotion path, FSC is **de-scoped as an authoritative dependency**.

Operational meaning:

1. V3 validation profiles should run primary-only with `seer.funding_lane_mode = "disabled"` unless
   a future infrastructure change explicitly provides authoritative funding coverage without
   starving the primary stream.
2. Degraded or unavailable FSC must not be interpreted as negative evidence against a pool in the
   current V3 calibration path.
3. FSC must not be used as a hard gate, promotion prerequisite, or required evidence source for
   V3 P3/P3.5/P2 under the single-stream constraint.
4. Existing FSC code remains in the repository as dormant/fail-closed/diagnostic infrastructure.
   We are not deleting it now, because removal would create unnecessary churn and could break
   existing audit surfaces.
5. Future FSC activation requires a new ADR or explicit amendment proving that funding coverage is
   authoritative and does not reduce primary decision coverage.

This is a scope decision, not a claim that FSC is conceptually useless. FSC remains a potentially
valuable anti-sybil signal if the data-plane can support it later.

## Consequences

Positive:

- V3 validation is no longer blocked by an unavailable second stream.
- The project avoids false rejects caused by missing funding data.
- Primary pool detection and decision-row generation regain priority.
- P3/P3.5 work can proceed toward measurable decision quality: full replay, real ablation,
  shadow lifecycle economics, and outcome labels.
- Existing FSC implementation remains available for future use without introducing deletion risk.

Negative:

- V3 cannot claim full funding-chain or FSC completeness in the current validation cycle.
- Funding-source concentration cannot be used to detect shared-funder clusters in the current V3
  promotion decision.
- Some sybil/funding-cabal risks must be handled by other available signals or remain residual
  risk until infrastructure changes.

## Plan Impact

This changes the next V3 path:

1. Do not retry canonical full-chain P3.4 runs under the current one-stream provider constraint.
2. Treat r9-style primary-only full replay as the valid runtime shape for current P3.4/P3.5 work.
3. Move P3.5 toward outcome-label join and shadow lifecycle economics on full-replay primary-only
   runs.
4. Keep `NO P2 promotion` until V3 quality is measured against outcome labels, not merely against
   replay/ablation mechanics.
5. Keep `REJECT_V3_MANIPULATION_CONTRADICTION` under targeted evaluation, because it remains
   decision-causal in ablation, but do not let missing FSC become its hidden justification.

## Rejected Alternatives

### Delete FSC code now

Rejected.

This would cost more work, increase regression risk, and remove useful diagnostics. The current
problem is not that FSC code exists; the problem is that authoritative funding data is not available.

### Keep retrying full-chain runs

Rejected under the current provider constraint.

The observed result is operational starvation: a second funding lane competes with the primary
stream and prevents decision evidence from being generated.

### Treat the filtered primary stream as full-chain funding coverage

Rejected.

That would violate the existing FSC contract and create false confidence. A filtered primary stream
can remain useful for pool detection, but it must not be relabeled as authoritative funding coverage.

### Remove all filters from the single primary stream

Rejected for the current plan.

Removing all filters may increase raw coverage, but it also risks overwhelming the only available
stream, reducing pool-detection reliability, increasing latency/backpressure, and turning a selective
decision runtime into an unbounded data-ingest experiment. It is a separate ingest-capacity project,
not a low-cost V3 validation fix.

## Revisit Conditions

FSC may be reconsidered only if at least one condition is true:

- the provider offers independent stream capacity for an authoritative funding lane,
- Ghost has a proven single-stream ingest design that preserves primary pool coverage while also
  producing replay-safe authoritative funding-transfer coverage,
- offline historical funding reconstruction becomes available as an audit-only feature and is kept
  outside the hot decision path,
- a separate plan explicitly proves that FSC can be reintroduced without false rejects, hidden live
  behavior, or degraded primary coverage.

Until then, FSC remains disabled/degraded/diagnostic for V3 validation.

## Non-goals

This ADR does not authorize:

- P2 promotion,
- V3 threshold tuning,
- active V2/V2.5 policy changes,
- IWIM changes,
- live sender or execution changes,
- deletion of FSC implementation,
- relabeling partial funding observations as full-chain evidence.
