# ADR-8D: Shadow V2 L2-B Account Data Hash Propagation 20260704

## Status

Accepted implementation stage.

## Decision

Implementujemy addytywna, evidence-only propagacje `account_data_hash` dla
Shadow V2 account-state proof:

```text
final_verdict=L2_B_ACCOUNT_DATA_HASH_PROPAGATED_STILL_L2_BLOCKED
```

Hash jest liczony jako:

```text
BLAKE3(original raw account update bytes)
```

Punkt liczenia to Seer, blisko Geyser `AccountUpdate`, zanim raw bytes zostana
zdekodowane do rezerw. Dalej przenoszone sa tylko hash i minimalne metadata
provenance, nie raw bytes.

## Context

L2-A0 ustalil, ze raw account bytes sa dostepne w Seer/Geyser
`AccountUpdate`, ale byly tracone przed IPC i nie docieraly do:

```text
AccountUpdateEvent
AccountStateUpdate
CanonicalPoolState
ShadowV2EntryBoundaryPayload
PoolStateSampleV2
```

Bez tego Shadow V2 mogl miec executable L1 roundtrip, ale nie mial mocnego
proof, ze pool-state sample odpowiada konkretnemu surowemu account state z
chaina.

## Implemented Contract

Addytywna sciezka L2-B:

```text
Seer GeyserEvent::AccountUpdate.data
-> BLAKE3(data)
-> DetectedAccountUpdateEvent
-> AccountUpdateEvent
-> AccountStateUpdate
-> CanonicalPoolState
-> ShadowV2EntryBoundaryPayload
-> PoolStateSampleV2
```

Propagowane pola:

```text
account_data_hash
account_data_len
source_account_pubkey
source_account_owner_or_program
source_write_version
source_account_slot
```

`source_account_slot` w `PoolStateSampleV2` pochodzi z
`CanonicalPoolState.last_update_slot`. `source_write_version` pochodzi z
`AccountStateUpdate.write_version`.

## Evidence

Hash source and Seer propagation:

```text
off-chain/components/seer/src/lib.rs
off-chain/components/seer/src/ipc.rs
```

Launcher and account-state propagation:

```text
ghost-launcher/src/events.rs
ghost-launcher/src/components/seer.rs
ghost-launcher/src/oracle_runtime.rs
ghost-core/src/account_state_core/types.rs
ghost-core/src/account_state_core/reducer.rs
```

Shadow V2 boundary/sample propagation:

```text
ghost-launcher/src/components/trigger/component.rs
ghost-launcher/src/components/post_buy_runtime.rs
ghost-brain/src/guardian/post_buy/shadow_v2.rs
```

Report and summary:

```text
PLANS/AUDYT/RAPORT_SHADOW_V2_L2_B_ACCOUNT_DATA_HASH_PROPAGATION_20260704.md
reports/selector/shadow_v2_l2_b_account_data_hash_propagation_summary.csv
```

## Consequences

1. New observed account-state boundary samples can carry raw-byte provenance.
2. Missing hash remains a typed blocker.
3. Missing account-state metadata now has typed blockers.
4. Raw bytes are not retained in hot canonical state.
5. No new provider streams are required.
6. No decision path consumes Shadow V2 account hash evidence.
7. L2 remains blocked until temporal proof, density, denominator and research
   validation stages pass.

## Rejected Alternatives

### Hash decoded reserves or CanonicalPoolState

Rejected. Hashing decoded structs does not prove chain-observed raw account
state and would create false L2 confidence.

### Carry raw bytes through launcher and canonical state

Rejected for L2-B. Carrying raw bytes expands hot-state memory and scope. The
accepted contract is to compute BLAKE3 near ingest and carry hash plus metadata.

### Add provider streams for account_data_hash

Rejected. L2-A0 proved that existing Seer/Geyser `AccountUpdate` already
contains the required raw bytes.

### Loosen temporal predicates

Rejected. `has_complete_chain_order()` remains unchanged. Account-state proof
and transaction-order proof are separate proof families.

### Remove missing-hash blockers globally

Rejected. Blockers are removed only when real propagated hash exists. Missing
or legacy rows continue to fail closed.

## Backward Compatibility

New serialized fields use optional serde defaults:

```text
#[serde(default, skip_serializing_if = "Option::is_none")]
```

Old IPC/event JSON without account hash metadata remains deserializable.

## Verification

Required checks:

```bash
cargo test -p seer account_data_hash -- --nocapture
cargo test -p seer account_update -- --nocapture
cargo test -p ghost-core account_data_hash -- --nocapture
cargo test -p ghost-launcher account_data_hash -- --nocapture
cargo test -p ghost-brain shadow_v2_pool_state -- --nocapture
cargo test -p ghost-launcher shadow_v2_no_decision_consumption_static_guard -- --nocapture
cargo check -p ghost-core
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
python3 -m py_compile scripts/shadow_v2_manifest_audit.py
git diff --check
git diff --cached --check
forbidden staged-file guard
```

## Final Decision

```text
final_verdict=L2_B_ACCOUNT_DATA_HASH_PROPAGATED_STILL_L2_BLOCKED
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
next_stage=L2-C_TEMPORAL_SOURCE_JOIN_CLOSURE
```
