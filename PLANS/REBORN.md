Plan napraw krytycznych niezgodności i braków.

# 0. Cel planu

Ten plan nie jest zwykłą listą bugfixów.  
To jest:
# **program odzyskania kontraktów systemu i uproszczenia architektury Gatekeepera**

Po audycie problem nie wygląda już jak:
- pojedyncze bugi parsera,
- trochę gorszy coverage,
- trochę za dużo logów.

Problem wygląda tak:
# **różne warstwy systemu nie zgadzają się, czym jest jedno zdarzenie, kiedy ono zaszło, kiedy jest prawdziwe, kiedy jest zatwierdzone i kto jest właścicielem commit orchestration.**

Dlatego celem nie jest tylko:
- poprawić wskaźniki,
- wyciszyć logi,
- zwiększyć coverage.

Celem jest:
# **przywrócić spójny model prawdy od Seer do ShadowLedgera i usunąć drugiego, architektonicznie szkodliwego Gatekeepera z `ghost-core`.**

---

# 1. Docelowy model architektury

## 1.1. Gatekeeper runtime ma być tylko jeden
Docelowo jedynym prawdziwym Gatekeeperem ma być:

```rust name=ghost-launcher/src/components/gatekeeper.rs
ghost-launcher/src/components/gatekeeper.rs
```

To jest warstwa odpowiedzialna za:
- observation window,
- policy decyzyjną,
- stan `tracked / approved / committed`,
- buffering pre-commit,
- trigger commit,
- routing tx między pre-commit i post-commit,
- współpracę z `OracleRuntime`.

To jest:
# **runtime policy SSOT**

---

## 1.2. `ghost-core` nie może być drugim Gatekeeperem
Plik:

```rust name=ghost-core/src/shadow_ledger/gatekeeper.rs
ghost-core/src/shadow_ledger/gatekeeper.rs
```

docelowo:
# **ma zniknąć**

Ale nie od razu.

Na dziś trzeba go traktować jako:
# **quarantine / transition layer**

To znaczy:
- dopuszczalne są tam tylko stabilizujące bugfixy,
- nie wolno tam dopisywać nowej logiki biznesowej,
- nie wolno tam przenosić kolejnych odpowiedzialności runtime,
- każda nowa semantyka Gatekeepera ma iść do launcherowego Gatekeepera,
- z tego pliku trzeba stopniowo wyciągnąć tylko neutralne domain/storage primitives.

---

## 1.3. `ghost-core` ma być czystą warstwą domain/storage
Po migracji w `ghost-core/shadow_ledger/*` powinny zostać wyłącznie:
- `ShadowLedger`
- `LivePipeline`
- `TxKey`
- `TradeSnapshot`
- append/merge rules
- domain math
- neutral structures wynikowe / snapshotowe / replay helpers

Czyli:
# zero runtime policy
# zero observation semantics
# zero approval state machine
# zero commit orchestration behavior

---

# 2. Nadrzędne kontrakty, które plan ma przywrócić

Te zasady muszą obowiązywać po zakończeniu programu napraw.

## 2.1. Event identity contract
- `1 signature != 1 trade`
- pojedynczy signature może legalnie zawierać wiele semantic trade events
- canonical identity eventu musi uwzględniać coś więcej niż signature:
  - `tx_index`
  - lub `event_ordinal`
  - plus stabilny `TxKey`

---

## 2.2. Timestamp contract
- `timestamp_ms` = canonical event-time
- `arrival_ts_ms` = telemetry / latency / fallback-only
- arrival time nie może być primary ordering axis dla canonical path

---

## 2.3. Status contract
- `tracked != approved != committed`
- `approved` nie może być traktowane jak canonical truth
- `committed` oznacza wyłącznie:
  - canonical history została realnie zapisana
  - i commit success jest potwierdzony przez wynik operacji, a nie przez lokalne założenie

---

## 2.4. Commit truthfulness contract
- system nie może zachowywać się “jak po sukcesie”, jeśli commit nie został zapisany
- `pending_live` nie może żyć na fałszywym commicie
- bootstrap `LivePipeline` nie może zależeć od kruchych readbacków / identity gate w ścieżce post-commit

---

## 2.5. Single authoritative ingest contract
- `SnapshotEngine` nie może mieć dwóch konkurencyjnych authoritative ingressów dla tego samego tx
- enrichment jest dozwolony
- konkurencyjny drugi writer path — nie

---

## 2.6. Mapping integrity contract
- `mint=111111...` nie może wyjść poza unresolved/staging path
- unresolved trade może być buforowany
- unresolved trade nie może być emitowany jako poprawny downstream trade identity

---

# 3. Zasady prowadzenia prac

To jest bardzo ważne, bo bez tego znowu zrobi się chaos.

## 3.1. Najpierw correctness, potem migracja, potem cleanup
Kolejność faz:
1. **Correctness repair**
2. **Migration / extraction**
3. **Deletion / transport / performance**

Nie odwrotnie.

---

## 3.2. Nie wolno maskować problemów poprawą transportu albo logów
Transport i log storm są ważne,
ale:
# nie mogą przykryć kontraktowych błędów wejścia i commitu

Dlatego idą później.

---

## 3.3. PR-y muszą mieć bardzo czyste granice semantyczne
To jest jedna z moich najważniejszych korekt.

Przykład:
- PR-4 i PR-5 dotykają podobnego obszaru,
ale nie mogą znaczyć tego samego

### Granica ma być twarda:
- **PR-4 = runtime semantics / policy / SSOT launcher Gatekeeper**
- **PR-5 = hardening przejściowej warstwy commit/core, bez rozwijania jej roli**

---

## 3.4. Extraction nie może utrwalić złej architektury
To była moja trzecia ważna uwaga.

Jeśli w extraction wrzucisz do “neutral core types” rzeczy, które wcale nie są neutralne,
to:
# zakonserwujesz złą architekturę pod nową nazwą

Dlatego już teraz ustalamy:
### neutralne:
- `TxKey`
- `CommitResult`
- `BufferedTx` bez runtime policy
- `ReconstructedState`
- czyste helpery budujące/replayujące
- typy snapshotowe
- append result / merge result

### nieneutralne:
- approval logic
- observation windows
- tracking policy
- ownership `pending_live`
- commit trigger rules
- pre/post-commit routing
- gatekeeper state machine
- runtime heurystyki

---

# 4. Faza I — Correctness Repair

To jest faza, która ma zatrzymać fałszowanie danych i coverage loss zanim ruszy migracja Gatekeepera.

Fazę tę obejmujemy:
- PR-1
- PR-2
- PR-3a
- PR-3b

---

# 5. PR-1 — `seer-parser-contract`

## Tytuł roboczy
# **Seer / parser contract: canonical CPI trade semantics, multi-event transaction identity, and clean producer output**

---

## 5.1. Po co istnieje ten PR
To jest pierwszy punkt planu, bo parser i producer są dziś głównym źródłem skażonych danych wejściowych.

Na dziś audyt potwierdził, że parser potrafi:
- odwrócić semantic side named `CpiSwapBuy/CpiSwapSell`,
- wyprodukować trade oparty o inferred `sol_amount`,
- używać `arrival_ts` jako `timestamp_ms`,
- budować kilka konkurencyjnych interpretacji dla jednego tx,
- nie mieć pełnego kontraktu dla wielu trade events pod jednym signature.

To oznacza, że już na wejściu pipeline możecie mieć:
- side-flipy,
- zero-volume outputs,
- phantom trades,
- złe timestampy,
- niekanoniczną tożsamość zdarzenia.

Ten PR ma:
# **zamknąć producer-level corruption**

---

