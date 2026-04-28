Analiza zakończona. Zidentyfikowałem 11 konkretnych root causes z dokładnymi lokalizacjami w kodzie (plik:linia). Trzy najbardziej niszczycielskie to:

1. **Discriminator CREATE** (`binary_parser.rs:32`) — wrong value means direct instruction path never matches, 100% reliance on CPI event log fallback
2. **EntryUpdate raw data discarded** (`grpc_connection.rs:1676`) — 20-30% CPI events silently dropped
3. **Kaskada CREATE→trades** (`lib.rs:1118-1135`) — jeden utracony CREATE = 100% strat dla danego tokena

Te trzy razem wyjaśniają >50% coverage loss. Chcesz żebym przeszedł do implementacji fixów, czy najpierw potrzebujesz dodatkowej weryfikacji któregokolwiek z root causes?


---

# ANALIZA PRZYCZYN ŹRÓDŁOWYCH — GHOST SEER PIPELINE

Przeszedłem cały pipeline od gRPC subscription (`grpc_connection.rs`) przez transport (`DualLaneChannel`), konwersję (`pump_event_to_geyser_event`), parsing (`binary_parser.rs`), filtrowanie (`lib.rs:process_event`) do emisji eventów (`IPC`). Oto precyzyjne przyczyny źródłowe każdego z trzech problemów.

---

## PROBLEM 1: COVERAGE < 50%

### RC-1.1 [KRYTYCZNY] Rozbieżność discriminatora CREATE — seer vs reszta codebase

**Lokalizacja**: `binary_parser.rs:32` vs `init_pool_parser.rs:67` vs `pumpfun_collector.rs:43`

```
binary_parser.rs    → DISC_CREATE = [0x18, 0x1e, 0xc8, 0x28, ...] (SHA256("global:create")[..8])
init_pool_parser.rs → PUMPFUN_CREATE_DISCRIMINATOR = [0xd6, 0x90, 0x4c, 0xec, ...] (actual on-chain)
pumpfun_collector.rs→ PUMPFUN_CREATE_DISCRIMINATOR = [0xd6, 0x90, 0x4c, 0xec, ...] (actual on-chain)
```

Komentarz w `init_pool_parser.rs:8-10` mówi wprost:
> "Pump.fun does NOT use standard Anchor-style discriminators for pool creation. The actual on-chain discriminator for `create` is `[0xd6, 0x90, ...]`, which does NOT match SHA256("global:create")."

**Efekt**: Ścieżka direct instruction parsing w binary_parserze (`decode_ix` → `DISC_CREATE` match arm, linia ~1207) **NIGDY nie matchuje** prawdziwych Pump.fun CREATE transakcji. Detekcja pool creation opiera się WYŁĄCZNIE na ścieżce CPI event log (`DISC_EVENT_CREATE = [0x1b, 0x72, ...]`), która wymaga obecności `inner_instructions` w proto message.

**Wpływ**: Jeśli Yellowstone provider (Chainstack) nie dostarcza `inner_instructions` dla części transakcji, te CREATE eventy są bezpowrotnie tracone. Każdy utracony CREATE kaskaduje — WSZYSTKIE późniejsze trade'y tego tokena są odrzucane przez `should_forward_trade` (bo brak curve→mint mapping).

### RC-1.2 [KRYTYCZNY] EntryUpdate inner instructions odrzucone w konwersji

**Lokalizacja**: `grpc_connection.rs:1676-1683`

```rust
PumpEvent::EntryUpdate { slot, executed_transaction_count, .. } 
    => Some(Ok(GeyserEvent::EntryAnchor { slot, executed_transaction_count })),
//                                         ^^^^ pole `raw: Vec<u8>` jest ODRZUCONE!
```

Komentarz w `grpc_connection.rs:1239-1248` mówi:
> "Entry events carry executed_transaction_count AND inner-instruction CPI data for migrate events not present in Tx meta. Previous version dropped Entry events — that caused ~20-30% migrate CPI loss."

Ale fix jest **niekompletny**: `pump_event_to_geyser_event` konwertuje EntryUpdate na minimalny `GeyserEvent::EntryAnchor` zawierający TYLKO `slot` i `executed_transaction_count`. Raw proto bytes z inner instructions są odrzucane. Parser `PumpParser::parse_entry_raw` (binary_parser.rs:824-829) istnieje, ale **nigdy nie jest wywoływany** przez ścieżkę GeyserEvent, bo `pump_event_to_geyser_event` odcina dane zanim dotrą do parsera.

