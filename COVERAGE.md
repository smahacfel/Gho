
## Analiza trzech punktów utraty TX coverage (70-80% → 98-99%)

### L1 — ultrafast_mode skip (NAJGROŹNIEJSZY)

**Lokalizacja**: lib.rs

**Mechanizm**: Gdy kolejka IPC przekroczy 80% pojemności (`ultrafast_enter_threshold`), ustawiany jest `ultrafast_mode = true`. W tym trybie **całe parsowanie trade'ów jest pomijane** — `return Ok(())` przed jakimkolwiek wywołaniem `parse_trades()`. Wyjście z trybu następuje dopiero gdy kolejka spadnie poniżej 50% (`ultrafast_exit_threshold`).

**Wpływ**: Może powodować **masową utratę** — każdy trade w okresie backpressure jest porzucany. W peak load (np. launch nowego tokena) to potencjalnie setki TX.

**Proponowany fix**: Zmienić zachowanie ultrafast_mode — zamiast pomijać parsowanie trade'ów, pomijać **tylko** budowanie `EnhancedCandidate` i inne ciężkie operacje (RPC calls, ShadowLedger update). Parsowanie `parse_trades()` i emisja przez IPC są tanie (in-memory decode + channel send) i powinny zawsze działać. Alternatywnie: podnieść `ultrafast_enter_threshold` do 95-98%.

---

### L2 — should_forward_trade mapping miss (NAJCZĘSTSZY)

**Lokalizacja**: lib.rs

**Mechanizm**: `should_forward_trade()` sprawdza mapę `curve_to_mint`. Gdy mapping jest nieznany (trade przyszedł PRZED eventem CREATE), trade jest **permanentnie porzucany** (`return false`). W tle uruchamiane jest `queue_curve_mint_resolve()` aby odzyskać mapping przez RPC, ale **bieżący trade jest stracony na zawsze**.

**Wpływ**: Bardzo częsty scenariusz — race condition między CREATE a pierwszymi trade'ami. Każdy nowy pool traci pierwsze 1-N transakcji.

**Proponowany fix**: Zamiast zwracać `false` i tracić trade, buforować go w `pending_trades: HashMap<[u8;32], Vec<TradeEvent>>`. Gdy `set_curve_mapping()` zostanie wywołane (z CREATE lub RPC resolve), odtwarzać zbuforowane trade'y i emitować je przez IPC. To wymaga:
1. Dodania pola `pending_trades` do struktury Seer
2. W `should_forward_trade` — zamiast `return false` → wrzucenie trade'a do bufora
3. W `set_curve_mapping` — drainowanie bufora i emisja zakolejkowanych trade'ów
4. TTL/limit na bufor (np. max 1000 wpisów, 60s timeout)

---

### L3 — parse_miss (discriminator mismatch) (POMIAROWY)

**Lokalizacja**: init_pool_parser.rs vs binary_parser.rs

**Mechanizm**: `tx_contains_supported_trade_instruction()` w lib.rs używa `ghost_core::is_trade_instruction()`, która rozpoznaje **TYLKO** PumpFun buy/sell discriminatory (`0x66...`, `0x33...`) — nawet dla PumpSwap! Tymczasem `PumpParser` w binary_parser.rs używa **dodatkowych** PumpSwap discriminatorów:
- `DISC_SWAP_OUTER_WRAPPER` (`0xe4, 0x45, ...`)
- `DISC_SWAP_EVENT_BUY` (`0x67, 0xf4, ...`)
- `DISC_SWAP_EVENT_SELL` (`0x3e, 0x2f, ...`)
- `DISC_SWAP_BUY_EXACT_QUOTE_IN` (`0xc6, 0x2e, ...`)

**Skutek**: Są dwa scenariusze rozbieżności:
1. **False negative** w `tx_contains_supported_trade_instruction`: TX PumpSwap z `DISC_SWAP_BUY_EXACT_QUOTE_IN` na top-level → `has_trade_candidate = false`, ale `PumpParser` **parsuje** trade z CPI inner instructions → trade jest emitowany, ale nie liczony w `trade_parsed_total` (zajaniżony coverage w logach, ale trade nie jest tracony)
2. **False positive** (parse_miss): TX ma PumpFun buy/sell disc w instrukcji (np. router call), ale `PumpParser` nie wyciąga z niej trade'a bo prawdziwy trade jest w CPI event logs z innymi discriminatorami → `parse_miss_total` rośnie, ale to **fałszywy alarm**

**Wpływ**: Ten bug **nie powoduje realnej utraty trade'ów** — PumpParser parsuje CPI event logs niezależnie od `tx_contains_supported_trade_instruction`. Ale **zaburza metryki** — coverage ratio jest niedokładny. Mogą być trade'y policzone w `parsed` ale nie w `emitted` (lub odwrotnie).

**Proponowany fix**: Wyrównać discriminatory w `is_trade_instruction` z PumpParser — dodać PumpSwap-specific discriminatory:
```rust
AmmType::PumpSwap => {
    if discriminator == PUMPFUN_BUY_DISCRIMINATOR
        || discriminator == DISC_SWAP_OUTER_WRAPPER
        || discriminator == DISC_SWAP_BUY_EXACT_QUOTE_IN {
        Some(true)
    } else if discriminator == PUMPFUN_SELL_DISCRIMINATOR {
        Some(false)
    } else {
        None
    }
}
```

---

## Priorytet implementacji

| # | Fix | Wpływ na coverage | Trudność | Ryzyko |
|---|-----|-------------------|----------|--------|
| 1 | **L2** — bufor pending trades | **WYSOKI** — odzyskuje WSZYSTKIE trade'y przed CREATE | Średnia | Niskie (dodanie bufora) |
| 2 | **L1** — zmiana ultrafast_mode | **WYSOKI** — eliminuje masowe droppowanie | Niska | Niskie (zmiana warunku) |
| 3 | **L3** — wyrównanie discriminatorów | **NISKI** (kosmetyczny) — poprawia metryki | Niska | Żadne |

**L2 + L1 razem powinny podnieść coverage do 95-99%.** L3 jest opcjonalny — poprawia jedynie jakość metryk.