

# Problem i ogólny cel: 

Naprawa utraty coverage trade tx w warstwie Seer gRPC/parser/transport dla nowych pump.fun pools uruchamianych w trakcie pracy bota

---

# Kontekst i cel biznesowy

Aktualny system Seer ma problem z bardzo niskim coverage trade tx (`buy` / `sell`) względem rzeczywistych transakcji on-chain dla:

- **wyłącznie nowych pooli pump.fun**
- **utworzonych w trakcie działania bota**
- **nie historycznych / nie istniejących wcześniej**

Objaw produkcyjny:
- parser / transport / forward path nie rejestruje wszystkich buy/sell,
- część trade tx jest tracona,
- część coverage metrics jest zaniżona lub myląca,
- część ścieżek buforowania / replay jest niespójna lub niepełna.

Celem zadania jest:
1. **usunąć realne przyczyny utraty trade tx**
2. **usunąć race conditions powodujące drop unresolved trades**
3. **naprawić dedup i replay semantics**
4. **doprowadzić metryki i logi do stanu zgodnego z rzeczywistością**
5. **zostawić kompletny zestaw testów regresyjnych**

---

# Zakres zadania

Zmiany dotyczą głównie plików:

```text name=files.txt
off-chain/components/seer/src/lib.rs
off-chain/components/seer/src/binary_parser.rs
off-chain/components/seer/src/grpc_connection.rs
off-chain/components/seer/src/ipc.rs
off-chain/components/seer/src/types.rs
```

W razie potrzeby można dodać pomocnicze typy/enumy/moduły testowe, ale **nie wolno rozlewać zmian poza ten obszar bez uzasadnienia**.

---

# Główne problemy do naprawy

## P0 — CRITICAL
1. `CpiTrade` może powstać bez znanego `pool_amm_id`, co kończy się `Pubkey::default()`
2. `handle_trade_event()` dropuje taki trade jako `ROLE_MISMATCH` **zanim** trafi do bufora
3. race CREATE vs TRADE między workerami powoduje, że brak mappingu jest normalnym stanem przejściowym, a nie finalnym błędem
4. unresolved trades muszą być **buffered, replayed, recovered**, nie dropped

## P1 — SIGNIFICANT
5. `dedup_trade_events()` nie dedupuje `SwapTrade` vs `CpiSwapBuy/Sell`
6. `cm_reg` jest przekazywany, ale nieużywany
7. **LEGACY / CLOSED:** dawny trop `CURVE_RESOLVE_MAX_CONCURRENT = 2` jako bottleneck recovery jest nieaktualny po usunięciu całej ścieżki resolve RPC; zob. `ADR-0079-remove-dead-seer-curve-resolve-config.md`

## P2 — MODERATE / MINOR
8. metrics replay/live coverage mają niespójną semantykę
9. `DelayedAccountQueue` istnieje, ale production semantics są niejednoznaczne
10. resubscribe / watch / exact-account pruning są słabo mierzone
11. część labels/logów opisuje złą przyczynę

---

# Wymagania implementacyjne — szczegółowe

---

## 1. Naprawa unresolved trades — absolutny priorytet

### 1.1. `handle_trade_event` nie może finalnie dropować trade’a z `pool_amm_id == Pubkey::default()` tylko dlatego, że mapping jeszcze nie istnieje

### Wymaganie:
W `lib.rs`, w `Seer::handle_trade_event`, zmień logikę tak, aby:

- trade z `pool_amm_id == Pubkey::default()` **nie był finalnym dropem**
- zamiast tego trafiał do bufora unresolved trades
- po zarejestrowaniu mappingu był replayed i forwarded

### Obowiązkowe zachowanie:
- jeśli `trade.pool_amm_id == Pubkey::default()` i `trade.mint != Pubkey::default()`:
  - bufferuj
  - uruchom / dopilnuj resolve path
  - nie oznaczaj tego jako finalny `ROLE_MISMATCH`
- jeśli `trade.pool_amm_id == Pubkey::default()` i `trade.mint == Pubkey::default()`:
  - też bufferuj, ale oznacz jako słabszy przypadek unresolved
  - dopiero po TTL może być finalnie expired/dropped

---

## 2. Wprowadzenie jawnej semantyki bufferingu

### 2.1. Dodaj enum przyczyny buforowania

Wprowadź typ, np.:

```rust name=pending_trade_reason.rs
enum PendingTradeReason {
    MissingPoolFromMint,
    MissingMintAndPool,
    MappingConflict,
    CurveMappingMissing,
}
```

### Wymaganie:
`PendingTrade` ma przechowywać nie tylko trade, ale też:
- `source_label`
- `is_coverage_source`
- `queued_at`
- `reason: PendingTradeReason`

Nie używaj stringów jako source of truth dla reason.

---

## 3. Zmień klucz buforowania pending trades

### Problem:
Obecne indeksowanie po `pool_amm_id.to_bytes()` jest błędne dla unresolved trades z default pool.

### Wymaganie:
Wprowadź jawny klucz, np.:

```rust name=pending_trade_key.rs
enum PendingTradeKey {
    ByCurve([u8; 32]),
    ByMint([u8; 32]),
    BySignature(String),
}
```

### Reguła:
- jeśli `pool_amm_id != default` → `ByCurve`
- jeśli `pool_amm_id == default` i `mint != default` → `ByMint`
- jeśli oba unknown → `BySignature`

### Zakaz:
Nie wolno trzymać wszystkich unresolved default-pool trades pod jednym kluczem default pubkey.

---

## 4. Replay unresolved trades po `register_curve_mapping`

### Wymaganie:
Po każdym:

```rust
register_curve_mapping(curve, mint, source, authoritative)
```

system ma:
1. odszukać pending trades po curve
2. odszukać pending trades po mint
3. naprawić pola trade:
   - ustawić `pool_amm_id = curve`, jeśli był default
   - ustawić `mint = mint`, jeśli był default
4. wypuścić je tą samą ścieżką forwardowania co live trade
5. uniknąć duplikatów

### Dotyczy wszystkich źródeł mappingu:
- CREATE
- TRADE
- entry CPI create
- RPC resolve
- account update path

---

## 5. Nie serializować całego pipeline’u

### Zakaz:
Nie wolno naprawiać race condition przez:
- pojedynczy worker globalnie
- mutex na `process_event`
- sztuczne sekwencjonowanie całego run loop

