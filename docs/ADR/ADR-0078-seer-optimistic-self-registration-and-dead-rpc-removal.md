# ADR-0078: Seer — Optimistic Self-Registration + Usunięcie Martwego Kodu RPC

**Date:** 2026-04-07
**Status:** Accepted
**Author:** Ghost Father

**Update 2026-04-03:** tymczasowo zachowany compat-knob `CURVE_RESOLVE_MAX_CONCURRENT` / `config.curve_resolve_max_concurrent` został później usunięty całkowicie; zob. `ADR-0079-remove-dead-seer-curve-resolve-config.md`.

---

## Context

Logi produkcyjne wykazywały regularne wpisy:

```
TX_BUFFERED_INACTIVE reason=mapping_conflict sig=... pool=... mint=...
```

 Seer buforował trade'y z `can_forward_with_trade_mint = true` (pool ≠ default, mint ≠ default, mint ≠ WSOL, pool ≠ mint) w oczekiwaniu na mapowanie pool→mint. Mapowanie to miało zostać dostarczone przez `queue_curve_mint_resolve()` → asynchroniczne RPC.

Audyt kodu wykazał że **`queue_curve_mint_resolve()` była martwym kodem od początku**:
- Wstawiała do `pending_curve_resolves: HashSet` ale nic nigdy tego HashSet nie czytało
- `curve_resolve_rpc_semaphore` nigdy nie był `acquire()`'owany
- Importy `GetConfirmedSignaturesForAddress2Config`, `RpcTransactionConfig`, `CommitmentConfig`, `UiTransactionEncoding` — nigdy nie używane
- Stałe `CURVE_RESOLVE_SIGNATURE_LIMIT`, `CURVE_APPLY_SLA_MS` — nigdy nie używane

W efekcie każdy trade z `mapping_conflict` trafiał do bufora z TTL=30ms i przepadał jeśli CREATE event nie dotarł w tym oknie. Ponieważ pump.fun pool addresses są **1:1 z mintami by protokół** (nigdy nie są reużywane dla różnych mintów), trade sam w sobie jest źródłem prawdy dla mapowania pool→mint.

---

## Decision

### 1. Optimistic Self-Registration w `should_forward_trade()`

Gdy `can_forward_with_trade_mint = true`, trade jest używany jako autoritative-enough źródło mapowania:

```rust
// PRZED:
if can_forward_with_trade_mint {
    if !self.pending_curve_resolves.read().contains(&pool_bytes) {
        self.queue_curve_mint_resolve(*pool_id, Pubkey::default()); // martwy kod
    }
    self.buffer_pending_trade(...);
    return TradeForwardDecision::BufferedPendingMapping;
}

// PO:
if can_forward_with_trade_mint {
    self.set_curve_mapping(*pool_id, *mint, "trade_optimistic", false);
    ::metrics::increment_counter!("seer_trade_optimistic_mapping_total");
    info!("TRADE_OPTIMISTIC_FORWARD pool={} mint={} ...");
    return TradeForwardDecision::ForwardWithReplay(*pool_id, *mint);
}
```

`authoritative=false` gwarantuje że CREATE event (który przychodzi z `authoritative=true`) może override'ować ten wpis jeśli jest potrzeba.

### 2. Nowy wariant `TradeForwardDecision::ForwardWithReplay(Pubkey, Pubkey)`

Osobny wariant od `Forward` pozwala `handle_trade_event` wywołać `replay_pending_trades` po rejestracji, a następnie wykonać dedup-check zanim wyemituje live trade. Zapobiega to duplikatom gdy buffered i live trade mają identyczną (signature, event_ordinal).

Logika dedup — suppress live tylko gdy obie strony mają `Some(ordinal)` i są równe:
```rust
matches!(
    (queued.trade.event_ordinal, trade.event_ordinal),
    (Some(a), Some(b)) if a == b
)
```
Gdy ordinals są `None` (legacy path) lub różne (sibling events) — oba przechodzą.

### 3. Usunięcie martwego kodu

Usunięto:
- Funkcja `queue_curve_mint_resolve()` — całkowicie
- Pole struct `pending_curve_resolves: Arc<RwLock<HashSet<[u8; 32]>>>`
- Pole struct `curve_resolve_rpc_semaphore: Arc<Semaphore>`
- Stałe `CURVE_RESOLVE_SIGNATURE_LIMIT`, `CURVE_APPLY_SLA_MS`
- Importy: `GetConfirmedSignaturesForAddress2Config`, `RpcTransactionConfig`, `CommitmentConfig`, `UiTransactionEncoding`, `Semaphore` (tokio::sync)
- Wywołania `queue_curve_mint_resolve` we wszystkich call sites
- `pending_curve_resolves.write().remove()` w `register_curve_mapping()`
- Test `test_curve_resolve_max_concurrent_config_is_used` (testował martwy semaphore)

