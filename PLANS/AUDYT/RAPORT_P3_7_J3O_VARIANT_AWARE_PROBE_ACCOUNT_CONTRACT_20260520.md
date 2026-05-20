# P3.7-J3O Variant-Aware Probe Account Contract

Status: CODE-LEVEL REPAIR READY FOR R15-R8I SMOKE

## Context

J3N confirmed that the remembered historical shadow-burnin payer contract was
real:

```text
GHOST_TRIGGER_KEYPAIR_PATH = wallets/shadow-burnin-test.json
configured_pubkey = 9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw
account_exists = true
```

R15-r8h used this configured payer and no longer showed the earlier missing
ephemeral payer blocker. The remaining blockers were execution-account
readiness classes:

```text
missing_execution_route_identity
execution_account_not_ready:bonding_curve_v2:<pubkey>
execution_account_not_ready:creator_vault:<pubkey>
```

A selected pool inspection showed observed legacy and routed Pump.fun buy
events in the same buffered transaction set. The probe builder path was still
treating the legacy buy variant like the newer routed account layout, which
caused precheck to require routed-only accounts for legacy-compatible probes.

## Repair

The repair separates required account semantics by buy variant:

- `LegacyBuy` now uses the compact Pump.fun buy account layout: global, fee
  recipient, mint, bonding curve, associated bonding curve, user ATA, payer,
  system program, token program, rent, event authority and Pump program.
- `RoutedExactSolIn` keeps the extended routed layout with creator vault,
  volume/fee accounts, bonding curve v2 and buyback fee recipient.
- `creator_pubkey` is treated as route metadata, not as an RPC account to fetch.
- P3.7 counterfactual probes can preserve an observed `legacy_buy` route from
  buffered decision-time transaction evidence.
- The generic active override resolver remains unchanged and still rejects
  unverified legacy overrides outside the probe-specific path.

## Files Changed

```text
off-chain/components/trigger/src/direct_buy_builder.rs
ghost-launcher/src/components/trigger/component.rs
ghost-launcher/src/oracle_runtime.rs
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i.toml
```

## Validation

Targeted validation run before the R15-r8i smoke gate:

```text
cargo test -p trigger legacy_buy -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_probe_execution_account_readiness_report.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Results:

```text
trigger legacy_buy: PASS
ghost-launcher p37_counterfactual_probe: PASS
ghost-launcher p37_shadow_probe: PASS
python py_compile/unittest: PASS
```

## Next Gate

Run a short bounded smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i.toml
```

The run must be stopped early if a new dominant blocker appears. Full/bounded
collection remains blocked until a smoke produces probe transport/entry rows
with exact join continuity, or a new precise NOT_READY diagnosis.

## Non-Goals Preserved

- No collection.
- No P2.
- No live sender.
- No active policy change.
- No IWIM change.
- No threshold tuning.
- No precheck bypass.
- No treatment of `AccountNotFound` as success.