### Wymóg:
Pipeline ma pozostać równoległy, ale **odporny na reorder** przez buffering + replay.

---

## 6. Przepisać `dedup_trade_events`

### Funkcja do pełnej przebudowy:
- `binary_parser.rs::dedup_trade_events`

### Wymagania:
#### Pump.fun
- jeśli istnieją jednocześnie `Trade` i `CpiTrade` dla tej samej operacji → zostaw `CpiTrade`

#### PumpSwap
- jeśli istnieją jednocześnie `SwapTrade` i `CpiSwapBuy`/`CpiSwapSell` → zostaw CPI variant

### Dodatkowo:
- `cm_reg` ma być realnie używany
- `let _ = cm_reg;` ma zniknąć
- istniejący test oczekujący `len == 2` dla przypadku z CPI ma zostać poprawiony

---

## 7. LEGACY — dawniej: zwiększyć przepustowość resolve path

### Zmień:
`CURVE_RESOLVE_MAX_CONCURRENT`

### Wymaganie:
- default minimum: `16`
- preferowane: `32`
- najlepiej wpiąć do configu z bezpiecznym defaultem

### Dodatkowo:
Dodać metryki:
- liczba oczekujących resolve tasks
- wait time na semaphore permit
- RPC resolve latency
- resolve failure count
- pending trade expired before resolve count

---

## 8. Naprawić semantykę coverage / replay metrics

### Wymaganie:
Rozdzielić metryki na:
- **event-level**
- **signature-level**

### Obowiązkowe liczniki:
#### Event-level
- parsed
- forwarded_live
- forwarded_replay
- buffered
- expired
- dedup_dropped

#### Signature-level
- parsed_signatures
- forwarded_live_signatures
- forwarded_replay_signatures

### Wymóg:
Nie wolno używać jednego licznika jako jednocześnie event-level i signature-level.

---

## 9. Naprawić `replay_pending_trades_from_state`

### Wymaganie:
- każdy replayed trade zwiększa event-level replay counter
- unikalne sygnatury zwiększają signature-level replay counter
- stary niejednoznaczny `pooltx_emitted_total` ma zostać:
  - albo usunięty,
  - albo jednoznacznie przedefiniowany i przemianowany

---

## 10. Uporządkować `SEER_COVERAGE`

### Wymaganie:
Log coverage ma rozdzielać:
- parsed event coverage
- forwarded live event coverage
- replay forwarded event coverage
- eventual total coverage
- signature-level coverage
- buffered count
- expired count

### Zakaz:
Nie wolno prezentować jednego procentu coverage, jeśli miesza on:
- live
- replay
- buffered unresolved
- event-level
- signature-level

---

## 11. `DelayedAccountQueue` — podjąć jawną decyzję

### Agent ma zrobić dokładnie jedną rzecz:

#### Opcja A — podłączyć do production hot path
albo

#### Opcja B — jawnie zdegradować/usunąć z production semantics

### Preferencja:
Jeśli pełna integracja jest zbyt kosztowna w tym zadaniu, wybierz **Opcję B**:
- usuń mylące eksponowanie
- zostaw tylko jeden aktywny mechanizm bufferingu account updates
- zaktualizuj komentarze i testy

### Zakaz:
Nie wolno zostawić „martwego” mechanizmu sugerującego aktywną ochronę, jeśli nie działa w hot path.

---

## 12. Resubscribe / watch / exact-account telemetry

### Wymaganie:
Dodać metryki:
- `watch_pool` → first account update latency
- registry change → resubscribe send latency
- pending exact-watch count
- curves pruned by cap
- pools pruned by cap
- exact-watch evictions

### Dodatkowo:
Komentarze i testy mają jasno opisywać, że:
- exact-account watch set jest ograniczony
- pruning jest świadomym mechanizmem
- mint watches nie mają tej samej semantyki co curve/pool exact accounts

---

## 13. Naprawa labels/logów final outcome trade

### Wymaganie:
Wprowadzić jednolity zestaw outcome kategorii, np.:

```rust name=trade_outcome.rs
enum TradeOutcome {
    ForwardedLive,
    ForwardedReplay,
    BufferedMissingPool,
    BufferedMissingMapping,
    FilteredInvalidPool,
    FilteredWsolPool,
    FilteredMappingConflictUnrecoverable,
    ExpiredWaitingForMapping,
    DedupDropped,
    IpcSendFailed,
}
```

### Te same kategorie mają być używane do:
- logów
- metryk
- test assertions

### Zakaz:
Nie wolno zostawić generycznych labeli typu `trade_filtered_unwatched_pool`, jeśli realna przyczyna była inna.

---

# Wymagane testy

---

## A. Testy krytyczne — must have

1. `CpiTrade` z `pool_amm_id == default()`:
   - nie jest finalnie dropowany
   - trafia do pending buffer
2. Mapping pojawia się później:
   - trade zostaje naprawiony
   - trade zostaje wysłany przez IPC
3. Race CREATE vs TRADE:
   - niezależnie od kolejności workerów trade nie ginie
4. Replay unresolved trade:
   - poprawia `pool_amm_id`
   - nie tworzy duplikatu
5. TTL expiry:
   - unresolved trade po TTL przechodzi do expired counter
   - nie jest liczony jako forwarded

---

## B. Testy dedup

6. Pump.fun `Trade + CpiTrade` → zostaje 1
7. PumpSwap `SwapTrade + CpiSwapBuy` → zostaje 1
8. PumpSwap `SwapTrade + CpiSwapSell` → zostaje 1
9. Brak CPI → nie dedupuj agresywnie

---

## C. Testy metrics semantics

10. replay 2 events / 1 signature:
   - event counter = 2
   - signature counter = 1
11. live + replay metrics są rozdzielone
12. buffered event nie jest liczony jako forwarded
13. expired event jest liczony jako expired, nie filtered_invalid_pool

---

## D. Testy transport / watch / pruning

14. exact watch pruning liczy curves i pools oddzielnie
15. registry change powoduje resubscribe w bounded czasie
16. `DelayedAccountQueue`:
   - jeśli opcja A: realny test integracyjny hot path
   - jeśli opcja B: testy potwierdzają, że nie jest production path dependency

---

# Zakazy implementacyjne

Agent **nie może**:

