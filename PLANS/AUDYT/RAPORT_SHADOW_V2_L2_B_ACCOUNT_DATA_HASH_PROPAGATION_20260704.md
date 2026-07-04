# Raport Shadow V2 L2-B: account_data_hash propagation 20260704

## Status

```text
final_verdict=L2_B_ACCOUNT_DATA_HASH_PROPAGATED_STILL_L2_BLOCKED
runtime_behavior_changes=NONE
new_provider_streams=NONE
decision_path_consumption=NONE
approval_flags=false
```

L2-B implementuje wynik audytu L2-A0: raw account bytes sa dostepne w
Seer/Geyser `AccountUpdate`, wiec `account_data_hash` moze byc liczony bez
dodawania nowych streamow providera. Zmiana jest evidence-only i przenosi hash
oraz minimalne metadata przez istniejaca sciezke account state do Shadow V2.

L2-B nie przyznaje L2. Usuwa glowna luke account-state provenance dla probek,
ktore pochodza z raw account update bytes, ale nadal pozostaja blokery:
temporal PASS / exact source join, density / horizon / retention, Gatekeeper
denominator / starvation oraz dedykowany research validation run.

## Zakres

W zakresie:

- policzyc `BLAKE3(original raw account update bytes)` blisko ingestu Seer;
- objac live `AccountUpdate` path;
- objac pending replay `AccountUpdate` path;
- przeniesc `account_data_hash`, `account_data_len`,
  `source_account_pubkey`, `source_account_owner_or_program`,
  `source_write_version` i slot zrodlowego stanu do Shadow V2 evidence;
- zachowac backward-compatible serde defaults;
- utrzymac typed blockers, gdy hash lub metadata sa niedostepne;
- przygotowac raport, ADR i CSV summary.

Poza zakresem:

- brak nowych NLN/provider subscriptions;
- brak `BUY/REJECT` zmian;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak `shadow_close_only` albo active close;
- brak `runtime_approval`, `research_grade`, `live_equivalence` lub strategy
  unlock;
- brak density/horizon contract;
- brak Gatekeeper denominator audit;
- brak research validation run.

## Implementowana Sciezka

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

Raw bytes nie sa przenoszone do hot canonical state. Runtime przenosi tylko:

```text
account_data_hash
account_data_len
source_account_pubkey
source_account_owner_or_program
source_write_version
source_account_slot
```

## Szczegoly Zmian

### Seer / Geyser

`off-chain/components/seer/src/lib.rs` liczy hash przed dekodowaniem account
bytes:

```text
account_data_hash = BLAKE3(raw Geyser AccountUpdate data)
account_data_len = raw data len
```

Dotyczy to obu sciezek:

- live `handle_account_update`;
- pending curve update replay.

Test `account_data_hash_uses_raw_bytes_not_decoded_reserves` potwierdza, ze
hash zalezy od raw bytes, a nie tylko od zdekodowanych rezerw.

### Seer IPC

`off-chain/components/seer/src/ipc.rs` rozszerza
`DetectedAccountUpdateEvent` addytywnie o:

```text
account_data_hash
account_data_len
source_account_pubkey
source_account_owner_or_program
```

Pola maja `#[serde(default, skip_serializing_if = "Option::is_none")]`, wiec
stare eventy bez tych pol pozostaja deserializowalne.

### Launcher Event Bus

`ghost-launcher/src/events.rs` rozszerza `AccountUpdateEvent` o te same pola.
`ghost-launcher/src/components/seer.rs` przenosi je z IPC do event busa.

### OracleRuntime / AccountStateCore

`ghost-launcher/src/oracle_runtime.rs` przenosi metadata do
`AccountStateUpdate`.

`ghost-core/src/account_state_core/types.rs` i
`ghost-core/src/account_state_core/reducer.rs` przenosza je dalej do
`CanonicalPoolState`. `source_write_version` jest mapowany z
`AccountStateUpdate.write_version`.

### Entry Boundary

`ghost-launcher/src/components/trigger/component.rs` przenosi hash i metadata z
`CanonicalPoolState` do `ShadowV2EntryBoundaryPayload`.

`ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME` nie jest juz dodawany bezwarunkowo.
Pozostaje dodawany fail-closed, gdy `account_data_hash` jest pusty albo
niedostepny.

### PoolStateSampleV2

`ghost-brain/src/guardian/post_buy/shadow_v2.rs` rozszerza
`PoolStateSampleV2` o:

```text
account_data_len
source_account_pubkey
source_account_owner_or_program
source_account_slot
source_write_version
```

`PoolStateSampleV2::from_account_state_core` bierze `account_data_hash` z
jawnego argumentu albo fallbackuje do `CanonicalPoolState.account_data_hash`.

`PoolStateSampleV2::research_blockers()` nadal blokuje brak hashy i dodatkowo
blokuje brak wymaganych metadata account-state proof.

## Zachowane Kontrakty

```text
has_complete_chain_order() unchanged
terminal truth remains DERIVED
Shadow V2 evidence is not decision input
no Gatekeeper policy changes
no selector runtime changes
no live TX/Jito changes
no new provider streams
no runtime approval
no research_grade grant
no live_equivalence grant
```

Account-state proof nie udaje transaction-order proof. Dla probek
account-backed dowod jest osobny:

```text
account_pubkey + slot + write_version + BLAKE3(raw account bytes)
```

Nie zmieniono semantyki `log_index`; nie jest ona elementem L2-B.

## Co Jest Nadal Blokowane

L2 pozostaje zablokowane przez:

- temporal PASS / exact source join dla zdarzen transaction-like i boundary
  lifecycle;
- density / horizon / retention contract;
- Gatekeeper coverage / denominator / starvation audit;
- dedicated research validation run z wystarczajaca probka;
- formalne zamkniecie unknown/untyped blockers w finalnym L2 runie.

`account_data_hash` nie jest juz oczekiwanym glownym blockerem dla nowych
observed account-state boundary samples, jezeli ich source pochodzi z raw
Seer/Geyser account update bytes. Derived, legacy i backward-compatible rows
nie wchodza do denominatora hash coverage.

## Denominator Dla Hash Coverage

Poprawny denominator dla L2-B:

```text
observed account-state boundary samples in L2 scope where raw account update
bytes are expected/available
```

Wykluczone z denominatora:

- terminal truth / derived after-state;
- rekordy legacy/backward-compatible bez raw source;
- zdarzenia transaction-like, ktore wymagaja transaction source proof zamiast
  account-state proof;
- rows oznaczone jako not-applicable.

## Weryfikacja

Wymagane sprawdzenia dla L2-B:

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

## Final Verdict

```text
final_verdict=L2_B_ACCOUNT_DATA_HASH_PROPAGATED_STILL_L2_BLOCKED
account_data_hash_source=RAW_ACCOUNT_UPDATE_BYTES
hash_algorithm=BLAKE3
raw_bytes_hot_state_retention=false
new_provider_streams=0
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
recommended_next_stage=L2-C_TEMPORAL_SOURCE_JOIN_CLOSURE
```
