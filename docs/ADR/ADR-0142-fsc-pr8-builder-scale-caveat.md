# ADR-0142: FSC PR8 Builder Scale Caveat and Bounded Offline Reporting

Date: 2026-06-01

## Status

Accepted for FSC PR8 capture/evidence closure only.

## Context

The FSC v2 PR8 runtime capture lane produced usable capture/evidence artifacts
for `shadow-burnin-v3-fsc-capture-fastloop-r2`, but a later one-shot final
offline rebuild attempted to process a multi-million-row `system.transfers`
JSONL file with `scripts/build_fsc_v2_provider_qualification.py`.

The previous builder implementation loaded all transfer rows into Python lists
and then built additional indexes and parameter-grid structures. On the current
15 GiB RAM host with no swap, this created host-level memory and IO pressure and
made full-scan rebuilds operationally unsafe.

This is a builder scalability problem, not a runtime FSC semantics change.

## Decision

`scripts/build_fsc_v2_provider_qualification.py` may use bounded/windowed
transfer processing for FSC PR8 capture/evidence reporting.

The bounded mode must:

- keep FSC as capture/evidence only;
- keep active Gatekeeper FSC policy disabled;
- keep hard reject disabled;
- keep Program Streams out of the R2 SSOT contract;
- preserve UNKNOWN, NEUTRAL and low-coverage semantics;
- avoid treating low coverage as organic-positive evidence;
- avoid treating FSC reports as replacements for Phase 1 dataset artifacts;
- emit explicit scope metadata in the manifest and parameter-grid report.

Required manifest fields:

```text
parameter_grid_scope = full | windowed | sampled
transfer_processing_mode = streaming | full_scan | bounded_tail
builder_scale_caveat = true | false
```

If the builder uses bounded/windowed rows, it must not claim full provider
qualification.

## Scope

In scope:

- `scripts/build_fsc_v2_provider_qualification.py`
- its tests
- the PR8/Phase 0 plan amendment
- this ADR/status record

Out of scope:

- Ghost runtime
- Seer ingest
- FundingSourceIndex runtime semantics
- MaterializedFeatureSet
- Gatekeeper policy
- decision or hard-reject enablement
- R2 SSOT
- Phase 1 candidate/lifecycle/feature/training-view builders

## Consequences

The builder can be used safely for canary/windowed evidence summaries on large
raw NLN capture files without loading the entire transfer stream into memory.

The stronger contract remains unchanged for future full provider qualification
or any active FSC scoring decision:

```text
full provider qualification = NOT_CLAIMED
active FSC policy = OFF
hard reject = OFF
provider independent benchmark = NOT_AVAILABLE unless an external audit feed exists
```

Phase 1 may consume FSC only as optional supporting evidence with explicit
status, coverage and degraded-reason metadata. FSC is not a denominator source,
not a label, not R2 SSOT and not an active Gatekeeper signal.
