# ADR-8D: Prospective Pump Exact-State Tape V2 — P0 source frontier i Event-CPI authority

**Data:** 2026-08-23

**Status:** SUPERSEDED IN PART BY `ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_PARENT_CHAIN_AND_PROJECTION_BINDING_20260823.md` / V2 RAW NOT CREATED / NO PROVIDER I/O / ALLOWLIST-ONLY COMMIT AUTHORIZED

**Typ:** ADR-8D / prospective research evidence / offline fail-closed correction

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie był dostępny w tym
> checkoutie. Dokument zachowuje obowiązującą lokalnie strukturę ADR-8D używaną
> przez pozostałe ADR V2.

## D0. Problem i decyzja

Niezależne review odrzuciło lokalny diff V2 z dwoma P0 i jednym P1:

1. równość filtered-Pump mapy i full-block Pump mapy nie dowodziła, że oba
   lane'y były kompletne per slot; równoczesne pominięcie transakcji Pump i
   całego FullBlocku mogło pozostawić dwie równe, ale niepełne mapy;
2. publiczny E2E nie wykonywał rzeczywistego inner Anchor Event-CPI, a manifest
   traktował quote-regime-dependent reserve fields jako bezwarunkową authority;
3. długości observation i forward opierały się na ingress wall clock.

Decyzja jest konserwatywna:

```text
source availability authority
  = reconciled BlockMeta + FullBlock pair

window duration/cutoff authority
  = ingress_monotonic_ts_ms

canonical exact state
  = final BondingCurve account anchor

Event-CPI
  = strict identity and transition evidence, never competing state authority
```

Wszystkie trzy korekty są wprowadzane przed pierwszym realnym raw V2. Nie ma
migracji istniejących raw runs, ponieważ żaden V2 raw, capability artifact ani
window output nie został utworzony.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

GO-D/V1 nie jest przedmiotem tej zmiany.

## D1. Per-slot ledger i granica availability

`FullBlockPayloadStartedV2` nosi teraz pełny `event_time` (wall i monotonic
ingress) przed zapisaniem pierwszego bajtu payloadu. Offline collector prowadzi
per-slot ledger z osobnymi witnesses:

```text
Slot canonicality evidence
BlockMeta:  parent_slot, blockhash, parent_blockhash, executed_tx_count, ingress
FullBlock:  parent_slot, blockhash, parent_blockhash, executed_tx_count, ingress
```

W każdej przyjętej kohorcie każdy zobaczony `BlockMeta` musi mieć dokładnie
jeden `FullBlock` o tym samym slocie i literalnie identycznych czterech polach;
każdy `FullBlock` musi mieć analogiczny `BlockMeta`. Brak partnera, duplikat
lub mismatch kończy qualification fail-closed. Jest to konieczna per-slot
reguła niezależna od reconciliation filtered-Pump multiset, ale późniejsze
niezależne review poprawnie wykazało, że nie wystarcza wobec równoczesnego
pominięcia całego pośredniego bloku. Cross-slot parent-chain conservation oraz
retained-payload projection binding są domknięte wyłącznie przez successor ADR.

Para, która sama dowodzi wykonania bloku, musi ponadto mieć finalized `Slot`
z jednym, zgodnym `parent_slot`. W przeciwnym razie known executed transaction
mógłby zostać pominięty przez rooted denominator wyłącznie z powodu utraty lane
Slot. To jest kierunek odwrotny do zakazanego uproszczenia `Slot -> BlockMeta`.

Nie ma wymuszenia `Slot -> BlockMeta`. Yellowstone `Slot` update nie rozróżnia
skipped slotu Solany od provider omission, dlatego taka reguła byłaby albo
fałszywie restrykcyjna dla skipped slotów, albo nadal nieudowodniona. Slot
pozostaje canonicality evidence, lecz naked Slot nigdy nie rozszerza source
availability. Frontier bierze ostatnią kompletną parę BlockMeta+FullBlock i
czas późniejszego z jej dwóch ingressów: oba witnesses muszą już lokalnie
istnieć, zanim od tego miejsca uznamy forward coverage.

## D2. Event-CPI oraz quote regime

Pinned manifest semantics v7 usuwa bezwarunkowe błędne reserve equalities:

- `CreateEvent.virtual_sol_reserves` i `virtual_quote_reserves` nie są już
  równocześnie wiązane z jednym BondingCurve field;
- `TradeEvent.virtual_quote_reserves` i `real_quote_reserves` nie są już
  wiązane z finalnym state dla wszystkich native-SOL i token-quote variants.

