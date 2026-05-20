# RAPORT P3.7-J3 P0R Counterfactual Shadow Probe Repair

Date: 2026-05-19

Status: code-level P0R PASS, R15 runtime smoke pending

## Verdict

P3.7-J3 P0R repairs the original P0 harness gap. The probe plane is still
disabled by default and remains a counterfactual shadow-only collection plane,
not a BUY path.

P0R does not claim runtime smoke completion. The next gate is a bounded R15
smoke that must produce real runtime probe transport/entry rows and pass the
probe join-key audit.

## Corrected Status Model

```text
Original J3 P0: PARTIAL harness PASS / superseded by P0R
J3 P0R code-level repair: PASS
R15 runtime smoke: PENDING
Full R14: HOLD
Phase B V3 selector prototype: HOLD
P2/live: NO-GO
```

## Implemented Repairs

Config/startup guards:

- added `run_id` and `session_id` to `[p37_shadow_probe]`;
- kept `enabled=false` as the default;
- kept fail-closed validation outside `entry_mode=shadow_only`,
  `execution_mode=shadow`, and `trigger.shadow_run.enabled=true`;
- added fail-closed validation for existing probe outputs when `append=false`;
- added `append=true` validation requiring non-empty `run_id` and `session_id`;
- added fail-closed validation that `[p37_shadow_probe].enabled=true` requires
  `[gatekeeper_v3].replay_payload_enabled=true` in the configured Ghost Brain
  config.

Runtime bounds:

- added shared probe runtime state;
- enforced `max_probes_per_run`;
- enforced `max_probes_per_minute`;
- enforced `max_concurrent` through a non-blocking semaphore reservation;
- enforced `dedupe_by_probe_id`;
- converted bounded-runtime pressure into `probe_skipped` rows with explicit
  skip reasons instead of blocking or mutating the active decision path.

Shadow simulation path:

- added a probe-only `TriggerComponent::simulate_counterfactual_shadow_probe`
  helper;
- added a fixed-lamports request override so
  `probe_amount_source="fixed_lamports"` controls the prepared shadow request
  amount instead of only annotating the probe logs;
- routed selected probes through `shadow_simulator.simulate_buy(...)`;
- avoided `dispatch_prepared_buy_shadow_only`, active position reservation,
  live sender paths, and active BUY logs;
- retained the synthetic no-simulation record builder only as a harness fixture.

Log schema and audit:

- added probe dispatch timestamp, amount source, amount lamports, slippage,
  run/session IDs, bucket metadata, and age-status fields to probe rows;
- extended `ShadowEntryRecord` additively with optional probe fields;
- kept legacy rows backward-compatible;
- strengthened `v3_p37_mfs_lifecycle_join_key_audit.py` so probe PASS requires
  exact join back to the decision/V3 row by `ab_record_id` and V3 hashes;
- added bucket, skip reason, amount-source, decision-join and hash-match
  reporting to the audit.

Smoke profile:

- updated `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke.toml`
  to bounded probe limits:
  - `max_probes_per_run = 5`,
  - `max_probes_per_minute = 5`,
  - `max_concurrent = 1`,
  - `sample_modulus = 100`,
  - `sample_threshold = 100`.

## Validation

Commands run:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/config.rs ghost-launcher/src/oracle_runtime.rs ghost-launcher/src/components/trigger/component.rs
git diff --check
```

Observed result:

- `p37_shadow_probe`: PASS, 19 tests.
- `p37_counterfactual_probe`: PASS, 3 tests.
- join-key audit unittest: PASS, 5 tests.
- py_compile: PASS.
- rustfmt check: PASS.
- diff whitespace check: PASS.

## Still Pending

P0R intentionally does not run or claim:

- R15 runtime smoke,
- lifecycle close,
- on-chain lifecycle report,
- lifecycle labels,
- full R14 collection,
- Phase B V3 selector prototype.

## Acceptance For Next Gate

R15 smoke can be attempted only after code-level checks remain green.

R15 smoke must show:

- V3/MFS decision rows,
- probe selected rows,
- probe transport rows,
- probe entry rows,
- exact `ab_record_id` continuity,
- exact `probe_id` continuity,
- exact decision/V3 join and hash match,
- active BUY count unchanged,
- no live/P2 path enabled.

Lifecycle close is useful but not required for P0R smoke PASS. Lifecycle labels
remain P1 unless a close occurs naturally.

## Non-Goals Preserved

P0R did not:

- enable P2,
- enable live execution,
- change active Gatekeeper V2/V2.5 policy,
- change IWIM,
- change live sender behavior,
- tune thresholds,
- promote V3,
- treat probes as active BUY decisions,
- treat lifecycle outcomes as decision-time features.
