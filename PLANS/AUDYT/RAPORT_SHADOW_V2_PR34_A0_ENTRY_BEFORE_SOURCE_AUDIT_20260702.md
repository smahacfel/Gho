# Raport PR34-A0: Shadow V2 ENTRY_BEFORE Source Audit

Data: 2026-07-02

Finalny werdykt:

```text
ENTRY_BOUNDARY_SOURCE_PRESENT
```

## 1. Zakres

PR34-A0 byl audytem odczytowo-analitycznym po merge PR33. Nie uruchomiono
runtime burnina, validation runu ani PR34 validation runu.

Celem bylo ustalenie, czy obecny runtime ma deterministyczne, no-lookahead
zrodlo stanu poola przed entry fill (`ENTRY_BEFORE`), ktore moze zasilic
`ShadowV2FillEngine` bez falszowania provenance.

Zakres nie obejmowal podlaczenia engine, uruchamiania burnina ani zmian
decyzyjnych.

## 2. Decyzja audytu

Zrodlo `ENTRY_BEFORE` istnieje, ale nie w miejscu, w ktorym obecny Shadow V2
writer probuje emitowac entry evidence.

Poprawne zrodlo jest upstream, w Trigger/AccountStateCore, przed wyslaniem lub
symulacja BUY:

```text
ghost-launcher/src/components/trigger/component.rs
TriggerComponent::canonical_pool_state()
TriggerComponent::run_local_buy_preflight()
TriggerComponent::resolve_buy_instruction_token_param()
AccountStateReducer::get_canonical_state()
```

Obecny `PostBuyRuntime` nie dostaje tego stanu przez handoff. Produkcyjna
sciezka `maybe_emit_shadow_v2_entry_evidence()` przekazuje `None` jako
`entry_pool_state_before`, przez co entry fill pozostaje `BLOCKED_BY_DATA`.
To jest luka wiringowa, nie dowod braku zrodla upstream.

## 3. Najwazniejsze ustalenia

### 3.1. Kandydat PRESENT: Trigger/AccountStateCore canonical state

`TriggerComponent::canonical_pool_state()` zwraca
`AccountStateCore::get_canonical_state(mint)`:

```text
ghost-launcher/src/components/trigger/component.rs:1773
ghost-launcher/src/components/trigger/component.rs:1777
```

`run_local_buy_preflight()` przed BUY pobiera `latest_observed_slot`,
`CanonicalPoolState`, liczy `state_age_slots`, materializuje bonding curve i
symuluje token output:

```text
ghost-launcher/src/components/trigger/component.rs:3350
ghost-launcher/src/components/trigger/component.rs:3363
ghost-launcher/src/components/trigger/component.rs:3372
ghost-launcher/src/components/trigger/component.rs:3384
ghost-launcher/src/components/trigger/component.rs:3393
```

`CanonicalPoolState` zawiera raw reserves, slot i timestamp aktualizacji:

```text
ghost-core/src/account_state_core/types.rs:101
ghost-core/src/account_state_core/types.rs:115
ghost-core/src/account_state_core/types.rs:116
ghost-core/src/account_state_core/types.rs:117
ghost-core/src/account_state_core/types.rs:118
ghost-core/src/account_state_core/types.rs:124
ghost-core/src/account_state_core/types.rs:125
```

`AccountStateReducer::get_canonical_state()` zwraca kopie aktualnego stanu:

```text
ghost-core/src/account_state_core/reducer.rs:168
ghost-core/src/account_state_core/reducer.rs:169
```

Ocena:

- dostepne przed entry fill: tak, jezeli stan zostanie uchwycony w Trigger
  boundary;
- deterministyczne: tak, dla snapshotu `CanonicalPoolState` uchwyconego w tym
  punkcie;
- no-lookahead: tak, pod warunkiem capture przed submit/simulation handoff i
  bez poznego odczytu w PostBuyRuntime;
- reserves: tak, raw virtual/real reserves sa w `CanonicalPoolState`;
- decimals/lamports normalization: mozliwe jawnie przez
  `PoolStateSampleV2::from_account_state_core(..., token_decimals=6)` i
  `sol_lamports=1_000_000_000`;
- boundary slot/order: slot i wall time sa dostepne; chain-order components,
  ktorych runtime nie zna, musza byc zapisane jako explicit `UNKNOWN`;
