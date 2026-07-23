# ADR-8D: RUG SCALP V2 Validation Tape technical capture

Status: `RUNTIME_FEE_AUTHORITY_WIRED / YELLOWSTONE_TX_INDEX_PROPAGATION_FIXED / TX_INDEX_PREFLIGHT_PENDING / RUN A NOT AUTHORIZED`

Typ: ADR-8D / prospective shadow evidence

Data: `2026-07-22`

Repo: `smahacfel/Gho`

Branch: `agent/rug-scalp-v2-prospective-shadow-20260721`

Plan SSOT: `PLANS/DO_REALIZACJI/PLAN_RUG_SCALP_V2_PROSPECTIVE_SHADOW_20260721.md`

## D0. Decyzja

Dodano jedną addytywną warstwę `RugScalpValidationTapeV1` oraz podłączono
istniejący typed Pump quote contract do aktualnej on-chain authority.

```text
accepted signal -> canonical typed state trajectory -> PM terminal evidence
```

Tape nie wysyła transakcji, nie rejestruje pozycji, nie emituje exit intentu i nie zamyka pozycji.

## D1. Zakres

Zmiana obejmuje wyłącznie tape, minimalne writer/wiring, runtime materializację
Pump fee authority, konfigurację oraz jeden shadow-only rollout TOML.

Nie zmieniono reducera, progów, notionali, `rug_scalp_exit_v1`, Gatekeepera, Oracle policy, Trigger execution logic ani Position Managera.

## D2. Kontrakt evidence

Accepted intent ma jeden `AttemptStarted`, kolejne `State`/`Latency` oraz dokładnie jeden `AttemptTerminal`.

Stan zawiera typed `BuyV2` i `LegacySell` re-quotes dla `0.10` i kontrfaktycznego `0.20 SOL`, canonical order key, PnL, reserve/value changes i completeness.

Tape używa tylko `RugScalpPumpQuoteContractV1` / `PumpQuoteV1`; nie używa mark price, `CanonicalFixture`, `FEE_BPS=100` ani `simulate_buy/simulate_sell`.

Przy starcie launcher pobiera `Pump::global` i `PumpFees::fee_config` w jednym
private-RPC context. Sprawdzane są adresy, ownerzy, dyskryminatory, pełne
layouty i SHA-256 danych. Z dwóch kont powstają wyłącznie `BuyV2` i
`LegacySell` `OnChainConfig` schedules, z tym samym observed/effective slotem.

Każdy run otrzymuje manifest record z schedule IDs oraz agregowanym evidence
hash. Późniejsza zmiana albo brak obu kont kończy launcher fail-closed; nie
zamienia stale authority w hot-swapped economics i wymusza nową materializację
przed kolejnym czystym runem.

Brak runtime execution-authorised fee authority nadal daje
`NON_EVALUABLE_FEE_AUTHORITY`, bez entry lub lifecycle.

Brak canonical order key, state/order gap, duplicate/reorder, slot gap, unavailable curve, broadcast lag lub overflow daje `DATA_INVALIDATED`.

Timer observera kończy tylko własny record po pięciu sekundach ciszy; nie syntetyzuje `SLOT_COMPLETE`, flow stopu ani PM decyzji.

`0.20 SOL` nie dostaje position ID, handoffu, PM registration ani drugiego lifecycle.

## D3. Technical rollout

`configs/rollout/rug-scalp-v2-technical-validation-20260722.toml` wymusza shadow, `shadow_only`, ephemeral payer, wyłączone P37 i osobne, bezwzględne artifact paths dla kolejnego clean namespace.

Brak latency freeze jest dopuszczalny wyłącznie dla `validation_tape.technical_capture = true`; nie odblokowuje live lane ani nie zmienia exit profile.

Rollout nie zawiera statycznego Pump schedule. Authority pochodzi wyłącznie z
aktualnego canonical RPC snapshotu; serializowane schedule values nie mogą jej
autoryzować ani zastąpić.