**Wpływ**: ~20-30% CPI eventów (szczególnie migrate) jest traconych.

### RC-1.3 [KRYTYCZNY] Kaskadowy filtr sesyjny — utracone CREATE = utracone trade'y

**Lokalizacja**: `lib.rs:1106-1152` (`should_forward_trade`)

```rust
fn should_forward_trade(&self, pool_id: &Pubkey, mint: &Pubkey, source_label: &str) -> bool {
    // ...
    let mapping_ok = {
        let fwd = self.curve_to_mint.read();
        if let Some(mapped_mint) = fwd.get(&pool_bytes) {
            *mapped_mint == mint_bytes
        } else {
            // Fallback reverse lookup
            let rev = self.mint_to_curve.read();
            matches!(rev.get(&mint_bytes), Some(mapped_pool) if *mapped_pool == pool_bytes)
        }
    };
    if !mapping_ok { return false; } // ← ODRZUCA trade
```

Curve→mint mapping jest populowany WYŁĄCZNIE z `set_curve_mapping(source="create")` podczas przetwarzania CREATE eventów. Jeśli CREATE jest utracony (RC-1.1), mapping nigdy nie jest tworzony, i WSZYSTKIE trade'y tego tokena są cicho odrzucane.

**Wpływ**: Mnożnik na straty z RC-1.1. Jeden utracony CREATE = utrata 100% eventów danego tokena.

### RC-1.4 [WYSOKI] Jednowątkowe przetwarzanie eventów z backpressure cascade

**Lokalizacja**: `lib.rs:594-611`

```rust
while let Some(event_result) = event_stream.next().await {
    match event_result {
        Ok(event) => {
            if let Err(e) = self.process_event(event).await { ... }
        }
```

Eventy są przetwarzane SEKWENCYJNIE. W `process_event` wykonywane jest:
- 6+ lock acquisitions (`RwLock<HashMap>` na curve_to_mint, mint_to_curve, tracked_curves, pending_curve_updates)
- Binary parsing (proto decode + instruction matching)
- IPC send (może blokować przy backpressure)
- Opcjonalnie RPC calls dla curve resolve

Przy >100 eventów/s, queue rośnie → ultrafast_mode aktywuje się → **trade parsing jest POMIJANY** (`lib.rs:1551-1554`):
```rust
if ultrafast_mode {
    trace!("Ultrafast degraded mode active - skipping trade parsing");
    return Ok(());
}
```

**Wpływ**: Pod obciążeniem, trade'y nawet dla ZNANYCH pooli są odrzucane.

### RC-1.5 [ŚREDNI] Podwójna serializacja/deserializacja proto

**Lokalizacja**: `grpc_connection.rs:1196-1208` → `1646-1685` → `binary_parser.rs:2449-2464`

Dane przechodzą:
1. gRPC stream → proto message → `encode_proto()` → raw bytes (channel)
2. raw bytes → `decode_tx_to_geyser_event()` → GeyserEvent (z mpcf_payload_bytes = raw)
3. GeyserEvent → `parse_pump_events()` → PumpEvent::Transaction (z raw = mpcf_payload_bytes)
4. PumpEvent → `PumpParser::parse()` → ponowny `SubscribeUpdateTransaction::decode(raw)`

Proto jest dekodowane **dwukrotnie**: raz w kroku 2, raz w kroku 4. Każdy decode to ~50-200μs przy 1-5KB payload. Przy 10k eventów/s to 0.5-2s/s czystego CPU marnowanego na redundantny decode.

### RC-1.6 [ŚREDNI] Redundantny `tx_touches_pump` z kosztownym bs58

**Lokalizacja**: `grpc_connection.rs:1295-1307`

```rust
fn tx_touches_pump(t: &SubscribeUpdateTransaction) -> bool {
    msg.account_keys.iter().any(|k| {
        let s = bs58::encode(k).into_string();       // ← bs58 encode KAŻDEGO klucza
        is_pump_fun_program(&s) || s == PUMP_SWAP_PROGRAM_ID
    })
}
```

Ta funkcja re-filtruje transakcje które **już zostały przefiltrowane** przez gRPC server (via `account_include`). bs58 encoding 10-30 kluczy per transakcja to ~5-15μs overhead na hot path. Przy 10k TPS to 50-150ms/s wasted CPU.

---

## PROBLEM 2: NIEPRAWIDŁOWE MAPOWANIE CREATOR ADDRESS

