Najpierw sprawdzę dokładny ślad tego konkretnego poola w logach oraz miejsca w kodzie, które mogły go przepuścić, żeby ustalić nie teorię, tylko faktyczny tor wejścia do runtime.

Wyszukano wyrażenie regularne `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD|8myu6YffAxoTQbjK1jpkw1SZWvN7Ss8pfSavMf5wuUzt|CreatePool|NewPoolDetected` (`**/logs/system.log.2026-03-17`), brak wyników

Wyszukano wyrażenie regularne `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD|8myu6YffAxoTQbjK1jpkw1SZWvN7Ss8pfSavMf5wuUzt|CreatePool|NewPoolDetected` (`**/logs/oracle_decision.log.2026-03-17`), wyniki 32

Wyszukano wyrażenie regularne `CURVE_MAP_SET|Emitting PoolTransaction|Seer: ✅ PoolTransaction emitted to Event Bus|Trade detected via IPC|Event Bus: receivers` (`**/ghost-launcher/src/**`), wyniki 3

Wyszukano wyrażenie regularne `PoolTransaction|GhostEvent::PoolTransaction|subscribe\(|receiver|Event Bus` (`**/ghost-launcher/src/**`), wyniki 200

Mam już pierwszy twardy trop: `oracle_decision.log` nie pokazuje tego poola jako `NewPoolDetected`, ale seer.rs ewidentnie mostkuje trade’y z IPC prosto na event bus. Teraz doczytam dokładnie tę ścieżkę oraz sprawdzę, które komponenty poza `OracleRuntime` konsumują takie `PoolTransaction`, bo to wygląda na bypass wcześniejszego filtra, nie na jego częściową awarię.

Wyszukano wyrażenie regularne `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD|8myu6YffAxoTQbjK1jpkw1SZWvN7Ss8pfSavMf5wuUzt` (`**/logs/system.log.2026-03-17`), wyniki 200