- `account_data_hash`: obecnie brak. To blokuje `research_provenance_ready`,
  ale po PR33 nie musi blokowac `DIAGNOSTIC_SIM`, jezeli reserves/amount/fee/
  decimals/order wystarcza do formuly.

### 3.2. Obecna sciezka PostBuyRuntime jest MISSING jako source

`PostBuySubmitted` niesie `entry_token_amount_raw`, `buy_landed_slot` i
`entry_simulation_rpc_slot`, ale nie niesie `CanonicalPoolState`,
`PoolStateSampleV2`, raw reserves ani account data hash:

```text
ghost-launcher/src/events.rs:1277
ghost-launcher/src/events.rs:1304
ghost-launcher/src/events.rs:1309
ghost-launcher/src/events.rs:1312
ghost-launcher/src/events.rs:1317
ghost-launcher/src/events.rs:1424
ghost-launcher/src/events.rs:1457
```

`PostBuyRuntimeConfig` ma dostep do `AccountStateReducer`, a `main.rs` przekazuje
go do PostBuyRuntime:

```text
ghost-launcher/src/components/post_buy_runtime.rs:199
ghost-launcher/src/components/post_buy_runtime.rs:200
ghost-launcher/src/main.rs:2097
```

Ten pozny dostep nie jest wystarczajacy jako `ENTRY_BEFORE`, bo reducer nie
udostepnia historycznego snapshotu wedlug entry boundary. Pozny odczyt moze
zwrocic stan nowszy niz entry handoff.

### 3.3. Shadow V2 writer ma adapter, ale produkcyjnie dostaje None

Produkcyjna sciezka przy accepted shadow handoff wywoluje:

```text
maybe_emit_shadow_v2_entry_evidence(...)
```

i ta funkcja przekazuje `None` do wariantu z pool state:

```text
ghost-launcher/src/components/post_buy_runtime.rs:2638
ghost-launcher/src/components/post_buy_runtime.rs:2895
ghost-launcher/src/components/post_buy_runtime.rs:2911
ghost-launcher/src/components/post_buy_runtime.rs:2926
```

W efekcie helper dodaje typed blockers:

```text
ENTRY_POOL_STATE_BEFORE_UNAVAILABLE
ENTRY_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME_HANDOFF
```

Dowod kodowy:

```text
ghost-launcher/src/components/post_buy_runtime.rs:3077
ghost-launcher/src/components/post_buy_runtime.rs:3088
ghost-launcher/src/components/post_buy_runtime.rs:3089
```

Jednoczesnie istnieje wariant `maybe_emit_shadow_v2_entry_evidence_with_pool_state`
i test pokazuje, ze gdy caller dostarczy `PoolStateSampleV2`, canonical stream
emituje `ENTRY_ATTEMPT -> POOL_STATE_SAMPLE -> ENTRY_FILL`:

```text
ghost-launcher/src/components/post_buy_runtime.rs:2931
ghost-launcher/src/components/post_buy_runtime.rs:3119
ghost-launcher/src/components/post_buy_runtime.rs:3120
ghost-launcher/src/components/post_buy_runtime.rs:4742
ghost-launcher/src/components/post_buy_runtime.rs:4797
ghost-launcher/src/components/post_buy_runtime.rs:4817
```

To potwierdza, ze brak jest transportu/wiringu, a nie formatu canonical sample.

## 4. Ocena pozostalych kandydatow

Pelna macierz znajduje sie w:

```text
reports/selector/shadow_v2_pr34_a0_entry_before_candidate_matrix.csv
```

Skrot:

- `TriggerComponent::canonical_pool_state` / `AccountStateCore`:
  `PRESENT`.
- `BuyBuildProfile` / `BuyAccountOverrides.legacy_buy_curve`:
  `AMBIGUOUS`; ma curve/quote, ale bez samodzielnego slot/order/hash i powinien
  byc tylko uzupelnieniem canonical state.
- `PostBuySubmitted`:
  `MISSING`; brak raw reserves/pool state sample.
- pozny `PostBuyRuntime.account_state_core`:
  `AMBIGUOUS`; mozliwe reserves, ale nie gwarantuje ENTRY_BEFORE bez late-read
  lookahead risk.
