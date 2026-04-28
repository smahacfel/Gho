# Audyt pipeline'u gRPC → mapowanie pool → Shadow Ledger → AccountUpdate/Reconcile

Data audytu: 2026-03-12
Repozytorium: `Gh`
Zakres: ścieżka A–D wskazana w zleceniu użytkownika

---

## TL;DR

Stan obecny pipeline'u jest **częściowo poprawny**, ale **nie spełnia w pełni wymagań A–D** w sensie „kompletny i prawidłowy”.

Najważniejsze ustalenia:

1. **A / ingest gRPC + mapowanie tx do pool** — architektura jest sensowna i ma kilka warstw ochronnych (mapy `curve↔mint`, pending buffer, RPC resolve), ale **nie gwarantuje kompletności** dla wszystkich przypadków. Część trade'ów może zostać zbuforowana i wygasnąć albo zostać odrzucona, jeśli tożsamość pool/mint nie zostanie rozwiązana na czas.
2. **B / zapis parametrów tx i przekazanie do Shadow Ledger** — większość kluczowych pól biznesowych trade'a jest przekazywana do `PoolTransaction`, ale **payload MPCF / raw bytes są celowo zrzucane przy bridge'u Seer → EventBus**, więc przekazanie nie jest kompletne.
3. **C / aktualizacja stanu krzywej w pamięci po tx** — tx path **aktualizuje stan rekonstrukcyjny i snapshoty rynku**, ale **nie aktualizuje bezpośrednio cache `ShadowLedger.curves`** używanego jako kanoniczna pamięć krzywej po `bonding_curve`. To oznacza, że „stan krzywej w pamięci” rozjeżdża się semantycznie na dwa byty: snapshot history i curve cache.
4. **D / AccountUpdate + porównanie z prawdziwą krzywą + cykliczne reconcile** — to jest dziś **największa luka funkcjonalna**:
   - event-driven reconcile istnieje,
   - ale **cykliczny loop porównujący on-chain vs shadow co ~400 ms nie jest zaimplementowany produkcyjnie**,
   - dodatkowo reconcile jest dziś spięty po `base_mint`, podczas gdy curve cache produkcyjnie jest utrzymywany po `bonding_curve`, co **może unieważniać porównanie albo kierować je do złego klucza**.

Ocena końcowa:

- **A:** częściowo OK, ale niekompletne
- **B:** częściowo OK, ale niekompletne
- **C:** logicznie rozszczepione; nie spełnia wprost wymogu „zaktualizować w pamięci stan krzywej pool o tę transakcję” jeśli przez stan krzywej rozumiemy `ShadowLedger.curves`
- **D:** **nie spełnione w pełni**

---

## Metodyka audytu

Audyt został wykonany przez:

- analizę ścieżek kodu w:
  - `off-chain/components/seer/src/lib.rs`
  - `off-chain/components/seer/src/types.rs`
  - `off-chain/components/seer/src/ipc.rs`
  - `ghost-launcher/src/components/seer.rs`
  - `ghost-launcher/src/events.rs`
  - `ghost-launcher/src/oracle_runtime.rs`
  - `ghost-core/src/shadow_ledger/ledger.rs`
  - `ghost-core/src/shadow_ledger/live_pipeline.rs`
  - `ghost-core/src/shadow_ledger/reconciliation.rs`
  - `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`
- przeglądzie powiązań symboli i wywołań,
- uruchomieniu testów celowanych wokół reconcile / runtime / shadow ledger.

Uruchomione testy:

- `cargo test -p ghost-core reconciliation_runtime --lib`
- `cargo test -p ghost-core shadow_ledger::reconciliation --lib`
- `cargo test -p ghost-launcher reconciliation_runtime --lib`

Wynik: **testy przeszły**, ale same testy **nie wykrywają głównej luki produkcyjnej**, bo część helperów testowych buduje curve state pod kluczem `mint`, co maskuje problem `base_mint` vs `bonding_curve`.

---

## Mapa rzeczywistego przepływu danych

### 1. Ingest i parser

`Seer` odbiera zdarzenia `GeyserEvent::Transaction` oraz `GeyserEvent::AccountUpdate`.

Dla trade'ów:

- parser produkuje `seer::types::TradeEvent`
- Seer próbuje uzupełnić relację `pool_amm_id` / `mint`
- jeśli mapowanie nie jest jeszcze znane, trade trafia do `pending_trades`
- po rozwiązaniu mapowania jest replay do IPC