## 5.2. Zakres plików
- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/types.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`

Opcjonalnie, jeśli wymagane przez kontrakt:
- drobne korekty w strukturach przekazywanych dalej do runtime.

---

## 5.3. Co dokładnie ma zostać naprawione

### A. Named CPI side jest autorytatywne
Dla:
- `CpiSwapBuy`
- `CpiSwapSell`

inference:
- może pomagać w resolve metadata,
- może ewentualnie pomagać w rozumieniu kontekstu,
- ale:
# nie może nadpisywać semantic side named eventu

---

### B. `sol_amount` nie może być czystą fantazją inference
SOL amount dla canonical trade output:
- ma pochodzić z eventu kanonicznego
- albo z runtime context, jeśli to naprawdę uzasadnione i bezpieczne
- ale nie z “brak danych, więc dajmy inferred / zero / weak guess”

W praktyce:
# nie wolno dopuścić do finalnego canonical output z `volume_sol=0`, jeśli trade jest realnym swapem

---

### C. `timestamp_ms` nie może być już `arrival_ts`
Parser nie może emitować canonical trade timestamp jako transportowego arrival time,
jeśli istnieje lepsze event-time source.

To jest kluczowe, bo inaczej cały downstream dalej będzie żył na fałszywej osi czasu.

---

### D. Trzeba wprowadzić `tx_index` albo `event_ordinal`
To jest fundament pod cały późniejszy `TxKey`.

Ponieważ:
# `1 signature != 1 trade`

parser lub producer path musi zacząć emitować dodatkowy identyfikator eventu w obrębie transakcji.

To nie może zostać odłożone na później, bo bez tego downstream nadal będzie kusiło do signature-only dedup.

---

### E. `mint=111111...` nie może zostać wypuszczony jako poprawny trade identity
Jeśli parser albo producer ma unresolved mapping,
to taki event:
- ma być oznaczony unresolved,
- ma zostać w staging/buffer path,
- ale nie może wyjść jako normalny downstream trade na defaultowym garbage mint.

---

## 5.4. Czego ten PR nie robi
Żeby nie zrobić z niego worka bez dna:

Ten PR:
- **nie** rozwiązuje jeszcze RC6 end-to-end,
- **nie** naprawia `SnapshotEngine`,
- **nie** migruje Gatekeepera,
- **nie** naprawia commit loop,
- **nie** rusza transportu/backfill,
- **nie** robi finalnego single-ingress contract.

On wyłącznie:
# naprawia producenta i kontrakt emitowanego trade/event payload.

---

## 5.5. Testy obowiązkowe
- `same_signature_multi_trade_survives_parser`
- `cpi_swap_contract_preserves_side_sol_time`
- `default_mint_never_forwarded`
- regresja na:
  - side flip
  - zero-volume
  - phantom output
  - `arrival_ts` as primary time

---

## 5.6. Merge gate
Nie merge’ować, dopóki:
- named CPI side da się jeszcze odwrócić przez inference,
- parser dalej produkuje `arrival_ts` jako canonical time,
- downstream payload nie niesie `tx_index/event_ordinal`,
- legalne multi-trade under same signature wciąż się zapadają w jedno.

---

# 6. PR-2 — `seer-mapping-rc6-replay`

## Tytuł roboczy
# **Seer mapping / RC6 / replay wiring: unresolved trades, delayed account recovery, and mapping-safe replay**

---

## 6.1. Po co istnieje ten PR
Po naprawie parsera nadal możesz tracić lub zatruwać eventy,
jeśli mapping/replay path jest popsuty.

Audyt potwierdził:
- `pending_curve_updates + queue_curve_mint_resolve` nadal są half-wired,
- `DelayedAccountQueue` nie jest realnym hot path,
- `mint=111111...` potrafi wyciekać poza unresolved buffer,
- replay po mappingu nie jest zamknięty kontraktowo.

Czyli:
# nawet poprawnie sparsowany tx może potem zostać popsuty lub zgubiony na etapie mappingu

Ten PR ma:
# **domknąć unresolved/mapping/replay contract po stronie Seer**

---

## 6.2. Zakres plików
- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/grpc_connection.rs`

Jeśli konieczne:
- minimalne helpery config / replay utilities.

---

## 6.3. Co dokładnie ma zostać naprawione

### A. `mint=111111...` zostaje w unresolved path
To jest twardy invariant.

Jeżeli mapping nie jest gotowy:
- event nie wychodzi jako normalny downstream payload,
- tylko trafia do unresolved/staging/replay path.

---

### B. Replay po mappingu ma być pojedynczy i deterministyczny
Po pojawieniu się mappingu:
- unresolved trade ma replayować się raz,
- poprawnie,
- bez duplikatów,
- bez utraty event identity.

---

### C. Delayed account recovery ma być realnym hot path
`DelayedAccountQueue` / podobna ścieżka nie może być tylko dekoracją.
Jeśli account update przychodzi przed mappingiem/create,
system musi potrafić:
- go zachować,
- skorelować,
- zreplayować po zamknięciu zależności.

---

### D. `pending_curve_updates` nie może zostać półmartwym bokiem systemu
Albo:
- staje się częścią prawdziwego replay path,
albo:
- zostaje zastąpione przez coś spójniejszego.

Ale nie może być tak, że architektura “na papierze obsługuje race”, a hot path realnie z niej nie korzysta.

---

## 6.4. Czego ten PR nie robi
- nie rusza jeszcze `SnapshotListener`
- nie usuwa double-ingress do `SnapshotEngine`
- nie naprawia runtime Gatekeepera
- nie rusza commit loop
- nie rusza core Gatekeepera

Ten PR zamyka tylko:
# mapping / unresolved / replay contract w Seer

---

## 6.5. Testy obowiązkowe
- `trade_before_create_replays_once`
- `account_update_before_mapping_replays`
- `mint_111_never_reaches_ipc`

Dodatkowo warto:
- test na replay bez duplikacji przy overlapping arrivals,
- test na preserve `tx_index/event_ordinal` po replay.

---

## 6.6. Merge gate
Nie merge’ować, dopóki:
- `mint=111111...` da się jeszcze wypchnąć poza unresolved path,
- replay po mappingu może dać duble,
- account update before mapping nadal ginie zamiast być odzyskany.

---

# 7. PR-3a — `snapshot-listener-buffering-and-mapping`

To jest właśnie miejsce, gdzie uwzględniam moją wcześniejszą sugestię:
# **rozdzielić stary szeroki PR-3**
na dwa mniejsze, bo inaczej zmiesza się coverage fix z ingress refactorem.

---

## Tytuł roboczy
# **Snapshot listener buffering and mapping safety: stop unknown_pool hard drops before SnapshotEngine**

---

## 7.1. Po co istnieje ten PR
Audyt pokazał bardzo mocno, że:
- największy confirmed coverage loss downstream
- dzieje się zanim `SnapshotEngine` w ogóle dostanie szansę pracować

Konkretnie:
- `TrackedBuffered`
- `unknown_pool`
- listener filtering
- mapping-unready events

powodują, że duża część tx:
# jest odcinana jeszcze przed wejściem do engine

To jest bardzo zły punkt straty, bo:
- późniejsze replay/recovery mogą już nie mieć czego odzyskiwać,
- coverage counters rozjeżdżają się drastycznie,
- system wygląda jakby engine albo ledger zjadał dane, chociaż często one nigdy tam nie docierają.

---

## 7.2. Zakres plików
- `ghost-launcher/src/components/snapshot_listener.rs`

Pomocniczo, jeśli konieczne:
- drobne powiązane elementy staging/mapping path.