Pola te pozostają obowiązkowo w pełni strict-Borsh decoded, ale przy braku
wariantowo udowodnionego quote-regime contractu są `StrictDecodeOnly`.
Manifest wciąż wymaga identity i transition fields, w tym:

```text
TradeEvent: mint, user, is_buy, ix_name, quote_mint,
            virtual_token_reserves, real_token_reserves
CreateEvent: mint, bonding_curve, user, creator, token_program,
             is_mayhem, is_cashback, quote_mint,
             virtual_token_reserves, real_token_reserves, total_supply
```

Finalny BondingCurve account anchor jest wyłączną authority dla exact state.
Błędny mint, user, instruction name, quote mint lub wymagany declared canonical
state field pozostaje `Unknown`, co blokuje capability przez conservation
denominator. Nie istnieje fallback do RPC, imputation ani legacy reserve map.

Publiczny raw → qualify → export E2E zawiera teraz autentyczne Anchor
`emit_cpi!` inner `CreateEvent` oraz `TradeEvent` pod literalnymi Pump parent
instructions. Fixture celowo rozdziela wartości native/SOL i quote reserves,
aby test nie mógł przypadkiem utrwalić fałszywej equality.

## D3. Monotoniczna oś czasu

`ingress_wall_ts_ms` i `ingress_monotonic_ts_ms` są utrwalane razem, ale mają
odrębne role:

| Dane | Authority |
| --- | --- |
| `ingress_monotonic_ts_ms` | sortowanie observed evidence, observation duration i forward cutoff |
| `ingress_wall_ts_ms` | audytowa etykieta i korelacja operatorowa |

Wersje descriptorów exact/window zostały podniesione przed pierwszą publikacją,
aby oba timestamp domains oraz reconciled frontier były cryptographically
bound w artifacts. Regresja z dużym wall-clock stepem i krótkim monotonicznym
przebiegiem nie może stworzyć `Complete` window.

## D4. Regresje wymagane przez decyzję

Publiczne testy obejmują co najmniej:

1. późny `Slot` bez BlockMeta/FullBlock nie daje `Complete` window;
2. `BlockMeta` bez FullBlock blokuje raw qualification;
3. usunięcie FullBlocku i odpowiadającej filtered Pump transaction nadal
   blokuje qualification;
4. prawdziwe Event-CPI Create i Trade z rozdzielonymi native/quote reserves;
5. wrong mint, user, ix name, quote mint i required canonical reserve
   binding kończą się `Unknown`/`Blocked`;
6. wall-clock jump nie może spełnić 150000 ms observation + 90000 ms forward.
7. kompletna para BlockMeta/FullBlock bez finalized Slot blokuje raw
   qualification, zamiast zmniejszać rooted denominator.

Happy path ma przy 241000 ms kompletny set `Slot(105)`, `BlockMeta(105)` i
pusty `FullBlock(105)`; pełny blok bez Pump transaction jest poprawnym
completeness witness, a nie implicitnym brakiem evidence. Następny review
dodał warunek, że ta para musi również literalnie przedłużać zachowany parent
chain; nie jest już wystarczająca jako izolowany local pair.

## D5. Zakres wyłączony

Ta korekta nie wykonuje i nie autoryzuje:

- commita, push, merge ani modyfikacji PR;
- sealed preflightu, capture'u, RPC/GO-E, Yellowstone provider I/O, backfillu
  lub imputacji;
- usuwania/przenoszenia diagnostyk albo raw artifacts;
- zmian aktywnego Seer runtime, Event Bus, Gatekeepera, execution, strategii
  lub konfiguracji operatora;
- outcomes, PnL, SELECTED/REST ani live promotion.

`.codex/active-task.md` jest lokalnym checkpointem i pozostaje poza
allowlist-only commitem produktu.

## D6. Relacja do successor correction

Pierwotna lokalna macierz tej iteracji była zielona, lecz nie obejmowała
skorelowanego pominięcia całego parent blocku i nie dowodziła projection
binding dla Account/Slot/BlockMeta. Ten dokument nie jest więc dowodem
końcowego P0 PASS. Aktualne testy, finalna macierz i niezależny self-review
są rejestrowane wyłącznie w successor ADR
`ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_PARENT_CHAIN_AND_PROJECTION_BINDING_20260823.md`.

Następny próg po jego PASS pozostaje wyłącznie jawna zgoda użytkownika na
allowlist-only clean commit. Sealed preflight i realny capture pozostają
osobnymi operator decisions.
