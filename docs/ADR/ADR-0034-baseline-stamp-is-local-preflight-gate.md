# ADR-0034: Baseline stamp is a local preflight gate

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

A blocker was raised claiming that production startup could not proceed because `.ghost/baseline_accepted_revision` did not exist, causing `scripts/ghost_production_preflight.sh` to fail before any config validation.

Investigation showed three relevant facts:

1. `scripts/ghost_production_preflight.sh` intentionally requires a baseline stamp before runtime preflight starts.
2. `docs/RUNBOOK_PRODUCTION_ROLLOUT.md` already defines the operator step that creates the stamp after green baseline checks.
3. `.ghost/*` is git-ignored except for `.ghost/.gitkeep`, so the stamp is intentionally local state, not tracked repo state.

The observed confusion came from operator shortcuts lacking the baseline-stamp creation step and from the script error message not explaining how to remediate a missing stamp.

## Decision

Keep `.ghost/baseline_accepted_revision` as a mandatory local rollout gate.

Do not track the baseline stamp in git. Instead:

- keep the authoritative procedure in the rollout runbook,
- surface the baseline-stamp creation command in `POLECENIA_SKROT.md`, and
- make `scripts/ghost_production_preflight.sh` print an explicit remediation hint when the stamp is missing, empty, or stale.

This preserves the operational intent of the gate while removing ambiguity for operators.

## Architectural Impact

The baseline stamp remains a procedural SSOT for rollout acceptance of the current git revision. The gate stays outside tracked repository state and outside runtime configuration parsing.

This keeps a clean separation between:

- tracked rollout config,
- ignored local operational approval state, and
- runtime preflight validation.

## Risk Assessment

**Risk:** Low

No runtime behavior, account layout, or execution logic changed. The update only clarifies operator workflow and improves failure remediation for an existing gate.

The only practical risk would be operators misusing stale local stamps; that risk already exists and remains explicitly checked by matching the file content to `git rev-parse HEAD`.

## Consequences

Operators must continue generating or refreshing the baseline stamp locally after green baseline checks.

Production startup is intentionally blocked when the stamp is absent, empty, or stale. The difference after this decision is that the failure mode is now self-remediating and more visible in shortcut documentation.

## Alternatives Considered

### Track `.ghost/baseline_accepted_revision` in git

Rejected because the stamp is meant to represent local acceptance of the currently checked-out revision, not repository content that should follow commits across clones.

### Remove the baseline gate from preflight

Rejected because PR-6 explicitly requires operational gates before rollout and this file is part of that contract.

### Leave the current behavior unchanged

Rejected because it keeps operator confusion alive even though the gate itself is correct.

## Validation Steps

1. Run preflight with a missing baseline path and confirm exit code `1` plus remediation hints.
2. Run normal preflight with a valid local stamp and confirm baseline passes and runtime checks continue.
3. Verify `POLECENIA_SKROT.md` contains the baseline-stamp creation command before the preflight command.
4. Verify `.gitignore` still ignores `.ghost/*` except `.ghost/.gitkeep`.