1. naprawić problemu przez wyłączenie równoległości workerów
2. dodać globalnego locka na cały `process_event`
3. zostawić `dedup_trade_events` jako częściowy patch
4. zostawić niejednoznacznych metryk coverage
5. zostawić „martwego” `DelayedAccountQueue` bez decyzji architektonicznej
6. wprowadzić forwardowania wszystkich invalid pool trades „na wszelki wypadek”
7. dodać hacków tylko po to, by test przeszedł bez poprawy semantyki

---

# Oczekiwany rezultat końcowy

Po zakończeniu zadania system ma:

1. odzyskiwać unresolved trades zamiast je gubić,
2. być odporny na reorder CREATE/TRADE między workerami,
3. nie generować duplikatów PumpSwap na wejściu do IPC,
4. mieć sensowny throughput resolve path pod burstami,
5. raportować coverage zgodnie z rzeczywistością,
6. mieć jednoznaczne logi i metryki,
7. posiadać komplet testów regresyjnych zabezpieczających tę logikę.

---

# Definition of Done

Zadanie jest ukończone dopiero, gdy jednocześnie:

- [ ] trade z unresolved pool mapping nie jest finalnie dropowany przed bufferingiem
- [ ] pending unresolved trades są replayowane po pojawieniu się mappingu
- [ ] race CREATE vs TRADE nie powoduje utraty eventu
- [ ] `dedup_trade_events()` dedupuje również PumpSwap CPI vs SwapTrade
- [ ] resolve concurrency nie wynosi 2 i jest telemetrycznie mierzalny
- [ ] event-level i signature-level metrics są rozdzielone
- [ ] coverage logi uwzględniają replay i buffering
- [ ] `DelayedAccountQueue` ma jednoznaczną decyzję architektoniczną
- [ ] pruning / resubscribe / watch latency mają metryki
- [ ] labels/logi odzwierciedlają prawdziwą przyczynę
- [ ] wszystkie nowe testy przechodzą
- [ ] istniejące testy nie są „naprawione” przez osłabienie asercji bez realnej poprawy logiki

---

Jeśli chcesz, mogę teraz zrobić jeszcze **wersję 4**:  
## gotowy tekst w formacie **GitHub Issue / PR problem statement**  
czyli krócej, bardziej formalnie, bez tłumaczeń — dokładnie pod wrzutkę do narzędzia tworzącego issue albo PR task.


---

# PLAN NAPRAWCZY — CZĘŚĆ 1  
## Zakres: CRITICAL + SIGNIFICANT
Dotyczy:
- **BUG 1** — `CpiTrade` race → drop jako `ROLE_MISMATCH`
- **BUG 2** — race window CREATE vs TRADE między workerami
- **BUG 3 / 8** — `dedup_trade_events` ignoruje `cm_reg`, duplikaty PumpSwap
- **BUG 4** — **LEGACY / CLOSED:** dawny bottleneck `CURVE_RESOLVE_MAX_CONCURRENT = 2`; nieaktualne po usunięciu resolve path w `ADR-0079-remove-dead-seer-curve-resolve-config.md`

---

# 0. Cel nadrzędny zmian

System ma spełniać następujące reguły po wdrożeniu poprawek:

1. **Żaden poprawny trade buy/sell dla świeżo wykrytego pump.fun poola nie może zostać bezpowrotnie odrzucony tylko dlatego, że mapping `curve ↔ mint` nie był jeszcze gotowy w momencie parsowania.**
2. Jeśli trade nie może być od razu poprawnie zmapowany do poola:
   - ma zostać **zbuforowany**,
   - ma zostać **naprawiony i wypuszczony później** po pojawieniu się mappingu,
   - nie może zostać zalogowany jako finalny drop typu `ROLE_MISMATCH`.
3. Duplikaty PumpSwap (`SwapTrade` + `CpiSwapBuy/Sell`) mają być usuwane **w parserze**, zanim trafią do IPC.
4. **LEGACY history note:** dawniej plan zakładał strojenie ścieżki RPC resolve; po audycie i cleanupie ta ścieżka została usunięta z Seera, więc ten punkt nie jest już aktywnym wymaganiem (zob. `ADR-0079-remove-dead-seer-curve-resolve-config.md`).

---

# 1. CRITICAL — naprawa utraty trade’ów przy braku mappingu

## 1.1. Problem do naprawy

Aktualnie `ParsedEventKind::CpiTrade` w `binary_parser.rs` wyznacza `pool_amm_id` przez reverse lookup `curve_for_mint()`.  
Jeśli mapping nie istnieje, parser buduje trade z:

- `pool_amm_id = Pubkey::default()`

Następnie `lib.rs::handle_trade_event()` odrzuca taki trade **natychmiast**, zanim ten trafi do bufora pending trades.

To jest niedopuszczalne.

---

## 1.2. Wymaganie implementacyjne

### Należy zmienić zachowanie systemu tak, aby:

- trade z:
  - `pool_amm_id == Pubkey::default()`
  - lub `pool_amm_id == WSOL`
  - lub innym stanem „nie da się jeszcze bezpiecznie przypisać do poola”
  
**nie był finalnie dropowany na wejściu**, jeśli istnieje możliwość, że brak poola wynika z chwilowego braku mappingu.

### Szczególnie:
`pool_amm_id == Pubkey::default()` dla trade z poprawnym `mint != Pubkey::default()` ma być traktowane jako:
- **stan przejściowy**
- **kandydat do buforowania**
- **nie finalny role mismatch**

---

## 1.3. Konkretne zmiany w `lib.rs`

### Funkcja do zmiany
- `Seer::handle_trade_event`

### Obecny stan błędny
Nie wolno zostawić logiki w stylu:

```rust
if Self::is_invalid_trade_pool(&trade.pool_amm_id) {
    ...
    return false;
}
```

dla przypadku:
- `trade.pool_amm_id == Pubkey::default()`
- oraz `trade.mint != Pubkey::default()`

bo to powoduje bezpowrotną utratę trade’a.

---

## 1.4. Nowa logika decyzyjna w `handle_trade_event`

### Zaimplementuj dokładnie taki podział:

#### Case A — `trade.pool_amm_id == Pubkey::default()` i `trade.mint != Pubkey::default()`
To **nie jest finalny invalid trade**.  
To jest **trade wymagający opóźnionego przypisania poola**.