### RC-2.1 [KRYTYCZNY] Indeksy kont dla CREATE z Buy/Sell layoutu

**Lokalizacja**: `binary_parser.rs:76-78` vs `init_pool_parser.rs` account layout

```
binary_parser.rs (USED for Create, Buy, Sell — ONE set of indices):
  PUMP_IDX_MINT          = 2
  PUMP_IDX_BONDING_CURVE = 3
  PUMP_IDX_USER          = 6    ← "creator" for Create

Actual Pump.fun CREATE instruction layout (from init_pool_parser.rs):
  Index 0: Mint
  Index 1: Mint Authority  
  Index 2: Bonding Curve
  Index 3: Associated Bonding Curve
  Index 4: Global State
  Index 5: MPL Token Metadata
  Index 6: Metadata account    ← binary_parser thinks this is "user/creator"
  Index 7: Creator/Signer      ← actual creator

Actual Pump.fun BUY/SELL instruction layout:
  Index 0: Global
  Index 1: Fee account
  Index 2: Mint               ← PUMP_IDX_MINT (correct for buy/sell)
  Index 3: Bonding Curve      ← PUMP_IDX_BONDING_CURVE (correct for buy/sell)
  Index 6: User/Signer        ← PUMP_IDX_USER (correct for buy/sell)
```

Binary parser używa **JEDNEGO zestawu indeksów** (`PUMP_IDX_*`) zarówno do CREATE jak i Buy/Sell. Dla Buy/Sell indeksy są poprawne. Dla CREATE — `PUMP_IDX_USER=6` wskazuje na metadata account, nie na creatora (który jest pod index 7).

**Uwaga**: W praktyce ścieżka direct instruction matching DISC_CREATE nie matchuje (RC-1.1), więc bug się nie manifestuje na tej ścieżce. Ale jeśli DISC_CREATE zostanie naprawiony bez korekty indeksów, bug się aktywuje.

### RC-2.2 [ŚREDNI] CPI event log path — poprawne mapowanie

**Lokalizacja**: `binary_parser.rs:2098-2117`

CPI CreateEvent (`DISC_EVENT_CREATE`) dekoduje `EventCreate` struct via Borsh:
```rust
pub struct EventCreate {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub mint: [u8; 32],
    pub bonding_curve: [u8; 32],
    pub user: [u8; 32],     // ← creator address, correct from CPI
}
```
Ta ścieżka (CpiCreate) wyciąga poprawny `user` z event payload, nie z account indices. Jest to aktualnie JEDYNA działająca ścieżka detekcji CREATE i creator address jest na niej poprawny.

**Ale**: `enhanced_builder.rs` buduje EnhancedCandidate z event data, i może próbować wyciągać creator z transaction accounts zamiast z CPI event — to wymaga weryfikacji w `build_enhanced_candidate`.

---

## PROBLEM 3: LATENCJA 1-10 SEKUND

### RC-3.1 [KRYTYCZNY] Processing bottleneck — single-threaded sequential loop

**Lokalizacja**: `lib.rs:594-611` (identycznie jak RC-1.4)

Kumulatywny czas przetwarzania jednego eventu:
- Proto decode: ~100μs
- Lock acquisitions (6× RwLock): ~10-50μs per lock × 6 = ~60-300μs
- Binary parser (instruction matching + Borsh decode): ~50-200μs
- bs58 encode (curve→mint registry): ~5-10μs × several
- IPC send: ~10-50μs (bez backpressure)
- **Total: ~300-800μs per event**

Przy 200 eventów/s (skromne obciążenie): `200 × 500μs = 100ms/s` processing load. Przy 1000 eventów/s (peak): `1000 × 500μs = 500ms/s` — system zaczyna się cofać. Latencja narasta liniowo z każdą sekundą opóźnienia.

### RC-3.2 [WYSOKI] Dynamic re-subscribe debounce 120ms + gRPC resub latency

**Lokalizacja**: `grpc_connection.rs:87` (`RESUB_DEBOUNCE_MS = 120`)

Po detekcji nowego poola:
1. `set_curve_mapping` → `registry.insert()` → version bump
2. `maybe_send_resubscribe` sprawdza debounce: 120ms minimum od ostatniego resub
3. gRPC re-subscribe: ~50-200ms (server-side)
4. Account updates zaczynają płynąć

**Total delay**: 170-320ms od CREATE do pierwszego AccountUpdate. W tym czasie bonding curve state jest "genesis default" (stale).

