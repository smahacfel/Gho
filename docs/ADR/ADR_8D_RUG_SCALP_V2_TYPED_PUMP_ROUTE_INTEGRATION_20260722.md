# ADR-8D: RUG SCALP V2 — typed Pump route integration after PR #79

Status: `IMPLEMENTED / DRAFT PR #78 / NOT A SMOKE OR RUN AUTHORIZATION`

Typ: ADR-8D / prospective shadow experiment / typed execution-and-PM boundary

Data: `2026-07-22`

Repo: `smahacfel/Gho`

Prerequisite merge SHA: `a12ef9cfb7199d44841cde27be2ecd8af13e2f3f` (`#79`)

Plan SSOT: `PLANS/DO_REALIZACJI/PLAN_RUG_SCALP_V2_PROSPECTIVE_SHADOW_20260721.md`

## D0. Decyzja

Po kontrolowanym rebase PR #78 na merge SHA #79 przepięto wyłącznie
ekonomiczny łańcuch izolowanego RUG SCALP V2 na typowany kontrakt
`PumpQuoteV1`:

```text
OnChainConfig / effective-slot fee evidence
  -> BuyV2 exact-base-out quote under max_sol_cost
  -> isolated BuyV2 shadow entry builder
  -> non-zero modelled/confirmed fill + PM ACK
  -> typed market facts with LegacySell executable value
  -> PM-owned target / stop / flow / material-sell decision
  -> one PM terminal disposition and one outcome record
```

Pierwsza i jedyna execution-authorized kombinacja dla RUG ma postać:

```text
ENTRY = BuyV2
EXIT  = LegacySell
```

`BuyExactQuoteInV2`, `LegacyBuy` i `SellV2` nie są fallbackiem dla tej
ścieżki. Każdy nieautoryzowany route kończy entry jako
`ROUTE_NOT_EXECUTION_AUTHORIZED`/`ENTRY_FAILED`, bez cichego przejścia na
historyczny builder.

## D1. Przyczyna

Merge #79 naprawił wspólną Pump boundary: `buy_v2` jest exact-token-out, a
`max_sol_cost` jest limitem settlementu użytkownika, nie curve inputem.
Pozostawienie #78 na `BondingCurve::simulate_buy/sell` lub historycznym
`FEE_BPS = 100` dawałoby spójne wewnętrznie, lecz nieautorytatywne Q_TP,
entry debit, executable PnL oraz TP/SL.

Samo zrebasowanie #78 nie byłoby dowodem migracji. Konieczne było osobne
przepięcie każdej wartości ekonomicznej RUG na nowy route/fee contract i
przeniesienie immutowalnego evidence do Position Managera oraz terminalnego
outcome.

## D2. Zmieniony kontrakt ekonomiczny

`RugScalpPumpQuoteAuthorityV1` materializuje tylko runtime registry z
on-chain/effective-slot evidence. `CanonicalFixture` jest odrzucany przy
materializacji runtime, więc nie może dostać się przez config ani testowy
constructor do aktywnej ścieżki.

`RugScalpPumpQuoteContractV1` rozdziela:

```text
ProgramStateTransition  = reserve before/after, curve quote, token amount
ProgramSettlement       = program fees, user wallet debit/credit
TransactionCosts        = base/priority/tip/ATA/retry envelope
```

W konsekwencji:

- primary `0,10 SOL` cytuje `BuyV2` exact-base-out pod capem; cap nie jest
  traktowany jak spend krzywej;
- sensitivity `0,20 SOL` korzysta z tej samej quote boundary, ale nie tworzy
  intentu, PM registration ani drugiego lifecycle;
- Q_TP, entry wallet debit, entry total debit, PM executable position value,
  +10% net target, -5% hard stop i terminal net PnL korzystają wyłącznie z
  BuyV2/LegacySell settlement oraz jawnego envelope;
- legacy scalar cost fields są tylko kompatybilnym lustrem typed
  `TransactionCosts`; preflight odrzuca ich rozbieżność i PM nie odejmuje ich
  drugi raz.

`TriggerComponent::prepare_rug_scalp_buy_v2_request` wymaga route-specific
canonical account evidence, autoryzacji `BuyV2`, canonical owner/token
program checks oraz buduje instrukcję wyłącznie przez `PumpV2RouteBuilder`.
Nie wywołuje kompatybilnego `DirectBuyBuilder`.

## D3. Authority, ordering i accounting

Do PM przekazywane są route/schedule IDs oraz exact full entry debit. PM
odrzuca pozycję RUG bez tych pól albo z route innym niż `buy_v2` /
`legacy_sell`. Po rejestracji tylko PM tworzy exit intent i terminal close.

Wartość `LegacySell` jest przekazywana jako canonical typed market fact.
`SLOT_COMPLETE` może zawierać aktualną pełnopozcyjną wycenę — jest to
niezbędne do `FLOW_EXHAUSTED`, bo dwa puste sloty nie mogą wymuszać powrotu do
mark price. PM traktuje tę wycenę jako jedyne economics authority dla
profilu RUG; generic `PriceTruthResolver` i historyczny one-percent model nie
są używane do jego targetu, stopu ani terminalnego PnL.

Po entry watermark PM nadal ignoruje trade wcześniejszy od fillu. Fact późniejszy
w tym samym slocie jest konsumowany według `(slot, tx_index, event_ordinal)`.
Spóźniony fact po `SLOT_COMPLETE` albo fact o niemonotonicznym canonical order
ustawia typed data blocker i nie może nadpisać ostatniej executable value.
`DATA_GAP` i niejednoznaczny modelled-fill same-slot tie pozostają
`DATA_INVALIDATED`, bez sztucznego PnL.

