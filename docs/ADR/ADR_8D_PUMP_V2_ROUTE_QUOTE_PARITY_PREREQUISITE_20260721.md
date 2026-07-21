# ADR-8D: Pump V2 route and quote parity prerequisite

Status: `IMPLEMENTED / PREREQUISITE / NO SMOKE AUTHORIZATION`

Typ: ADR-8D / execution-and-quote boundary repair

Data: `2026-07-21`

Repo: `smahacfel/Gho`

Zależny PR: `#78` RUG SCALP V2 pozostaje `Draft` i nie jest rozszerzany
w ramach tej zmiany.

## D0. Decyzja

Dodano jeden wersjonowany, typowany kontrakt Pump dla pięciu semantycznie
różnych instrukcji:

```text
LegacyBuy
BuyV2
BuyExactQuoteInV2
LegacySell
SellV2
```

Kontrakt nie używa już `is_buy`/`is_sell` jako authority route'u. Każdy
obsługiwany discriminator ma własny dekoder argumentów, exact account count,
IDL-pinned account layout, walidację order/flags/program ID oraz owner evidence
dla kont będących własnością programu lub token programów. Nieznany
discriminator, nadmiarowe konto, nieznany recipient lub owner mismatch kończy
się błędem fail-closed.

Dodano również versioned quote API:

```text
quote_exact_base_out
quote_exact_quote_in
quote_exact_base_in_sell
```

Wynik jawnie rozdziela `ProgramStateTransition`, `ProgramSettlement` oraz
`TransactionCosts`. `FEE_BPS = 100` z historycznego modelu nie jest authority
dla tego API.

## D1. Przyczyna

Aktualny Pump `buy_v2` jest exact-token-out. `max_sol_cost` jest limitem
all-in program wallet debit, a nie wejściem krzywej. Używanie tego capu jako
curve spendu mieszało dwa różne pojęcia i mogło wygenerować pozorną zgodność
modelu z chainem.

Dotychczasowe symulatory kompatybilności dodatkowo reprezentowały historyczny
jednoprocentowy model. Nie stanowią authority dla bieżących opłat i nie wolno
ich używać do Q_TP, executable PnL ani progów TP/SL.

## D2. Kontrakt obliczeń

`ProgramStateTransition` zawiera wyłącznie stan krzywej i gross curve quote:

```text
reserve before/after
base in/out
curve quote in/out
```

`ProgramSettlement` zawiera tylko programowe rozliczenie użytkownika:

```text
curve quote
protocol / LP / buyback / creator fee breakdown
program wallet debit albo credit
```

`TransactionCosts` zawiera wyłącznie envelope transakcji:

```text
base fee
priority fee
Jito tip
ATA rent / close refund
retry or failure cost
```

W szczególności:

- buy exact-base-out: requested token amount -> curve input -> program fees ->
  required total <= `max_sol_cost`;
- buy exact-quote-in: exact curve input oraz minimum token output są osobnymi
  checks i nigdy nie porównują tokenów do lamportów; wyliczony wallet debit
  jest settlement evidence, a nie nieistniejącym trzecim limitem instrukcji;
- sell: gross curve reserve decrement i net user credit są odrębnymi
  wielkościami; transaction costs nie są program fee.

## D3. Golden fixtures i źródło authority

Testy CI są offline i używają statycznych fixture'ów z przypiętego Pump public
IDL commit `9c82f61cb711b044a17f770ab8ce9f9bdf78f333` oraz SHA-256 IDL
`b90bc471327f671449271d5d1d42354d1fae6f5a06502f5834459a3108138e49`.

Fixture `buy_v2`:

```text
signature: 2cwZGUYroPdkAsfdN1jsPB6sbdqUhgzrzgUAoTohKnTzvYzMr5YEjJCVDwxLZJ9ugTuGdg4YHYmTLQoZNDQKykrh
slot: 434365563
```

Sprawdza exact requested token output, transition rezerw, curve input,
zidentyfikowane program fee, program wallet debit, cap oraz pełny 27-account
layout. Fixture zachowuje też osobny, successful public-RPC builder simulation
z idempotentnym utworzeniem base ATA; nie jest to latency capture ani smoke.

Fixture `legacy_sell`:

```text
signature: 2Y68uh5FrbALZFBetEdDkwrPYVfrC2BPuu1sHutGLoMSXrVx2vvfphskstv5t1JziHagqdGDLs1Eb4as54nEQUXk
slot: 434365533
```

Sprawdza exact 17-account layout, gross reserve decrement, pełne rozliczenie
LP/protocol/buyback/creator fees, net program credit oraz odrębne base,
priority i Jito costs. Test wykonuje pełne zachowanie lamport conservation,
bez porównywania jednej liczby równocześnie do gross i net.

## D4. Zachowane inwarianty

- Nie ma fallbacku z nieznanego route'u do `is_buy`/`is_sell`.
- Nie ma domyślnego schedule opłat ani ukrytego `1%`.
- Program fees nie są dublowane w transaction cost ledger.
- `max_sol_cost` nie jest traktowany jak curve input.
- Fee/config/owner evidence nie są pobierane z publicznego RPC w teście CI.
- RUG SCALP, Position Manager, jego lifecycle, progi i preflight nie zostały
  zmienione.
- PR #78 pozostaje Draft; smoke i Run A pozostają zakazane.

## D5. Granica migracji

Ten PR dostarcza jedyną dozwoloną nową surface dla aktualnego route'u:
typowane builders oraz quote contract. Nie przepina automatycznie historycznych
`DirectBuyBuilder`/`DirectSellBuilder` call sites, ponieważ ich argumenty nie
niosą kompletnego canonical creator/fee/config/owner evidence. Cicha migracja
z defaults byłaby naruszeniem fail-closed boundary.

Każdy future entry/exit, który chce użyć aktualnego Pump route, musi dostarczyć
pełny typed account contract i przejść jego walidację. Po merge prerequisite
kontrolowany rebase #78 musi konsumować ten contract w swojej isolated shadow
entry/exit boundary albo nadal pozostać zablokowany. Sam rebase bez takiego
przepięcia nie jest dowodem parity dla PR #78.

## D6. Weryfikacja

Lokalnie wykonano:

```text
cargo fmt --all --check
cargo test -p ghost-core pump_quote
cargo test -p trigger --lib pump_route_v2::tests
cargo test -p trigger --test pump_route_quote_parity
git diff --check
```

Testy przechodzą. Workspace emituje istniejące, niepowiązane warnings w
`shadow_ledger` i `trigger`; żaden nie pochodzi z nowych module/fixture tests.

## D7. Następne bramki

1. Review i merge tego prerequisite PR.
2. Kontrolowany rebase PR #78 oraz explicit migration jego route boundary do
   typed Pump contract.
3. Pełne CI obu PR-ów po rebase.
4. Freeze fee/CU/tip/ATA/retry/quote-age oraz binary/config/cost/PM hashes.
5. Osobny private-RPC/Jito p90 capture dla entry i exit, potem
   `PRIMARY_ENTRY`, `PRIMARY_EXIT`, `STRESS_1`, `STRESS_2`.
6. Dopiero wtedy smoke <= 2 h.

Smoke nie służy do odkrywania tego błędu i nie może być uruchomiony przed
zamknięciem powyższych bramek.
