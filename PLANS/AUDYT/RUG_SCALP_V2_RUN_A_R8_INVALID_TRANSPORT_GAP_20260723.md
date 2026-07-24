# RUG SCALP V2 — Run A R8: INVALID_TRANSPORT_GAP

**Status:** `INVALID_TRANSPORT_GAP`  
**Alpha:** `ALPHA_NOT_EVALUATED`  
**Recorded:** 2026-07-23 UTC

## Decision

Run A R8 was terminated after a Yellowstone/gRPC silent stall.  It must not
contribute to final EV, frequency, or prospective validation statistics.

## Evidence

- Run start: `2026-07-23T01:25:49.158Z`.
- First silent-stall detection: `2026-07-23T06:31:13.288Z`.
- The detector reported no source message for `21,796 ms`, then the watchdog
  remained in `SUBSCRIBING` / reconnecting state.
- The process was terminated intentionally at `2026-07-23T09:03:43Z`.

## Discovery-only prefix

Only events from the continuous prefix are admissible for the reality-reset
offline analysis:

```text
start:                  Run A R8 start
excluded boundary slot: 434663909
included through slot:  434663908
reason:                 no explicit SLOT_COMPLETE record proves boundary-slot
                        completeness before the stall
```

All later reconnect data are excluded.  The retained prefix is discovery-only:
it may be used for attrition, rug-like labeling, opportunity-envelope work,
and a chronological discovery/hold-out split, but never for final prospective
EV or the Run A denominator.