- `MaterializedFeatureSet.account_features`:
  `AMBIGUOUS`; decision snapshot ma `current_reserves`, ale nie ma pelnego
  `CanonicalPoolState`, account hash ani event-order.
- `OracleRuntime::resolve_price_context`:
  `MISSING` jako executable boundary; zwraca tylko normalized price/reserve
  context i ma fallbacki, w tym ShadowLedger diagnostic fallback.
- `ShadowBuySimulationReport` / `ShadowBuySimulationEvent`:
  `MISSING`; ma `entry_token_amount_raw` i `rpc_slot`, nie ma stanu poola.

## 5. Dlaczego finalny werdykt to PRESENT, a nie MISSING

Wymaganie PR34-A0 brzmi: ustalic, czy runtime ma deterministyczne,
no-lookahead zrodlo ENTRY_BEFORE. Odpowiedz brzmi: tak, ale tylko w upstream
Trigger boundary, zanim stan zostanie zgubiony w handoffie.

Nie mozna uczciwie powiedziec, ze obecny Shadow V2 writer ma juz gotowy
ENTRY_BEFORE. Nie ma. Obecny writer zapisuje typed missing reasons.

Mozna jednak uczciwie powiedziec, ze runtime ma source, ktory PR34-B moze
podlaczyc bez sidecara i bez poznego odczytu:

```text
AccountStateCore CanonicalPoolState captured at Trigger request/preflight boundary
```

## 6. Minimalny plan PR34-B

PR34-B powinien byc implementation PR, ale bez burnina jako czesci samego PR.

Minimalny zakres:

1. Dodac addytywny, shadow-only boundary payload dla prepared/shadow request,
   np. `ShadowV2EntryBoundarySnapshot`.
2. W `TriggerComponent` uchwycic `CanonicalPoolState` w punkcie tworzenia lub
   preflightu requestu, przed shadow simulation/post-buy handoff.
3. Zapisac:
   - `CanonicalPoolState`;
   - capture wall-clock ms;
   - state slot i state timestamp;
   - latest observed slot, jesli dostepny;
   - amount lamports;
   - min_out;
   - configured fee/slippage assumptions;
   - explicit UNKNOWN dla brakujacych chain-order components.
4. Przeniesc payload przez shadow-only handoff/event surface. Nie czytac
   `AccountStateCore` pozno w `PostBuyRuntime` jako skrotu.
5. W `PostBuyRuntime` skonwertowac payload do `PoolStateSampleV2` przez
   `PoolStateSampleV2::from_account_state_core(...)`.
6. Uruchomic `ShadowV2FillEngine` dla entry tylko wtedy, gdy:
   - state boundary jest scisle przed fill event boundary;
   - reserves sa kompletne;
   - token decimals i lamports normalization sa jawne;
   - input amount i min_out sa znane;
   - fee/slippage assumptions sa jawne.
7. Brak `account_data_hash` zapisac jako provenance blocker:
   `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME`, nie jako fake hash.
8. Wynik L1 oznaczac maksymalnie jako `DIAGNOSTIC_SIM`, dopoki
   `research_provenance_ready=false`.

## 7. Granice runtime

W PR34-A0:

- nie uruchomiono burnina;
- nie uruchomiono PR34 validation runu;
- nie zmieniono BUY/REJECT;
- nie zmieniono Gatekeeper policy;
- nie zmieniono selector runtime;
- nie dotknieto TX/Jito/live path;
- nie wlaczono `shadow_close_only`;
- nie wlaczono active close;
- nie nadano runtime approval;
- nie nadano research-grade;
- nie nadano live-equivalence;
- nie dotknieto R51;
- nie stage'owano raw JSONL/log/runtime scopes/local TOML.

## 8. Konkluzja

```text
ENTRY_BOUNDARY_SOURCE_PRESENT
```

Uzasadnienie:

`CanonicalPoolState` z `AccountStateCore` jest dostepny w `TriggerComponent`
przed entry fill i zawiera reserves oraz slot/time wymagane do zbudowania
diagnostic `PoolStateSampleV2`. To jest wystarczajacy fundament dla PR34-B.

Obecny Shadow V2 writer nie konsumuje tego zrodla. PR34-B musi przeniesc capture
z Trigger boundary do PostBuyRuntime. Nie wolno zastapic tego pozniejszym
odczytem `AccountStateCore` w PostBuyRuntime, bo to wprowadza lookahead risk.
