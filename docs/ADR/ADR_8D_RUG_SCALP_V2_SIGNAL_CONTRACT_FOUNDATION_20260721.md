# ADR-8D: RUG SCALP V2 — typed PM lifecycle, exit profile and shadow outcomes

Status: `IMPLEMENTED / SHADOW-ONLY / NOT A SMOKE OR RUN AUTHORIZATION`

Typ: ADR-8D / prospective shadow experiment / Position Manager integration

Data: `2026-07-21`

Repo: `smahacfel/Gho`

Branch base: PR #75 merge commit `42ed970412bbd82142bfc945e95fcd18a85af70f`

Plan SSOT: `PLANS/DO_REALIZACJI/PLAN_RUG_SCALP_V2_PROSPECTIVE_SHADOW_20260721.md`

## D0. Decyzja

Dodano wyłączony domyślnie kontrakt `rug_scalp_v2` oraz czysty,
ograniczony reducer sygnału. Ta zmiana nie daje eksperymentowi authority
Gatekeepera, nie tworzy live execution i nie tworzy drugiego lifecycle.

Reduktor:

- materializuje wyłącznie dwuslotowy successful-buy state;
- terminalnie odrzuca pre-entry successful sell, utraconą kolejność i lukę
  w accepted window;
- zwraca `NON_EVALUABLE`, a nie optymistyczny sygnał, bez canonical curve,
  clean state lub znanego porządku;
- liczy primary `0,10 SOL` i sensitivity `0,20 SOL` oddzielnie;
- używa `BondingCurve::simulate_buy` i `BondingCurve::simulate_sell`, czyli
  tej samej programowej matematyki curve, którą wykorzystuje obecna ścieżka
  Triggera do local quote;
- nie posiada żadnej metody entry, exit, close, retry ani reconciliation.

Preflight nie pozwoli włączyć flagi bez zamrożonej entry/exit latency,
kompletnego modelu kosztów, profilu `rug_scalp_exit_v1`, one-signal/no-reentry,
dedykowanych ścieżek artefaktów i `execution.execution_mode=shadow`.

Fundację domknięto bez tworzenia drugiego lifecycle. Adapter materializuje
mały, typed `RugScalpMarketFactV1`, nie przekazuje do Position Managera
całego mutable `TradeEvent`. Fact zawiera identity (`position_id`, `mint`,
`slot`, `tx_index`, `event_ordinal`), kind (`SUCCESSFUL_BUY`, `SUCCESSFUL_SELL`,
`SLOT_COMPLETE`, `DATA_GAP`, `ROUTE_STATE_CHANGED`) oraz wyłącznie potrzebne
evidence: successful-buy count dla slotu, sell quote, reserve/value before/
after i completeness.

Po accepted assessment ścieżka jest dokładnie następująca:

```text
canonical Pump builder -> isolated shadow probe -> confirmed/modelled non-zero fill
-> PM.register_position(strategy_id="rug_scalp_v2",
                         exit_profile_id="rug_scalp_exit_v1")
-> typed PM market facts
```

Nie odczytuje ona Gatekeeper verdictu ani P37 selection record. Dopiero PM
acknowledgement pozwala adapterowi związać active fact stream; zero/unknown
fill nie zakłada pozycji.

## D1. Przyczyna

Plan V1 nie był prawidłową podstawą: używał `0,05 SOL`, zakładał własny exit
lifecycle i wymuszał `+1 slot`. V2 wymaga 0,10/0,20, Position Managera jako
jedynego ownera i latency zamrożonej po smoke. Dotychczasowy P37 jest
powiązany z `GatekeeperBuyLog`, więc nie może być źródłem sygnału RUG bez
naruszenia granicy authority.

## D2. Zachowane inwarianty

- `rug_scalp_v2.enabled = false` jest wartością domyślną.
- P37 i RUG nie mogą współdzielić isolated probe monitor w jednym runie.
- Brak latency/cost evidence jest błędem preflightu, nie `0 slot` ani `0 fee`.
- Sensitivity nie emituje osobnego lifecycle i nie zwiększa accepted count.
- Reducer nie czyta Gatekeeper verdictu i nie zmienia BUY/REJECT.
- Flaga włącza wyłącznie dedicated probe-monitor path; nie rezerwuje primary
  live/shadow capacity.
- PM akceptuje profil tylko dla dokładnej pary metadata
  `rug_scalp_v2` / `rug_scalp_exit_v1`; niepełna albo sprzeczna para jest
  odrzucana.
- Duplicate fact jest deduplikowany, a fact po terminalnym close nie może
  stworzyć drugiego exit.
- Outcome watcher konsumuje PM-owned terminal disposition; launcher nie liczy
  konkurencyjnego PnL i nie domyka pozycji samodzielnie.

## D3. Profil PM i exact exit semantics

`rug_scalp_exit_v1` jest small typed profile istniejącego Position Managera,
nie drugim managerem. Nie ma partial exits, trailingu ani adaptive targetu.
Kolejność decyzji jest dokładnie:

```text
PENDING / RECONCILIATION
  -> DATA / IDENTITY / ROUTE BLOCKER
  -> MATERIAL_SELL_EMERGENCY
  -> TARGET_REACHED_10PCT_NET
  -> BASELINE_HARD_LOSS_5PCT_NET
  -> FLOW_EXHAUSTED
  -> MAX_HOLD
  -> HOLD
```

