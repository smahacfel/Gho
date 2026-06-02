# ADR-0143: NLN Pro 2-Stream Smoke Status

**Date:** 2026-06-02

**Status:** Accepted as capture/evidence smoke status.

## Decision

The NLN Pro 2-stream smoke is accepted for FSC/Pump.fun capture/evidence:

```text
NLN Pro 2-stream live smoke = PASS
FSC/Pump.fun telemetry evidence = PASS
BCV2 hydration independence for FSC evidence = PASS
Ghost birth lane = PASS
FSC active policy = OFF
Provider completeness = NOT CLAIMED
Scoring readiness = NO-GO / NOT CLAIMED
Full PLAN_FINAL_NLN_PRO_2STREAM = PENDING_BRIDGE_RUNTIME_PROOF
```

The profile removes BCV2 RPC hydration as a dependency for FSC/Pump.fun telemetry
evidence only. It does not solve account-state readiness, R2 labels, or the
canonical market path.

## Evidence

Final smoke snapshot:

```text
reports/selector/shadow-burnin-v3-fsc-capture-nln-pro-2stream-smoke-20260602T162014Z/
```

Bridge audit snapshot:

```text
reports/selector/shadow-burnin-v3-fsc-capture-nln-pro-2stream-bridge-audit-20260602T162039Z/
```

The run was stopped after the final snapshot. The snapshot does not store full
process command lines because tmux launch commands can contain provider
credentials.

## Boundaries

In scope:

```text
prod.rpc.solana.pumpfun.trade raw capture
prod.rpc.solana.system.transfers raw capture
native SOL funding transfer normalization
FSC evidence as diagnostic/shadow evidence
Ghost NewPoolDetected/Candidate as birth source
```

Out of scope:

```text
NLN pumpfun.create as birth SSOT
NLN Program Streams as R2 SSOT
active FSC policy
hard reject / size-down from FSC
provider completeness claim
BCV2/account-state readiness rewrite
live/P2 mutation
```

## Residual Risk

The smoke did not produce runtime bridge proof for:

```text
nln_trade_resolved_to_pool_total > 0
nln_trade_forwarded_pool_transaction_total > 0
```

The code contains a bridge surface for NLN pumpfun.trade to PoolTransaction, but
the runtime audit did not find durable log/metric evidence. Therefore the
capture/evidence lane is closed as smoke PASS, while full plan closure remains
pending bridge runtime proof.

## Follow-Up

Run a targeted bridge-proof smoke or implement a narrow `PR-NLN2-BRIDGE` repair
if bridge proof remains absent. The follow-up must not change active policy,
Gatekeeper thresholds, R2 source of truth, or live/P2 behavior.
