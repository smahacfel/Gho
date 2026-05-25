# P3.7-E5A DirectBuyBuilder Route ABI / Final Manifest Audit

Date: 2026-05-25

## Verdict

`BUILDER_LEGACY_LAYOUT_USES_BCV2`

E5A is an offline builder/ABI audit. No runtime smoke was run.

The current `DirectBuyBuilder` can build a `LegacyBuy` discriminator/payload, but
it does not build a clean legacy account layout. The final instruction manifest
for `PumpfunBuyVariant::LegacyBuy` still uses the extended account list and
contains `bonding_curve_v2` at account index 16.

Therefore E4R-style handoff patches must stop. The runtime safety gate observed
in E4R3S was correct: selected legacy fallback could not be allowed to simulate
because the final manifest still contained the primary extended-route BCV2 role.

## Security Incident

The E4R3S config path contained literal Chainstack endpoint/token material.
Treat that token as burned.

Actions in this checkpoint:

- Added tracked-config scanner: `scripts/check_no_committed_chainstack_tokens.py`
- Redacted tracked config-like files to env placeholders:
  - `${CHAINSTACK_GRPC_ENDPOINT}`
  - `${CHAINSTACK_GRPC_TOKEN}`
  - `${CHAINSTACK_RPC_URL}`
- Added config-loader support for explicit `${ENV_VAR}` secret placeholders.

Required operator action outside git:

- Rotate/revoke the exposed Chainstack token before any future runtime.

No git history rewrite was performed.

## Evidence

### DirectBuyBuilder ABI

`off-chain/components/trigger/src/direct_buy_builder.rs`

- The builder explicitly documents that current `global:buy` transactions use the
  24-byte legacy payload with the newer extended account list.
- `LegacyBuy` switches only discriminator/data layout.
- The account meta construction is shared by both `LegacyBuy` and
  `RoutedExactSolIn`.
- Account index 16 is always appended as `bonding_curve_v2`.

New audit tests:

- `e5a_legacy_buy_final_manifest_uses_extended_bcv2_layout`
- `e5a_routed_exact_sol_in_final_manifest_uses_bcv2_layout`

Result encoded by tests:

```text
LegacyBuy data discriminator = LEGACY_BUY_DISCRIMINATOR
LegacyBuy data length = 24
LegacyBuy account count = 18
LegacyBuy account[16] = derive_bonding_curve_v2(mint)
LegacyBuy account[3] != account[16]
```

### Trigger PreparedBuyRequest Boundary

`ghost-launcher/src/components/trigger/component.rs`

`create_buy_build_profile()` always calls:

```text
DirectBuyBuilder::build_buy_ix_with_accounts_and_bonding_curve_v2(...)
```

The `buy_variant` is propagated, but the builder still constructs the extended
manifest for `LegacyBuy`.

New audit test:

- `e5a_prepared_legacy_buy_final_manifest_still_contains_bcv2`

Result encoded by test:

```text
PreparedBuyRequest build_profile.buy_variant = LegacyBuy
request.account_overrides.bonding_curve_v2 = None
final buy_instruction.accounts.len = 18
final buy_instruction.accounts[16] = derive_bonding_curve_v2(mint)
counterfactual required role filtering excludes bonding_curve_v2
```

This proves the semantic mismatch:

```text
P3.7 readiness/precheck layer can filter BCV2 out for LegacyBuy,
but final transaction manifest still physically contains BCV2.
```

### Runtime Handoff Boundary

`ghost-launcher/src/oracle_runtime.rs`

E4R2/E4R3 clears `bonding_curve_v2` from selected fallback overrides, but the
final request is rebuilt through `prepare_buy_request...()`, which re-enters the
same `DirectBuyBuilder` extended account layout.

This explains the E4R3S contradiction:

```text
selected_route_handoff_status = selected_route_handoff_applied
selected legacy fallback logically selected
final manifest still contains bonding_curve_v2
selected_route_handoff_mismatch blocks simulation
```

## E5A Decision Matrix

| Candidate verdict | Result | Reason |
| --- | --- | --- |
| `BUILDER_LEGACY_LAYOUT_USES_BCV2` | YES | `LegacyBuy` final manifest contains BCV2 by builder construction. |
| `BUILDER_LEGACY_LAYOUT_CLEAN_BUT_RUNTIME_REBUILDS_PRIMARY` | NO | Builder itself is not clean legacy; runtime is not the first fault. |
| `OBSERVED_LEGACY_ROUTE_NOT_TRUE_LEGACY` | PARTIAL | Existing observed-chain fixture says `global:buy` uses extended layout. E5A does not prove no clean legacy ABI exists elsewhere. |
| `E5A_AUDIT_GAP` | NO | Builder and PreparedBuyRequest boundaries are directly test-covered. |

## Consequence

No further runtime smoke is allowed before the next route-support decision.

Do not implement E4R4.
Do not continue handoff marker patches.
Do not run R18/L2D2.
Do not change Gatekeeper thresholds or V3 policy.

## Recommended Next Path

Choose one:

1. `E5B_IMPLEMENT_TRUE_LEGACY_BUY_LAYOUT`

   Only if we have an authoritative clean legacy ABI that omits BCV2.
   E5B must add a separate builder path, not mutate the current extended
   `LegacyBuy` path.

2. `LEGACY_BUY_UNSUPPORTED_SELECT_NEXT_ROUTE`

   If no authoritative clean legacy ABI is available. In that case, the current
   `legacy_buy` label should be treated as `global_buy_extended_layout`, and the
   route-support front returns to E1 matrix / scope restriction.

Current recommendation from E5A:

```text
LEGACY_BUY_UNSUPPORTED_SELECT_NEXT_ROUTE
unless a clean legacy ABI is supplied and verified before E5B.
```

## Validation Commands

```text
python3 scripts/check_no_committed_chainstack_tokens.py
cargo test -p trigger e5a_ -- --nocapture
cargo test -p ghost-launcher --lib e5a_prepared_legacy_buy_final_manifest_still_contains_bcv2 -- --nocapture
cargo test -p ghost-launcher --lib test_from_file_resolves_explicit_secret_env_placeholders_from_dotenv -- --nocapture
cargo check -p ghost-launcher
git diff --check
```