`MATERIAL_SELL_EMERGENCY` jest oceniany wyłącznie po `SLOT_COMPLETE` z
complete evidence: reserve drain >= 5% lub spadek executable position value
>= 15%. `FLOW_EXHAUSTED` wymaga dwóch kolejnych complete slotów z
`successful_buy_count_in_slot == 0`.

`DATA_GAP` nie jest empty slotem. Ustawia sticky typed blocker; PM zapisuje
jeden `position_unresolved` / `DATA_INVALIDATED`, bez sell intentu i bez
synthetic PnL. Profile fixed costs wchodzą do net target/stop i terminalnego
PnL. Primary exit latency pozostaje explicit/config-driven, a higher-priority
material sell może zastąpić oczekujący lower-priority trigger bez drugiego
exit.

Entry posiada immutable watermark `(slot, tx_index, event_ordinal)`. Adapter
retains facts między fill evidence a ACK PM, a po ACK replayuje wyłącznie
facts późniejsze od watermarku. PM egzekwuje tę samą granicę defensywnie:
fakt wcześniejszy jest `IgnoredPreEntry`; późniejszy same-slot jest
konsumowany; modelled fill bez tx/event order traktuje raw same-slot trade
jako `DATA_INVALIDATED`, nie jako domniemany post-entry market fact.

## D4. Weryfikacja

Wykonano po domknięciu PM ingress/adapter/profile/outcomes:

```text
cargo fmt --all --check
RUSTFLAGS='-Awarnings' cargo check -q -p ghost-brain
RUSTFLAGS='-Awarnings' cargo check -q -p ghost-launcher --bin ghost-launcher
RUSTFLAGS='-Awarnings' cargo test -q -p ghost-launcher rug_scalp_v2::tests --lib
RUSTFLAGS='-Awarnings' cargo test -q -p ghost-brain guardian::post_buy::rug_scalp::tests --lib
RUSTFLAGS='-Awarnings' cargo test -q -p ghost-brain guardian::post_buy::engine::tests::rug_scalp_data_gap_ends_as_one_pm_owned_data_invalidated_terminal --lib
RUSTFLAGS='-Awarnings' cargo test -q -p ghost-brain guardian::post_buy::engine::tests::rug_scalp_two_complete_empty_slots_produce_one_pm_flow_exit_and_close --lib
RUSTFLAGS='-Awarnings' cargo test -q -p ghost-launcher components::post_buy_runtime::tests::rug_scalp --lib
git diff --check
```

Testy kontraktowe obejmują:

- 0,10 SOL jako jedyny primary intent i 0,20 SOL jako sensitivity-only;
- zero/unknown fill bez active fact stream i bez PM lifecycle;
- jedną PM rejestrację z `rug_scalp_exit_v1` oraz typed fact ingress;
- material sell, który czeka na complete slot i wygrywa z targetem;
- dwa complete empty sloty, jeden `FLOW_EXHAUSTED` exit i jeden terminal close;
- `DATA_GAP`, który nie udaje pustego slotu i daje jeden PM-owned
  `DATA_INVALIDATED`;
- material sell zbuforowany przed PM ACK, replayowany od canonical entry
  watermarku i domykany przez PM dokładnie raz;
- fakt wcześniejszy niż fill watermark jest ignorowany, a nierozstrzygalny
  modelled-fill same-slot tie jest data-invalidated;
- dedupe fact oraz PM-owned net PnL/exit landed slot w terminalnym outcome.

`DATA_INVALIDATED` ma jawne pola outcome accounting:
`ev_disposition = excluded_data_invalidated` i
`invalidates_smoke_or_run = true`. Nie ma PnL, nie jest usuwany z evidence
denominator i nie może zasilać EV. Jest awarią evidence po accepted entry,
która unieważnia smoke/run.

## D5. Rollback

Rollback jest prosty: pozostawić `rug_scalp_v2.enabled = false` (domyślnie)
lub usunąć jeszcze nieopublikowaną zmianę z brancha. Nie istnieją pozycje,
run artifacts ani historyczne dane do kasowania albo migracji.

## D6. Operacyjny warunek przed smoke

Ten ADR nie autoryzuje smoke. Nie dodano bypassu preflightu dla niezamrożonej
latency ani kosztów. Przed maksymalnie dwugodzinnym smoke operator musi
przypiąć aktualnie zmierzoną wartość `PRIMARY` dla prywatnej ścieżki RPC/Jito
oraz zapisać `STRESS_1 = PRIMARY + 1` i `STRESS_2 = PRIMARY + 2` razem z
hashami/config evidence. Dopiero wtedy można włączyć shadow-only eksperyment,
wykonać smoke, a następnie podjąć decyzję o Run A.

W ramach tej zmiany nie uruchomiono smoke, Run A ani live execution.

## D7. Otwarte audit gates przed smoke

In-repo test parity potwierdza `simulate_buy` używany przez builder,
`simulate_buy_pure`, transition reserve i `simulate_sell_pure` na tym samym
fixture oraz sprawdza zgodność skompilowanych fee constants. To jest tylko
internal parity — nie dowodzi poprawności wspólnego Pump fee schedule.

Przed smoke musi istnieć osobny, wersjonowany fixture/evidence dla aktualnego
Pump fee schedule oraz canonical reserve transition, obejmujący co najmniej:
Pump fee, CU limit/price, base fee, Jito tip policy, ATA rent/close, retry i
quote-age policy. Do momentu przypięcia takiego fixture/hash status brzmi
`SMOKE_NOT_AUTHORIZED`, nawet gdy test internal parity przechodzi.
