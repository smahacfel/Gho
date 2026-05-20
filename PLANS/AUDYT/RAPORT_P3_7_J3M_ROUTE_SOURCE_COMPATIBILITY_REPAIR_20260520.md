# RAPORT P3.7-J3M Route-Source Compatibility Repair

Date: 2026-05-20

Status:

```text
P3.7-J3M code-level repair: PASS
R15-r8f runtime smoke: NOT_READY_DIAGNOSED
Next runtime gate: fresh short smoke, early-stop on first structural blocker
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Problem

R15-r8f was stopped early after the artifacts showed a stable blocker:

```text
probe_selection_rows = 30
probe_skip_rows = 76
probe_transport_rows = 0
probe_entry_rows = 0
active_shadow_buy_rows = 1
active_shadow_execution_outcome = shadow_data_problem / AccountNotFound
```

The probe plane exact-joined selected rows back to V3 decision rows, but no
probe transport or entry rows were generated. A non-probe active shadow BUY also
failed with `AccountNotFound`, so the issue was not isolated to probe dispatch.

The inspected source transaction pattern showed that a single transaction can
contain more than one pump.fun buy-like instruction. The previous enrichment
logic stopped at the first complete top-level pump.fun instruction. That could
preserve a legacy-like `buy_variant` even when a later top-level routed buy
instruction was present for the same trade.

Downstream, the trigger/shadow builder is routed-oriented. Feeding it
inconsistent source route metadata makes strict execution-account readiness
fail on route accounts such as `bonding_curve_v2` or `creator_vault`.

## Change

Updated `off-chain/components/seer/src/binary_parser.rs`:

- added `pump_buy_enrichment_priority`;
- top-level pump.fun enrichment now scans matching source instructions and
  chooses the best match before filling fields;
- routed pump.fun buy / exact-quote-in buy has higher priority than legacy buy
  when both are present at the same source priority level;
- inner CPI fallback uses the same best-match rule, but top-level enrichment
  still takes priority once complete;
- legacy-only and routed-only enrichment remain supported.

This is a parser/enrichment compatibility repair. It does not:

- bypass strict required execution accounts;
- reinterpret `AccountNotFound` as success;
- change active Gatekeeper verdicts;
- change IWIM;
- enable P2/live;
- tune thresholds;
- start collection.

## Tests

Passed:

```text
cargo test -p seer enrich_trade -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_probe_execution_account_readiness_report.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

New parser test:

```text
enrich_trade_prefers_top_level_routed_over_top_level_legacy_when_both_match
```

The new test verifies that a transaction with both top-level legacy-like and
routed pump.fun buy instructions enriches:

```text
buy_variant = routed_exact_sol_in
fee_recipient = routed fee account
associated_bonding_curve = routed associated bonding curve
```

Existing tests still verify:

```text
legacy-only enrichment remains legacy_buy
routed-only enrichment remains routed_exact_sol_in
top-level complete enrichment still beats inner CPI fallback
```

## Decision

J3M is code-level PASS. It is not runtime proof.

Next step is a fresh, short smoke in a clean namespace. The run must be watched
as an early-failure detector:

- stop early if `probe_transport_rows` / `probe_entry_rows` appear;
- stop early if the same structural blocker reappears;
- generate reports immediately;
- do not wait for full timeout once the gate outcome is already obvious.

Collection and Phase B remain blocked until a runtime smoke produces probe
transport/entry rows with exact join metadata and no active BUY/live mutation.