Dla account update:

- Seer parsuje curve account,
- rozwiązuje `base_mint` przez registry `curve_to_mint` / `tracked_curves`,
- zapisuje curve do `ShadowLedger`,
- forwarduje event reconcile dalej przez IPC.

### 2. Bridge Seer → launcher EventBus

W `ghost-launcher/src/components/seer.rs`:

- `TradeEvent` jest mapowany do `GhostEvent::PoolTransaction`
- `AccountUpdate` jest mapowany do `GhostEvent::AccountUpdate`

### 3. Launcher / OracleRuntime

W `ghost-launcher/src/oracle_runtime.rs`:

- `GhostEvent::PoolTransaction` trafia do:
  - pre-commit buffer / Gatekeeper path, albo
  - post-commit `LivePipeline`
- `GhostEvent::AccountUpdate` trafia do:
  - `OracleRuntime::process_account_update(...)`
  - a dalej do `ReconciliationRuntime::process_account_update(...)`

### 4. Shadow Ledger

`ShadowLedger` trzyma **dwa różne światy stanu**:

1. `curves` — keyed by **`bonding_curve`**
2. `snapshots` — keyed by **`base_mint`**

To jest centralny fakt tego audytu.

---

## A) Przetworzenie danych tx z gRPC i kompletne mapowanie do pool

## Ocena

**Częściowo poprawne, ale niekompletne.**

## Co działa poprawnie

### 1. Seer ma jawny mechanizm mapowania curve ↔ mint

W `off-chain/components/seer/src/lib.rs` istnieją:

- `tracked_curves`
- `curve_to_mint`
- `mint_to_curve`
- `register_curve_mapping(...)`
- `lookup_curve_mint(...)`

To daje kilka ścieżek rozwiązania tożsamości poola.

### 2. Trade bez pełnego mapowania nie jest od razu bezwarunkowo porzucany

Jeżeli trade przyjdzie przed pełnym poznaniem mappingu, trafia do:

- `pending_trades`
- z kluczami `ByCurve`, `ByMint`, albo `BySignature`

To jest dobra architektura na race condition `trade before mapping`.

### 3. Istnieje replay po poznaniu mapowania

Po `register_curve_mapping(...)` Seer wykonuje:

- `replay_pending_trades(...)`
- `replay_pending_curve_update(...)`

czyli próbuje domknąć okno między tx a AccountUpdate / init mapping.

### 4. Istnieje awaryjne RPC resolve

Jeżeli curve→mint nie jest znane, Seer może użyć:

- `queue_curve_mint_resolve(...)`
- `resolve_curve_mint_via_rpc(...)`

To poprawia recoverability.

## Luki / ryzyka

### 1. To nie jest mapowanie kompletne w sensie gwarancyjnym

Trade może:

- wpaść do `pending_trades`,
- nie doczekać resolve,
- wygasnąć po TTL,
- albo zostać odrzucony, jeśli nie ma forwardable identity.

To oznacza, że obecna ścieżka nie daje gwarancji „każdy tx zostanie prawidłowo przypisany do pool”.

### 2. Bridge do EventBus odrzuca trade bez rozwiązanego identity

W `ghost-launcher/src/components/seer.rs` trade bez forwardable identity jest dropowany przed emitowaniem do EventBus.

Skutek:

- część tx może nigdy nie wejść do dalszego pipeline'u,
- kompletność A zależy od jakości i szybkości mappingu przed bridge'em.

### 3. Backpressure na `AccountUpdate` jest stratny

W `off-chain/components/seer/src/ipc.rs` `send_account_update(...)` używa:

- `EventPriority::Low`
- `BackpressurePolicy::DropNew`

To znaczy, że pod obciążeniem AccountUpdate mogą zostać utracone. Gdyby istniał silny scheduler reconcile, byłoby to akceptowalne. Ale obecnie go nie ma — więc utrata update może realnie osłabić naprawę dryfu.

## Wniosek dla A

Pipeline A jest **dobrze pomyślany**, ale **nie jest kompletny gwarancyjnie**. W praktyce:

- **tak, wiele tx będzie zmapowanych poprawnie**,
- **nie, repo nie daje dziś podstaw, by stwierdzić kompletność i pełną poprawność dla wszystkich tx z gRPC**.

---

## B) Zapis parametrów każdego tx i prawidłowe przekazanie do shadow ledger

