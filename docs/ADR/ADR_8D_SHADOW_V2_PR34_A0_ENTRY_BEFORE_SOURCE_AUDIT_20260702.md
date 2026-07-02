# ADR-8D: Shadow V2 PR34-A0 ENTRY_BEFORE Source Audit

## Status

Accepted as audit evidence.

Final verdict:

```text
ENTRY_BOUNDARY_SOURCE_PRESENT
```

## D1. Problem

Po PR33 Shadow V2 ma centralny L1 deterministic execution engine, ale engine
nie moze byc bezpiecznie uzyty bez prawdziwego `ENTRY_BEFORE`.

Problem PR34-A0:

```text
czy runtime ma deterministyczne, no-lookahead zrodlo stanu poola przed entry fill?
```

Bez tego PR34/PR35 groza powrotem do tego samego bledu: engine istnieje, ale
records pozostaja `BLOCKED_BY_DATA`, albo co gorsza dostaja pozny stan z
lookahead risk.

## D2. Decyzja

Uznajemy, ze zrodlo `ENTRY_BEFORE` jest obecne, ale tylko w upstream Trigger
boundary:

```text
TriggerComponent -> AccountStateCore -> CanonicalPoolState
```

Nie uznajemy obecnego PostBuyRuntime Shadow V2 writer path za gotowe zrodlo.
Writer ma tylko optional adapter i produkcyjnie przekazuje `None` jako
`entry_pool_state_before`.

## D3. Evidence

Kodowe dowody obecnosci upstream source:

- `TriggerComponent::canonical_pool_state()` zwraca `AccountStateCore` canonical
  state dla mintu:
  `ghost-launcher/src/components/trigger/component.rs:1773`.
- `run_local_buy_preflight()` pobiera latest observed slot, canonical state,
  sprawdza staleness i symuluje curve output przed BUY:
  `ghost-launcher/src/components/trigger/component.rs:3350`.
- `CanonicalPoolState` zawiera raw reserves, state slot i state timestamp:
  `ghost-core/src/account_state_core/types.rs:101`.
- `PoolStateSampleV2::from_account_state_core()` potrafi opakowac
  `CanonicalPoolState` w canonical Shadow V2 pool-state sample:
  `ghost-brain/src/guardian/post_buy/shadow_v2.rs:1180`.
- `ShadowV2FillEngine` wymaga `pool_state_before`, reserves, decimals,
  lamports normalization i ordering boundary:
  `ghost-brain/src/guardian/post_buy/shadow_v2_execution.rs:166`.

Kodowe dowody braku obecnego wiring:

- `PostBuySubmitted` niesie token amounts i slots, ale nie raw reserves ani
  `PoolStateSampleV2`: `ghost-launcher/src/events.rs:1277`.
- `maybe_emit_shadow_v2_entry_evidence()` przekazuje `None` do wariantu z pool
  state: `ghost-launcher/src/components/post_buy_runtime.rs:2911`.
- Helper dodaje `ENTRY_POOL_STATE_BEFORE_UNAVAILABLE` i
  `ENTRY_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME_HANDOFF`:
  `ghost-launcher/src/components/post_buy_runtime.rs:3087`.

## D4. Konsekwencje

PR34-B moze isc w implementation, ale tylko z kontraktem:

- capture upstream w Trigger boundary;
- przeniesienie snapshotu przez shadow-only handoff;
- brak poznego odczytu `AccountStateCore` w PostBuyRuntime jako shortcut;
- brak fake account hash;
- brak runtime approval;
- brak research-grade;
- brak live-equivalence.

Brak `account_data_hash` degraduje provenance do diagnostic:

```text
execution_label_grade = DIAGNOSTIC_SIM
research_provenance_ready = false
```

Nie musi automatycznie blokowac samej deterministic L1 symulacji, jezeli
reserves, amount, min_out, fee, decimals, lamports normalization i ordering sa
wystarczajace.

## D5. Rejected Alternatives

Odrzucone:

1. Pozny odczyt `PostBuyRuntime.account_state_core`.
   Powod: reducer zwraca aktualny stan, a nie historyczny snapshot wedlug entry
   boundary. To moze uzyc stanu po entry.
2. Uzycie `MaterializedFeatureSet.account_features` jako pelnego
   `ENTRY_BEFORE`.
   Powod: snapshot decyzyjny nie niesie pelnego `CanonicalPoolState`, account
   hash ani event-order boundary.
3. Uzycie `OracleRuntime::resolve_price_context`.
   Powod: to normalized price/reserve context z fallbackami, nie canonical
   pool-state boundary dla executable fill.
4. Wpisanie fake `account_data_hash`.
   Powod: falszuje provenance i lamie kontrakt audytowalnosci.

## D6. Runtime Boundary

PR34-A0 nie zmienia runtime:

- BUY/REJECT: bez zmian;
- Gatekeeper policy: bez zmian;
- selector runtime: bez zmian;
- TX/Jito/live path: bez zmian;
- `shadow_close_only`: false / bez zmian;
- active close: false / bez zmian;
- runtime approval: false;
- research-grade: false;
- live-equivalence: false;
- R51: nietkniete.

Nie uruchomiono burnina ani validation runu.

## D7. Required PR34-B Contract

PR34-B powinien:

1. Dodac addytywny, shadow-only `ENTRY_BEFORE` boundary payload do request/handoff.
2. Capture wykonac przed shadow simulation/post-buy handoff w Trigger boundary.
3. Przeniesc `CanonicalPoolState`, capture time, state slot/time, amount,
   min_out i jawne assumptions fee/slippage.
4. Zbudowac `PoolStateSampleV2` w PostBuyRuntime z tego payloadu.
5. Uruchomic `ShadowV2FillEngine` tylko przy spelnieniu ordering contract.
6. Zapisac explicit `UNKNOWN` dla brakujacych chain-order components.
7. Pozostawic `research_provenance_ready=false`, jezeli brakuje account hash lub
   pelnego chain-order.

## D8. Definition of Done dla PR34-B

PR34-B moze byc gotowy do walidacji dopiero gdy testy pokaza:

- entry boundary jest capture'owany przed fill boundary;
- PostBuyRuntime nie robi poznego AccountStateCore shortcut;
- `shadow_entry_fill_v2` przechodzi przez `ShadowV2FillEngine`;
- brak account hash jest provenance blockerem, nie fake value;
- brak source nadal daje typed `BLOCKED_BY_DATA`;
- zadne Shadow V2 pole nie jest konsumowane przez Gatekeeper, selector, BUY/REJECT
  ani TX/Jito/live path.