**Akcja obowiązkowa:**
1. wywołaj `buffer_pending_trade(...)`
2. jeśli nie ma aktywnego resolve dla tego obiektu — uruchom resolve
3. **nie inkrementuj licznika finalnych filtered drops**
4. **nie loguj jako `ROLE_MISMATCH`**
5. inkrementuj osobną metrykę typu:
   - `trade_buffered_missing_pool_mapping`
   - albo analogiczną, ale musi odróżniać buffered od dropped

#### Case B — `trade.pool_amm_id == Pubkey::default()` i `trade.mint == Pubkey::default()`
To jest stan znacznie słabszy informacyjnie.

**Akcja obowiązkowa:**
- również buforuj, ale oznacz jako **unresolved trade missing pool+mint**
- taki trade nie może być finalnie wypuszczony bez późniejszej naprawy
- dodaj osobną ścieżkę metryk/logowania
- jeśli z systemu nie da się go już uratować po TTL — wtedy dopiero licz jako expire/drop

#### Case C — `trade.pool_amm_id == WSOL`
To pozostaje invalid pool i może być dropowany finalnie, **o ile nie jest to przypadek przejściowy wynikający z błędnego mappingu**.
Tutaj nie zmieniaj semantyki na ślepo dla każdego przypadku.  
Ale jeśli da się wykazać, że to przejściowy efekt unresolved mapping — buforuj zamiast dropować.

#### Case D — poprawny `pool_amm_id`, ale mapping conflict
To ma dalej przechodzić przez `should_forward_trade()`.

---

## 1.5. Nowa pomocnicza funkcja — obowiązkowa

Dodaj nową funkcję w `lib.rs`, np.:

- `fn should_buffer_unresolved_trade(&self, trade: &types::TradeEvent) -> bool`

### Funkcja ma zwracać `true`, jeśli:
- `trade.pool_amm_id == Pubkey::default()`
- lub istnieją oznaki, że parser nie zdołał jeszcze rozwiązać mappingu
- i trade nie jest ewidentnym śmieciem

### Funkcja ma zwracać `false`, jeśli:
- dane są tak uszkodzone, że nie ma żadnej możliwości odzyskania eventu
- np. `pool_amm_id == default` i `mint == default` i signer/default i brak sensownych pól identyfikujących

### Cel:
Cała logika decyzji „buffer vs final drop” ma być jawna i testowalna, a nie rozsiana po `handle_trade_event`.

---

## 1.6. Zmiana kontraktu `buffer_pending_trade`

### Funkcja do rozbudowy
- `buffer_pending_trade`

### Obowiązkowe zmiany:
Bufor ma przechowywać wystarczającą informację do późniejszej naprawy trade’a.

To oznacza, że w `PendingTrade` musisz zachować:
- cały `TradeEvent`
- `source_label`
- `is_coverage_source`
- `queued_at`
- oraz **powód buforowania**

### Dodaj pole enum, np.:
- `PendingTradeReason`

Warianty minimalne:
- `MissingPoolFromMint`
- `MissingMintAndPool`
- `MappingConflict`
- `CurveMappingMissing`

Nie używaj stringów jako reason; ma to być enum.

### Dlaczego:
Agent implementujący replay ma wiedzieć dokładnie, *jak* naprawiać trade zależnie od przyczyny.

---

## 1.7. Zmiana klucza buforowania pending trades

### Obecny problem
Aktualnie pending trades są indeksowane po `trade.pool_amm_id.to_bytes()`.

To **nie wystarczy** dla trade’ów z:
- `pool_amm_id == Pubkey::default()`

bo wszystkie takie trade’y trafią pod jeden wspólny klucz default pubkey, co jest błędne.

### Obowiązkowa zmiana
Wprowadź jawny klucz bufora, np. enum:

- `PendingTradeKey`

Warianty minimalne:
- `ByCurve([u8; 32])`
- `ByMint([u8; 32])`
- `BySignature(String)` — tylko jako fallback, jeśli nic lepszego nie ma

### Reguła:
- jeśli `pool_amm_id != default` → klucz `ByCurve(pool)`
- jeśli `pool_amm_id == default`, ale `mint != default` → klucz `ByMint(mint)`
- jeśli oba unknown → fallback po sygnaturze

### To jest wymaganie krytyczne.
Bez tego bufor unresolved trades będzie logicznie uszkodzony.

---

## 1.8. Replay unresolved trades po zarejestrowaniu mappingu

### Funkcje do zmiany
- `register_curve_mapping`
- `replay_pending_trades`
- `take_pending_trades_from_store`
- ewentualnie rozdzielić na osobne replaye:
  - po curve
  - po mint

### Obowiązkowe zachowanie
Po wywołaniu:

```rust
register_curve_mapping(curve, mint, ...)
```

system ma:

1. odszukać pending trades oczekujące:
   - po curve
   - po mint
2. dla każdego trade’a:
   - jeśli `pool_amm_id == default`, ustawić `pool_amm_id = curve`
   - jeśli `mint == default`, ustawić `mint = mint`
3. przepuścić przez tę samą końcową ścieżkę forwardowania co live trade
4. nie tworzyć duplikatu, jeśli dany trade już został wypuszczony

### Bardzo ważne:
Replay unresolved trades ma działać zarówno gdy mapping pojawia się z:
- CREATE
- TRADE
- ENTRY CPI CREATE
- RPC resolve
- account update path

Nie wolno zakładać jednego źródła mappingu.

---

## 1.9. Zmiana logowania i metryk

### Zabronione po poprawce
Nie wolno logować unresolved trade jako:

- `TX_DROPPED ... reason=ROLE_MISMATCH`

jeśli realny problem to brak mappingu.

### Wymagane nowe logi:
- `TX_BUFFERED_UNRESOLVED_POOL`
- `TX_BUFFERED_UNRESOLVED_MAPPING`
- `TX_REPLAYED_AFTER_MAPPING`
- `TX_EXPIRED_WAITING_FOR_MAPPING`

### Wymagane nowe metryki:
- `seer_trade_buffered_missing_pool_total`
- `seer_trade_buffered_missing_mapping_total`
- `seer_trade_replayed_after_mapping_total`
- `seer_trade_expired_waiting_for_mapping_total`

Muszą rozróżniać:
- buffered
- replayed
- expired
- final dropped

---

# 2. CRITICAL — race CREATE vs TRADE między workerami

## 2.1. Założenie naprawcze

**Nie wolno próbować naprawiać tego przez serializację całego pipeline’u** ani przez globalny mutex na `process_event`.