Ale:
# ten PR ma być możliwie skoncentrowany na listenerze i bufferingu

---

## 7.3. Co dokładnie ma zostać naprawione

### A. `unknown_pool` nie może być hard dropowany
Jeśli tx jest sensowny, ale pool identity nie jest jeszcze znane,
to:
- nie drop
- tylko buffer/staging/unresolved path

To jest główny cel tego PR.

---

### B. Listener nie może traktować braku identity jak ostatecznej negacji wartości tx
Brak identity w tej chwili ≠ “ten tx jest bezwartościowy”.

To oznacza tylko:
- “jeszcze nie mamy kompletnego mappingu”.

---

### C. Listener ma forwardować do ścieżki odzyskiwalnej
To nie musi od razu oznaczać pełnego SnapshotEngine accept,
ale musi znaczyć:
- event nie znika,
- może zostać później zmapowany / zreplayowany / ponownie oceniony.

---

### D. Listener nie może przepuszczać garbage identity
Jeśli event niesie default mint / unresolved tożsamość,
to:
- nie idzie jako finalny poprawny downstream event,
- tylko do bezpiecznej ścieżki stagingowej.

---

## 7.4. Czego ten PR nie robi
I to ważne:
- nie rozwiązuje jeszcze double-ingress
- nie robi single authoritative tx ingress
- nie przebudowuje dedup contractu `SnapshotEngine`
- nie rusza runtime Gatekeepera

To celowo:
# coverage fix ma być odseparowany od ingress refactoru

---

## 7.5. Testy obowiązkowe
- `tracked_buffered_unknown_pool_is_buffered`
- `default_mint_never_forwarded`
- `tx_before_pool_identity_is_staged_not_dropped`

---

## 7.6. Merge gate
Nie merge’ować, dopóki:
- listener nadal może hard dropować `unknown_pool`,
- unresolved tx może zniknąć zamiast trafić do recoverable path,
- default mint może przejść listener jako normalny trade identity.

---

# 8. PR-3b — `snapshot-engine-single-ingress-and-dedup-contract`

## Tytuł roboczy
# **SnapshotEngine ingress contract: single authoritative tx path and multi-event-safe dedup**

---

## 8.1. Po co istnieje ten PR
To jest druga połowa starego szerokiego PR-3.

Po tym, jak PR-3a naprawi coverage loss w listenerze,
trzeba naprawić problem głębszy:
# `SnapshotEngine` nie może dostawać tego samego tx dwoma konkurencyjnymi ścieżkami
i nie może dedupować po samym signature.

Bo wtedy:
- biedniejszy event może wygrać z bogatszym,
- drugi legalny trade pod tym samym signature może zginąć,
- engine staje się źródłem jakościowej degradacji, nawet jeśli coverage już wzrosło.

---

## 8.2. Zakres plików
- `ghost-brain/src/oracle/snapshot_engine.rs`
- `ghost-launcher/src/oracle_runtime.rs`

Ewentualnie minimalnie:
- `snapshot_listener.rs`, jeśli trzeba domknąć authoritativeness źródła.

---

## 8.3. Co dokładnie ma zostać naprawione

### A. Jeden authoritative tx ingress do `SnapshotEngine`
Musisz wskazać jedną ścieżkę, która jest źródłem prawdy dla tx ingest do engine.

Druga ścieżka:
- może enrichować,
- może dostarczać metadata/reserves/context,
- ale nie może być konkurencyjnym writerem tx.

---

### B. Koniec signature-only dedup
Engine i inactive buffer nie mogą zakładać:
- “ten signature już był, więc drugie zdarzenie to dubel”

To jest wprost niezgodne z kontraktem `1 signature != 1 trade`.

---

### C. Enrichment path nie może przegrywać z first-arrival poverty path
Jeśli biedniejszy event dojdzie pierwszy,
to system nie może przez to utracić:
- pełniejszego reserve context,
- lepszej ceny,
- lepszego snapshotu,
- poprawniejszego semantic event payload.

---

## 8.4. Czego ten PR nie robi
- nie zmienia już parsera
- nie dotyka RC6
- nie naprawia commit semantics
- nie ustala jeszcze finalnie launcher/core migration

To jest:
# ingress + dedup correctness tylko dla SnapshotEngine

---

## 8.5. Testy obowiązkowe
- `snapshot_engine_does_not_drop_second_trade_same_signature`
- `single_ingress_wins`
- `enriched_tx_path_does_not_lose_to_poorer_duplicate_path`

---

## 8.6. Merge gate
Nie merge’ować, dopóki:
- engine ma dwa konkurencyjne writer pathy,
- legalny drugi trade z tym samym signature nadal może zostać skasowany,
- enriched path nadal może przegrywać z gorszym first-arrival eventem.

---

# 9. Podsumowanie Iteracji 1/3

Po tej fazie system **nie będzie jeszcze docelowy**, ale powinien przestać robić najgorsze rzeczy na wejściu.

Po zakończeniu:
- parser ma emitować lepszy, mniej skażony kontrakt,
- unresolved mapping nie ma przeciekać jako garbage identity,
- listener nie ma brutalnie ucinać unknown_pool,
- `SnapshotEngine` nie ma mieć dwóch konkurencyjnych ingressów,
- signature-only collapse ma zostać wyeliminowany co najmniej w pierwszych kluczowych miejscach.

To jest:
# **odzyskanie poprawności wejścia i ingestu**
przed wejściem w runtime Gatekeeper / commit semantics / migration.

---
## Faza II / Runtime SSOT + Commit Hardening (PR-4 i PR-5)**

To jest najdelikatniejsza część całego programu naprawy, bo tutaj łatwo ponownie pomieszać:
- runtime policy,
- canonical truth,
- commit orchestration,
- odpowiedzialność launchera,
- odpowiedzialność core.

I dokładnie tutaj uwzględniam Twoją uwagę oraz moje wcześniejsze zastrzeżenie:

# **PR-4 i PR-5 częściowo dotykają tego samego kontraktu, ale nie mogą robić tej samej rzeczy.**
Muszą mieć twardą granicę:

- **PR-4 = semantyka runtime / policy / SSOT launcher Gatekeeper**
- **PR-5 = stabilizacja przejściowej warstwy commit/core, bez rozwijania jej roli**

To jest absolutnie kluczowe.

---

# 1. Cel Fazy II

Po Fazi I wejście systemu powinno być mniej skażone:
- parser przestaje kłamać o side/time/volume,
- unresolved mapping nie wypływa jako garbage,
- `SnapshotListener` nie ucina tak brutalnie,
- `SnapshotEngine` ma bardziej sensowny ingest contract.

Ale to **nie wystarczy**, bo nadal możecie mieć rozpad semantyki w środku runtime:

- `approved` mieszane z `committed`
- runtime żyjący na arrival-time, a ledger na event-time
- commit loop zachowujący się jak po sukcesie mimo odrzuconego zapisu
- bootstrap post-commit zależny od identity readbacków
- `pending_live` oparte o fałszywy commit

Dlatego Faza II ma dwa cele równoległe:

## Cel A
# ustanowić launcherowy Gatekeeper jako jedyny runtime SSOT

## Cel B
# zabezpieczyć przejściową warstwę commit/core tak, żeby przestała fałszować sukces commitów

To nie jest jeszcze migracja usuwająca core gatekeepera.
To jest:
# **ustanowienie poprawnej semantyki zanim zacznie się właściwa ekstrakcja i kasacja**

---

# 2. Najważniejsza zasada tej fazy

Jeszcze raz, bo to jest rdzeń:

## PR-4 nie może zacząć leczyć core-gatekeepera przez dokładanie mu nowej polityki
bo wtedy:
- utrwalicie zły podział odpowiedzialności,
- a późniejsza migracja będzie cięższa.

