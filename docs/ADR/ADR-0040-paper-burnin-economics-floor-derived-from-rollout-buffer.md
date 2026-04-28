# ADR-0040: Paper burn-in economics floor derived from rollout buffer

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

The paper burn-in formal report previously failed `economics_not_fatal` whenever aggregate `net_pnl_sol` from closed `PositionClosed` events was below `0.0`, unless the operator explicitly passed `--min-net-pnl-sol`.

That default was too strict for current paper lifecycle semantics:

- `ghost-brain` paper `PositionClosed.net_pnl_sol` is computed from synthetic entry/exit notional only,
- no wallet balance delta is emitted into the burn-in artifacts,
- no paper wallet ledger or equity snapshot is present in report inputs,
- the rollout config already defines wallet safety budget using:
  - `emergency_floor_sol`
  - `position_size_buffer_sol`
  - `max_position_size_sol`

This created a mismatch: a tiny synthetic loss like `-5.771084625501386e-06 SOL` could force `NO-GO` despite being negligible relative to the configured rollout wallet buffer.

## Decision

When `--min-net-pnl-sol` is not explicitly provided, the report now derives the default economics floor from rollout config:

$$
\text{effective\_min\_net\_pnl\_sol} = -\text{position\_size\_buffer\_sol}
$$

This makes the default economics gate wallet-context aware using the existing rollout safety model.

Rules:

1. If operator provides `--min-net-pnl-sol`, that explicit floor is used unchanged.
2. Otherwise, if `[trigger].position_size_buffer_sol > 0`, the floor becomes `-position_size_buffer_sol`.
3. If no positive buffer is available, the report falls back to `0.0`.

The report JSON now also exposes:

- `configured_min_net_pnl_sol`
- `economics_floor_source`
- `max_position_size_sol`
- `emergency_floor_sol`
- `position_size_buffer_sol`

## Architectural Impact

This changes report semantics only:

- `scripts/shadow_run_report.py` now resolves an effective economics floor from rollout config when no explicit CLI floor is supplied.
- No runtime trade path, event schema, paper lifecycle, or Solana execution path is modified.
- Existing report consumers retain `min_net_pnl_sol` in the profile, but it now reflects the effective floor rather than an unconditional zero default.

## Risk Assessment

**Rate:** Medium

Risks:

- a report can now pass `economics_not_fatal` for small negative paper PnL values that previously failed by default,
- operators must understand that the default gate now represents wallet-buffer non-catastrophe, not strict non-negative paper PnL,
- downstream tooling that implicitly assumed default `min_net_pnl_sol == 0.0` may need to read `economics_floor_source`.

Mitigations:

- explicit `--min-net-pnl-sol` still overrides the derived floor,
- unit tests cover both within-buffer and beyond-buffer loss cases,
- runbook documentation was updated.

## Consequences

What becomes easier:

- distinguishing economically catastrophic burn-in sessions from trivial synthetic paper drift,
- aligning report verdicts with rollout wallet safety semantics,
- preventing false `NO-GO` outcomes caused by dust-scale negative synthetic paper traces.

What becomes harder:

- report interpretation now requires awareness of derived default floor semantics,
- strict non-negative economics now requires explicit operator intent.

## Alternatives Considered

1. **Keep default floor at `0.0`**
   - Rejected because it keeps failing sessions on wallet-irrelevant dust losses and contradicts the user's bug report.

2. **Derive default floor from full wallet budget (`emergency_floor + buffer + size`)**
   - Rejected because `emergency_floor` is a protected reserve, not tolerated loss budget.

3. **Derive default floor from `max_position_size_sol`**
   - Rejected because position size is trade notional, not explicit loss cushion.

4. **Add live wallet balance fetching to the report**
   - Rejected for this phase because the report must remain artifact-driven and offline-capable.

## Validation Steps

1. Run `tools/tests/test_shadow_run_report.py` and confirm the new economics tests pass.
2. Re-run the latest paper-burnin report and confirm:
   - `economics_not_fatal = passed`
   - `economics_floor_source = derived_from_position_size_buffer_sol`
3. Re-run the frozen pre-last-candidate slice and confirm full `GO` when lifecycle is complete.
4. Verify explicit CLI override still works, e.g. `--min-net-pnl-sol -0.001`.