### Jedyna właściwa naprawa:
System ma być odporny na dowolną kolejność:
- CREATE przed TRADE
- TRADE przed CREATE
- CREATE i TRADE równolegle
- CREATE z entry path, TRADE z tx path
- trade przed sync do parsera

### Czyli:
Naprawa BUG 2 jest realizowana przez:
- poprawne buforowanie unresolved trades
- poprawny replay po mappingu
- brak finalnego dropu przed buforowaniem

To jest wymóg projektowy.

---

## 2.2. Dodatkowa obowiązkowa synchronizacja semantyczna

### Funkcja do zmiany
- `sync_curve_mapping_to_parser`
- `register_curve_mapping`

### Wymaganie
Po `register_curve_mapping(...)` kolejność działań ma być:

1. zapis do map Seera
2. zapis do map parsera
3. replay pending curve updates
4. replay pending trades

i ta kolejność ma być zachowana.

### Nie wolno:
- robić replay pending trades przed zsynchronizowaniem mappingu do parsera
- bo w takim przypadku kolejne parse’y wciąż mogą zwracać unresolved

---

# 3. SIGNIFICANT — deduplikacja PumpSwap w parserze

## 3.1. Problem do naprawy

`dedup_trade_events()` obecnie:
- obsługuje tylko `Trade` vs `CpiTrade`
- ignoruje `SwapTrade` vs `CpiSwapBuy/CpiSwapSell`
- ignoruje `cm_reg`

To ma zostać naprawione.

---

## 3.2. Funkcja do całkowitego przepisania
- `binary_parser.rs::dedup_trade_events`

### Nie wolno robić kosmetycznej poprawki
To ma być **pełna deduplikacja semantyczna**, nie `if`-patch.

---

## 3.3. Docelowa reguła dedupu

### Dla Pump.fun bonding curve:
Jeśli dla tej samej transakcji istnieją oba:
- `ParsedEventKind::Trade { side=Buy/Sell }`
- `ParsedEventKind::CpiTrade(...)` z tym samym side

to:
- **preferuj `CpiTrade`**
- usuń `Trade`

### Dla PumpSwap:
Jeśli dla tej samej transakcji istnieją oba:
- `ParsedEventKind::SwapTrade { side=Buy/Sell, pool, ... }`
- `ParsedEventKind::CpiSwapBuy(...)` albo `CpiSwapSell(...)`

to:
- **preferuj CPI swap event**
- usuń `SwapTrade`

---

## 3.4. Klucz deduplikacji — obowiązkowy

Dedup ma działać po kluczu logicznym, nie „na oko”.

Wprowadź wewnętrzny klucz porównawczy, oparty minimum o:
- `side`
- `pool` lub `bonding_curve`
- `user` jeśli dostępny
- `base_amount/quote_amount` lub `token_amount/sol_amount`
- najlepiej także slot/signature jeśli dostępne w surrounding context

Jeśli w samym `ParsedPumpEvent` nie ma signature, dedup ma działać **w obrębie jednego wyniku parse dla pojedynczego TX**, więc to wystarcza.

---

## 3.5. Wykorzystanie `cm_reg`

### `cm_reg` nie może już być martwym parametrem

Ma służy�� do:
- rozstrzygania czy CPI event ma wystarczającą informację, by zastąpić SwapTrade
- w razie potrzeby sprawdzenia, czy mint/pool można wiarygodnie skorelować

### Zabronione:
- zostawić `let _ = cm_reg;`

---

## 3.6. Testy obowiązkowe

Przepisz i dodaj testy:

1. **Pump.fun**
   - `Trade + CpiTrade` → zostaje 1
2. **PumpSwap buy**
   - `SwapTrade + CpiSwapBuy` → zostaje 1
3. **PumpSwap sell**
   - `SwapTrade + CpiSwapSell` → zostaje 1
4. **Brak CPI**
   - `SwapTrade` bez CPI → zostaje
5. **Brak możliwości korelacji**
   - jeśli nie da się bezpiecznie zmatchować → nie dedupuj agresywnie

### Obowiązkowo zmień istniejący test:
- `dedup_keeps_pumpswap_candidates_until_trade_stage`

Po poprawce test **nie może** oczekiwać `len == 2` dla przypadku z dostępnym CPI.

---

# 4. SIGNIFICANT — LEGACY history note: bottleneck RPC resolve

## 4.1. Problem do naprawy

Ten punkt jest historyczny. W chwili pisania planu zakładano, że Seer posiada aktywną ścieżkę RPC resolve dla `curve -> mint`.
Po późniejszym audycie i cleanupie usunięto zarówno resolve path, jak i compat-knob `CURVE_RESOLVE_MAX_CONCURRENT`.
Finalny stan architektury opisuje `ADR-0079-remove-dead-seer-curve-resolve-config.md`.

---

## 4.2. LEGACY — dawny kierunek zmian konfiguracji

### Dawniej planowano zmienić:
```rust
const CURVE_RESOLVE_MAX_CONCURRENT: usize = 2;
```

### Na:
- minimum `16`
- preferowane `32`

### Dawniej planowano dodatkowo:
nie hardcoduj tego ślepo na zawsze — wprowadź możliwość konfiguracji przez `SeerConfig`, ale:
- ustaw default na 32
- testy muszą nadal działać deterministycznie

> **Nieaktualne:** poprawnym stanem po cleanupie jest brak takiego knobu, bo nie istnieje już resolver RPC do strojenia.

---

## 4.3. LEGACY — dawne metryki i logi dla resolve pressure

Dodaj:

- gauge: liczba aktualnie oczekujących resolve tasks
- histogram: czas oczekiwania na permit semafora
- histogram: czas pełnego RPC resolve
- counter: resolve timeout / resolve failure
- counter: pending trade expired before resolve completed

### Nazwy przykładowe:
- `seer_curve_resolve_waiting_tasks`
- `seer_curve_resolve_semaphore_wait_ms`
- `seer_curve_resolve_rpc_ms`
- `seer_curve_resolve_failed_total`
- `seer_trade_expired_before_resolve_total`

---

## 4.4. LEGACY — dawna zmiana `queue_curve_mint_resolve`

