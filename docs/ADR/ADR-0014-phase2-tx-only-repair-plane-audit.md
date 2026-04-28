# ADR-0014: Weryfikacja spełnienia wymagań Fazy 2 dla trybu tx-only i repair plane

**Date:** 2026-03-20  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Po formalnym domknięciu Fazy 1 użytkownik zażądał zimnej weryfikacji trzech konkretnych warunków wyjścia Fazy 2 z `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`:

1. istnieje jawna definicja `tx_only` vs `tx_plus_account_repair`,
2. wyłączenie `AccountUpdate` nie łamie kontraktu decision path,
3. każdy write repair ma jawny `write_strength=P2`.

Celem nie było projektowanie nowych zmian, lecz udowodnienie na podstawie aktualnego kodu, dokumentacji architektonicznej i wykonanych testów, że te trzy warunki są już spełnione.

## Decision

Uznano, że **trzy wskazane warunki Fazy 2 są obecnie spełnione**.

### 1. Jawna definicja `tx_only` vs `tx_plus_account_repair` istnieje

Definicja trybów jest zakodowana jawnie przez flagę `account_updates_enabled` i opisana równolegle w konfiguracji oraz dokumentacji architektonicznej.

Źródła prawdy:

- `ghost-launcher/src/config.rs`
  - komentarz przy `OracleConfig.account_updates_enabled` wprost definiuje tryb `tx-only` jako wyłączenie ścieżki `AccountUpdate` end-to-end,
  - opisuje skutki: brak ShadowLedger write w Seer, brak forwardu IPC, brak obsługi `GhostEvent::AccountUpdate` w runtime,
  - domyślna wartość to `false`.
- `off-chain/components/seer/src/config.rs`
  - komentarz przy `SeerConfig.account_updates_enabled` wprost definiuje `tx-only` jako wyłączenie downstream `AccountUpdate` path,
  - domyślna wartość to `false`.
- `docs/ADR/20260320_production_pipeline_data_flow.md`
  - dokumentuje dwa jawne warianty pracy:
    - `account_updates_enabled=false` => realny `tx-only`,
    - `account_updates_enabled=true` => wariant z repair/reconciliation plane.

To spełnia wymóg Fazy 2, aby rozróżnienie nie było ukrytą interpretacją implementera, tylko jawnym kontraktem runtime.

### 2. Wyłączenie `AccountUpdate` nie łamie kontraktu decision path

Kod nie tylko wyłącza `AccountUpdate`, ale robi to w sposób, który zachowuje kanoniczny transaction-driven decision path.

Warstwowe dowody:

- `off-chain/components/seer/src/lib.rs`
  - `handle_account_update(...)` w trybie `account_updates_enabled=false` natychmiast kończy się bez zapisu do ShadowLedger i bez wysyłki IPC,
  - komentarz explicite mówi, że w `tx-only` metoda wraca natychmiast bez dotykania ShadowLedger ani kanału IPC.
- `ghost-launcher/src/components/seer.rs`
  - bridge forwarduje `SeerEvent::AccountUpdate` tylko wtedy, gdy `account_updates_enabled=true`,
  - w `tx-only` zdarzenie jest odcinane end-to-end na granicy bridge.
- `ghost-launcher/src/oracle_runtime.rs`
  - arm dla `GhostEvent::AccountUpdate` jest jawnie no-op w `tx-only`,
  - aktywny RPC reconciliation cycle nie jest w ogóle uruchamiany, gdy `account_updates_enabled=false`.
- `ghost-launcher/src/events.rs`
  - `GhostEvent::AccountUpdate` jest opisany jako corrective/reconciliation event, a nie primary data plane.
- `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`
  - komentarz przy `process_account_update(...)` jawnie określa `AccountUpdate` jako corrective authority; tx-driven path pozostaje primary.

To nie jest więc „wycięcie ważnej ścieżki”, tylko odłączenie pomocniczego repair plane, przy zachowaniu kanonicznego kontraktu: prawda decyzyjna pozostaje transaction-driven.

Dodatkowy dowód wykonawczy:

- `ghost-launcher/src/oracle_runtime.rs::test_tx_only_account_update_event_is_noop`
  - wykonany,
  - potwierdza, że `GhostEvent::AccountUpdate` nie wywołuje reconciliation i nie zmienia ShadowLedger w `tx-only`.
- `ghost-launcher/src/oracle_runtime.rs::test_tx_only_rpc_reconciliation_cycle_not_started`
  - wykonany,
  - potwierdza, że w `tx-only` nie startuje aktywny RPC reconciliation cycle.
- `off-chain/components/seer/src/lib.rs::test_handle_account_update_noop_in_tx_only_mode`
  - wykonany,
  - potwierdza, że Seer dropuje `AccountUpdate` przed zapisem do ShadowLedger.
- `ghost-launcher/tests/gatekeeper_v2_pipeline_integration.rs::test_runtime_router_does_not_spawn_unknown_pool_from_tx_only`
  - wykonany,
  - potwierdza, że tx-only unknown pool nie tworzy runtime lifecycle, identity registry, SnapshotEngine state ani buy-capable path.

Wniosek: wyłączenie `AccountUpdate` nie psuje decision path; przeciwnie, domyka go do jawnie zamierzonego modelu tx-first.

### 3. Każdy repair write ma jawny `write_strength=P2`

Aktywne repair pathy zapisują jawne metadane `Repair` na poziomie storage contract.

Dowody w kodzie:

- `off-chain/components/seer/src/lib.rs`
  - `store_repair_curve(...)` używa:
    - `ShadowLedgerWriteSource::AccountUpdate`,
    - `ShadowLedgerWriteStrength::Repair`,
    - `ShadowLedgerWriteReason::DirectAccountUpdate`.
- `ghost-core/src/shadow_ledger/reconciliation.rs`
  - zarówno finality refresh, jak i severe repair używają:
    - `ShadowLedgerWriteSource::Reconciliation`,
    - `ShadowLedgerWriteStrength::Repair`,
    - `ShadowLedgerWriteReason::FinalityRefresh` lub `ShadowLedgerWriteReason::ReconciliationRepair`.
- `ghost-core/src/shadow_ledger/ledger.rs`
  - testy storage-level potwierdzają, że zapisany rekord zachowuje `stored.write_strength == ShadowLedgerWriteStrength::Repair` dla repair metadata.

W modelu Fazy 2 ta jawna rola `Repair` jest właśnie kontraktowym odpowiednikiem `P2`. Nie ma tu już ukrytego repair write bez jawnej klasy siły zapisu.

## Architectural Impact

Ta decyzja formalnie przesuwa status Fazy 2 dla wskazanego zakresu z:

- „wymaga dowodu”

na:

- „spełnione i potwierdzone kodem, dokumentacją oraz testami wykonawczymi”.

Konsekwencje architektoniczne:

1. `tx_only` jest prawdziwym, jawnie opisanym trybem pracy, a nie efektem ubocznym wyłączonych handlerów.
2. `AccountUpdate` zostało ustalone jako repair plane / healing layer, a nie alternatywna ścieżka prawdy decyzyjnej.
3. repair writes są storage-level klasyfikowane jawnie jako `Repair` (`P2`), co domyka precedence semantics dla tej warstwy.
4. Decision path pozostaje kanonicznie transaction-driven.

## Risk Assessment

**Rate: Low**

Ryzyko regresji względem samego celu Fazy 2 jest niskie, ponieważ:

- definicja trybów istnieje jednocześnie w configu i dokumentacji,
- gating jest wielowarstwowy: Seer, bridge, runtime, cycle startup,
- testy wykonawcze potwierdzają brak aktywacji repair path w `tx_only`,
- repair write metadata są jawne i sprawdzalne.

Pozostają zwykłe ryzyka repo niezwiązane bezpośrednio z tym werdyktem:

- historyczne warningi w workspace,
- potencjalne future refactory, które mogłyby rozluźnić gating bez aktualizacji docs/tests.

Nie ma jednak obecnie dowodu na aktywną lukę blokującą domknięcie tego zakresu Fazy 2.

## Consequences

Co stało się łatwiejsze:

- można odhaczyć trzy wskazane checkboxy Fazy 2 bez dopisywania nowych interpretacji,
- kolejne fazy mogą opierać się na jawnym rozdziale `tx_only` vs repair plane,
- przyszłe audyty mają twardy punkt odniesienia: `AccountUpdate` nie jest primary decision path.

Co ta decyzja nie oznacza:

- cały workspace nie jest wolny od warningów,
- wszystkie dalsze fazy planu są automatycznie zamknięte,
- `AccountUpdate` nie przestaje istnieć — pozostaje świadomie ograniczonym mechanizmem korekcyjnym.

## Alternatives Considered

### 1. Uznać, że definicja trybów jest tylko implicit w kodzie
Odrzucono, bo konfiguracja i ADR architektoniczny zawierają już literalne opisy trybu `tx-only` oraz repair-enabled.

### 2. Uznać, że odcięcie `AccountUpdate` może ukrycie uszkadzać decision path
Odrzucono, bo testy oraz kod pokazują, że decision path pozostaje transaction-driven, a reconciliation path nie jest w `tx_only` potrzebny do działania kanonicznego runtime.

### 3. Wymagać osobnego symbolu `P2` zamiast `ShadowLedgerWriteStrength::Repair`
Odrzucono, bo plan Fazy 2 wymaga jawnej klasy repair write; w implementacji tym kontraktem jest właśnie `ShadowLedgerWriteStrength::Repair`, użyte explicite w aktywnych repair pathach.

## Validation Steps

Weryfikacja została wykonana przez:

1. odczyt konfiguracji i kontraktów trybów w:
   - `ghost-launcher/src/config.rs`
   - `off-chain/components/seer/src/config.rs`
   - `docs/ADR/20260320_production_pipeline_data_flow.md`
2. odczyt aktywnych ścieżek runtime i bridge w:
   - `off-chain/components/seer/src/lib.rs`
   - `ghost-launcher/src/components/seer.rs`
   - `ghost-launcher/src/events.rs`
   - `ghost-launcher/src/oracle_runtime.rs`
   - `ghost-core/src/shadow_ledger/reconciliation.rs`
   - `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`
   - `ghost-core/src/shadow_ledger/ledger.rs`
3. wykonanie testu:
   - `cargo test -p ghost-launcher test_tx_only_account_update_event_is_noop -- --nocapture`
4. wykonanie testu:
   - `cargo test -q -p ghost-launcher test_tx_only_rpc_reconciliation_cycle_not_started`
5. wykonanie testu:
   - `cargo test -q -p ghost-launcher test_runtime_router_does_not_spawn_unknown_pool_from_tx_only`
6. wykonanie testu:
   - `cargo test -p seer test_handle_account_update_noop_in_tx_only_mode -- --nocapture`
7. potwierdzenie storage-level asercji `stored.write_strength == ShadowLedgerWriteStrength::Repair` w `ghost-core/src/shadow_ledger/ledger.rs`.