## PR-5 nie może zacząć przenosić całej logiki do launchera
bo wtedy:
- nie będzie wiadomo, czy to hardening, czy już migracja,
- a zachowanie systemu może się zmieniać zbyt szeroko naraz.

Czyli:

### PR-4 odpowiada na pytanie:
# **“co jest prawdą runtime’ową i jak launcher ma to rozumieć?”**

### PR-5 odpowiada na pytanie:
# **“jak sprawić, żeby przejściowa warstwa commit/core przestała kłamać o sukcesie i przestała blokować post-commit flow?”**

---

# 3. PR-4 — `runtime-gatekeeper-ssot`

## Tytuł roboczy
# **Runtime Gatekeeper SSOT: tracked/approved/committed semantics, canonical event-time, and policy ownership in launcher**

---

## 3.1. Po co istnieje ten PR
Ten PR ma zrobić z launcherowego Gatekeepera:
# **jedyny prawdziwy runtime policy owner**

Dzisiaj problem polega na tym, że:
- runtime i commit/core mieszają statusy,
- `approved` bywa używane jak substytut `committed`,
- arrival-time bywa traktowany jak canonical ordering axis,
- dedup/order w runtime nie są spójne z kontraktem multi-event tx,
- routing pre/post-commit nie ma jednego właściciela semantyki.

Bez naprawy tego poziomu:
- nawet poprawniejsze tx z parsera,
- nawet lepszy ingest,
- nadal będą trafiały do runtime, który nie wie spójnie:
  - co obserwuje,
  - co zatwierdził,
  - co naprawdę jest committed,
  - kiedy tx należy do pre-commit,
  - kiedy należy do post-commit.

Ten PR ma:
# **ustanowić semantykę runtime bez ruszania jeszcze migracji commit orchestration**

---