### Obowiązkowe zachowania:
1. jeśli resolve dla danego key już trwa — nie spawnuj duplikatu
2. jeśli task czeka długo na permit — zaloguj warning
3. po udanym resolve:
   - zawsze wywołaj wspólną ścieżkę `register_curve_mapping(...)`
   - nie rób lokalnej, odrębnej semantyki replay z pominięciem centralnej logiki, jeśli da się tego uniknąć

### Cel:
Nie rozdwajać logiki napraw unresolved state.

---

## 4.5. Testy obowiązkowe

Dodaj testy stresowe / logiczne:

1. 100 pending unresolved trades dla różnych pooli/mintów
2. limited resolve concurrency > 1
3. mappingi pojawiają się asynchronicznie
4. trades nie expirowane przedwcześnie
5. replay kończy się kompletnym forwardem

Nie musi to być benchmark, ale musi być test logiki kolejkowania i replay.

---

# 5. Zmiany, których agent NIE MA robić w tej części

Żeby uniknąć złej interpretacji:

## Nie robić teraz:
- serializacji całego `Seer::run()` do pojedynczego workera
- globalnego mutexa na `process_event`
- przepisywania całego gRPC transportu
- migracji wszystkiego na `DelayedAccountQueue`
- refaktoru całego parsera eventów
- zmian „na skróty” polegających na zamianie każdego invalid pool na forward

## Ma zrobić dokładnie:
- **buffer zamiast drop** dla unresolved trades
- **replay po mappingu**
- **dedup PumpSwap**
- **poszerzenie resolve concurrency**
- **pełne testy regresyjne**

---

# 6. Definition of Done dla części 1

Ta część zadania jest ukończona tylko wtedy, gdy:

1. Trade z `pool_amm_id == default()` nie jest finalnie dropowany przed buforowaniem.
2. Po `register_curve_mapping(curve, mint, ...)` pending unresolved trades:
   - są odnajdywane,
   - są naprawiane,
   - są emitowane.
3. Race CREATE vs TRADE nie powoduje utraty eventu.
4. `dedup_trade_events()` nie wypuszcza duplikatu `SwapTrade + CpiSwapBuy/Sell`.
5. **LEGACY / superseded:** dawniej oczekiwano strojenia `CURVE_RESOLVE_MAX_CONCURRENT`; po usunięciu resolve path poprawnym stanem jest całkowity brak tego knobu.
6. Istnieją testy pokrywające każdy z powyższych scenariuszy.
7. Żaden istniejący test poprawnej obsługi pump.fun nie przestaje przechodzić.


Jasne — poniżej **część 2**, skupiona na problemach **MODERATE / MINOR**, ale opisana równie rygorystycznie, żeby agent wykonujący zadanie nie musiał niczego zgadywać.

---

# PLAN NAPRAWCZY — CZĘŚĆ 2  
## Zakres: MODERATE + MINOR + telemetry / operacyjność
Dotyczy:
- **BUG 5** — błędne / mylące liczenie `pooltx_emitted_total`
- **BUG 6 / 9** — `DelayedAccountQueue` istnieje, ale nie jest wpięty w hot path
- **BUG 7** — resubscribe cadence i wpływ na świeżość account updates / ShadowLedger
- **BUG 8 z tabeli podsumowującej** — pruning / exact account capacity
- **BUG 9 z tabeli podsumowującej** — mylące labelki telemetryczne
- oraz uporządkowanie metryk, definicji coverage i testów regresyjnych wokół tych obszarów

---

# 0. Cel tej części

Po wdrożeniu zmian z tej części system ma spełniać następujące warunki:

1. Metryki coverage mają rozróżniać:
   - eventy rzeczywiście utracone,
   - eventy czasowo zbuforowane,
   - eventy później zreplayowane,
   - eventy zduplikowane, ale odfiltrowane świadomie.
2. `DelayedAccountQueue` ma być:
   - albo **naprawdę podłączony do produkcyjnego hot patha**,
   - albo **jawnie usunięty / zdegradowany do internal-only test utility**,
   - ale nie może pozostać „martwą infrastrukturą”, która sugeruje ochronę przed race, a w praktyce nie działa.
3. Watch/resubscribe/account-update path ma być przewidywalny i mierzalny.
4. Limity exact-account watch set mają być jawnie kontrolowane, mierzone i testowane.
5. Nazwy metryk i logów mają odpowiadać rzeczywistym przyczynom.

---

# 1. MODERATE — naprawa metryk replay / emitted / coverage

## 1.1. Problem do naprawy

Obecnie `pooltx_emitted_total` nie jest dobrą definicją „ile trade’ów realnie zostało wypuszczonych”, ponieważ:
- w live path licznik działa inaczej niż w replay path,
- replay używa `HashSet<Signature>`, więc może celowo zliczać mniej niż faktycznie wysłanych eventów,
- przez to coverage logowany w `SEER_COVERAGE` może wyglądać gorzej niż realny forwarding.

To trzeba uporządkować.

---

## 1.2. Wymaganie projektowe

### Od teraz muszą istnieć **dwie różne klasy liczników**:

#### A. Liczniki event-level
Liczą realną liczbę eventów trade, które:
- zostały sparsowane,
- zostały wypuszczone do IPC,
- zostały zreplayowane,
- zostały zdropowane po TTL,
- zostały odrzucone jako duplikat.

#### B. Liczniki transaction/signature-level
Liczą unikalne tx/signatures, używane do coverage raportowanego „per tx”.

### Nie wolno mieszać tych dwóch światów w jednym liczniku.

---

## 1.3. Obowiązkowe nowe liczniki

Dodaj rozdzielone liczniki:

### Event-level
- `seer_trade_events_parsed_total`
- `seer_trade_events_forwarded_live_total`
- `seer_trade_events_forwarded_replay_total`
- `seer_trade_events_buffered_total`
- `seer_trade_events_expired_total`
- `seer_trade_events_dedup_dropped_total`

### Signature-level
- `seer_trade_signatures_parsed_total`
- `seer_trade_signatures_forwarded_live_total`
- `seer_trade_signatures_forwarded_replay_total`

### Coverage-specific
- `seer_trade_candidate_total` — zostawić, ale doprecyzować semantykę
- `seer_trade_parse_miss_total`
- `seer_trade_forwarded_total` — jeśli zostaje, musi być jawnie zdefiniowane czy to event-level czy signature-level

---

## 1.4. Zmiana `replay_pending_trades_from_state`

### Funkcja do zmiany
- `replay_pending_trades_from_state`

