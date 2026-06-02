# ADR-0144: NLN Pro 2-Stream Bridge Proof

**Date:** 2026-06-02

**Status:** Accepted as runtime bridge-proof closure.

## Decision

The NLN Pro 2-stream bridge-proof smoke closes
`PLAN_FINAL_NLN_PRO_2STREAM`:

```text
PLAN_FINAL_NLN_PRO_2STREAM = PASS
bridge_runtime_proof = PASS
proof_basis = durable_log
scope = shadow-burnin-v3-fsc-capture-nln-pro-2stream
active_policy = OFF
provider_completeness = NOT_CLAIMED
R2/account-state = NOT_CLAIMED
```

The result proves the runtime bridge:

```text
NLN pumpfun.trade -> resolver mint/pool -> PoolTransaction -> session path
```

It does not claim provider completeness, active FSC policy, R2 labels, canonical
account-state readiness, or scoring readiness.

## Evidence

Bridge-proof artifact:

```text
reports/selector/shadow-burnin-v3-fsc-capture-nln-pro-2stream-bridge-proof-20260602T174035Z/
```

Code provenance:

```text
591492bedec36be9bdabefbb2ec960baa68d105e
```

Bridge summary:

```text
status = PASS
bridge_runtime_proof = PASS
proof_basis = durable_log
forward_log_count = 1012
nln_trade_forwarded_pool_transaction_metric = not_exported_in_snapshot
nln_trade_resolved_to_pool_metric = not_exported_in_snapshot
```

Representative proof:

```text
Seer: NLN pumpfun.trade forwarded to PoolTransaction topic=prod.rpc.solana.pumpfun.trade ... resolver_action="forward_now"
```

Run deltas from start to stop:

```text
pumpfun_trade_raw_v1 delta = 2934
system_transfers_raw_v1 delta = 1062
funding_events_v1 delta = 382
nln_normalization_errors_v1 delta = 0
```

Additional observed evidence:

```text
seer_candidate_forwarded_to_oracle_total{amm_program="pumpfun"} = 26
seer_initialize_pool_detected_total{amm_program="pumpfun"} = 26
fsc_authoritative_funding_stream_available = 1
fsc_warmup_ready = 1
```

## Boundaries

Still in scope:

```text
prod.rpc.solana.pumpfun.trade raw capture
prod.rpc.solana.system.transfers raw capture
native SOL funding transfer normalization
NLN pumpfun.trade bridge into PoolTransaction
FSC evidence as diagnostic/shadow evidence
Ghost NewPoolDetected/Candidate as birth source
```

Still out of scope:

```text
NLN pumpfun.create as birth SSOT
NLN Program Streams as R2 SSOT
active FSC policy
hard reject / size-down from FSC
provider completeness claim
BCV2/account-state readiness rewrite
live/P2 mutation
```

## Consequence

`PENDING_BRIDGE_RUNTIME_PROOF` is closed. The next selector work may proceed to
Phase 2 only under the existing dataset/label/denominator plan boundaries.
