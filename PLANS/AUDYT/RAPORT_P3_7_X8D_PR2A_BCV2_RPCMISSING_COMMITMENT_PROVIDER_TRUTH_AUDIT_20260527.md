# P3.7-X8D-PR2A - BCV2 RpcMissing / Commitment / Provider Truth Audit

Generated at UTC: 2026-05-27T00:29:53Z

## Status

```text
verdict = PR2A-B_ZERO_READY_CURRENT_MISSING
unique_bcv2_pubkeys = 354
attempt_rows = 4248
ready_unique_pubkeys = 0
mixed_ready_unique_pubkeys = 0
readiness_policy_changed = false
R18 = NO-GO
Sender/live = NO-GO
Gatekeeper/scoring/fallback = NO-GO
```

## Inputs

```text
x8d_pr1_json = /tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_unique_bcv2_pubkey_join.formal.json
config_path = /tmp/gho-x8c-s2-pr7a-smoke/shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke.toml
providers = [{'label': 'primary', 'rpc_url_redacted': 'https://solana-mainnet.core.chainstack.com/***', 'available': True}]
commitments = ['processed', 'confirmed', 'finalized']
delays_ms = [0, 250, 1000, 3000]
```

## Buckets

```json
{
  "attempt_error_class_counts": {
    "missing_on_rpc": 4248
  },
  "audit_bucket_unique_pubkeys": {
    "conflicting_account_update": 75,
    "missing_all_commitments_all_delays": 354
  },
  "primary_bucket_unique_pubkeys": {
    "conflicting_account_update": 75,
    "missing_all_commitments_all_delays": 279
  }
}
```

## Interpretation

PR2A did not find any current RPC-loadable BCV2 account across all configured
commitments and delays. This is not evidence that `RpcReady` is the wrong
contract. It is evidence that these historical BCV2 pubkeys are not normally
durable/loadable after the fact.

Operational consequence: D2 `AccountUpdateReceived` execution-ready proof is
cancelled for this path, D3 final burnin variant A is cancelled, and the only
valid next step is live diagnostic smoke for timing / ephemeral-account truth
or formal route exclusion.

## Sample Pubkeys

```text
pubkey = 23R11oD4btyHptpm1xzqfpKcMeVKKLQUfvSww13uvyHq
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 25L7wLuvZAC4TYiVtv67T3EZnUdJjDgFhJdCHVDyYDCR
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 25W7aSuzdAoFPxk1tTntmSZXskoZCZrGJhwLxVebe1rr
primary_bucket = conflicting_account_update
audit_buckets = ['conflicting_account_update', 'missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = same_pubkey_update_but_not_execution_ready
same_pubkey_account_update = True
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2AxX7iujigTkt5RA5BR5JUjVT6iat5kDg38Jz5gPugfM
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2BMDnXAAbsYu5DH5fPsfwc5rvKRWtK5BWaip9z8nMfiG
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2EEPHYm6ekWJqqRuekNrgcRrg7ddY2oqhBTx6C54AJV4
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2EvWFaTPryY1wT2sFWqgav6PtaAYGUpQEj9AjqVn12uy
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2G7XA4ARhHiXciU8vhSyaptjrEj6dawbefWNh7zVjaLU
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2GpgYESEGDwRAtDfoCww4dgmRoWDeTMTgxujs36YdQJm
primary_bucket = conflicting_account_update
audit_buckets = ['conflicting_account_update', 'missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = same_pubkey_update_but_not_execution_ready
same_pubkey_account_update = True
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2NAAqCJDvEc9vPzrzyhLfF4i7DZBHSHYyKH214xPmCnn
primary_bucket = conflicting_account_update
audit_buckets = ['conflicting_account_update', 'missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = same_pubkey_update_but_not_execution_ready
same_pubkey_account_update = True
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2Qe7aMwQv9YsNGykUseWiBF9GZKjpRqRkQbKKRvQnPXk
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2S3m5ZDNxFbZaqAkoFoahMrxGU5V44wsXmNMLn5EGWjS
primary_bucket = conflicting_account_update
audit_buckets = ['conflicting_account_update', 'missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = same_pubkey_update_but_not_execution_ready
same_pubkey_account_update = True
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2WCpsEis4RyxuokH9YACqFibBrcLnEKbCtZ2tPNuHfqD
primary_bucket = conflicting_account_update
audit_buckets = ['conflicting_account_update', 'missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = same_pubkey_update_but_not_execution_ready
same_pubkey_account_update = True
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2WTTgBiWH76XZT3W1tbw7MZnSu8DSMzfPG9beHjJTkhb
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2X1Dr7ZPdgUJSbr1LSd6JuLqEe7pic12Gwgi7R4NAE1D
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2YUn1BrkJVLEAbXhCvUgJkMWQfZd5YbLzH6SqNuD9dRn
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2csiqV5AxcHGgR4Qib8diUzP9qpsCQJazjHw38UXa841
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2g6mdipDcAaAtNyYipZ44xAiWhHAirTe2n7FiHSnDz73
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2gk1er6J7BLnoJTmzjR1rWkRVStrb2rKnf3WUqUqcYcF
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```

```text
pubkey = 2gqNcAD2nakBUcBStiojMavTtqRdCoNBtvjaJAbJqm7N
primary_bucket = missing_all_commitments_all_delays
audit_buckets = ['missing_all_commitments_all_delays']
x8d_pr1_primary_bucket = included_rpc_missing_no_same_update
same_pubkey_account_update = False
ready_commitments = []
ready_delays_ms = []
error_class_counts = {'missing_on_rpc': 12}
```