## D4. Weryfikacja

Przechodzą `cargo fmt --check`, dwa testy runtime fee authority, osiem testów
validation tape, cztery istniejące integracyjne testy `PostBuyRuntime`,
`cargo check --bin ghost-launcher`, build release i rollout `--preflight` z
realnym private-RPC materialization.

Testy authority obejmują aktualny uniform `FeeConfig` layout i fail-closed dla
nieznanego/nonzero trailing account surface. Osiem testów tape obejmuje
re-quotes, state-local quote, brak sensitivity lifecycle, data gap, fee
authority, `DUMP_WINS`, observer-only emission i jeden bounded terminal record.

Naprawa ingestu ma dwa testy graniczne: surowy protobuf Yellowstone z
`index=0` i `index=37` materializuje odpowiednio `GeyserEvent::Transaction`
z `tx_index=Some(0)` i `Some(37)`; parser utrzymuje `Some(0)` w
`TradeEvent`, a adapter Seer -> `PoolTransaction` utrzymuje `Some(37)`.
Test reducera z pierwszym trade'em `tx_index=0` przechodzi dalej do
`ShadowEdgeCandidate`, więc zero nie jest traktowane jako brak order key.

## D5. Rollback

`validation_tape.enabled = false` lub `rug_scalp_v2.enabled = false` wyłącza warstwę bez migracji.

Nie powstał live submit, live position ani nowy manager.

## D6. Wynik technical transport/writer smoke

Czysty run `rug-scalp-v2-technical-20260722-r4` został zatrzymany po około
70 sekundach. Potwierdził on private-RPC authority i zapis `run_manifest=1`,
`birth=24`; manifest zawiera oba route IDs, schedule IDs oraz evidence hash.
Nie jest to Run A ani wynik walidacji EV.

Wszystkie 661 assessment rows zakończyły się `missing_trade_order`. Przyczyną
nie był brak danych u providera: Yellowstone `SubscribeUpdateTransactionInfo`
dostarcza canonical `index`, lecz poprzedni adapter odrzucał go przy
materializacji `GeyserEvent::Transaction`, a parser wpisywał `None` do
`TradeEvent`. Naprawa prowadzi teraz dokładnie:

```text
Yellowstone index -> GeyserEvent.tx_index -> TradeEvent.tx_index
-> PoolTransaction.tx_index -> RUG assessment
```

`index=0` jest zachowane jako `Some(0)`. Nadmiarowa wartość nie mieszcząca się
w `u32` kończy dekodowanie typed `ParseError`, bez fallbacku do `None`.
Preflight r5 został zatrzymany po około 30 sekundach — zgodnie z zasadą, że
po dowodzie transportu nie zbiera się pustego czasu. Zapisano 40 birth rows,
502 assessment rows i 498 durable `PoolTransaction` rows; wszystkie 498
zawierają numeryczny `tx_index`, a `missing_trade_order=0`. Assessmenty
doszły do rzeczywistej klasyfikacji (`universe_ineligible`), zamiast kończyć
się na braku canonical order key. r5 nie jest Run A ani wynikiem EV.

Po tym dowodzie uruchomiono świeży r6 technical namespace z tym samym
shadow-only binary/config profilem. r6 nie jest jeszcze Run A; zbiera tylko
trajectory/latency evidence wymagane przez jego techniczny kontrakt.

W artefaktach r4 nie zmienia się historia: względne pola RUG zostały rozwinięte
względem katalogu procesu do `/logs/...`. Kolejny, jeszcze nieuruchomiony
namespace r5 ma już bezwzględne ścieżki w repo.

Run A pozostaje niedozwolony do czasu, gdy preflight r5 pokaże
`births > 0`, `trades > 0` oraz zero `missing_trade_order` w transaction
streamie. Nie wolno syntetyzować kolejności ani poluzować warunku.
