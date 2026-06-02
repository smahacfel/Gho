# PLAN_FINAL_NLN_PRO_2STREAM

## Status

```text
NLN Pro 2-stream capture/evidence smoke: PASS
FSC/Pump.fun telemetry evidence: PASS
BCV2 RPC hydration independence for FSC evidence: PASS
Ghost birth lane: PASS
FSC active policy: OFF / PASS
Provider completeness: NOT CLAIMED
Scoring readiness: NO-GO / not claimed
Full plan closure: PENDING_BRIDGE_RUNTIME_PROOF
```

This plan deliberately says that the NLN Pro 2-stream profile removes BCV2 RPC
hydration as a dependency for FSC/Pump.fun telemetry evidence. It does not solve
account-state readiness, R2 labels, or the canonical market path.

## Scope

The dedicated Pro profile uses exactly two NLN Program Streams:

```text
prod.rpc.solana.pumpfun.trade
prod.rpc.solana.system.transfers
```

The Ghost birth lane remains the source for pool/candidate identity:

```text
NewPoolDetected -> Candidate -> candidate universe
```

NLN `pumpfun.create`, `pumpfun.transaction`, swaps, and partitionless swaps are
disabled optional topics for this profile. They are not globally forbidden for
other profiles with different quota/plan constraints.

## Non-Goals

```text
No active BUY/REJECT policy change.
No FSC hard reject.
No size-down from FSC.
No R2 label source from NLN Program Streams.
No AccountUpdate emitted from NLN trade reserves.
No BCV2/account-state readiness rewrite.
No dynamic AccountUpdate subscription fix.
No pumpfun.transaction runtime dependency.
No provider completeness claim.
No live/P2 mutation.
```

## Runtime Contract

The two-stream profile is an additive capture/evidence lane:

```text
NLN pumpfun.trade -> normalized PumpFunTrade evidence
NLN system.transfers -> normalized native SOL funding evidence
FundingTransferObserved -> FundingSourceIndex
FSC evidence -> diagnostic/shadow evidence only
```

BCV2 RPC hydration must not gate FSC evidence. Missing BCV2/account-state data is
degraded account-state evidence, not a semantic reject and not a blocker for FSC
capture.

Primary FSC transfer evidence accepts native SOL only:

```text
token_address == "solana"
```

WSOL, SPL mints, and unknown token address variants are excluded from primary
FSC and must not be counted as clean native SOL funding evidence.

## Quota Guard

For this profile:

```text
max_streams = 2
quota_policy = "fail_fast"
enabled_topics = [
  "prod.rpc.solana.pumpfun.trade",
  "prod.rpc.solana.system.transfers",
]
```

If enabled topics exceed the configured quota, runtime must fail before opening
streams. Silent optional-topic dropping is allowed only in other profiles that
explicitly choose `drop_optional`.

## Bridge Requirement

Full plan closure requires explicit runtime proof that NLN pumpfun.trade reaches
the session path:

```text
nln_trade_resolved_to_pool_total > 0
nln_trade_forwarded_pool_transaction_total > 0
```

or an equivalent durable log/evidence row showing:

```text
NLN pumpfun.trade -> PoolTransaction forwarded
```

The implementation surface contains a bounded mint-to-pool resolver, bounded
trade dedupe, trade buffering, collision handling, and PoolTransaction emission.
However the 2026-06-02 smoke did not produce live bridge proof in logs or
metrics, so this plan remains `PENDING_BRIDGE_RUNTIME_PROOF`.

## Smoke Snapshot

Final smoke snapshot:

```text
reports/selector/shadow-burnin-v3-fsc-capture-nln-pro-2stream-smoke-20260602T162014Z/
```

Bridge audit snapshot:

```text
reports/selector/shadow-burnin-v3-fsc-capture-nln-pro-2stream-bridge-audit-20260602T162039Z/
```

The run was stopped after snapshotting. The snapshot intentionally avoids full
process command lines because tmux launch commands can contain provider
credentials.

## Smoke Evidence Summary

Observed during the smoke:

```text
pumpfun_trade_raw_v1.jsonl: nonzero
system_transfers_raw_v1.jsonl: nonzero
funding_events_v1.jsonl: nonzero
nln_normalization_errors_v1.jsonl: 0
fsc_authoritative_funding_stream_available: 1
fsc_warmup_ready: 1
fsc_coverage_window_ready: 1
NewPoolDetected: nonzero
Candidate: nonzero
AccountNotFound: 0 in checked runtime logs
BCV2_RPC_HYDRATION_MISSING: 0 in checked runtime logs
```

Observed but not treated as NLN/FSC capture blockers:

```text
Snapshot emitted with missing/zero reserves
BCV2_EXACT_WATCH_RETAIN_PRUNED
SEER_PARSE_MISS
```

These remain account-state/readiness signals and must be handled by the future
AccountUpdate SSOT/state-readiness work, not by this FSC capture profile.

## Next Step

If live bridge proof is still missing after the next targeted run, implement or
repair `PR-NLN2-BRIDGE` narrowly:

```text
1. Populate mint -> pool resolver from Ghost NewPoolDetected/Candidate.
2. Buffer NLN trades by mint with TTL and caps.
3. Flush buffered trades on resolver hit.
4. Deduplicate NLN trade stream with bounded TTL set.
5. Convert NLN pumpfun.trade to TradeEvent/PoolTransaction.
6. Emit into the existing session path.
7. Report unresolved-after-TTL as degraded/diagnostic.
8. Do not call RPC and do not require BCV2 hydration.
```

Acceptance:

```text
pumpfun.trade after known Candidate -> PoolTransaction
pumpfun.trade before Candidate -> buffer -> flush
unknown mint after TTL -> degraded, no RPC
no AccountUpdate emitted from NLN reserves
no semantic REJECT from missing BCV2 in this lane
FSC active policy OFF
hard reject OFF
live/P2 untouched
```
