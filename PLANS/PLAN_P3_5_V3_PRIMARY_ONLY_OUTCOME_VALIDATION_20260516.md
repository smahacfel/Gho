# Plan P3.5: V3 primary-only outcome validation after FSC de-scope

Date: 2026-05-16
Status: Proposed next work
Related ADR: `docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md`

## Executive decision

P3.5 will not wait for `FSC` / full-chain funding completeness.

The current provider allows only one Yellowstone stream. A dedicated full-chain funding lane is
therefore not operationally available without starving the primary stream that produces decision
rows. Under this constraint, authoritative `FSC` is removed from the critical path for V3 validation.

The next useful validation target is not "more FSC infrastructure". It is:

```text
primary-only full replay + real ablation + shadow lifecycle / outcome labels
```

## Goal

Answer whether V3 is economically and decision-quality useful compared with V2/V2.5, using evidence
that the current runtime can actually produce.

P3.5 should measure:

- which V3 `REJECT` / `PENDING` decisions avoided bad entries,
- which V3 `REJECT` decisions would have blocked economically good entries,
- whether `REJECT_V3_MANIPULATION_CONTRADICTION` is mostly protective or over-conservative,
- whether V3 adds decision value beyond V2/V2.5 under primary-only evidence.

## In scope

- Use V3 full-replay rows from primary-only profiles such as r9.
- Keep `funding_lane_mode = "disabled"` for V3 validation profiles unless a new ADR changes the
  infrastructure contract.
- Join V3 decisions with shadow lifecycle / outcome labels where available.
- Extend reports so sponsors can see quality metrics rather than only replay mechanics.
- Keep full replay strict mode as a prerequisite for any quality claim.
- Keep real counterfactual ablation as the explanation layer for why a verdict changed.

## Out of scope

- No P2 promotion.
- No live behavior changes.
- No active V2/V2.5 policy changes.
- No IWIM or execution/live sender changes.
- No FSC activation.
- No threshold tuning to make V3 look better.
- No relabeling filtered primary observations as full-chain funding evidence.

## Operational contract

For current V3 validation runs:

```toml
[seer]
funding_lane_mode = "disabled"
```

This is not a claim that FSC is healthy. It is a deliberate decision to keep the only available
stream focused on primary decision evidence.

Every P3.5 report must state:

- `FSC` is de-scoped for this validation cycle,
- missing/degraded FSC is not a negative decision signal,
- full funding-chain completeness is not being claimed,
- no P2 promotion follows from primary-only evidence alone.

## Acceptance gates

P3.5 can be considered analytically useful when all of these are true:

1. The selected run has fresh V3 rows.
2. `scripts/v3_full_replay_report.py --strict --json` returns:
   - `status=ok`,
   - `replay_status=full_replay_ok`,
   - `bad_rows=0`.
3. `scripts/v3_replay_ablation_report.py --json` runs in `full_replay_counterfactual` mode.
4. Outcome-label or shadow lifecycle coverage is reported explicitly.
5. V3 vs V2/V2.5 comparison reports at least:
   - avoided bad-entry count/rate,
   - blocked good-entry count/rate,
   - unknown / unlabeled count/rate,
   - net interpretation by reason bucket.
6. `REJECT_V3_MANIPULATION_CONTRADICTION` is evaluated as a decision-causal bucket, not as a generic
   "V3 rejected something" aggregate.

## Failure gates

P3.5 remains blocked if:

- replay falls back to `hash_only`,
- strict full replay fails,
- outcome labels are too sparse to support economic interpretation,
- V3 quality cannot be separated from V2/V2.5 behavior,
- missing FSC is used as a hidden reason to reject or approve pools,
- any step proposes P2 without outcome-quality evidence.

## What this changes in the roadmap

Previous wording that required "another canonical full-chain run" is superseded under the current
single-stream provider constraint.

The replacement path is:

1. freeze the FSC de-scope decision in ADR-0130,
2. use primary-only full replay as the valid current evidence shape,
3. build or extend outcome-label joins,
4. compare V3/V2.5 quality with explicit success/failure metrics,
5. only then decide whether V3 deserves more investment, tuning, or rejection.

## Sponsor-level success measure

V3 succeeds only if it improves capital protection or opportunity selection versus V2/V2.5.

Engineering readiness is necessary but not sufficient. The business question is:

```text
Does V3 reject more bad opportunities without rejecting too many good ones?
```

P3.5 is the phase that starts answering that question directly.

## Recommended next implementation step

Extend the reporting layer to join V3 decision rows with shadow lifecycle / outcome labels and emit
a sponsor-readable quality table:

- V2/V2.5 decision,
- V3 decision,
- V3 reason bucket,
- lifecycle/outcome label,
- whether V3 helped, hurt, or remained inconclusive.

This is lower-risk and more valuable than continuing to chase FSC under a known provider constraint.
