# RAPORT P3.7-J3Q3 Optional `bonding_curve_v2` Precheck Repair

Date: 2026-05-20

## Status

```text
P3.7-J3Q3 code repair: PASS
R15-r8m runtime smoke: see separate smoke report
Full/bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Problem

R15-r8l exact-joined selected probe rows to persisted V3 decision rows, but
stopped before transport/entry because strict execution-account readiness
treated `bonding_curve_v2` as a required RPC-existing account.

Manual inspection of a selected pool showed a real successful buy transaction
where the same `bonding_curve_v2` pubkey appeared as account index 16 of the
extended Pump.fun buy instruction, while that account had zero pre/post lamports
and no current RPC account data. The precheck was therefore rejecting an
observed optional/zero-lamport remaining account before the simulator could run.

## Change

The counterfactual probe required-account precheck now allows missing
`bonding_curve_v2` only when it matches account index 16 of the prepared buy
instruction.

This is intentionally narrow:

- `creator_vault` remains route-aware and strict when required.
- payer, bonding curve, associated bonding curve and other real execution
  accounts remain strict.
- the account is not removed from the instruction.
- active BUY, live sender, IWIM and thresholds are unchanged.

## Validation

Commands run:

```bash
cargo test -p ghost-launcher --lib p37_counterfactual_probe_required_accounts_use_legacy_extended_layout -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
rustfmt --edition 2021 --check ghost-launcher/src/components/trigger/component.rs off-chain/components/trigger/src/direct_buy_builder.rs
cargo build -p ghost-launcher --bin ghost-launcher
```

Results:

```text
targeted required-account test: PASS
p37_counterfactual_probe tests: PASS, 8/8
rustfmt check: PASS
ghost-launcher build: PASS
```

## Gate

The repair is accepted only as a code-level precheck correction. Runtime proof is
R15-r8m:

```text
expected: probe_transport_rows > 0 and probe_shadow_entries > 0
not required: lifecycle close
still blocked: collection, Phase B, P2, live, tuning
```