Terminal outcome zapisuje route IDs i fee schedule IDs wraz z jednym
`position_id`. `DATA_INVALIDATED` ma `net_pnl_lamports = null`,
`ev_disposition = excluded_data_invalidated` oraz
`invalidates_smoke_or_run = true`; nie zasila EV.

## D4. Zachowane inwarianty

- RUG pozostaje `enabled = false` i `observe_only` domyślnie.
- Nie ma drugiego managera, partial exits, trailingów ani adaptive targetu.
- CrashGuard i TimeStop nie są mapowane na `MATERIAL_SELL_EMERGENCY` ani
  `FLOW_EXHAUSTED`.
- `0,20 SOL` nie tworzy lifecycle.
- Nie ma fallbacku `BuyV2 -> LegacyBuy/RoutedExactSolIn` ani
  `LegacySell -> SellV2`.
- `CanonicalFixture` i nieznany route/schedule nie autoryzują runtime.
- Historyczne `FEE_BPS = 100` nie jest authority RUG: entry debit jest
  sprawdzany względem niezależnego exact `BuyV2` quote z kanonicznym
  95-bps schedule'em.
- Brak typed valuation blokuje close zamiast wyliczać PnL z mark price.
- Brak snapshotu ogólnego nie jest fałszywie opisywany jako źródło typed
  `LegacySell` valuation; lifecycle evidence zachowuje slot samego factu.
- Nie zmieniono progów sygnału, Position Manager authority, cost/latency
  freeze ani shadow/live mode.

## D5. Weryfikacja lokalna

Wykonano:

```text
rustfmt --edition 2021 <osiem zmienionych modułów Rust>
cargo test -q -p ghost-brain guardian::post_buy::rug_scalp::tests --lib
cargo test -q -p ghost-brain rug_scalp_ --lib
cargo test -q -p ghost-launcher rug_scalp_v2::tests --lib
cargo test -q -p ghost-launcher \
  rug_scalp_probe_handoff_registers_one_profiled_position_and_accepts_only_typed_facts --lib
cargo test -q -p ghost-launcher \
  rug_scalp_typed_pump_e2e_fixture_has_one_pm_owned_lifecycle_and_terminal_outcome --lib
cargo test -q -p ghost-launcher rug_scalp_terminal_watcher_ --lib
cargo test -q -p ghost-launcher --lib --no-run
git diff --check
```

Przechodzą testy obejmujące między innymi:

- runtime OnChainConfig fee evidence i odrzucenie `CanonicalFixture`;
- primary/sensitivity separation oraz route/schedule IDs z typed Q_TP;
- adapter replay facts po fill/PM ACK i material sell po watermarku;
- `MATERIAL_SELL_EMERGENCY` przed targetem oraz jeden terminal PM close;
- dwa complete empty slots z `SLOT_COMPLETE` LegacySell value, dokładny
  -300 lamport net PnL i jeden `FLOW_EXHAUSTED` close;
- data gap, late fact i non-monotonic order jako fail-closed evidence failure;
- PM registration z exact route/economics evidence na typed handoff;
- terminal outcome z PM-owned PnL, `position_id`, route i schedule evidence;
- brak drugiej rejestracji dla sensitivity/candidate duplicate.
- literalny fixture E2E: canonical `OnChainConfig` fee evidence -> RUG signal
  i self-impact/Q_TP -> zdekodowana instrukcja `BuyV2` -> modelled fill + PM
  ACK -> typed post-fill `SuccessfulSell`/`SLOT_COMPLETE` -> `LegacySell`
  executable valuation -> `MATERIAL_SELL_EMERGENCY` -> dokładnie jeden
  PM-owned terminal outcome i `position_closed` lifecycle record;
- E2E odrzuca `CanonicalFixture`, brak wymaganego `LegacySell` schedule'u i
  typed-but-not-authorised `SellV2`; potwierdza też, że różniący się od
  historycznego 100 bps schedule 95 bps determinuje entry wallet debit.

Workspace nadal emituje istniejące, niepowiązane warnings w historycznych
modułach. Nie są one dowodem green remote CI.

## D6. Rollback

Rollback polega na pozostawieniu `rug_scalp_v2.enabled = false` albo
wycofaniu nieopublikowanego diffu PR #78. Nie powstał live execution, smoke,
Run A ani artefakt operacyjny wymagający migracji lub kasowania.

## D7. Następne bramki — nadal obowiązkowe

Ten ADR nie autoryzuje smoke. PR #78 pozostaje Draft. Przed smoke <= 2 h
muszą zostać osobno wykonane i zapisane:

1. green remote CI oraz review PR #78 po rebase #79;
2. freeze exact commit/binary/config/PM/fee/cost hashes;
3. freeze base fee, CU limit/price, tip, ATA rent/close, retry i quote-age;
4. private-RPC/Jito p90 capture osobno dla entry i exit, następnie
   `PRIMARY_ENTRY`, `PRIMARY_EXIT`, `STRESS_1` i `STRESS_2`;
5. audit artifact transportu, PM lifecycle i accounting po smoke.

Smoke ma walidować transport i accounting, nie alfę. Jego wynik nie zmienia
progów RUG; Run A może rozpocząć się wyłącznie po czystym smoke.
