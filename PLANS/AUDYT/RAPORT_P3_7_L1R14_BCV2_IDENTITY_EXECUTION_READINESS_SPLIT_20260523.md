# RAPORT P3.7-L1R14 BCV2 Identity vs Execution-Readiness Split

## Status

P3.7-L1R14 code-level status: PASS.

Runtime smoke status: not run in this commit. This report claims code-level
repair and targeted validation only.

No L2 ablation, collection, Phase B, P2, live execution, IWIM change, or
Gatekeeper threshold change was performed.

## Problem

R16-r10 showed that the L1R13 observed route-account path did not unlock
execution:

```text
matrix_rows = 53
rpc_get_account_status_counts = {"missing": 53}
matrix_classifications = {"builder_bcv2_missing_on_rpc": 53}
diag_seen_exact_builder_bcv2_rows = 0
diag_seen_other_curve_for_mint_rows = 53
mfs_contains_builder_bcv2_pubkey_rows = 0
```

The code was still overloading identity authority and execution readiness. A
row could report:

```text
bonding_curve_v2_source = observed_tx_account_meta
bonding_curve_v2_authority_status = authoritative_observed_tx
bonding_curve_v2_ready = true
builder_required_curve_account_ready = true
```

even when the same BCV2 pubkey was missing on RPC and the row had
`execution_account_not_ready:bonding_curve_v2:<pubkey>`.

Observed transaction account meta can establish the identity source, but it is
not proof that the account is currently simulation-load-ready.

## Code Changes

L1R14 separates BCV2 identity from execution readiness:

- `ShadowSimulationAccountDiagnostics` now carries:
  - `bonding_curve_v2_identity_authority_status`
  - `bonding_curve_v2_rpc_load_status`
  - `bonding_curve_v2_rpc_load_ready`
  - `builder_required_curve_account_ready_reason`
- Probe selection, transport, execution diagnostics, and shadow entry rows carry
  the same additive fields.
- `builder_required_curve_account_ready` is now true only when the identity is
  authoritative and the account is RPC/local-load-ready.
- `observed_tx_account_meta` remains an authoritative identity source, but
  without a performed RPC/local readiness lookup it reports
  `identity_only_rpc_unverified` and remains not ready.
- If observed BCV2 appears in the missing-account precheck candidates, the row is
  classified as `bonding_curve_v2_observed_meta_missing_on_rpc`.
- Builder-only or derived-unverified BCV2 remains not ready even if a manifest
  exists.
- Account manifest summaries now include whether manifest account lookup was
  actually performed, so a manifest-only diagnostic cannot masquerade as
  successful RPC readiness.

## Audit Changes

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports probe and
active-shadow distributions for:

```text
bonding_curve_v2_identity_authority_status
bonding_curve_v2_rpc_load_status
bonding_curve_v2_rpc_load_ready
builder_required_curve_account_ready_reason
```

It also exposes L1R14 classes:

```text
bonding_curve_v2_observed_meta_missing_on_rpc_rows
bonding_curve_v2_identity_authoritative_but_not_load_ready_rows
active_shadow_bonding_curve_v2_observed_meta_missing_on_rpc_rows
active_shadow_bonding_curve_v2_identity_authoritative_but_not_load_ready_rows
```

Legacy rows without the new fields remain parsable through additive/optional
handling.

## Validation

Python:

```text
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Result:

```text
OK - 18 tests
```

Rust:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe_observed_bcv2 -- --nocapture
```

Results:

```text
p37_shadow_probe: 59 passed
p37_counterfactual_probe: 8 passed
p37_shadow_probe_observed_bcv2: 2 passed
```

Formatting/static checks:

```text
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs ghost-launcher/src/events.rs
git diff --check
```

Result: PASS.

Existing repository warnings were observed during cargo tests. They were not
introduced as part of L1R14 and were not treated as blockers for this narrow
repair.

## Known Limitations

L1R14 does not prove runtime entry recovery. It only fixes the semantic contract
that caused observed BCV2 identity to be interpreted as readiness.

Observed-meta provenance is still limited in the current runtime structures. The
implementation validates and reports identity/readiness separation, but full
source transaction signature, instruction discriminator, route kind, instruction
account position, and message account index provenance are not yet emitted as a
complete observed-meta parser validation bundle.

No broad route fallback was implemented. If the next smoke shows that observed
BCV2 remains RPC-missing, the next engineering decision is route fallback or
route exclusion, not policy ablation.

## Next Runtime Gate

Recommended next runtime gate:

```text
R16-r11 / L1R14 BCV2 Identity-Readiness Smoke
```

Acceptance:

- strict replay remains `full_replay_ok`;
- identity/hash contract remains PASS;
- exact decision/V3 join remains 100%;
- observed BCV2 rows populate identity and RPC-load fields separately;
- no row with `rpc_get_account_status = missing` reports
  `builder_required_curve_account_ready = true`;
- BCV2 does not reach simulation as post-simulation AccountNotFound;
- successful probe/active-shadow entries appear, or route exclusion is explicit
  and counted;
- active BUY, live, P2, Phase B, IWIM, and thresholds remain untouched.

If observed BCV2 remains missing on RPC, route fallback/exclusion is the next
step. If some rows are RPC-load-ready, the diagnostic path can return to R16/L2
policy work only after successful entries or explicit fail-closed evidence.