Odczytaj [](file:///root/Gho/ghost-launcher/src/oracle_runtime.rs#6000-6000), wiersze 6000 do 8000

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/seer.rs#275-275), wiersze 275 do 760

Odczytaj [](file:///root/Gho/ghost-launcher/src/main.rs#680-680), wiersze 680 do 1080

Obraz robi się wyraźniejszy: `Seer` mostkuje każdy forwardowalny trade z IPC na event bus bez sprawdzenia „czy pool był live-detected w tej sesji”. Teraz sprawdzę dwie rzeczy: dokładny warunek w `OracleRuntime` oraz które downstream komponenty dalej przetwarzają taki `PoolTransaction`, nawet jeśli sam Oracle go oleje.

Wyszukano wyrażenie regularne `fn start_oracle_runtime_task|GhostEvent::PoolTransaction|Ignoring tx-first event for non-canonical pool|tx-first|unknown_ignored_total` (`**/ghost-launcher/src/oracle_runtime.rs`), wyniki 5

Wyszukano wyrażenie regularne `GhostEvent::PoolTransaction\(|pool_transaction\(|on_transaction\(|handle_pool_transaction|track_pool\(` (`**/ghost-launcher/src/components/**`), wyniki 150

Odczytaj [](file:///root/Gho/ghost-launcher/src/main.rs#396-396), wiersze 396 do 450

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/snapshot_listener.rs#319-319), wiersze 319 do 520

Teraz dociągam brakujące ogniwa: sam warunek ignorowania w `OracleRuntime` i poboczne konsumenty event busa. To powinno rozdzielić „Oracle nie przyjął” od „system i tak przetwarzał gdzie indziej”.

Wyszukano tekst `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD` (`**/logs/**`), wyniki 200

Odczytaj [](file:///root/Gho/ghost-launcher/src/oracle_runtime.rs#5660-5660), wiersze 5660 do 5825

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/snapshot_listener.rs#180-180), wiersze 180 do 260

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/west_listener.rs#1-1), wiersze 1 do 140

Mam już mocny dowód bypassu: ten pool pojawia się najpierw jako `CURVE_MAP_SET source=trade`, a chwilę później `Seer` emituje `PoolTransaction` na event bus — bez poprzedzającego `PoolDetected/NewPoolDetected` dla tego samego poola. Dociągam teraz brakujący fragment początku tej sekwencji oraz warunek w `Seer`, który na to pozwala.

Wyszukiwanie wyrażenia regularnego `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD.*(Pool detected via IPC|Emitting NewPoolDetected|Event emitted to Event Bus for new pool|OBSLUGUJE EVENT NewPoolDetected)` (`**/logs/**`)

Wyszukiwanie wyrażenia regularnego `fn trade_has_forwardable_identity|trade_has_forwardable_identity\(` (`**/ghost-launcher/src/components/seer.rs`)

Odczytaj [](file:///root/Gho/off-chain/components/seer/src/lib.rs#2130-2130), wiersze 2130 do 2395

Odczytaj [](file:///root/Gho/logs/system.log.2026-03-17#360-360), wiersze 360 do 385

Jeszcze tylko domykam jeden szczegół: local bridge w `ghost-launcher::components::seer` nie ma żadnej sesyjnej walidacji poola — sprawdza wyłącznie, czy trade ma rozwiązywalną tożsamość. To jest prawdopodobny rdzeń problemu, więc odczytam dokładnie tę funkcję i ewentualne istniejące flagi konfiguracyjne, które można wykorzystać zamiast dorabiać trzecią protezę.

Wyszukano tekst `trade_has_forwardable_identity` (`**/ghost-launcher/src/components/seer.rs`), wyniki 5

Wyszukano wyrażenie regularne `snapshot_listener_forward_mode|TrackedBuffered|ApprovedOnly|Provisional` (`**/ghost-launcher/src/config.rs`), wyniki 12

Wyszukano wyrażenie regularne `WestListener|west_listener|Starting West|Wallet Energy` (`**/ghost-launcher/src/main.rs`), brak wyników

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/seer.rs#1-1), wiersze 1 do 140

Mam już źródło problemu prawie rozebrane na śruby: fix w `OracleRuntime` działa tylko na **jego** gałęzi routingu, ale pool przecieka wcześniej przez bridge `Seer -> EventBus`, więc dalej oglądają go inni konsumenci. Dla planu wykonawczego sprawdzę jeszcze, jak SnapshotListener buforuje takie „nieautorytatywne” tx-y, żeby ocenić realny koszt i dobrać właściwą granicę cięcia.

Wyszukano wyrażenie regularne `Ignoring tx-first event for non-canonical pool|oracle_runtime_tx_first_unknown_ignored_total` (`**/logs/system.log.2026-03-17`), brak wyników

Wyszukano tekst `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD` (`**/logs/oracle_decision.log.2026-03-17`), brak wyników

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/snapshot_listener.rs#1-1), wiersze 1 do 180

Odczytaj [](file:///root/Gho/ghost-launcher/src/config.rs#35-35), wiersze 35 do 130

Jeszcze sprawdzę, czy `SnapshotListener` ma jakiekolwiek TTL/GC dla takich staged tx-ów. Jeśli nie ma, to problem nie kończy się na „spamie bridge’a” — robi się z tego cichy bufor śmieci dla pooli spoza sesji.

Wyszukano wyrażenie regularne `staged_transactions|listener_buffered|PendingMappingBuffered|inactive_tx_buffer|ttl_margin|cleanup|prune` (`**/ghost-launcher/src/components/snapshot_listener.rs`), wyniki 15

Wyszukano wyrażenie regularne `track_pool\(|handle_tx_event\(` (`**/ghost-brain/src/**`), wyniki 61

Odczytaj [](file:///root/Gho/ghost-launcher/src/components/snapshot_listener.rs#520-520), wiersze 520 do 760

Mam już pełen obraz: to nie jest „niedziałający fix OracleRuntime”, tylko **dziura w granicy filtrowania**. Oracle odrzuca ten pool u siebie, ale bridge `Seer -> EventBus` przepuszcza go wcześniej, a `SnapshotListener` dodatkowo buforuje takie śmieci bez widocznego TTL/GC. Zapisuję stan i przechodzę do końcowej diagnozy z planem wykonawczym.

Added 2 todos

Masz rację — ten przypadek pokazuje, że problem **nie został domknięty na właściwej granicy**. I co ważne: po analizie wychodzi, że to nie jest tyle „niedziałający fix w `OracleRuntime`”, ile **bypass wcześniejszego etapu**, który pozwala starym poolom wejść do wspólnego runtime/event-bus mimo braku `CreatePool` w bieżącej sesji.

## Co ustaliłem

### Twarde fakty z logów

Dla poola `3DnLZV9sLbNx2HHxKvjbMCkiEU8GhGvPUWmpFbWYXepD` i minta `8myu6YffAxoTQbjK1jpkw1SZWvN7Ss8pfSavMf5wuUzt`:

- on-chain create wg Solscan:
  - `2026-03-17 09:46:23 UTC`
- start sesji bota:
  - `2026-03-17T10:36:45`

Czyli pool powstał **~50 minut przed startem bota**.

W naszych logach pierwszy ślad tego poola to:

- `2026-03-17T10:36:48.010Z INFO seer: CURVE_MAP_SET source=trade ...`

a zaraz potem:

- `seer: Emitting PoolTransaction ... pool=3Dn...`
- `ghost_launcher::components::seer: Seer: 🚀 Emitting PoolTransaction ...`

Natomiast:

- **nie ma** wcześniejszego `Pool detected via IPC`,
- **nie ma** `Emitting NewPoolDetected`,
- **nie ma** `OBSLUGUJE EVENT NewPoolDetected` dla tego poola w `oracle_decision.log`.

To oznacza, że dla tego poola ścieżka była:

$$\text{trade} \rightarrow \text{CURVE\_MAP\_SET source=trade} \rightarrow \text{PoolTransaction on Event Bus}$$

a nie:

$$\text{CreatePool / NewPoolDetected} \rightarrow \text{pool canonical in this session} \rightarrow \text{trade}$$

## Gdzie jest źródło problemu

### 1) `Seer` core nadal przepuszcza trade’y dla pooli niewidzianych w tej sesji

W lib.rs:

- `handle_trade_event(...)` dla trade’a z konkretnym `pool_amm_id` i `mint`
- robi `register_curve_mapping(trade.pool_amm_id, trade.mint, "trade", false)`
- a potem `should_forward_trade(...)` może zwrócić `Forward`

Kluczowe: ta decyzja **nie zależy od tego**, czy pool miał live `CreatePool` w bieżącej sesji.

To dokładnie tłumaczy log:

- `CURVE_MAP_SET source=trade`
- potem od razu `Emitting PoolTransaction`

### 2) Launcherowy bridge `Seer -> EventBus` nie ma sesyjnego filtra pooli

W seer.rs warunek jest dziś bardzo słaby:

- `trade_has_forwardable_identity(trade)` wymaga tylko:
  - `trade.pool_amm_id != default`
  - `trade.mint != default`

Jeśli trade ma rozwiązaną tożsamość, wrapper po prostu robi:

- `trade_event_to_pool_transaction(trade)`
- `tx.send(GhostEvent::pool_transaction(pool_tx))`

Czyli: **brak jakiegokolwiek sprawdzenia, czy ten pool został live wykryty w tej sesji**.

To jest właściwy bypass.

### 3) Fix w `OracleRuntime` działa — ale za późno

W oracle_runtime.rs obecny fix jest widoczny i poprawny:

- unknown `tx-first` pool bez canonical runtime state
- nie tworzy taska
- nie trafia do orphan buffer
- jest ignorowany z metryką:
  - `oracle_runtime_tx_first_unknown_ignored_total`

To tłumaczy, dlaczego:

- w `oracle_decision.log` nie widzisz tego poola,
- nie ma tam `NewPoolDetected`,
- nie ma też jego orphan-spamu.

Czyli sam `OracleRuntime` najpewniej **go nie kanonizuje**.

Problem polega na tym, że zanim event dotrze do `OracleRuntime`, został już:

- wyemitowany przez `Seer` wrapper na event bus,
- zobaczony przez innych subskrybentów runtime.

## Dlaczego temat „wymknął się spod kontroli”

Bo obecnie masz **dwie różne granice prawdy**:

- `OracleRuntime`: „pool jest kanoniczny dopiero po live `NewPoolDetected`”
- `ghost_launcher::components::seer`: „jak trade ma pool + mint, to można go wrzucić na Event Bus”

I te dwie zasady są ze sobą sprzeczne.

W praktyce:
- Oracle mówi „to nie jest canonical pool tej sesji”,
- ale bridge Seera mówi „spoko, i tak puszczam dalej”.

To właśnie dlatego użytkowo widzisz:
- brak śladu `CreatePool`,
- a mimo to mnóstwo dalszych logów runtime/system.

## Co dzieje się dalej z takim eventem

Najbardziej niepokojące jest to, że downstream nie zawsze tylko „ignoruje”.

### `SnapshotListener` nadal to konsumuje

Domyślny tryb to:

- `SnapshotListenerForwardMode::TrackedBuffered`

W snapshot_listener.rs dla `PoolTransaction`:

- jeśli pool nie ma autorytatywnej tożsamości sesyjnej,
- event jest **stage’owany** jako `unknown_pool`

czyli trafia do:

- `staged_transactions: HashMap<Pubkey, VecDeque<...>>`

I z tego co sprawdziłem:
- **nie widać tam TTL/GC/prune** dla tych staged tx-ów,
- replay następuje dopiero gdy pojawi się `NewPoolDetected` / `GatekeeperCommitted`.

To oznacza, że stare poole mogą:
- nie wejść do kanonicznej ścieżki Oracle,
- ale nadal zaśmiecać bridge i lokalne bufory listenera.

To już nie jest tylko problem logów — to jest problem **niechcianego ruchu i pamięci po drodze**.

## Diagnoza końcowa

Źródło problemu to:

### główny root cause
**brak sesyjnego filtra pooli w seer.rs przed emisją `GhostEvent::PoolTransaction` na event bus**

### wtórny root cause
`Seer` core traktuje trade z konkretnym `pool+mint` jako wystarczający do:
- `CURVE_MAP_SET source=trade`
- forwardu trade’a

co jest poprawne dla recovery/parsera, ale **nie może samo w sobie oznaczać „pool wolno wpuścić do launcher runtime tej sesji”**.

### efekt uboczny
`SnapshotListener` buforuje takie nieautorytatywne tx-y bez widocznego TTL/GC.

## Najlepszy plan wykonawczy

Nie rekomenduję ruszać twardo `Seer` core jako pierwszego miejsca cięcia. To nadal byłoby ryzykowne przez kontrakty race-recovery.

Najbezpieczniejszy plan to przeciąć to **na bridge’u launchera**, czyli dokładnie tam, gdzie trade staje się `GhostEvent::PoolTransaction`.

### Etap 1 — wprowadzić sesyjny filtr na bridge `Seer -> EventBus`

W seer.rs:

- utrzymywać lokalny registry/set pooli wykrytych **w tej sesji**
- zasilać go wyłącznie przez:
  - `SeerEvent::PoolDetected`
- **nie** zasilać go przez:
  - `CURVE_MAP_SET source=trade`
  - sam trade

Na ścieżce `SeerEvent::Trade`:

- jeśli `pool_amm_id` nie jest w session registry:
  - **nie emitować** `GhostEvent::PoolTransaction`
  - log + metryka dropu
- jeśli jest:
  - emitować jak dziś

To zatrzyma problem na jednej wspólnej bramce dla wszystkich downstream konsumentów.

### Etap 2 — dodać mały bufor na wyścig `PoolDetected <-> Trade`

Trzeba uważać, żeby nie zabić świeżych pooli, gdzie pierwszy trade wpada minimalnie przed `PoolDetected`.

Dlatego w tym samym bridge’u:

- dla trade’a z nieznanym sesyjnie poolem:
  - nie dropować od razu bezmyślnie,
  - tylko włożyć do **krótkiego bufora oczekiwania**
- jeśli w krótkim oknie pojawi się `PoolDetected` dla tego poola:
  - flush buffered tx-ów na event bus
- jeśli nie:
  - drop

Proponowany rząd wielkości:
- TTL: `1000–2000 ms`
- per-pool cap
- global cap

To pozwala odsiać stare poole, ale nie zabić prawdziwego live race’a.

### Etap 3 — dołożyć TTL/GC dla `SnapshotListener::staged_transactions`

Nawet po naprawie bridge’a warto domknąć drugi problem obronny:

w snapshot_listener.rs:
- dodać TTL dla `staged_transactions`
- dodać cap per-pool i globalny
- okresowy prune
- metryki:
  - staged dropped expired
  - staged dropped capped
  - staged replayed

Bo obecny stan wygląda jak klasyczny „pending queue bez strażnika”. A takie kolejki lubią się rozmnażać nocą.

### Etap 4 — observability, żeby już nigdy nie zgadywać

Dodać jawne metryki/logi:

- `seer_bridge_session_pool_rejected_total`
- `seer_bridge_session_pool_buffered_total`
- `seer_bridge_session_pool_replayed_total`
- `seer_bridge_session_pool_expired_total`

I log rate-limited w stylu:

- pool
- mint
- signature
- source
- reason=`pool_not_detected_in_current_session`

Wtedy kolejny taki przypadek będzie czytelny w 10 sekund, a nie po archeologii logów.

## Konkretne pliki do zmiany

Najbardziej prawdopodobny minimalny zestaw:

- seer.rs
  - session registry
  - short pending buffer
  - drop/flush logic przed `GhostEvent::pool_transaction(...)`

- snapshot_listener.rs
  - TTL/cap/prune dla `staged_transactions`

- config.rs
  - nowe parametry dla bridge buffer:
    - TTL
    - per-pool cap
    - global cap
    - opcjonalnie log rate-limit

- testy w:
  - seer.rs
  - snapshot_listener.rs
  - ewentualnie integracyjne w `ghost-launcher/tests/...`

## Testy, które trzeba obowiązkowo dopisać

### Bridge / session gating

1. **Old pool without `PoolDetected` is never emitted**
   - trade dla poola spoza sesji
   - brak `PoolDetected`
   - wynik: brak `GhostEvent::PoolTransaction`

2. **Live pool emits after `PoolDetected`**
   - `PoolDetected`
   - potem trade
   - wynik: trade idzie na event bus

3. **Trade-before-detect within grace window**
   - trade
   - po chwili `PoolDetected`
   - wynik: trade flushuje się raz

4. **Trade-before-detect expired**
   - trade
   - brak `PoolDetected` do TTL
   - wynik: drop

### SnapshotListener

5. **Unknown pool staged tx expires**
   - staged tx dla poola bez identity
   - po TTL prune
   - wynik: usunięty

6. **No replay for stale old-session junk**
   - brak `NewPoolDetected`
   - staged tx nie siedzi wiecznie

### Regression

7. **OracleRuntime unknown tx-first still ignored**
   - obecny kontrakt zostaje
   - brak orphans dla unknown tx-only pool

## Najważniejsza rekomendacja architektoniczna

**Nie zmieniałbym teraz `Seer` core jako pierwszego ruchu.**  
Najpierw trzeba naprawić **launcherowy bridge**, bo to on łamie sesyjną semantykę.

To daje:
- minimalny blast radius,
- jedną wspólną bramkę dla wszystkich konsumentów event bus,
- zachowanie kontraktów recovery po stronie `Seer`,
- zgodność z Twoim oczekiwaniem:
  - „pool powstały przed sesją ma być dla runtime niewidzialny”.

## Wniosek w jednym zdaniu

Ten pool nie „przebił” fixa w `OracleRuntime`; on **ominął właściwą granicę filtrowania**, bo został wpuszczony wcześniej przez `ghost_launcher::components::seer` na event bus tylko na podstawie `trade + mint`, bez live `CreatePool/NewPoolDetected` w bieżącej sesji.