### Obowiązkowa zmiana semantyki
Nie używaj jednego `HashSet<Signature>` jako jedynego licznika „emitted”.

### Wymagane zachowanie:
1. Dla **każdego** skutecznie wysłanego replayed trade:
   - inkrementuj event-level replay counter
2. Dodatkowo:
   - utrzymuj `HashSet<Signature>` tylko dla signature-level replay counter
3. `pooltx_emitted_total`:
   - albo usuń całkowicie,
   - albo przedefiniuj jednoznacznie jako signature-level counter i zmień nazwę na coś w rodzaju `forwarded_trade_signatures_total`

### Zakaz:
Nie wolno zostawić obecnej niejednoznacznej semantyki.

---

## 1.5. Zmiana `emit_trade_only`

### Funkcja do zmiany
- `emit_trade_only`

### Wymagane zachowanie
Po skutecznym `ipc_sender.send_trade(...)`:
- inkrementuj **event-level live forwarded**
- inkrementuj signature-level live forwarded tylko jeśli to pierwsza emisja dla danej sygnatury w obrębie danego scope/licznika

Jeśli nie chcesz utrzymywać globalnego in-memory setu w tej warstwie:
- zostaw signature-level tylko tam, gdzie i tak już istnieje lokalny batching / agregacja
- ale wtedy coverage logi nie mogą udawać „tx-level” jeśli są event-level

---

## 1.6. Obowiązkowa zmiana logiki `SEER_COVERAGE`

### Funkcja do zmiany
- `maybe_log_coverage`

### Wymaganie
W logu coverage mają być pokazane oddzielnie co najmniej:

- parsed event count
- live forwarded event count
- replay forwarded event count
- total forwarded event count
- parsed signature count
- forwarded signature count
- buffered count
- replayed count
- expired count

### Zakaz
Nie wolno dalej prezentować jednego procentu coverage, jeśli w tle mieszają się:
- buffered, ale jeszcze nie replayed
- replayed później
- event-level i signature-level

### Dopuszczalne podejście
Loguj dwa procenty:
- **event coverage**
- **signature coverage**

oraz osobno:
- **instant live coverage**
- **eventual coverage including replay**

---

## 1.7. Testy obowiązkowe

1. Replay dwóch trade eventów z tym samym signature:
   - event-level counter ma wzrosnąć o 2
   - signature-level counter ma wzrosnąć o 1
2. Live + replay path mają spójne liczenie
3. Buffered-but-not-yet-replayed event nie może być liczony jako forwarded
4. Expired pending trade zwiększa expired counter, nie forwarded counter

---

# 2. MINOR / ARCHITEKTURALNE — `DelayedAccountQueue` nie może zostać „martwy”

## 2.1. Problem do decyzji architektonicznej

W repo istnieje kompletna implementacja `DelayedAccountQueue`, ale z komentarza i flow wynika, że:
- jest tworzona,
- jest eksportowana,
- health tick ją sweepuje,
- testy ją sprawdzają,
- ale produkcyjny hot path nie używa `push()` / `drain()`.

To jest zły stan architektoniczny, bo kod sugeruje ochronę przed race, której faktycznie nie daje.

---

## 2.2. Wymaganie: wybrać jedną z dwóch dróg

Agent ma wykonać **jedną** z poniższych opcji.  
Nie wolno zostawić stanu pośredniego.

---

### OPCJA A — zalecana: realnie podłączyć `DelayedAccountQueue` do hot patha

#### Cel
Wpiąć kolejkę na poziomie `PumpEvent` / transport, zanim event przejdzie w wyższą warstwę.

#### Zakres prac
1. W `grpc_connection.rs`:
   - doprowadzić `DelayedAccountQueue` do miejsca, gdzie account update może zostać odłożony, jeśli mapping curve→mint jeszcze nie istnieje
2. Zdefiniować wyraźny kontrakt:
   - kto woła `push(pubkey, ev)`
   - kto woła `drain(curve_pubkey)`
   - kiedy po successful mapping registration następuje drain
3. Zintegrować z parser/Seer path tak, by:
   - nie zdublować istniejącego `pending_curve_updates`
   - albo jawnie zastąpić `pending_curve_updates`
4. Ostatecznie ma istnieć **jedno źródło prawdy** dla delayed account updates

#### Warunek obowiązkowy
Po wdrożeniu nie mogą istnieć dwa niezależne mechanizmy bufferingu account updates, które robią to samo bez jasnego podziału odpowiedzialności.

---

### OPCJA B — alternatywa: usunąć z produkcyjnej architektury

Jeśli integracja `DelayedAccountQueue` jest zbyt inwazyjna na teraz, to agent ma:

1. usunąć publiczne eksponowanie `delayed_account_queue()` jeśli nie jest używane przez production path
2. zaktualizować komentarze tak, żeby nie sugerowały aktywnej ochrony hot patha
3. ograniczyć tę strukturę do:
   - test helper
   - future work
   - internal module
4. zachować tylko jeden aktywny mechanizm bufferingu account updates:
   - `pending_curve_updates` w `Seer`

### Ważne
Nie wolno zostawić:
- „implemented but not really used”
- plus testów sugerujących, że to działa operacyjnie

---

## 2.3. Preferowana decyzja
Jeśli nie ma czasu na pełne spięcie z transportem, **wybierz opcję B teraz**, a pełną integrację zaplanuj osobnym zadaniem.

### Powód
W tej chwili ważniejsze jest, by architektura była uczciwa i jednoznaczna, niż żeby mieć pół-aktywny mechanizm, który wprowadza w błąd.

---

## 2.4. Testy obowiązkowe
Zależnie od wybranej opcji:
- jeśli A: test end-to-end delayed account update recovery
- jeśli B: testy muszą potwierdzać, że jedyną aktywną ścieżką bufferingu account update jest `pending_curve_updates`

---

# 3. MINOR — resubscribe cadence / account update freshness / ShadowLedger

## 3.1. Problem do naprawy

Current behavior:
- registry changes nie powodują natychmiastowego resubscribe
- rzeczywiste resubscribe zachodzi przy health tick / debounce
- to może opóźnić account updates dla nowych curves/pools
- nie powinno wpływać na trade coverage, ale wpływa na jakość curve state i timing ShadowLedger

To trzeba uczynić mierzalnym i bardziej przewidywalnym.

---

## 3.2. Wymagania implementacyjne