Tymczasowo zachowano w tej fazie:
- `CURVE_RESOLVE_MAX_CONCURRENT: usize = 32` — nadal referowany przez `config.rs`
- Pole `config.curve_resolve_max_concurrent` — backward compat z config files

Później usunięto oba te leftovery w ADR-0079, gdy potwierdzono brak jakiegokolwiek runtime consumer.

### 4. Aktualizacje testów

Zaktualizowano 15 testów:
- **3 testy behawioralne** całkowicie przepisane (odzwierciedlają nową semantykę forward-zamiast-bufor)
- **7 testów** — usunięto martwe asercje o `pending_curve_resolves`
- **1 test** (`pumpswap_account_update_before_mapping_replays`) — przepisany by używał BondingCurve data zamiast AmmPool (AmmPool self-resolves mint, nie buforuje nigdy)
- **2 testy** (`test_should_forward_*`) — zaktualizowane do `ForwardWithReplay(..)` pattern
- **1 test** — usunięty całkowicie (`test_curve_resolve_max_concurrent_config_is_used`)

---

## Architectural Impact

### Hot Path — `should_forward_trade()`

Stара ścieżka (mapping_conflict):
```
trade arrives → buffer → wait 30ms TTL → [CREATE arrives] → replay
                                        → [no CREATE]     → LOST TRADE
```

Nowa ścieżka:
```
trade arrives → set_curve_mapping(non-auth) → replay buffered sibling → emit live → IPC
                                                                                    ↑ 0ms latency
```

Metryka `seer_trade_optimistic_mapping_total` pozwala obserwować częstotliwość tego path w produkcji.

### Żaden RPC w hot path

Seer nie dotyka RPC w ścieżce forwarding trade'ów. Jedyne RPC w Seer to `grpc_connection.rs` (gap backfill), poza hot pathem.

---

## Risk Assessment

**Overall: Low**

| Risk | Detail | Mitigation |
|------|--------|------------|
| Non-authoritative mapping przetrwa | `authoritative=false` może zostać w mapie jeśli CREATE nie przyjdzie | Pump.fun: pool 1:1 z mint by protokół. CREATE zawsze przychodzi. `authoritative=true` CREATE override'uje gdy potrzeba. |
| Duplicate emissions do IPC | Replay + live z identyczną signature | Dedup-check na `(signature, event_ordinal)` przed emit live. Sibling ordinals (różne) przechodzą poprawnie. |
| Downstream (Gatekeeper) widzi duplikaty | Jeśli ordinal=None — oba przechodzą | Gatekeeper posiada własny dedup po signature. Dwa zdarzenia z tą samą signature są idempotentne. |

---

## Consequences

**Co staje się łatwiejsze:**
- Zero-latency forwarding trade'ów z mapping_conflict → eliminacja klasy `TX_BUFFERED_INACTIVE reason=mapping_conflict` logów
- Kod jest lżejszy: -1 funkcja, -2 pola struct, -2 stałe, -5 importów, -1 test (martwy)
- Eliminacja mylącego kodu który sugerował że RPC jest w hot path

**Co staje się trudniejsze:**
- Mapowanie pool→mint może być non-authoritative przez chwilę (aż do CREATE). W teorii downstream widzi trade z "tymczasowym" mapowaniem. W praktyce jest to identyczne jak mapowanie z CREATE (pump.fun 1:1 protocol guarantee).

---

## Alternatives Considered

### A) Buforowanie z wydłużonym TTL
Zwiększenie TTL z 30ms do np. 500ms. Odrzucone — CREATE może nigdy nie dojść lub przyjść poza oknem; fundamental problem pozostaje.

### B) Asynchroniczne RPC resolve
Przywrócenie i poprawienie `queue_curve_mint_resolve` żeby faktycznie robiło RPC. Odrzucone — RPC jest zbyt wolne dla hot path (~50-150ms p50), adds complexity, adds failure modes. Trade sam jest szybszym źródłem prawdy.

### C) Status quo (nie naprawiać)
Odrzucone. Każdy trade z `mapping_conflict` jest tracony lub opóźniany. Przy TTL=30ms i latencji CREATE>30ms całe klasy trade'ów przepadają.

---

## Validation Steps

1. ✅ `cargo test --lib` — 331 passed, 0 failed
2. Monitor `seer_trade_optimistic_mapping_total` w produkcji — powinna rosnąć gdy mapping_conflict trade'y są optimistycznie forwardowane
3. Monitor `TX_BUFFERED_INACTIVE reason=mapping_conflict` — powinna zanikać
4. Monitor duplikatów po signature w Gatekeeper — nie powinny wzrosnąć