## 3.2. Zakres plików
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/oracle_runtime.rs`

Opcjonalnie:
- lekkie korekty typów/eventów, jeśli są wymagane do domknięcia kontraktu runtime.

Ale:
# nie dokładamy tu nowej logiki do `ghost-core/src/shadow_ledger/gatekeeper.rs`

---

## 3.3. Główne odpowiedzialności tego PR

### A. Zdefiniować jednoznacznie `tracked`, `approved`, `committed`
To musi zostać zapisane nie tylko w komentarzach, ale w realnych inwariantach kodu.

#### `tracked`
oznacza:
- runtime zna pool/tx jako obserwowany kandydat
- może go buforować / śledzić / oceniać
- ale nie ma jeszcze ani canonical commit, ani gwarancji aktywacji post-commit

#### `approved`
oznacza:
- runtime policy uznała pool/flow za dopuszczony do dalszego toku
- ale to:
# nie znaczy, że canonical history została zapisana

#### `committed`
oznacza:
- canonical history jest realnie zapisana
- post-commit path może zostać aktywowany
- to jest jedyny stan, który może uruchamiać pełną semantykę canonical/live transition

To rozróżnienie musi być twarde.

---

### B. Usunąć traktowanie `approved` jako zamiennika `HardTruth`
To jest jeden z najważniejszych problemów z audytu.

Jeśli runtime lub listener traktuje `approved` jak “już można ufać jak kanonicznej prawdzie”,
to:
# system produkuje fałszywy SSOT

Ten PR ma sprawić, że:
- `approved` jest wyłącznie stanem runtime policy,
- nie stanem canonical history,
- nie triggerem do udawania, że commit już semantycznie zaszedł.

---

### C. Ustalić canonical ordering axis w launcherowym Gatekeeperze
Ten PR ma zakończyć sytuację, w której runtime bierze arrival-time jako domyślną oś prawdy.

Rekomendacja pozostaje ta sama:
- `timestamp_ms` = primary event-time axis
- `arrival_ts_ms` = telemetry / fallback / SLA

Czyli:
# runtime Gatekeeper ma myśleć na tej samej osi czasu co docelowy canonical path

---

### D. Ujednolicić runtime dedup / order z kontraktem multi-event tx
Po PR-1 parser powinien już nieść `tx_index`/`event_ordinal` albo odpowiednik.
PR-4 ma doprowadzić do tego, że launcherowy Gatekeeper:
- nie dedupuje legalnych drugich eventów po samym signature,
- nie zapada multi-event tx do jednej jednostki,
- rozumie `PoolTransaction` zgodnie z nowym `TxKey`.

---

### E. Ustalić właściciela semantyki pre-commit vs post-commit
Runtime Gatekeeper ma być warstwą, która:
- decyduje, czy tx jest jeszcze częścią observation/pre-commit,
- czy już ma trafić do canonical/post-commit routing.

To nie znaczy jeszcze, że launcher wykonuje cały commit orchestration — to przyjdzie później.
Ale:
# launcher ma już być właścicielem semantyki tego przejścia

---

## 3.4. Czego ten PR celowo nie robi

To jest ważne, żeby nie zrobić z PR-4 wszystkiego naraz.

PR-4:
- **nie** naprawia `commit_history()` w core
- **nie** usuwa core gatekeepera
- **nie** przenosi `pending_live` ownership do launchera
- **nie** buduje jeszcze launcherowego commit coordinatora
- **nie** zmienia głęboko `LivePipeline` bootstrap path
- **nie** robi extraction neutral types

Czyli:
# PR-4 definiuje semantykę runtime policy, ale nie robi jeszcze pełnej migracji komitu

---

## 3.5. Największe ryzyko PR-4
Największe ryzyko polega na tym, że ktoś spróbuje “dla wygody” zacząć leczyć problemy commitu poprzez dopisywanie kolejnych zachowań do runtime Gatekeepera, które tak naprawdę należą do migracji albo do PR-5.

Dlatego:
# trzeba pilnować, by PR-4 nie stał się ukrytym commit refactorem

Jeżeli w review pojawią się rzeczy typu:
- nowe workaroundi wokół `commit_history`
- dodatkowe mostki do core gatekeepera
- nowe tymczasowe heurystyki bootstrapu `LivePipeline`
to trzeba je odrzucać albo przenieść do PR-5 / późniejszej migracji.

---

## 3.6. Testy obowiązkowe
- `approved_not_equal_committed`
- `event_time_primary_in_runtime_gatekeeper`
- `multi_event_same_signature_not_deduped`

Dodatkowo warto:
- test na out-of-order arrival przy stałym canonical event-time ordering
- test na brak HardTruth semantics dla samego `approved`

---

## 3.7. Merge gate
Nie merge’ować, dopóki:
- `approved` nadal wycieka jako semantyczny substytut `committed`
- runtime gatekeeper nadal primary-orderuje po `arrival_ts_ms`
- multi-event same-signature nadal może zostać zredukowany do jednego eventu
- pre/post-commit semantics nadal są rozmyte albo podwójnie interpretowane

---

# 4. PR-5 — `shadowledger-commit-hardening`

## Tytuł roboczy
# **ShadowLedger commit hardening: truthful commit result, no false committed state, and bootstrap from real commit output**

---

## 4.1. Po co istnieje ten PR
To jest PR, który ma ustabilizować warstwę przejściową:
- `ghost-core/src/shadow_ledger/gatekeeper.rs`
- `ledger.rs`
- commit loop
- bootstrap post-commit

Najważniejszy problem, który audyt wykazał, jest bardzo ciężki:

- `commit_history()` może odrzucić zapis
- a kawałek dalej runtime/local flow zachowuje się tak, jakby commit się udał

To oznacza:
# system potrafi skłamać o stanie kanonicznym

I to trzeba zatrzymać natychmiast.

Ten PR ma:
# **naprawić prawdomówność commit path, ale bez rozwijania roli core gatekeepera**

---

## 4.2. Zakres plików
- `ghost-core/src/shadow_ledger/ledger.rs`
- `ghost-core/src/shadow_ledger/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/oracle_runtime.rs`

I tu bardzo ważna zasada:
# tylko stabilizacja i hardening
# żadnego “a może jeszcze dołóżmy tam nową inteligencję”

---

## 4.3. Główne odpowiedzialności tego PR

### A. `commit_history()` musi zwracać wynik semantyczny
Nie wystarczy “wywołać commit i założyć, że jakoś poszło”.

Commit musi zwracać wynik, który pozwala runtime rozstrzygnąć:
- sukces persisted commit,
- odrzucenie,
- brak zmiany,
- ewentualne szczegóły potrzebne do dalszego bootstrapu.

Ten wynik ma być:
- jawny,
- strukturalny,
- używany przez commit loop,
- a nie zgadywany z logów albo wtórnego readbacku.

---

### B. `committed=true` tylko po realnym persisted commit
To jest główny invariant tego PR.

Jeżeli zapis do canonical history nie nastąpił,
to:
- lokalny bufor nie może zostać oznaczony committed,
- runtime nie może wejść w success path,
- `pending_live` nie może być traktowany jak po sukcesie.

---

### C. Commit loop ma działać na wyniku commitu, nie na gated readback
Obecnie z audytu wynika, że post-commit flow bywa zależny od:
- identity lookup,
- gated read path do snapshotów,
- wtórnych prób ustalenia, czy commit “chyba się udał”.

To jest kruche i błędne.

Po tym PR:
# commit loop ma korzystać bezpośrednio z commit result
czyli z informacji zwróconej przez udany commit.

To ogranicza:
- zależność od `PoolIdentityRegistry`
- zależność od gated `get_snapshots()`
- zależność od chwilowego stanu zewnętrznych lookupów

---

### D. Brak fałszywego `pending_live` po odrzuconym commicie
`pending_live` ma sens tylko wtedy, gdy commit path ma rzeczywiste przejście do canonical/post-commit.

Jeśli commit odrzucono,
to nie można zostawić systemu w stanie:
- “no commit, but maybe live path already semantically armed”.

To jest bardzo ważne, bo inaczej po commit failure dostajesz dziwne half-live stany.

---

### E. Canonical commit nie może być blokowany przez approval/identity heurystyki
W aktualnym stanie approval checker / identity gate potrafią mieszać się w rzeczy, które powinny dotyczyć samego zapisu canonical history.

Ten PR ma ograniczyć to tak, żeby:
- approval/identity mogły wpływać na niektóre ścieżki odczytu lub aktywacji,
- ale nie fałszowały samego wyniku commit operation.

Uwaga:
to nie jest jeszcze pełne architektoniczne rozwiązanie.
To jest:
# stabilizacja, żeby commit path przestał kłamać i przestał być kruchy

---

## 4.4. Czego ten PR celowo nie robi
I to jest krytyczne.

PR-5:
- **nie** ustanawia launchera jako pełnego commit coordinatora
- **nie** migruje ownership `pending_live` do launchera
- **nie** usuwa core gatekeepera
- **nie** przenosi observation logic do core albo odwrotnie
- **nie** dokłada nowej polityki biznesowej do `ghost-core` gatekeepera

To musi być:
# naprawa warstwy przejściowej
a nie jej dalsza rozbudowa

---

## 4.5. Największe ryzyko PR-5
Największe ryzyko jest odwrotne niż w PR-4:
że ktoś pod pretekstem hardeningu zacznie “doprojektowywać” core-gatekeepera jako trwałą warstwę systemu.

To byłby błąd.

Przykłady rzeczy, których nie wolno tu robić:
- dodawanie nowych runtime heurystyk approval
- rozbudowa state machine w core
- dokładanie nowych semantycznych trybów gatekeeper registry
- przenoszenie tam policy ownership “bo łatwiej”

Jeżeli coś takiego jest potrzebne,
to znaczy, że należy do:
- PR-4,
- albo przyszłego PR-7,
a nie do PR-5.

---

## 4.6. Testy obowiązkowe
- `gatekeeper_does_not_mark_committed_on_commit_failure`
- `commit_loop_bootstraps_live_pipeline_from_commit_result`
- `no_false_pending_live_after_rejected_commit`

Dodatkowo warto:
- test na brak zależności bootstrapu od chwilowo brakującego `PoolIdentityRegistry`
- test na brak “success path without persisted history”

---

## 4.7. Merge gate
Nie merge’ować, dopóki:
- da się uzyskać `committed=true` mimo odrzuconego commitu
- commit loop nadal używa gated readback jako głównego źródła prawdy
- `pending_live` zostaje uzbrojone mimo braku persisted commit
- core gatekeeper dostaje nowe odpowiedzialności runtime/policy

---

# 5. Precyzyjna granica między PR-4 a PR-5

To jest sekcja, którą warto potem niemal przepisać do opisu issue/PR review checklist.

## 5.1. Co należy do PR-4
PR-4 odpowiada za:
- definicję runtime semantics
- statusy `tracked/approved/committed`
- canonical ordering w launcherze
- runtime dedup/order
- routing policy
- kto jest runtime SSOT

Czyli:
# pytanie “jak launcher rozumie świat?”

---

## 5.2. Co należy do PR-5
PR-5 odpowiada za:
- prawdomówność commit result
- brak false commit
- bezpieczny bootstrap po sukcesie
- odcięcie od kruchych gated readbacków
- ograniczenie szkód w warstwie przejściowej

Czyli:
# pytanie “jak przestać kłamać o stanie canonical commitu, zanim ta warstwa zostanie wycięta?”

---

## 5.3. Czego nie wolno mieszać
### Nie wolno w PR-4:
- naprawiać core gatekeepera przez rozbudowę jego API
- wprowadzać nowych core side effects
- łatać commit failure workarounds kosztem semantyki runtime

### Nie wolno w PR-5:
- decydować na nowo, czym jest `approved`
- przenosić policy ownership do core
- ustanawiać core jako długoterminowy owner commit windows
- budować tam semantyki docelowego SSOT

---

# 6. Co powinno być prawdą po zakończeniu Fazy II

Po PR-4 i PR-5 razem system nadal nie będzie jeszcze docelowo uproszczony,
ale powinien przestać robić dwie najbardziej niebezpieczne rzeczy:

## A. Runtime nie miesza już statusów i czasu
- `approved != committed`
- event-time jest primary
- multi-event tx nie zapada się przez runtime policy layer

## B. Commit path nie kłamie
- brak local success po commit failure
- brak fałszywego bootstrapu live
- commit result staje się prawdziwym nośnikiem sukcesu/porażki

To daje fundament, na którym można bezpiecznie robić:
- extraction
- launcher commit coordinator
- usunięcie core gatekeepera

---

## 7. Podsumowanie fazy III

Faza II jest najtrudniejsza nie dlatego, że ma najwięcej linii kodu,
ale dlatego, że:
# tutaj najłatwiej ponownie pomylić role launchera i core

Dlatego finalna zasada brzmi:

- **PR-4** buduje launcherowy Gatekeeper jako jedyny runtime SSOT
- **PR-5** tylko stabilizuje przejściowy commit/core path, żeby przestał kłamać
- żaden z nich nie może stać się pół-migracją drugiego

Jeśli ta granica zostanie utrzymana,
to potem da się bezpiecznie zrobić extraction i kasację core gatekeepera.

---



# FAZA III — Extraction, Migration, Deletion, Operational Cleanup + globalne warunki akceptacji dla wszystkich 3 etapów**

To domyka cały program.  
Po tej odpowiedzi będziesz mieć:
1. **Fazę III**:
   - PR-6
   - PR-7
   - PR-8
   - PR-9
2. **rollout i shadow-run**
3. **twarde warunki usunięcia `ghost-core` gatekeepera**
4. **ogólne warunki akceptacji dla Etapu I, II i III**
5. **końcowy definition of done dla całego programu**

---

# 1. Cel Fazy III

Po Etapie I i II system powinien już:
- przestać psuć wejście parserem,
- przestać wyciekać `mint=111111...`,
- przestać dropować `unknown_pool` zbyt wcześnie,
- przestać collapse’ować legalne multi-event tx,
- mieć sensowną semantykę runtime statusów,
- przestać kłamać o sukcesie commitów.

Ale nadal będzie miał:
- przejściową warstwę `ghost-core/src/shadow_ledger/gatekeeper.rs`,
- nie do końca czysty podział runtime/core,
- stare zależności importów i stary graph odpowiedzialności,
- jeszcze niezamknięty temat transportu i log pressure.

Faza III ma:
# **usunąć architektoniczny relikt, przenieść ostateczne ownership do launchera i domknąć operacyjną stabilizację systemu**

To jest faza:
- ekstrakcji,
- migracji,
- kasacji,
- i dopiero na końcu tuningów operacyjnych.

---

# 2. PR-6 — `extract-neutral-core-types`

## Tytuł roboczy
# **Extract neutral core types from `shadow_ledger/gatekeeper` and reduce it to a thin adapter**

---

## 2.1. Po co istnieje ten PR
To jest pierwszy krok właściwej migracji.
Nie przenosi jeszcze ownership runtime behavior,
ale:
# odchudza core gatekeepera do roli przejściowego adaptera

To ważne, bo nie da się bezpiecznie usunąć `ghost-core` gatekeepera, jeśli:
- jest jednocześnie kontenerem neutralnych typów,
- helperów domenowych,
- i logiki przejściowej.

Najpierw trzeba wyjąć z niego wszystko, co:
- nie jest policy,
- nie jest runtime state machine,
- nie powinno umrzeć razem z tym plikiem.

---

## 2.2. Zakres plików
- `ghost-core/src/shadow_ledger/gatekeeper.rs`
- `ghost-core/src/shadow_ledger/mod.rs`

oraz import cleanup w:
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/config.rs`