## Ocena

**Częściowo poprawne, ale niekompletne.**

## Co jest zachowywane poprawnie

Bridge `TradeEvent -> PoolTransaction` w `ghost-launcher/src/components/seer.rs` zachowuje m.in.:

- `pool_amm_id`
- `slot`
- `event_ordinal`
- `timestamp_ms`
- `arrival_ts_ms`
- `signer`
- `is_buy`
- `volume_sol`
- `sol_amount_lamports`
- `token_amount_units`
- `is_dev_buy`
- `signature`
- `success`
- `error_code`
- `compute_units_consumed`
- `owner_token_deltas`
- `token_mint`
- reserve hints (`v_tokens_in_bonding_curve`, `v_sol_in_bonding_curve`, `market_cap_sol`)
- compute budget metadata (`cu_price_micro_lamports`, `compute_unit_limit`, `inner_ix_count`, `cpi_depth`, `ata_create_count`)
- `signer_pre_balance_lamports`
- `jito_tip_detected`
- `curve_data_known`

To oznacza, że dla typowej logiki trade / Gatekeeper / LivePipeline zachowywany jest duży zestaw informacji.

## Krytyczna niekompletność

### 1. `mpcf_payload` jest zrzucany przy bridge'u

W bridge'u do `PoolTransaction` ustawiane jest:

- `mpcf_payload: vec![]`
- `mpcf_payload_missing_reason: FilteredByConfig`

To nie jest przypadkowa utrata — to jawne odcięcie danych.

Skutek:

- parametry tx nie są przekazywane **kompletnie**,
- downstream nie ma pełnego payloadu transakcji,
- jeśli użytkownik oczekuje „wszelkich parametrów” tx, to wymóg nie jest spełniony.

# KOMENTARZ: mpcf_payload jest CELOWO ZWRZUCANY I JEST TO OK, PONIEWAŻ MPCF JEST RELIKTEM, KTÓRY NIE FUNKCJONUJE W BIEŻACYM PIPELINE I ZOSTANIE WKRÓTCE USUNIĘTY.

### 2. Shadow Ledger i tak konsumuje tylko podzbiór pól

Do faktycznej ewolucji stanu używane są głównie:

- side
- d_sol
- d_tok
- trader
- dev flag
- tx ordering metadata

Czyli nawet jeśli `PoolTransaction` niesie dużo pól, nie oznacza to jeszcze, że Shadow Ledger wykorzystuje je do aktualizacji krzywej.

## Wniosek dla B

- **Prawidłowe przekazanie do downstream trade path istnieje**.
- **Kompletność parametrów nie jest zachowana**, bo bridge jawnie czyści część danych (`mpcf_payload`).

Ocena: **B nie jest spełnione w pełni**.

---

## C) Odebranie poprawnych danych przez shadow ledger i zaktualizowanie w pamięci stanu krzywej pool

## Ocena

**Semantycznie rozszczepione; częściowo tak, częściowo nie.**

## Co działa

### 1. Po commit / approve tx trafiają do właściwej ścieżki runtime

W `ghost-launcher/src/oracle_runtime.rs`:

- przed commitem trade może trafić do bufora commit coordinatora,
- po commicie idzie do `LivePipeline::process_event(...)`.

### 2. `LivePipeline` robi deterministyczną ewolucję stanu transakcyjnego

W `ghost-core/src/shadow_ledger/live_pipeline.rs`:

- event jest zamieniany na `LiveTxEvent`
- trafia do `MintLiveState`
- przy flush jest sortowany po `TxKey`
- `ReconstructedState::apply_trade_strict(...)` aktualizuje rezerwy
- generowane są `TradeSnapshot` i `MarketSnapshot`
- snapshoty trafiają do `ShadowLedger.append_live(...)`

### 3. Flush loop jest realnie uruchamiany produkcyjnie

Istnieje rzeczywisty background task `ghost-launcher/src/components/live_pipeline_flush_loop.rs`, który cyklicznie woła:

- `live_pipeline.flush_ready(&shadow_ledger)`

Czyli live tx path nie kończy się na buforze — jest flush do ledgera.

## Główna luka semantyczna

### 1. Live tx aktualizuje snapshoty, ale nie aktualizuje `ShadowLedger.curves`

To jest najważniejsze rozróżnienie.

`ShadowLedger` ma:

- `curves` keyed by `bonding_curve`
- `snapshots` keyed by `base_mint`

`LivePipeline` aktualizuje:

- `MintLiveState` (rekonstrukcja),
- `MarketSnapshot` w `snapshots`.

Natomiast **nie wykonuje aktualizacji curve cache** przez `insert_with_slot_known(...)` dla odpowiadającego `bonding_curve`.

W praktyce oznacza to, że:

- historia i rynek po tx są aktualizowane,
- ale „kanoniczna pamięć krzywej” jako obiekt `BondingCurve` niekoniecznie jest aktualizowana każdą transakcją,
- aktualizacja `curves` odbywa się głównie z `AccountUpdate` lub repair reconcile.

### 2. Wymóg użytkownika mówi o aktualizacji „stanu krzywej pool w pamięci”

Jeśli interpretować to dosłownie jako stan bieżącej krzywej AMM w pamięci procesu, to obecna architektura jest rozszczepiona:

- tx path aktualizuje **snapshot-based market state**,
- account-update path aktualizuje **curve cache**.

To nie jest to samo.

## Wniosek dla C

- Jeśli przez „stan krzywej” rozumieć **snapshot history / reconstructed state**, to pipeline działa.
- Jeśli przez „stan krzywej” rozumieć **`ShadowLedger.curves` / `BondingCurve` cache**, to tx path **nie spełnia wymogu bezpośrednio**.

Ocena: **C nie jest spełnione jednoznacznie i w ścisłym sensie — nie**.

---

## D) Powiązanie AccountUpdate z pool + cykliczne porównanie prawdziwej krzywej z shadow ledgerem + naprawa przy dużych różnicach

## Ocena

**Największa luka audytu. Wymóg nie jest spełniony w pełni.**

## Co działa

### 1. AccountUpdate są mapowane do `base_mint` i forwardowane do runtime

W `Seer::handle_account_update(...)`:

- account jest parsowane,
- rozwiązywany jest `base_mint`,
- `send_account_update(base_mint, bonding_curve, sol_reserves, token_reserves, complete, slot)` idzie przez IPC.

Potem bridge launcherowy emituje:

- `GhostEvent::AccountUpdate { base_mint, ... }`

### 2. OracleRuntime naprawdę konsumuje ten event

W runtime tasku `ghost-launcher/src/oracle_runtime.rs`:

- `GhostEvent::AccountUpdate` trafia do `oracle_runtime.process_account_update(...)`
- a dalej do `ReconciliationRuntime::process_account_update(...)`

### 3. ReconciliationRuntime istnieje i umie naprawiać severe drift

W `ghost-core/src/shadow_ledger/reconciliation_runtime.rs` i `reconciliation.rs`:

- drift jest klasyfikowany na `None / Noise / Meaningful / Severe`
- severe drift wywołuje repair przez `insert_with_slot_known(...)`

To znaczy: **mechanizm naprawy jako taki istnieje**.

## Krytyczne problemy

### 1. Reconcile pracuje po `base_mint`, ale curve cache produkcyjnie jest keyed by `bonding_curve`

To jest problem klasy **critical**.

W produkcji Seer zapisuje curve do `ShadowLedger` przez:

- `store_curve_with_snapshots(ledger, curve_key, base_mint, curve, ...)`
- czyli curve trafia pod `curve_key` = bonding curve

Z kolei reconcile runtime jest rejestrowany po:

- `register_pool(base_mint)`

oraz przetwarza update przez:

- `process_account_update(&base_mint, ...)`

A w samym reconcilerze porównanie robi:

- `self.ledger.get(mint)`

To oznacza, że reconcile próbuje czytać curve po `base_mint`, podczas gdy produkcyjny curve cache jest zapisany po `bonding_curve`.

### Skutki

Możliwe skutki są dwa, oba złe:

1. `ledger.get(base_mint)` zwraca `None` → reconcile realnie nic nie robi
2. jeżeli przez przypadek coś zostało zapisane pod `base_mint` w innej ścieżce/testach, reconcile porównuje nie ten byt, który powinien

### 2. Testy maskują ten problem

Testy reconcile przechodzą, bo helpery testowe wstawiają curve do ledgera pod `mint`, np. styl:

- `ledger.insert_with_slot(mint, curve, ...)`

To działa w testach, ale **nie odpowiada kluczowaniu produkcyjnemu**, gdzie curve są po `bonding_curve`.

