# P3.7-L1R11 / J3O — BondingCurveV2 Source Authority & Materialization Contract

Date: 2026-05-22

## Verdict

Status: CODE-LEVEL PASS / RUNTIME GATE REQUIRED

L1R11 implements the source-authority side of the `bonding_curve_v2` execution
contract. A `bonding_curve_v2` account that is only present because the route
builder placed it in transaction metas is no longer treated as execution-ready.
It must be materialized by an authoritative source before the shadow/probe path
can claim simulation readiness.

No policy thresholds, IWIM, live/P2 behavior, probe amount, slippage, or baseline
configs were changed.

## Problem

L1R10 showed that the remaining `bonding_curve_v2` blocker is not a generic
readiness delay:

```text
active_shadow_bcv2_not_ready_rows = 2
probe_bcv2_not_ready_rows = 3
classification = builder_pubkey_not_seen_in_diag
diag_seen_exact_pubkey_rows = 0
diag_seen_other_curve_pubkey_rows = 5
mfs_contains_bonding_curve_v2_key_rows = 0
mfs_contains_builder_bcv2_pubkey_rows = 0
```

That means the builder-required `bonding_curve_v2` pubkey is not the same account
that DIAG sees as the bonding curve, and MFS does not materialize the exact
builder pubkey. Treating a builder-only pubkey as ready would reintroduce the
same class of simulation-load mismatch that L1R5/L1R9 already moved to
fail-closed precheck behavior.

## Implementation

Runtime rows now carry additive `bonding_curve_v2` source-authority diagnostics:

```text
bonding_curve_v2_pubkey
bonding_curve_v2_source
bonding_curve_v2_authority_status
bonding_curve_v2_mismatch_reason
bonding_curve_pubkey_from_diag
bonding_curve_pubkey_from_mfs
bonding_curve_v2_seen_in_diag
bonding_curve_v2_seen_in_mfs
bonding_curve_v2_seen_in_account_state
bonding_curve_ready
bonding_curve_v2_ready
builder_required_curve_account_ready
```

The supported authority statuses are:

```text
authoritative_exact_diag
authoritative_mfs
authoritative_account_state
derived_unverified
builder_only
mismatch_diag_curve
not_materialized
unknown
```

Only the three `authoritative_*` statuses are treated as ready for simulation.
`route_builder` currently maps to:

```text
bonding_curve_v2_authority_status = builder_only
bonding_curve_v2_mismatch_reason = builder_pubkey_not_materialized
builder_required_curve_account_ready = false
```

For probe and active shadow paths, a non-authoritative builder-required
`bonding_curve_v2` fails before `simulate_buy` with:

```text
bonding_curve_v2_source_not_authoritative:<status>:<source>:<pubkey>
```

Manifest entries also carry source authority fields so audit tooling can explain
why a row was fail-closed.

## Audit Tooling

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports:

```text
bonding_curve_v2_authority_status_counts
bonding_curve_v2_mismatch_reason_counts
bonding_curve_v2_source_counts
builder_required_curve_account_ready_counts
skip_bonding_curve_v2_authority_status_counts
skip_bonding_curve_v2_mismatch_reason_counts
skip_bonding_curve_v2_source_counts
active_shadow_bonding_curve_v2_authority_status_counts
active_shadow_bonding_curve_v2_mismatch_reason_counts
active_shadow_bonding_curve_v2_source_counts
active_shadow_builder_required_curve_account_ready_counts
```

Readiness now explicitly blocks on:

```text
bonding_curve_v2_source_not_authoritative
bonding_curve_v2_source_not_authoritative_skip
active_shadow_bonding_curve_v2_source_not_authoritative
```

## Validation

Commands run:

```bash
python3 -m py_compile \
  scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  scripts/v3_p37_bonding_curve_v2_reconciliation.py \
  scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py \
  scripts/test_v3_p37_bonding_curve_v2_reconciliation.py

python3 -m unittest \
  scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py \
  scripts/test_v3_p37_bonding_curve_v2_reconciliation.py \
  -v

cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
```

Results:

```text
python unittest: 20 passed
p37_shadow_probe: 56 passed
p37_counterfactual_probe: 8 passed
```

The Rust runs emitted existing warning noise from the broader workspace; no test
failures were observed.

## Known Limitations

- Runtime rows can now express whether a `bonding_curve_v2` source is
  authoritative, but exact DIAG/MFS reconciliation is still performed by the
  L1R10 offline report. Full MFS materialization of execution-account identities
  remains a later source-of-truth cleanup if this path becomes the selected fix.
- `route_builder` is intentionally fail-closed as `builder_only`. If a route can
  prove `bonding_curve_v2` is not required, the correct repair is to remove it
  from the builder account metas for that route, not to reintroduce a precheck
  exception.

## Next Runtime Gate

Run a small R16-r10 smoke only after this code-level repair is committed:

```text
shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r10-bcv2-source-authority
```

Acceptance:

- strict replay remains `full_replay_ok`;
- identity/hash contract remains PASS;
- exact decision/V3 join remains 100%;
- rows with builder-required `bonding_curve_v2` have populated source authority;
- builder-only `bonding_curve_v2` is fail-closed before simulation;
- no `bonding_curve_v2` `AccountNotFound` appears after simulation;
- active BUY/live/P2 remain untouched.

Expected diagnostic outcomes:

- PASS-A: authoritative `bonding_curve_v2` appears and successful shadow/probe
  entries can simulate.
- PASS-B: builder-only `bonding_curve_v2` is skipped/fail-closed as
  `bonding_curve_v2_source_not_authoritative`.

Both outcomes are useful. The invalid outcome is a silent `builder_only` account
being treated as ready.