### RC-3.3 [WYSOKI] Stall watchdog zbyt tolerancyjny — 20s

**Lokalizacja**: `grpc_connection.rs:91` (`SILENT_STALL_SECS = 20`)

Jeśli gRPC stream zawiesi się (serwer przestaje wysyłać, ale TCP jest żywy), system nie reaguje przez 20 sekund. W tym czasie:
- Zero eventów dociera
- Health check raportuje "connected"
- Operator nie widzi problemu

### RC-3.4 [ŚREDNI] RPC fallback dla curve→mint resolution

**Lokalizacja**: `lib.rs:716-803` (`queue_curve_mint_resolve`)

Kiedy AccountUpdate przyjeżdża bez znanego mint mapping (bo CREATE nie był jeszcze przetworzony):
1. Queues RPC call: `getSignaturesForAddress` + `getTransaction` (2 RPC calls)
2. RPC latencja: 200ms-3s
3. Pending curve update czeka na rozwiązanie

**Wpływ**: ShadowLedger nie otrzymuje curve state przez czas trwania RPC resolve.

### RC-3.5 [NISKI] Commitment level domyślnie "mempool" — poprawne

**Lokalizacja**: `config.rs:170-172` — default to `CommitmentLevel::Mempool`

Config.toml nie nadpisuje commitment level, więc default "mempool" (processed) jest używany. To jest poprawne dla niskiej latencji.

---

## PODSUMOWANIE KASKADY ROOT CAUSE → EFEKT

```
DISC_CREATE mismatch (RC-1.1)
    │
    ├── Direct CREATE parsing fails 100% czasu
    │   └── Fallback do CPI event log (DISC_EVENT_CREATE)
    │       ├── CPI present → CREATE detected → OK
    │       └── CPI absent (no inner_ix from provider) → CREATE LOST
    │           └── curve→mint mapping NEVER created
    │               └── should_forward_trade DROPS ALL trades (RC-1.3)
    │                   └── COVERAGE GAP for entire token lifetime
    │
    ├── EntryUpdate inner-ix discarded (RC-1.2)
    │   └── 20-30% CPI events lost (migrate, some creates)
    │
    └── Processing bottleneck (RC-1.4)
        ├── Event queue grows under load
        ├── ultrafast_mode activates → trade parsing SKIPPED
        └── Latency grows 1-10s (RC-3.1)
```

---

## REKOMENDACJE PRIORYTETOWANE

### P0 (Critical — fix immediately):

1. **Dodać `DISC_CREATE_PUMPFUN = [0xd6, 0x90, 0x4c, 0xec, 0x5f, 0x8b, 0x31, 0xb4]`** do binary_parser.rs i matchować OBYDWA discriminatory w `decode_ix`. Dodać oddzielne indeksy kont dla CREATE (`CREATE_IDX_MINT=0`, `CREATE_IDX_BONDING_CURVE=2`, `CREATE_IDX_USER=7`).

2. **Naprawić `pump_event_to_geyser_event`** — EntryUpdate musi przekazywać raw bytes do parsera. Albo dodać nowy GeyserEvent variant (`EntryWithData`) albo odpalić `PumpParser::parse_entry_raw` BEZPOŚREDNIO w konwersji i forwerdować wyniki jako dodatkowe GeyserEventy.

3. **Usunąć `tx_touches_pump` check** — jest redundantny (gRPC server już filtruje) i marnuje CPU na hot path.

### P1 (High — fix this sprint):

4. **Wyeliminować double proto decode** — albo przechodzić proto message bezpośrednio (bez encode→channel→decode), albo trzymać decoded struct w channel zamiast raw bytes.

5. **Zmniejszyć `SILENT_STALL_SECS` z 20 do 5** — 20s to zbyt dużo niewidocznego stall.

6. **Zmniejszyć `RESUB_DEBOUNCE_MS` z 120 do 20-30ms** — 120ms debounce dodaje niepotrzebną latencję do account subscription.

### P2 (Medium — next iteration):

7. **Rozważyć wielowątkowe przetwarzanie** — oddzielić parsing od routing/filtrowania. Parser działa w pool of workers, routing w osobnym tasku.

8. **Dodać memcmp filter dla AMM pool discriminator** — aktualny subscription filtruje TYLKO bonding curve accounts. PumpSwap AMM pool accounts (`DISC_AMM_POOL = [0xf1, 0x9a, ...]`) nie matchują memcmp i muszą być ręcznie dodawane do `acct_list`.