Wniosek: test suite daje złudne poczucie bezpieczeństwa.

### 3. Produkcyjny task 30 s to tylko health reporter, nie scheduler reconcile

W `ghost-launcher/src/oracle_runtime.rs` jest jawny komentarz i implementacja:

- task co 30 s loguje `ReconciliationRuntimeStatus`
- **nie robi polling on-chain state**
- **nie uruchamia `run_reconciliation_cycle(fetch)` produkcyjnie**

Komentarz w kodzie wręcz mówi, że to **health reporter only**.

To jest wprost sprzeczne z wymaganiem użytkownika o:

- cyklicznym porównaniu co np. 400 ms
- i naprawie przy większych różnicach

### 4. AccountUpdate mogą być dropowane pod backpressure

Ponieważ AccountUpdate idą przez `DropNew`, a nie ma aktywnego cycle polling, to:

- utracony AccountUpdate nie ma gwarantowanego kompensacyjnego checku po 400 ms,
- czyli heal może w ogóle nie zajść do czasu kolejnego update.

## Wniosek dla D

- **Powiązanie AccountUpdate z pool istnieje częściowo i logicznie**, przez curve→mint resolve.
- **Event-driven reconcile istnieje**.
- **Cykliczny reconcile loop ~400 ms — nie istnieje produkcyjnie**.
- **Dodatkowo reconcile jest błędnie spięty kluczem `base_mint` vs `bonding_curve`**.

Ocena: **D nie jest spełnione**.

---

## Zidentyfikowane root cause'y

## Root cause 1 — rozjazd kluczy tożsamości

`ShadowLedger` używa różnych kluczy dla różnych bytów:

- curve cache: `bonding_curve`
- snapshot history: `base_mint`

To samo w sobie jest jeszcze do obrony, ale reconcile runtime został wpięty tak, jakby porównywał świat keyed by `base_mint` z curve cache keyed by `bonding_curve`.

To jest główny błąd architektoniczny.

## Root cause 2 — brak jednego jawnego source of truth dla „state of curve”

System ma dziś dwa równoległe obrazy:

- `BondingCurve` w `ShadowLedger.curves`
- `ReconstructedState / MarketSnapshot` w `snapshots` + `LivePipeline`

Nie ma twardo wymuszonego kontraktu, który z nich jest bieżącym stanem krzywej po każdej transakcji.

## Root cause 3 — reconcile scheduler został zaprojektowany, ale nie podłączony produkcyjnie

Kod ma:

- `OracleRuntime::run_reconciliation_cycle(fetch)`
- `ReconciliationRuntime::run_cycle(fetch)`

ale produkcyjny runtime nie dostarcza loopa z realnym fetch closure.

## Root cause 4 — testy integracyjne nie odzwierciedlają produkcyjnego keyingu

To maskuje najgroźniejszy błąd i utrudnia wykrycie regresji.

---

## Ocena ryzyka

## Krytyczne

### 1. Reconcile po złym kluczu

Ryzyko: bardzo wysokie

Skutek:

- healing może nie działać,
- drift może narastać mimo obecności AccountUpdate,
- operator może błędnie wierzyć, że reconcile aktywnie chroni ledger.

### 2. Brak cyklicznego compare loop

Ryzyko: wysokie

Skutek:

- utrata pojedynczego AccountUpdate nie ma kompensacji,
- system nie spełnia wymagania okresowego sanity-check.

## Wysokie

### 3. Rozszczepienie curve state vs snapshot state

Ryzyko: wysokie

Skutek:

- różne moduły mogą czytać „stan pool” z innych źródeł,
- diagnoza błędów jest trudniejsza,
- reconcile może naprawiać nie ten byt, który później jest używany przez inne moduły.

### 4. Utrata `mpcf_payload`

Ryzyko: średnio-wysokie

Skutek:

- niepełna telemetria,
- niepełna możliwość późniejszej klasyfikacji / analizy tx.

---

## Co jest dobre w obecnej architekturze

Żeby było uczciwie: tu nie ma tylko problemów.

Na plus:

1. Seer ma sensowny mechanizm pending/replay dla race condition.
2. Bridge `TradeEvent -> PoolTransaction` jest czytelny i jawny.
3. `LivePipeline` ma porządny model porządkowania (`TxKey`, flush, monotonicity guard).
4. Reconciliation runtime jest napisany w sposób operacyjny i obserwowalny.
5. Istnieje realny flush loop dla live pipeline.
6. Testy wokół reconcile / runtime są liczne — problemem nie jest brak testów, tylko **zły model testowanej tożsamości klucza**.