Możliwe także drobne korekty w:
- `trade_types.rs`
- `live_pipeline.rs`
- innych plikach `shadow_ledger/*`, jeśli typy powinny trafić tam zamiast siedzieć w relikcie gatekeepera.

---

## 2.3. Co dokładnie ma zostać wyciągnięte
Tu wracam do jednego z moich kluczowych zastrzeżeń:
# extraction może przypadkiem utrwalić złą architekturę, jeśli neutralne typy nie zostaną dobrze zdefiniowane

Dlatego trzeba już na starcie rozróżnić:

### Typy neutralne — mogą zostać w core
Przykładowo:
- `TxKey`
- `CommitResult`
- `CommitFailureReason`
- `BufferedTx` bez ownership semantics runtime
- `ReconstructedState`
- `SnapshotBuildResult`
- helpery składania replay payloadów
- proste typy konfiguracyjne bez polityki runtime
- helpery domenowe do przekształcania historii/snapshotów

### Typy nieneutralne — nie wolno ich utrwalać w core
Przykładowo:
- approval state machine
- observation windows
- policy gatekeeping
- routing pre/post-commit
- ownership `pending_live`
- trigger commit rules
- runtime dead-window semantics
- heurystyki typu “co i kiedy przepuścić”
- launcherowa semantyka `tracked / approved / committed`

To jest bardzo ważne:
# PR-6 nie może “zachować złego systemu, tylko pod ładniejszą nazwą”

---

## 2.4. Co dokładnie ma osiągnąć ten PR

### A. Core gatekeeper przestaje być składem wszystkiego
Po tym PR:
- jego API ma być mniejsze,
- jego rola w import graph ma być zredukowana,
- ma być miejscem tymczasowej delegacji, a nie centralnym punktem życia systemu.

### B. Neutralne typy mają dostać sensowne docelowe miejsce
Na przykład:
- `TxKey` nie powinien żyć w “gatekeeperze”, jeśli jest domain-wide contract type
- `CommitResult` nie powinien być przywiązany do reliktu, który ma umrzeć

### C. Nowe użycia nie mogą już trafiać do starego pliku
Po PR-6 review powinno blokować każdy nowy import do:
```rust name=ghost-core/src/shadow_ledger/gatekeeper.rs
ghost-core/src/shadow_ledger/gatekeeper.rs
```
jeśli to nie jest część przejściowego adaptera.

---

## 2.5. Czego ten PR nie robi
- nie przejmuje jeszcze commit orchestration do launchera
- nie usuwa core gatekeepera
- nie zmienia behavior systemu bardziej niż to konieczne
- nie przenosi runtime policy do core
- nie dotyka transportu / log pressure

To ma być:
# możliwie extraction-only PR

---

## 2.6. Testy i walidacja
Nie tylko testy funkcjonalne, ale też architektoniczne:
- compile-time import cleanup
- lexical search na nowe użycia starego pliku
- brak wzrostu public API reliktu
- brak behavioral regression w smoke testach po Etapie II

---

## 2.7. Merge gate
Nie merge’ować, jeśli:
- extraction dodaje nowe responsibilities do core gatekeepera
- neutralne typy nadal są wymieszane z runtime policy
- import graph robi się jeszcze bardziej splątany
- behavior systemu zmienia się szerzej niż wynika z samego extraction

---

# 3. PR-7 — `launcher-commit-coordinator`

## Tytuł roboczy
# **Launcher commit coordinator: move commit orchestration, `pending_live`, and post-commit routing out of core gatekeeper**

---

## 3.1. Po co istnieje ten PR
To jest właściwa migracja.

Po PR-6 core gatekeeper ma już być odchudzony.
Teraz launcher musi przejąć to, co docelowo do niego należy:
- commit orchestration
- ownership `pending_live`
- commit window semantics
- init `LivePipeline`
- post-commit routing

To jest punkt, w którym:
# launcher staje się nie tylko runtime policy SSOT, ale także realnym ownerem przejścia pre-commit → committed → live

To jest najważniejszy krok przed ostatecznym usunięciem core gatekeepera.

---

