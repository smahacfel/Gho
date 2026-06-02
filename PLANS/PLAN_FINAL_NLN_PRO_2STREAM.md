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
Full plan closure: PASS
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
The 2026-06-02 bridge-proof smoke produced durable runtime proof in logs, so
this plan is closed as `PASS`.

## Bridge Proof Smoke

Targeted bridge-proof smoke:

```text
reports/selector/shadow-burnin-v3-fsc-capture-nln-pro-2stream-bridge-proof-20260602T174035Z/
```

Code provenance:

```text
591492bedec36be9bdabefbb2ec960baa68d105e
```

Bridge result:

```text
bridge_runtime_proof = PASS
proof_basis = durable_log
forward_log_count = 1012
nln_trade_forwarded_pool_transaction_metric = not_exported_in_snapshot
nln_trade_resolved_to_pool_metric = not_exported_in_snapshot
```

Run deltas from start to stop:

```text
pumpfun_trade_raw_v1 delta = 2934
system_transfers_raw_v1 delta = 1062
funding_events_v1 delta = 382
nln_normalization_errors_v1 delta = 0
```

Representative durable proof line:

```text
Seer: NLN pumpfun.trade forwarded to PoolTransaction topic=prod.rpc.solana.pumpfun.trade ... resolver_action="forward_now"
```

This closes the runtime bridge proof only. Provider completeness, R2/account
state readiness, scoring readiness, and active FSC policy remain not claimed.

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

The NLN Pro 2-stream profile is closed for capture/evidence and bridge proof.
The next selector step may proceed only within the already defined boundaries:

```text
Phase 2A: feature_snapshots_v1
Phase 2B: R1 label coverage
Phase 2C: R2 canonical market path labels
Phase 2D: leakage audit
```

Still forbidden:

```text
baseline
Gatekeeper tuning
FSC policy activation
hard reject
size-down
R2 from NLN
AccountUpdate from NLN trade reserves
```
