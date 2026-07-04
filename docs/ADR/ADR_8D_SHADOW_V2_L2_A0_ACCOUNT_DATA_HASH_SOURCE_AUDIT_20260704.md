# ADR-8D: Shadow V2 L2-A0 Account Data Hash Source Audit 20260704

## Status

Accepted audit-only stage.

## Decision

L2-A0 ustala, ze `account_data_hash` ma czesciowo dostepna sciezke zrodlowa:

```text
final_verdict=ACCOUNT_DATA_HASH_SOURCE_PARTIAL_PATH_PRESENT
```

Raw account bytes istnieja w Seer/Geyser `AccountUpdate`, ale nie sa zachowane
przez IPC, launcher `AccountUpdateEvent`, `AccountStateUpdate`,
`CanonicalPoolState` ani Shadow V2 entry boundary. L2-B powinien implementowac
propagacje BLAKE3 z raw bytes przez istniejaca sciezke account update.

## Context

Shadow V2 L2 wymaga pool-state provenance silniejszego niz decoded runtime
state. Dla account-backed pool-state samples wymagany proof to:

```text
account_pubkey + slot + write_version + BLAKE3(original raw account bytes)
```

Obecny runtime umie zbudowac L1 diagnostic simulation bez hashy, ale
`PoolStateSampleV2::research_blockers()` traktuje brak hash jako blocker dla
research-grade.

## Evidence

Raw bytes source:

```text
off-chain/components/seer/src/types.rs:243-259
off-chain/components/seer/src/grpc_connection.rs:4341-4365
```

Live Seer decode and lossy IPC send:

```text
off-chain/components/seer/src/lib.rs:3239-3367
off-chain/components/seer/src/ipc.rs:331-372
off-chain/components/seer/src/ipc.rs:802-836
```

Pending replay keeps raw bytes only before IPC:

```text
off-chain/components/seer/src/lib.rs:1062-1088
off-chain/components/seer/src/lib.rs:3032-3053
off-chain/components/seer/src/lib.rs:3113-3132
```

Launcher and AccountStateCore do not retain hash:

```text
ghost-launcher/src/components/seer.rs:3751-3766
ghost-launcher/src/events.rs:1017-1051
ghost-launcher/src/oracle_runtime.rs:2898-2947
ghost-core/src/account_state_core/types.rs:111-157
ghost-core/src/account_state_core/reducer.rs:131-154
```

Shadow V2 has hash surface but producer sets None:

```text
ghost-launcher/src/events.rs:533-567
ghost-launcher/src/components/trigger/component.rs:2381-2427
ghost-launcher/src/components/post_buy_runtime.rs:3151-3164
ghost-brain/src/guardian/post_buy/shadow_v2.rs:1225-1459
```

Audit matrix:

```text
reports/selector/shadow_v2_l2_a0_account_data_hash_source_matrix.csv
```

Full report:

```text
PLANS/AUDYT/RAPORT_SHADOW_V2_L2_A0_ACCOUNT_DATA_HASH_SOURCE_AUDIT_20260704.md
```

## Consequences

1. L2-A0 does not grant L2.
2. L2-B can proceed without new provider streams.
3. Hash must be computed close to ingest from original raw account bytes.
4. Runtime should carry hash + metadata, not raw bytes.
5. Existing missing-hash blockers remain correct until L2-B propagates real
   values.
6. Pending replay path must be included in L2-B, not only live path.

## Implementation Guidance For L2-B

Additive data path:

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

Required metadata:

```text
source_account_pubkey
source_account_owner_or_program
source_slot
source_write_version
account_data_len
account_data_hash
```

Do not:

- hash decoded reserves;
- hash `CanonicalPoolState`;
- store raw bytes in hot canonical state unless a later audit explicitly
  requires it;
- remove missing-hash blockers when hash is absent;
- change Gatekeeper/BUY/REJECT/selector/live path.

## Rejected Alternatives

### Treat decoded state as hash source

Rejected. Hashing decoded reserves or `CanonicalPoolState` does not prove
chain-observed raw account state.

### Require new provider streams for L2-B

Rejected. Existing Seer/Geyser AccountUpdate has the raw bytes needed for
BLAKE3.

### Store raw account bytes in CanonicalPoolState

Rejected for L2-B default. The audit supports carrying hash + metadata instead
of raw bytes to avoid hot-state memory/scope expansion.

### Ignore pending replay path

Rejected. Pre-mapping AccountUpdates keep raw bytes until replay, but currently
lose them at IPC send. L2-B must preserve hash for replayed updates too.

## Verification

L2-A0 verification:

```bash
git diff --check
python3 -m py_compile scripts/shadow_v2_manifest_audit.py
```

No cargo tests are required for L2-A0 because this stage is audit-only and does
not change Rust code.

## Final Decision

```text
final_verdict=ACCOUNT_DATA_HASH_SOURCE_PARTIAL_PATH_PRESENT
research_grade=NOT_GRANTED
runtime_approval=false
live_equivalence=false
strategy_research_unblocked=false
recommended_next_stage=L2-B_ACCOUNT_DATA_HASH_PROPAGATION
```
