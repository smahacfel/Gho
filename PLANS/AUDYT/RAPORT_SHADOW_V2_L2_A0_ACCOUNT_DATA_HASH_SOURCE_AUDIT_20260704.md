# Raport Shadow V2 L2-A0: account_data_hash raw-bytes source audit 20260704

## Status

```text
final_verdict=ACCOUNT_DATA_HASH_SOURCE_PARTIAL_PATH_PRESENT
runtime_changes=NONE
new_provider_streams=NONE
next_stage=L2-B_ACCOUNT_DATA_HASH_PROPAGATION
```

L2-A0 byl wykonany jako audit-only/report-only. Celem bylo ustalenie, czy
Ghost ma dostep do raw account bytes potrzebnych do:

```text
account_data_hash = BLAKE3(original raw account update bytes)
```

Odpowiedz: raw bytes sa obecne w Seer/Geyser `AccountUpdate`, ale nie sa
zachowywane przez IPC, launcher event, `AccountStateUpdate`,
`CanonicalPoolState` ani Shadow V2 boundary. Sciezka jest wiec czesciowa:
source istnieje, ale retention do L2 evidence nie istnieje.

## Base I Zakres

Audit zostal wykonany na branchu:

```text
codex/shadow-v2-l2-a0-account-data-hash-audit
```

Branch zostal zalozony z `origin/main` po PR47:

```text
origin/main=42b0ccd82210b8d3d32f5ebe291ce546defd2c34
```

Zakres L2-A0:

- znalezc raw account bytes;
- potwierdzic, czy sa oryginalnym account update payloadem;
- ustalic, gdzie bytes/hash sa tracone;
- sprawdzic live i pending replay path;
- wskazac implementation path dla L2-B;
- nie zmieniac runtime behavior.

Poza zakresem:

- brak propagacji hashy;
- brak zmian Rust runtime;
- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector/live/TX/Jito path;
- brak nowych provider subscriptions.

## Kluczowe Ustalenia

### 1. Raw account bytes sa obecne przy Yellowstone -> Seer

`GeyserEvent::AccountUpdate` niesie raw bytes:

```text
off-chain/components/seer/src/types.rs:243-259
```

Pola:

```text
slot
event_time
write_version
pubkey
data: Vec<u8>
owner
```

Konwersja Yellowstone account update do `GeyserEvent` klonuje raw account
bytes z `SubscribeUpdateAccount.account.data`:

```text
off-chain/components/seer/src/grpc_connection.rs:4341-4365
```

To jest najwczesniejszy poprawny punkt dla L2-B. Bytes sa oryginalnym
payloadem account update z provider streamu, zanim zostana zdekodowane do
rezerw.

### 2. Seer live path dekoduje raw bytes, ale wysyla dalej tylko decoded state

`handle_account_update` rozpakowuje `GeyserEvent::AccountUpdate` do:

```text
slot
event_time
write_version
pubkey
data
owner
```

Dowod:

```text
off-chain/components/seer/src/lib.rs:3239-3248
```

Nastepnie `decode_canonical_account_update(owner, data)` dekoduje raw bytes do:

```text
sol_reserves
token_reserves
complete
token_mint
```

Dowod:

```text
off-chain/components/seer/src/lib.rs:1171-1217
off-chain/components/seer/src/lib.rs:3257-3264
```

Live IPC send dostaje juz tylko decoded reserves i metadata:

```text
off-chain/components/seer/src/lib.rs:3353-3367
```

Nie przekazuje:

```text
raw data
account_data_hash
account_data_len
owner
```

### 3. Pending curve update path zachowuje raw bytes tylko do momentu replay

Przed poznaniem curve->mint mapping Seer buforuje raw bytes w
`PendingCurveUpdateSnapshot.data`:

```text
off-chain/components/seer/src/lib.rs:1062-1088
off-chain/components/seer/src/lib.rs:3113-3132
```

Replay path nadal dekoduje `replay.data`, ale do IPC wysyla tylko decoded
reserves:

```text
off-chain/components/seer/src/lib.rs:3032-3053
```

To oznacza, ze L2-B musi objac oba miejsca:

```text
live AccountUpdate send
pending replay AccountUpdate send
```

W przeciwnym razie replayowane pre-mapping updates nadal beda bez hash proof.

### 4. IPC `DetectedAccountUpdateEvent` traci raw bytes/hash

`DetectedAccountUpdateEvent` zawiera:

```text
semantic
event_time
base_mint
bonding_curve
curve_finality
sol_reserves
token_reserves
complete
slot
write_version
replay_origin
replay_buffer_dwell_ms
detected_at
sequence_number
```

Dowod:

```text
off-chain/components/seer/src/ipc.rs:331-372
```

`send_account_update` konstruuje ten event z decoded reserves i metadata:

```text
off-chain/components/seer/src/ipc.rs:802-836
```

Brakuje:

```text
account_data_hash
account_data_len
source_account_owner_or_program
raw data
```

`bonding_curve` pelni role account pubkey dla aktualnego account update, ale
nie ma jawnego pola `source_account_pubkey` ani ownera.

### 5. Launcher event bus nie odzyskuje utraconych danych

Seer component mapuje IPC event na `GhostEvent::AccountUpdate(AccountUpdateEvent)`
bez nowych danych:

```text
ghost-launcher/src/components/seer.rs:3751-3766
```

`AccountUpdateEvent` ma decoded reserves, slot i `write_version`, ale nie ma
hashy ani raw bytes:

```text
ghost-launcher/src/events.rs:1017-1051
```

Po tym punkcie nie da sie juz policzyc poprawnego:

```text
BLAKE3(original raw account update bytes)
```

bez cofniecia sie do Seer.

### 6. OracleRuntime i AccountStateCore zachowuja write_version, ale nie hash

`OracleRuntime::build_account_state_update` buduje `AccountStateUpdate` z:

```text
pool_amm_id
base_mint
bonding_curve
sol_reserves
token_reserves
is_complete
slot
write_version
receive_ts_ms
receive_seq
curve_finality
source
```

Dowod:

```text
ghost-launcher/src/oracle_runtime.rs:2898-2947
```

`AccountStateUpdate` nie ma raw bytes/hash/data_len/owner:

```text
ghost-core/src/account_state_core/types.rs:143-157
```

`AccountStateReducer::apply_account_update` uzywa `write_version` w
monotonic guard, ale wynikowy `CanonicalPoolState` juz go nie przechowuje:

```text
ghost-core/src/account_state_core/reducer.rs:54-65
ghost-core/src/account_state_core/reducer.rs:131-154
```

`CanonicalPoolState` ma `last_update_slot` i `last_update_ts_ms`, ale nie ma:

```text
account_data_hash
account_data_len
source_write_version
source_account_owner_or_program
raw bytes
```

Dowod:

```text
ghost-core/src/account_state_core/types.rs:111-135
```

### 7. Shadow V2 boundary ma pole na hash, ale producent wpisuje None

`ShadowV2EntryBoundaryPayload` ma:

```text
account_data_hash: Option<String>
canonical_pool_state: CanonicalPoolState
```

Dowod:

```text
ghost-launcher/src/events.rs:533-567
```

`TriggerComponent::capture_shadow_v2_entry_boundary` pobiera
`CanonicalPoolState`, ale ustawia:

```text
account_data_hash: None
source_block_time: None
source_tx_signature: None
source_transaction_index: None
source_instruction_index: None
source_inner_instruction_index: None
source_log_index: None
```

i dodaje limitation:

```text
ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME
ENTRY_BOUNDARY_SOURCE_JOIN_NOT_PROVEN
```

Dowod:

```text
ghost-launcher/src/components/trigger/component.rs:2381-2427
```

Test utrwala obecne zachowanie:

```text
ghost-launcher/src/components/trigger/component.rs:7288-7314
```

### 8. PoolStateSampleV2 juz wymaga hash do research readiness

`PoolStateSampleV2` ma pole:

```text
account_data_hash: Option<String>
```

Dowod:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs:1225-1248
```

`PoolStateSampleV2::research_blockers()` dodaje:

```text
POOL_STATE_ACCOUNT_DATA_HASH_MISSING
```

gdy hash jest pusty:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs:1404-1412
```