---

## Rekomendowane działania naprawcze

## Priorytet P0 — naprawić kluczowanie reconcile

Należy doprowadzić do tego, aby reconcile porównywał on-chain state z tym samym bytem, który Seer zapisuje jako curve cache.

Dwie sensowne opcje:

### Opcja A
Przestawić runtime reconcile na `bonding_curve` jako klucz curve state.

Wtedy `AccountUpdate` musi nieść i wykorzystywać jawnie `bonding_curve` jako klucz odczytu/zapisu curve cache.

### Opcja B
Dodać w `ShadowLedger` osobny stabilny adapter `base_mint -> bonding_curve`, a reconcile niech wewnętrznie tłumaczy `base_mint` na `bonding_curve` przed `ledger.get_curve(...)` / repair.

Bez tego punkt D pozostaje niewiarygodny.

## Priorytet P0 — dodać realny cycle loop ~400 ms

Należy uruchomić produkcyjny scheduler, np.:

- co 400 ms,
- bounded subset pools,
- fetch z cache ostatnich account updates albo z RPC,
- `oracle_runtime.run_reconciliation_cycle(fetch)`.

Obecny 30-sekundowy reporter zdrowia nie spełnia tego wymagania.

## Priorytet P1 — ujednolicić definicję „state of curve”

Należy zdefiniować jednoznacznie:

- czy bieżący stan krzywej to `curves`,
- czy `LivePipeline::ReconstructedState`,
- czy snapshot tail.

I doprowadzić do tego, żeby wszystkie moduły używały tego samego kontraktu.

## Priorytet P1 — zdecydować, czy bridge ma przenosić pełny payload tx

Jeżeli wymagana jest pełna obserwowalność tx, bridge nie może robić:

- `mpcf_payload: vec![]`

Jeżeli to ma zostać, trzeba to uznać formalnie za świadome ograniczenie funkcjonalne, a nie „pełne przekazanie parametrów”.

## Priorytet P1 — testy produkcyjnego keyingu

Dodać testy, które:

1. zapisują curve pod `bonding_curve`,
2. rejestrują pool po `base_mint`,
3. puszczają `AccountUpdate`,
4. sprawdzają, czy reconcile rzeczywiście odnajduje właściwy curve entry.

Dzisiaj to jest największa dziura testowa.

---

## Odpowiedź końcowa na pytanie „czy pipeline działa poprawnie?”

### A) gRPC tx → poprawne mapowanie do pool

**Nie w pełni.**
Działa często i ma zabezpieczenia, ale nie ma gwarancji kompletności.

### B) zapis parametrów tx i przekazanie do shadow ledger

**Nie w pełni.**
Większość pól jest przekazywana, ale nie wszystkie — payload tx jest przy bridge'u obcinany.

### C) shadow ledger odbiera poprawne dane i aktualizuje stan krzywej w pamięci

**Nie wprost i nie jednoznacznie.**
Aktualizowany jest reconstructed/snapshot state, ale nie ten sam byt curve cache, który jest utrzymywany po `bonding_curve`.

### D) AccountUpdate → pool → cykliczne compare → repair

**Nie.**
Jest event-driven reconcile, ale:

- nie ma produkcyjnego cyklicznego loopa ~400 ms,
- reconcile jest obecnie bardzo prawdopodobnie błędnie spięty kluczem `base_mint` zamiast `bonding_curve`.

---

## Finalny werdykt audytu

**Pipeline nie jest obecnie kompletny ani w pełni poprawny względem wymagań A–D.**

Najpoważniejsze powody:

1. **błędne spięcie reconcile z tożsamością curve (`base_mint` vs `bonding_curve`)**,
2. **brak aktywnego produkcyjnego cyklicznego reconcile loopa**,
3. **rozszczepienie pojęcia „state of curve” między curve cache a snapshot/live state**,
4. **niepełne przekazywanie parametrów tx przy bridge'u**.

Jeżeli chcesz, następnym krokiem mogę od razu przygotować:

1. **plan naprawczy P0/P1 z kolejnością wdrożenia**, albo
2. **patch kodu**, który naprawi najgroźniejszy błąd: `base_mint` vs `bonding_curve` + doda realny reconcile scheduler.