## 3.2. Zakres plików
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/main.rs`

oraz wykorzystanie czystych typów wyciągniętych w PR-6.

---

## 3.3. Co dokładnie ma zostać przeniesione

### A. Ownership `pending_live`
To launcher ma decydować:
- co jest jeszcze w oknie commit,
- co jest oczekujące,
- co po sukcesie commitu ma wejść do live path,
- co po failure ma wrócić do bezpiecznego stanu.

### B. Commit trigger i commit loop orchestration
Launcher ma być właścicielem:
- kiedy commit jest uruchamiany,
- jakie payloady do niego wchodzą,
- jak wynik jest interpretowany,
- jak post-commit flow jest bootstrapowany.

### C. `LivePipeline` init ma wychodzić z runtime-coordinated result
A nie z pośredniego readbacku przez reliktowy gatekeeper/core path.

### D. Core ma zostać tylko wykonawcą domain/storage
Czyli:
- ledger append
- snapshot merge
- live pipeline domain behavior
ale nie:
- owner commit policy
- owner runtime gating
- owner pre/post transition state machine

---

## 3.4. Shadow-run: obowiązkowy etap przed pełnym przełączeniem
To była jedna z najmocniejszych rekomendacji i podtrzymuję ją w 100%.

Ten PR nie powinien wejść jako “switch and pray”.
Najpierw:
# nowy launcher coordinator liczy wynik równolegle
ale:
- stary path jeszcze wykonuje canonical write
- nowy path tylko porównuje

Porównywać należy co najmniej:
- `snapshot hash`
- `tx_count`
- `last_tx_key`
- success/failure classification
- liczność committed/live routed tx

Dopiero po zgodności:
- można przełączyć writer path

---

## 3.5. Czego ten PR nie robi
- nie usuwa jeszcze starego pliku z core
- nie zajmuje się transportem
- nie zajmuje się log pressure
- nie jest extraction-only
- nie ma już prawa dodawać logiki z powrotem do core gatekeepera

To jest:
# właściwa migracja odpowiedzialności do launchera

---

## 3.6. Testy obowiązkowe
- `runtime_gatekeeper_commits_without_core_gatekeeper`
- `pending_live_survives_commit_window`
- `committed_tx_goes_directly_to_live_pipeline`

Dodatkowo:
- shadow-run equivalence tests
- test na brak bootstrap dependency od gated readback
- test na commit failure recovery z launcher ownership

---

## 3.7. Merge gate
Nie merge’ować z przełączeniem writer path, jeśli:
- shadow-run pokazuje divergence
- launcher i stary path różnią się w `last_tx_key`
- snapshot hash się rozjeżdża
- tx_count się rozjeżdża
- nadal istnieją przypadki “committed but not really persisted”
- `pending_live` nadal semantycznie mieszka w core

---

# 4. PR-8 — `delete-core-gatekeeper`

## Tytuł roboczy
# **Delete `ghost-core/src/shadow_ledger/gatekeeper.rs` after launcher migration and remove all runtime dependencies on it**

---

## 4.1. Po co istnieje ten PR
To jest docelowa kasacja reliktu.

Dopiero po PR-7 można bezpiecznie usunąć:
```rust name=ghost-core/src/shadow_ledger/gatekeeper.rs
ghost-core/src/shadow_ledger/gatekeeper.rs
```

To jest nie tylko porządkowanie kodu.
To jest:
# formalne domknięcie nowej architektury

Od tego momentu:
- jest tylko jeden prawdziwy Gatekeeper
- runtime policy i commit orchestration są w launcherze
- `ghost-core` przestaje udawać warstwę runtime.

---

## 4.2. Zakres plików
- usunięcie:
  - `ghost-core/src/shadow_ledger/gatekeeper.rs`

cleanup:
- `ghost-core/src/shadow_ledger/mod.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-launcher/src/config.rs`

oraz:
- testy
- stare importy
- stare feature flags / config wiring
- ewentualne dokumentacyjne odniesienia

---

## 4.3. Co dokładnie ma być prawdą po tym PR

### A. Zero importów runtime do `GatekeeperRegistry`
### B. Zero eksportów `shadow_ledger::gatekeeper`
### C. Zero ownership runtime behavior w core
### D. `ShadowLedger` i `LivePipeline` działają jako czysta warstwa domain/storage
### E. Launcher sam zarządza pełnym cyklem runtime gatekeeping + commit coordination

---

## 4.4. Czego ten PR nie robi
- nie wprowadza nowej semantyki
- nie zmienia już kontraktów
- nie naprawia nowych correctness bugs
- nie dodaje nowych heurystyk

To ma być:
# deletion and cleanup only

Jeżeli przy PR-8 wychodzą nowe bugi semantyczne,
to znaczy, że PR-7 albo wcześniejsze kroki były niedomknięte.

---

## 4.5. Testy i walidacja
- full workspace compile
- zero-reference verification:
  - `GatekeeperRegistry`
  - `shadow_ledger::gatekeeper`
  - stare config wiring
- e2e smoke na nowym path
- test `same_signature_two_trades_end_to_end`
- test post-commit routing na nowej architekturze

---

## 4.6. Merge gate
Nie merge’ować, jeśli:
- istnieją jeszcze runtime importy do starego gatekeepera
- testy nadal ukrywają zależność od reliktu
- launcher nie przejął pełnego ownership commit path
- kasacja wymaga “szybkich hotfixów” przywracających starą warstwę

---

# 5. PR-9 — `transport-and-log-pressure`

## Tytuł roboczy
# **Transport recovery, watch strategy, and log pressure reduction after correctness and migration fixes**

---

## 5.1. Po co istnieje ten PR
Ten PR idzie na końcu celowo.

Audyt wykazał:
- slot gaps
- watch truncation / `tracked_dropped`
- słabą politykę `per_pool`
- log storm (`TRADE_DEDUP`, `hyper/h1/h2`, websocket spam)

Ale wcześniej nie wolno tego dotykać jako głównego kierunku,
bo:
# łatwo byłoby poprawić symptomy bez naprawy kontraktów danych i commitu

Dopiero po zamknięciu correctness i migracji można sensownie ocenić:
- ile problemu naprawdę siedzi w transporcie,
- ile coverage odzyskano,
- ile log pressure samoistnie spadło po wycięciu wcześniejszych bugów.

---

## 5.2. Zakres plików
- `off-chain/components/seer/src/grpc_connection.rs`
- `config.toml`
- `off-chain/components/seer/src/config.rs`
- `off-chain/components/seer/src/binary_parser.rs`

plus ewentualnie:
- konfiguracja tracing/logging

---

## 5.3. Co dokładnie ma zostać naprawione

### A. `per_pool` ma przestać być domyślną polityką w niestabilnym mapping regime
Dopóki mapping i identity są kruche,
`per_pool` potęguje coverage loss.

Po wcześniejszych fixach trzeba dobrać politykę, która:
- nie wzmacnia mapping race,
- nie przycina hot pools przez zbyt agresywną selekcję.

### B. Reconnect / gap / backfill mają być traktowane jako normalny recovery path
Nie “best effort”, nie tylko diagnostyka.
Jeśli stream się zrywa:
- system ma mieć jawny model odzyskiwania.

### C. `tracked_dropped` / watch churn trzeba zmniejszyć i rozumieć
Po correctness fixes trzeba dopiero zobaczyć, ile loss zostaje realnie na transporcie,
i ograniczyć ten udział.

### D. Log pressure ma spaść radykalnie
Do wycięcia / ograniczenia:
- `TRADE_DEDUP`
- noisy `hyper/h1/h2`
- base64/websocket spam
- hot path logs, które nie są metryką ani pojedynczym structured warningiem

---

## 5.4. Czego ten PR nie robi
- nie naprawia podstawowych kontraktów parsera
- nie naprawia runtime SSOT
- nie naprawia commit semantics
- nie migruje gatekeepera

Jeśli po PR-9 nadal wychodzą fundamentalne correctness bugs,
to znaczy, że wcześniejsze etapy nie zostały domknięte.

---

## 5.5. Testy i walidacja
- artificial gap recovery
- reconnect scenario
- watch churn / burst tests
- runtime smoke z porównaniem log volume
- pomiar `tracked_dropped`, recovery success, duplicate suppression

---

## 5.6. Merge gate
Nie merge’ować, jeśli:
- zmiany w transporcie zamazują możliwość diagnozy correctness regressions
- logging jest ograniczany kosztem utraty niezbędnej obserwowalności
- nie ma pomiaru before/after dla log rate i recovery behavior

---

# 6. Twarde warunki usunięcia `ghost-core` gatekeepera

To jest osobna sekcja, bo to najważniejszy architektoniczny checkpoint całego programu.

Core gatekeeper można usunąć **dopiero wtedy**, gdy jednocześnie prawdziwe są wszystkie poniższe warunki:

## 6.1.
`ghost-launcher` nie importuje już:
- `GatekeeperRegistry`
- `GatekeeperConfig` z reliktowego path
- żadnego runtime behavior z `ghost-core/src/shadow_ledger/gatekeeper.rs`

## 6.2.
`pending_live` i commit window są własnością launcherowego Gatekeepera,
a nie core.

## 6.3.
`ShadowLedger.commit_history()` i `LivePipeline` są wywoływane bez pośrednictwa core-gatekeepera jako ownera logiki runtime.

## 6.4.
Bootstrap post-commit nie zależy od gated readbacku przez starą warstwę.

## 6.5.
Nie ma już logów typu:
- `commit_history rejected` + lokalny commit success
- `Missing ShadowLedger snapshot for mint` po realnym sukcesie commitu
- `Missing pool identity for committed mint` jako blocker ścieżki post-commit

## 6.6.
Testy przechodzą na nowym path, w tym:
- `same_signature_two_trades_end_to_end`
- commit failure recovery
- post-commit live routing
- brak collapse multi-event tx

---

# 7. Ogólne warunki akceptacji — Etap I

Etap I obejmował:
- PR-1
- PR-2
- PR-3a
- PR-3b

Jego celem było odzyskanie poprawności wejścia i intake.

## Etap I uznajemy za zaakceptowany dopiero gdy:

### 7.1. Parser contract jest naprawiony
- named CPI side nie jest nadpisywane przez inference
- `timestamp_ms` nie jest `arrival_ts`
- zero-volume/phantom outputs znikają z canonical producer path

### 7.2. `1 signature != 1 trade` jest realnie obsłużone
- parser niesie `tx_index/event_ordinal`
- intake i engine nie redukują legalnych multi-event tx do jednej jednostki

### 7.3. `mint=111111...` nie wychodzi poza unresolved/staging path
- nie pojawia się jako zwykły downstream identity

### 7.4. `SnapshotListener` nie hard-dropuje `unknown_pool`
- unknown/unresolved trafia do recoverable path

### 7.5. `SnapshotEngine` ma jeden authoritative tx ingress
- enrichment może istnieć
- drugi konkurencyjny writer path nie

### 7.6. Coverage counters przestają mieć skrajne załamania przed engine
Przynajmniej jakościowo ma być widać poprawę na ścieżce:
- `seer_rx`
- `listener_forwarded`
- `snapshot_engine_accepted`

---

# 8. Ogólne warunki akceptacji — Etap II

Etap II obejmował:
- PR-4
- PR-5

Jego celem było ustanowienie runtime SSOT i zatrzymanie fałszywego commitu.

## Etap II uznajemy za zaakceptowany dopiero gdy:

### 8.1. `tracked`, `approved`, `committed` są rozdzielone semantycznie i implementacyjnie
- `approved != committed`
- `approved` nie uruchamia HardTruth semantics

### 8.2. Launcherowy Gatekeeper używa canonical event-time jako primary axis
- `arrival_ts_ms` nie steruje już canonical ordering w runtime

### 8.3. Multi-event same-signature nie są dedupowane przez runtime policy layer

### 8.4. `commit_history()` daje jawny wynik
- commit failure nie wygląda jak success

### 8.5. `committed=true` występuje tylko po realnym persisted commit

### 8.6. Commit loop bootstrapuje post-commit flow z commit result
- nie z kruchego readbacku przez gated path

### 8.7. Nie występują już pary logów:
- `commit_history rejected`
- a zaraz potem lokalna ścieżka zachowująca się jak po sukcesie

---

# 9. Ogólne warunki akceptacji — Etap III

Etap III obejmował:
- PR-6
- PR-7
- PR-8
- PR-9

Jego celem było uproszczenie architektury, usunięcie drugiego Gatekeepera i końcowa stabilizacja operacyjna.

## Etap III uznajemy za zaakceptowany dopiero gdy:

### 9.1. `ghost-core/src/shadow_ledger/gatekeeper.rs` jest usunięty
albo — przed kasacją — zredukowany do adaptera bez runtime ownership.

### 9.2. Launcher jest jedynym ownerem:
- runtime gatekeeping
- commit orchestration
- `pending_live`
- pre/post-commit routing

### 9.3. `ghost-core` trzyma wyłącznie neutralne domain/storage responsibilities

### 9.4. Shadow-run nowego launchera commit coordinatora nie wykazuje divergencji
co najmniej w:
- `snapshot hash`
- `tx_count`
- `last_tx_key`

### 9.5. Po przełączeniu nowego path:
- brak reliktowych importów
- brak regresji e2e
- brak powrotu do starych mostków

### 9.6. Transport i log pressure są ograniczone po correctness fixes
- log rate znacząco spada
- reconnect/gap recovery jest mierzalny i działa jako realny recovery path
- `tracked_dropped` / watch churn są zrozumiane i ograniczone

---

# 10. Globalny Definition of Done dla całego programu

Cały plan można uznać za zakończony dopiero wtedy, gdy prawdziwe są wszystkie poniższe warunki:

## 10.1. Kontrakt TX jest spójny end-to-end
- parser
- producer
- listener
- snapshot engine
- runtime gatekeeper
- commit path
- live pipeline
- ledger

wszystkie zgadzają się co do:
- event identity
- canonical time
- multi-event tx semantics

---

## 10.2. Nie istnieje już drugi Gatekeeper jako runtime actor
- launcher jest jedynym prawdziwym Gatekeeperem
- core jest tylko domain/storage

---

## 10.3. System nie kłamie o commitach
- brak false committed state
- brak false live bootstrap
- brak post-commit path bez realnie persisted history

---

## 10.4. `approved` nigdy nie znaczy `committed`
i nigdy nie uruchamia canonical truth semantics przed realnym commit.

---

## 10.5. `mint=111111...` nie wychodzi poza unresolved path

---

## 10.6. `unknown_pool` nie jest brutalnie dropowany przed możliwością odzyskania

---

## 10.7. Legalne multi-event tx pod tym samym signature przechodzą end-to-end bez dropu

---

## 10.8. `SnapshotEngine` nie ma dwóch konkurencyjnych authoritative ingressów

---

## 10.9. Canonical ordering axis jest jeden
- event-time primary
- arrival-time tylko telemetry/fallback

---

## 10.10. Coverage counters są ze sobą znacząco bardziej zbieżne
na ścieżce:
- `seer_rx`
- `listener_forwarded`
- `snapshot_engine_accepted`
- `gatekeeper_committed`
- `live_pipeline_received`

Nie chodzi o idealną równość 1:1 w każdych warunkach,
ale o brak wielkich, niewyjaśnionych przepaści kontraktowych.

---

# 11. Ostateczna sekwencja programu

Dla jasności, finalna kolejność po wszystkich korektach:

## Faza I — Correctness Repair
1. **PR-1** `seer-parser-contract`
2. **PR-2** `seer-mapping-rc6-replay`
3. **PR-3a** `snapshot-listener-buffering-and-mapping`
4. **PR-3b** `snapshot-engine-single-ingress-and-dedup-contract`

## Faza II — Runtime SSOT + Commit Hardening
5. **PR-4** `runtime-gatekeeper-ssot`
6. **PR-5** `shadowledger-commit-hardening`

## Faza III — Extraction / Migration / Deletion / Operations
7. **PR-6** `extract-neutral-core-types`
8. **PR-7** `launcher-commit-coordinator`
9. **PR-8** `delete-core-gatekeeper`
10. **PR-9** `transport-and-log-