Helper do poprawnego hashy juz istnieje:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs:1458-1459
```

ale runtime nie przekazuje do niego raw bytes.

PostBuyRuntime przekazuje tylko `boundary.account_data_hash.clone()`:

```text
ghost-launcher/src/components/post_buy_runtime.rs:3151-3164
```

Poniewaz boundary ma `None`, pool-state sample pozostaje bez research proof.

### 9. Side module `pool_state_ssot/yellowstone.rs` nie jest aktywnym Shadow V2 path

Repo zawiera modul `ghost-brain/src/pool_state_ssot/yellowstone.rs`, ktory ma:

```text
on_raw_account_update(store, account_pubkey, data)
```

Dowod:

```text
ghost-brain/src/pool_state_ssot/yellowstone.rs:280-311
```

Jednak `rg` pokazuje tylko testowe callsite'y dla `on_raw_account_update`.
Nie jest to obecny launcher -> OracleRuntime -> AccountStateCore -> Shadow V2
boundary path.

Wniosek: ten modul nie rozwiazuje L2-A0 dla aktywnej Shadow V2 sciezki.

## Matrix

Pelna matrix znajduje sie w:

```text
reports/selector/shadow_v2_l2_a0_account_data_hash_source_matrix.csv
```

Najwazniejszy wynik:

```text
raw bytes present until Seer live/replay decode
hash absent everywhere in active runtime path
write_version retained through AccountStateUpdate but dropped in CanonicalPoolState
owner/data_len absent after Seer
boundary has hash field but producer sets None
```

## Finalny Verdict

```text
ACCOUNT_DATA_HASH_SOURCE_PARTIAL_PATH_PRESENT
```

Uzasadnienie:

- raw bytes source istnieje w Seer `GeyserEvent::AccountUpdate`;
- bytes sa oryginalnym account update payloadem z Yellowstone account update;
- pending replay zachowuje raw bytes tylko lokalnie do momentu replay;
- IPC i launcher traca raw bytes/hash/data_len/owner;
- `AccountStateUpdate` i `CanonicalPoolState` nie przenosza hashy;
- Shadow V2 boundary ma pole `account_data_hash`, ale runtime wpisuje `None`;
- `PoolStateSampleV2` juz ma blocker research-grade dla braku hashy.

To nie jest:

```text
ACCOUNT_DATA_HASH_RAW_BYTES_SOURCE_PRESENT
```

jako finalny verdict, bo sama obecnosc raw bytes w Seer nie oznacza pelnego
source path do Shadow V2 L2 evidence.

To nie jest:

```text
BLOCKED_ACCOUNT_DATA_HASH_RAW_BYTES_NOT_RETAINED
```

jako finalny verdict, bo implementacyjna sciezka L2-B jest jasna i nie wymaga
nowych provider streamow ani rekonstrukcji z decoded struct.

## Implementation Path Dla L2-B

Minimalny poprawny path:

```text
Seer GeyserEvent::AccountUpdate.data
-> BLAKE3(data) computed close to ingest
-> DetectedAccountUpdateEvent.account_data_hash
-> AccountUpdateEvent.account_data_hash
-> AccountStateUpdate.account_data_hash
-> CanonicalPoolState.account_data_hash
-> ShadowV2EntryBoundaryPayload.account_data_hash
-> PoolStateSampleV2.account_data_hash
```

Minimalne metadata do przeniesienia razem z hash:

```text
source_account_pubkey
source_account_owner_or_program
source_slot
source_write_version
account_data_len
account_data_hash
```

Zasady:

- liczyc BLAKE3 z oryginalnego `data` slice, nie z decoded struct;
- liczyc hash dla live path i pending replay path;
- dalej przenosic hash + metadata, nie raw bytes;
- nie przechowywac raw bytes w `CanonicalPoolState`;
- nowe pola musza byc addytywne i serde-compatible;
- brak hashy w backward rows ma byc typed blockerem;
- nie zmieniac Gatekeeper/BUY/REJECT/selector/live path.

Rekomendowane miejsca L2-B:

1. Seer:
   - policzyc `account_data_hash` i `account_data_len` w `handle_account_update`
     przed `decode_canonical_account_update`;
   - w pending replay liczyc z `replay.data`;
   - rozszerzyc `IpcSender::send_account_update`.
2. IPC:
   - dodac addytywne pola do `DetectedAccountUpdateEvent`.
3. Launcher:
   - przeniesc pola do `AccountUpdateEvent`.
4. OracleRuntime:
   - przekazac pola do `AccountStateUpdate`.
5. AccountStateCore:
   - dodac pola do `AccountStateUpdate` i `CanonicalPoolState`;
   - zachowac `source_write_version` w canonical state.
6. Trigger/PostBuy:
   - wypelnic `ShadowV2EntryBoundaryPayload.account_data_hash`;
   - usunac limitation `ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME` tylko wtedy,
     gdy hash jest realnie obecny.

## L2-B Test Requirements

L2-B powinien dodac testy potwierdzajace:

- hash rowna sie BLAKE3 z raw bytes;
- hash nie jest liczony z decoded reserves/struct/JSON;
- live path przenosi hash przez IPC;
- pending replay path przenosi hash po curve->mint mapping;
- `AccountUpdateEvent` przenosi hash/data_len/owner/pubkey/write_version;
- `AccountStateUpdate` przenosi hash;
- `CanonicalPoolState` przenosi hash i source write_version;
- entry boundary wypelnia hash z canonical pool state;
- `PoolStateSampleV2` dostaje hash z boundary;
- brak hash pozostaje typed blockerem dla backward rows;
- static guard potwierdza brak decision consumption.

## Approval Flags

Ten audit nie zmienia i nie przyznaje:

```text
runtime_approval=false
live_equivalence=false
strategy_research_unblocked=false
active_close=false
shadow_close_only=false
research_grade=false
```

## No-Runtime Boundary

L2-A0 nie zmienia:

- Rust runtime;
- `MaterializedFeatureSet`;
- Gatekeeper policy;
- BUY/REJECT;
- selector runtime;
- TX/Jito/live path;
- active close;
- provider subscriptions.

## Next Stage

```text
L2-B_ACCOUNT_DATA_HASH_PROPAGATION
```

L2-B moze wystartowac bez nowych provider streamow. Wymagane sa addytywne
zmiany danych w istniejacej sciezce account update.