### Dodaj telemetryczne mierzenie:
- czasu od `watch_pool()` do pierwszego account update dla tego poola
- czasu od registry version change do realnego resubscribe send
- liczby pooli oczekujących na subskrypcję exact-account

### Nowe metryki:
- `seer_watch_pool_to_first_account_update_ms`
- `seer_registry_change_to_resubscribe_ms`
- `seer_exact_watch_pending_total`

---

## 3.3. Zmiana zachowania resubscribe

### Minimalna bezpieczna poprawka
Po zmianie watch registry:
- nie czekaj wyłącznie na health tick 5s, jeśli można wykonać wcześniejszy resubscribe przez registry ticker
- ale nie przywracaj agresywnego resubscribe na każdą wiadomość

### Wymagana polityka
1. registry change oznacza „pending resubscribe requested”
2. najbliższy tick resubscribe ma go wykonać możliwie szybko
3. debounce chroni przed spamem, ale nie może sztucznie dokładać wielu sekund opóźnienia

### Dopuszczalny target operacyjny
Zmiana registry → resubscribe w typowym przypadku < 1s

---

## 3.4. Zakaz
Nie wolno:
- wrócić do resubscribe na każdej wiadomości streamu
- ani zostawić niezmierzonego 5s+ opóźnienia bez telemetrii

---

## 3.5. Testy obowiązkowe
1. `watch_pool()` powoduje registry version change
2. resubscribe request jest wysłany w bounded czasie
3. system nie wpada w resubscribe storm
4. late account updates są obserwowalne telemetrycznie

---

# 4. MINOR — exact account watch capacity i pruning

## 4.1. Problem

Limity:
- `EXACT_ACCOUNT_FILTER_CAP`
- `EXACT_ACCOUNT_PAYLOAD_CAP`

powodują pruning exact-account watch setu.  
To samo w sobie nie musi być błędem, ale musi być:
- jawne,
- mierzalne,
- testowalne,
- oraz nie może udawać „watchujemy wszystko”.

---

## 4.2. Wymagania implementacyjne

### Musi istnieć jawna metryka:
- ile curve accounts zostało usuniętych przez cap
- ile pool accounts zostało usuniętych przez cap
- ile razy nowy watch wypchnął stary watch

### Dodaj metryki:
- `seer_exact_watch_curves_pruned_total`
- `seer_exact_watch_pools_pruned_total`
- `seer_exact_watch_evictions_total`
- `seer_exact_watch_current_total`

---

## 4.3. Wymaganie logowania
Każde pruning zdolne wpłynąć na poprawność operacyjną ma być logowane z:
- counts per lane
- cap
- TTL
- now size after pruning

### Nie wystarczy pojedynczy generyczny log.

---

## 4.4. Wymaganie dokumentacyjne
W komentarzach i kodzie ma być jasno napisane:
- mint watches nie są exact-account subscribed tak jak curves/pools
- exact-account branch jest ograniczony i priorytetyzowany
- pruning jest oczekiwanym mechanizmem, nie błędem implementacyjnym

---

## 4.5. Testy obowiązkowe
1. curve + pool pruning liczone osobno
2. najświeższe wpisy przeżywają pruning
3. metryki evictions/pruned rosną zgodnie z przypadkiem testowym

---

# 5. MINOR — poprawa etykiet i przyczyn w logach / metrykach

## 5.1. Problem
Część labeli telemetrycznych opisuje powód nieprecyzyjnie lub wręcz błędnie.

Np. „unwatched_pool” może w praktyce oznaczać:
- unresolved mapping
- invalid pool
- stale mapping conflict
- faktycznie unwatched pool

To trzeba rozdzielić.

---

## 5.2. Wymaganie
Każdy finalny outcome trade’a ma mieć **jednoznaczną kategorię**.

### Minimalny zestaw kategorii:
- `forwarded_live`
- `forwarded_replay`
- `buffered_missing_pool`
- `buffered_missing_mapping`
- `filtered_invalid_pool`
- `filtered_wsol_pool`
- `filtered_mapping_conflict_unrecoverable`
- `expired_waiting_for_mapping`
- `dedup_dropped`
- `ipc_send_failed`

### Te same kategorie mają się pojawiać:
- w metrykach
- w logach
- w testach

---

## 5.3. Zakaz
Nie wolno używać jednej etykiety „trade_filtered_unwatched_pool” dla kilku różnych przyczyn.

---

## 5.4. Obowiązkowe zmiany kodu
Przejrzyj i popraw co najmniej:
- `handle_trade_event`
- `should_forward_trade`
- replay path
- dedup path
- metrics increments
- warn/debug logs

---

## 5.5. Testy obowiązkowe
Dla każdego scenariusza outcome test ma potwierdzić poprawny label/counter increment.

---

# 6. Dodatkowe wymagania jakościowe dla agenta implementującego

## 6.1. Nie wolno robić tylko patcha „żeby test przeszedł”
Każda zmiana ma:
- uporządkować semantykę,
- zaktualizować metryki,
- zaktualizować testy,
- zaktualizować komentarze w kodzie.

---

## 6.2. Każdy nowy enum / typ outcome ma być współdzielony
Jeśli pojawi się np. enum przyczyn bufferingu lub final outcome trade’a, to:
- nie tworzyć osobnych stringów/logów w 5 miejscach
- tylko jeden typ i jego mapowanie do log/metrics labels

---

## 6.3. Komentarze techniczne mają zostać zaktualizowane
Po wdrożeniu zmian nie może zostać komentarz, który opisuje już nieprawdziwy flow.

Szczególnie:
- w `grpc_connection.rs`
- w `lib.rs`
- w okolicach bufferingu i replay
- w okolicach dedup path

---

# 7. Definition of Done dla części 2

Ta część jest ukończona tylko wtedy, gdy:

1. Coverage metrics rozróżniają event-level i signature-level semantics.
2. Replay path liczy forwarded/replayed poprawnie i jednoznacznie.
3. `DelayedAccountQueue` jest:
   - albo realnie aktywny w hot path,
   - albo jawnie zdegradowany/usunięty z production semantics.
4. Resubscribe latency jest mierzona i bounded.
5. Exact-account pruning jest liczony i logowany.
6. Labelki metryk/logów odpowiadają rzeczywistej przyczynie.
7. Testy regresyjne potwierdzają każdą nową semantykę.

---

