# RAPORT P3.7-J4B PROBE LIFECYCLE TRUTH RETENTION REPAIR

Date: 2026-05-21

## Verdict

```text
P3.7-J4B code-level repair: PASS
Runtime smoke: PENDING
Full collection / Phase B / P2 / live / threshold tuning: HOLD / NO-GO
```

## Problem

J4 proved that probe entries now reach a dedicated probe lifecycle monitor:

```text
probe_shadow_entry -> PostBuySubmitted(lane=probe) -> probe monitor -> probe_shadow_lifecycle
```

But every probe lifecycle row closed as:

```text
truth_status = failure
truth_detail = shadow time-stop expired before any canonical snapshot reached guardian
```

The J4 blocker was therefore no longer lifecycle handoff. It was canonical
snapshot delivery into the probe monitor after handoff.

## Diagnosis

Runtime log inspection showed the failing order:

1. Canonical account updates for the selected mint were relayed and applied
   before the terminal decision.
2. The pool reached a REJECT/TIMEOUT decision and returned
   `retain_runtime_pool=false`.
3. The router executed `pool_task_done_cleanup`.
4. Cleanup removed `AccountStateCore`, `ShadowLedger` snapshots, curve aliases,
   pending account updates, live-pipeline mint state, and runtime identity.
5. The counterfactual probe dispatch and probe post-buy handoff were running
   asynchronously, so the probe monitor started after canonical truth state had
   already been removed.

This explains why J4 produced probe lifecycle rows but no resolved economic
truth: the probe monitor was active, but its canonical snapshot source had been
cleaned before it could seed truth.

## Code Change

Changed `maybe_handle_p37_shadow_probe_decision(...)` in
`ghost-launcher/src/oracle_runtime.rs` to return whether a counterfactual probe
dispatch was actually scheduled.

When a probe row:

- passes selection and pre-dispatch eligibility,
- reserves a scan slot,
- and spawns the counterfactual probe dispatch task,

the function now returns `true` and emits:

```text
P37_SHADOW_PROBE_RUNTIME_RETENTION_REQUESTED
```

The per-pool observation result now uses that value:

```text
retain_runtime_pool = true
```

for REJECT/TIMEOUT/IWIM-rejected/BUY terminal paths when a probe lifecycle is
scheduled. That prevents `pool_task_done_cleanup` from deleting canonical
runtime truth before the probe monitor can consume it.

## Safety Boundaries

This repair does not:

- change active Gatekeeper verdicts;
- change active BUY/REJECT/TIMEOUT policy;
- change IWIM;
- change thresholds;
- enable P2/live;
- treat shadow/probe simulation as live inclusion;
- add synthetic lifecycle truth fallback.

The probe lifecycle still requires canonical snapshot truth. The repair only
keeps the existing decision-time runtime truth available long enough for the
probe monitor to use it.

## Validation

Commands run:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
```

Results:

```text
p37_shadow_probe: 47/47 PASS
p37_counterfactual_probe: 8/8 PASS
```

Repository warnings were pre-existing unused/deprecated warnings; no targeted
test failed.

## Next Gate

Run a bounded J4B smoke in a fresh namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1
```

Acceptance:

```text
V3 strict replay: PASS
probe transport/entry/lifecycle exact join: PASS
probe lifecycle rows: present
probe lifecycle truth no longer all closes as no-canonical-snapshot failure
active BUY mutation: PASS / none
P2/live untouched
```

If probe lifecycle still fails, the next report must classify whether the
failure is no snapshot, stale snapshot, unnormalizable price, exit truth failure,
or another explicit lifecycle truth class.
