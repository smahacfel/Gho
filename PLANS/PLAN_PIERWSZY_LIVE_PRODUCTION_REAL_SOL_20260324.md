# Plan dojścia do pierwszego live production z real SOL

## Cel tego dokumentu

Ten plan ma doprowadzić Ghosta nie do "kolejnego etapu prac", tylko do **pierwszego kontrolowanego wejścia na mainnet z realnym SOL**, przy zachowaniu minimalnego blast radius oraz z uczciwą odpowiedzią na pytanie:

> czy ten system potrafi wygenerować dodatni wynik netto po fee, tipach, slippage i błędach wykonania?

To nie jest obietnica zysku. To jest plan dojścia do momentu, w którym system albo:

1. udowodni dodatnią oczekiwaną wartość,
2. albo szybko i tanio pokaże, że edge'u nie ma i trzeba wrócić do researchu.

---

## Wniosek wprost

Repo **nie jest jeszcze gotowe** do pierwszego live BUY z realnym kapitałem, mimo że ma już bardzo mocny fundament infrastrukturalny.

Najważniejsze: Ghost jest dziś bliżej **production-grade runtime** niż "prototypu", ale nadal ma kilka twardych blokerów wejścia na live:

1. baseline workspace nie jest zielony,
2. execution semantics są nie w pełni domknięte i miejscami rozjeżdżają się między configiem a runtime,
3. safety bulkhead istnieje, ale nie jest podpięty do kanonicznej ścieżki BUY w launcherze,
4. durability wygląda dobrze na papierze, ale WAL jest realnie aktywowany env-em, nie samym `config.toml`,
5. operacyjny runbook production jest nadal zbyt ręczny,
6. w repo widać ślady sekretów / wallet material, czego nie wolno przenosić do prawdziwej produkcji.

W praktyce oznacza to:

- **tak**: można natychmiast ruszyć z finalnym planem wejścia na live,
- **nie**: nie wolno jeszcze odpalać "prawdziwego BUY" tylko dlatego, że pipeline już umie shadow / live submit.

---

## Co w repo jest już realnym fundamentem pod produkcję

### 1. Pipeline ingest -> decision -> execution istnieje i jest spójny architektonicznie

Aktualny flow jest dobrze opisany i osadzony w kodzie:

- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_commit_loop.rs`
- `ghost-core/src/shadow_ledger/live_pipeline.rs`
- `docs/ADR/20260320_production_pipeline_data_flow.md`

To nie jest "pomysł na pipeline". To jest działająca wielowarstwowa architektura z ingestem, bridge'em, event bus, Gatekeeperem, Triggerem, ShadowLedgerem i post-commit live path.

### 2. Recovery / durability ma mocny trzon

`ghost-launcher/src/main.rs` wykonuje startup w poprawnej kolejności:

- restore snapshotu,
- replay WAL,
- odtworzenie runtime state,
- dopiero potem live ingest.

Do tego są powiązane ADR-y i testy restartowe:

- `docs/ADR/ADR-0019-phase4-wal-ordering-recovery-closure.md`
- `ghost-launcher/src/wal_recovery.rs`
- `ghost-launcher/tests/wal_startup_recovery.rs`

To jest bardzo ważne, bo bez tego każde wejście na live z realnym SOL byłoby zwykłą rosyjską ruletką po restarcie procesu.

### 3. Trigger ma już wyraźne tryby execution

Repo ma rozdzielone:

- `execution.execution_mode`: `live | paper | dual`
- `trigger.entry_mode`: `live | dry_run_mock | shadow_only | live_and_shadow`

Potwierdzają to:

- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/components/trigger/component.rs`

To jest świetna baza do rolloutu:

- najpierw `paper + shadow_only`,
- potem `dual + live_and_shadow`,
- dopiero na końcu `live + live`.

### 4. Monitoring i telemetry istnieją

Repo ma już gotowy minimalny runbook metryk hot path:

- `docs/RUNBOOK_HOT_PATH_METRICS.md`

Są opisane m.in.:

- ingestion latency,
- event bus lag,
- freshness/finality curve state,
- Gatekeeper verdict latency,
- pending-curve terminal outcomes,
- live pipeline flush latency,
- legacy path blocks.

To jest wystarczający fundament do operacyjnego go/no-go, o ile zostanie naprawdę spięty z alertami.

### 5. Shadow execution i compare-only path już istnieją

To jest bardzo duża przewaga obecnego repo. Mamy gotowy etap pośredni między "zero ryzyka" a "realny BUY":

- `trigger.entry_mode = "shadow_only"`
- `trigger.entry_mode = "live_and_shadow"`
- JSONL z wynikami shadow buy
- event/metrics dla shadow path

Pliki:

- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`
- `logs/shadow_run/`

To oznacza, że pierwszy live można zrobić metodycznie, a nie skokiem na głęboką wodę.

---

## Twarde blokery przed pierwszym realnym BUY

### B1. Workspace nie ma zielonego baseline

Uruchomienie baseline walidacji zakończyło się błędem:

- `cargo test --workspace --quiet` -> exit `101`
- `cargo test --workspace --no-run` -> exit `101`

Najważniejsze błędy z logu:

1. `ghost-brain/src/oracle/decision_logger.rs`
   - inicjalizatory `GatekeeperBuyLog` nie uwzględniają nowych pól `iwim_snap_*`

2. `ghost-brain/tests/runtime_strategy_tests.rs`
   - `CandidatePool` nie zawiera nowych pól `event_ts_ms` i `semantic`

To nie są drobne warningi. To są niedomknięte kontrakty typów po ostatnich zmianach. Dopóki workspace nie przechodzi baseline build/test, **nie ma mowy o live z realnym SOL**.

### B2. Config execution jest semantycznie niejednoznaczny

Obecny `config.toml` ma:

- `mode = "production"`
- `oracle.dry_run = false`
- brak jawnego `[execution].execution_mode`
- jawne `trigger.entry_mode = "shadow_only"`

W loaderze (`ghost-launcher/src/config.rs`) oznacza to:

- `execution.execution_mode` domyślnie mapuje się na `live`,
- ale Trigger nadal działa jako `shadow_only`.

To jest zły stan produkcyjny, bo:

- część runtime myśli kategoriami "live/paper/dual",
- a realna ścieżka wejścia BUY jest ustawiona inaczej.

Przed rolloutem trzeba mieć **jednoznaczny, jawny profil execution** bez opierania się na legacy aliasach.

### B3. Bulkhead safety nie jest wpięty w kanoniczny BUY path launchera

Repo ma sensowny moduł bezpieczeństwa:

- `ghost-launcher/src/components/trigger/safety.rs`

ale wyszukiwanie po `validate_trade()` / `check_emergency_floor()` pokazuje, że w kanonicznym `ghost-launcher/src/components/trigger/component.rs` te guardy nie są używane w ścieżce realnego BUY.

W praktyce launcherowy Trigger:

- czyta `keypair`,
- wylicza amount z `max_position_size_sol`,
- ale nie egzekwuje w tej ścieżce pełnego bulkheada zależnego od rzeczywistego salda.

To jest krytyczny blocker przed live.

### B4. Parametry kapitałowe są dziś zbyt agresywne albo martwe

W `config.toml`:

- `emergency_floor_sol = 0`
- `position_size_buffer_sol = 0`
- `max_concurrent_positions = 3`

Nawet gdyby safety było podpięte, taki profil nie jest profilem pierwszego live. Pierwszy live powinien być:

- 1 pozycja naraz,
- niezerowy emergency floor,
- niezerowy fee/tip buffer,
- dust-sized real exposure.

### B5. WAL nie włącza się samym wpisem w `config.toml`

To jest bardzo ważny niuans.

`config.rs` sugeruje, że `durability.wal_dir` odpowiada `GHOST_WAL_DIR`, ale realny startup w `ghost-launcher/src/main.rs` używa:

- `init_optional_wal_from_env()`
- i aktywuje WAL tylko wtedy, gdy istnieje env `GHOST_WAL_DIR`

Wniosek:

**sam wpis `durability.wal_dir = "data/wal"` nie daje jeszcze gwarancji, że współdzielony WAL naprawdę działa w runtime**.

Przed live trzeba to jawnie domknąć i potwierdzić w logach/metrykach.

### B6. W repo widać materiał sekretny / wallet pathy, które nie mogą zostać w produkcji

Z perspektywy bezpieczeństwa nie wolno iść na live, jeśli:

- provider credentials są wpisane bezpośrednio w śledzony config,
- wallet keypair żyje w repo albo repo-adjacent path,
- operator nie ma rozdzielonych ról: repo / config / secret storage / funding wallet.

Sama obecność `solana/id.json` i realnych endpointów w aktualnym drzewie jest wystarczającym sygnałem, że trzeba zrobić **sekretowy cleanup przed produkcją**.

### B7. Operacyjny runbook production jest jeszcze za cienki

Repo ma:

- `docs/RUNBOOK_HOT_PATH_METRICS.md`
- `POLECENIA_SKROT.md` z ręcznym odpalaniem przez `tmux`

Ale to jeszcze nie jest pełny production guide. Brakuje jawnie spiętego:

- preflightu przed startem,
- procedury rollback/abort,
- checklisty go/no-go,
- instrukcji restartu po recovery,
- polityki alertów,
- polityki rotacji logów i retencji,
- jednej autorytatywnej komendy / service unit dla produkcji.

### B8. Repo samo mówi, że część kontraktów runtime jest nadal "w remoncie"

`PLAN_WYKONAWCZY.md` i ADR-y z marca 2026 pokazują, że projekt bardzo dużo naprawił, ale jednocześnie sam identyfikuje nadal aktywne kontrakty correctness / authority / event semantics do domknięcia.

Dodatkowo `ghost-core/src/shadow_ledger/live_pipeline.rs` wprost mówi, że część canonical authority jest egzekwowana nadal "by convention", a nie pełnym runtime guardem.

To nie przekreśla pierwszego mikro-live, ale oznacza:

- **pierwszy live ma być ekstremalnie mały**,
- **skalowanie kapitału musi czekać na domknięcie tych kontraktów**.

---

## Co trzeba zrobić teraz, w jakiej kolejności

## Faza 0 - Production freeze i twarde domknięcie wejścia

To jest faza, bez której nie wolno odpalać realnego BUY.

### 0.1. Uczyń repo znowu zielonym

Najpierw naprawić baseline:

- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-brain/tests/runtime_strategy_tests.rs`
- wszystkie miejsca inicjalizacji `CandidatePool`
- wszystkie miejsca inicjalizacji `GatekeeperBuyLog`

Gate:

- `cargo test --workspace --quiet` przechodzi,
- `cargo test --workspace --no-run` przechodzi,
- brak compile blockers w workspace.

### 0.2. Usuń sekrety z repo i przestań traktować repo jak secret store

Przed live:

- wallet keypair przenieść poza repo,
- provider credentials przenieść do env/secrets managera,
- upewnić się, że żaden plik z kluczem prywatnym nie jest trackowany w git,
- ustawić dedykowany wallet tylko do pierwszego live, z małym fundingiem i bez innych aktywów.

### 0.3. Zrób jawny, jeden profil execution

Nie wolno polegać na mieszance:

- `oracle.dry_run`
- domyślnego mapowania `execution.execution_mode`
- ręcznego `trigger.entry_mode`

Minimalna zasada:

- **zawsze jawnie ustawiaj** `[execution].execution_mode`
- **zawsze jawnie ustawiaj** `[trigger].entry_mode`
- `oracle.dry_run` traktuj jako legacy i nie używaj do produkcyjnego rolloutu

### 0.4. Podłącz bulkhead do kanonicznej ścieżki BUY

Przed live launcher musi w realnym path:

- odczytać saldo,
- sprawdzić `emergency_floor_sol`,
- uwzględnić `position_size_buffer_sol`,
- odrzucić BUY, jeśli pozycja narusza floor/buffer,
- logować przyczynę odrzucenia jako explicit safety decision.

To ma działać w `ghost-launcher/src/components/trigger/component.rs`, nie tylko w przykładach lub starych off-chain helperach.

### 0.5. Domknij durability tak, by nie była "umowna"

Przed live trzeba udowodnić w logach/metrykach, że:

- snapshot restore działa,
- WAL replay działa,
- współdzielony WAL jest naprawdę aktywny,
- runtime po restarcie wraca do poprawnego stanu.

Minimalny warunek:

- jeśli chcesz używać WAL, ustawiasz i weryfikujesz `GHOST_WAL_DIR`,
- nie zakładasz, że sam wpis w `[durability] wal_dir = ...` wystarczy.

### 0.6. Zbuduj production preflight

Jedna komenda / jeden skrypt / jeden service ma przed startem sprawdzić:

- czy config jest jawny i spójny,
- czy keypair da się odczytać,
- czy wallet ma saldo > emergency floor + buffer,
- czy gRPC endpoint jest poprawny,
- czy Jito endpoint jest poprawny, jeśli `use_jito=true`,
- czy port metryk jest wolny,
- czy katalogi WAL/snapshot istnieją i są zapisywalne,
- czy revision ma zielony build/test baseline.

Bez preflightu nie ma produkcji, jest tylko "manualny eksperyment".

---

## Faza 1 - Mainnet shadow burn-in

Tu jeszcze nie chodzi o zarabianie. Tu chodzi o to, żeby sprawdzić, czy runtime naprawdę widzi, ocenia i wykonuje to, co myślimy, że robi.

### Profil rolloutu

Ustawić jawnie:

- `[execution].execution_mode = "paper"`
- `[trigger].entry_mode = "shadow_only"`
- `max_concurrent_positions = 1`
- bardzo mały `max_position_size_sol`
- niezerowy `emergency_floor_sol`
- niezerowy `position_size_buffer_sol`

Jeżeli docelowo live ma używać Jito, shadow burn-in ma iść jak najbliżej docelowego execution environment:

- te same feedy,
- ten sam host,
- ta sama telemetria,
- ten sam styl preflightu,
- ten sam recovery setup.

### Co musi zostać udowodnione w shadow

1. System nie gubi early-pool flow na event busie.
2. `PendingCurve`, freshness i finality zachowują się stabilnie.
3. Shadow buy kończy się pełnym, czytelnym telemetrix:
   - decyzja,
   - input,
   - symulowany wynik,
   - przyczyna fail/skip.
4. Event bus lag nie rośnie systemowo.
5. Recovery po restarcie nie psuje stanu.
6. Shadow PnL po fee/tip/slippage assumptions nie wygląda katastrofalnie.

### Artefakty, które trzeba analizować

- `logs/shadow_run/buys.jsonl`
- `logs/decisions.jsonl`
- `datasets/events/`
- `gatekeeper_v2_buys.jsonl` / pokrewne logi decyzyjne

### Gate wyjścia z Fazy 1

Przechodzisz dalej dopiero, gdy:

- masz stabilny shadow run na mainnecie,
- nie ma czerwonych metryk hot path,
- nie ma recovery surprises,
- nie ma ewidentnego rozjazdu między selekcją a wykonaniem,
- shadow economics nie są wyraźnie trwale ujemne po kosztach.

Jeżeli już tu wychodzi brak edge'u, **nie przechodzisz do live**. Kończysz eksperyment i wracasz do researchu.

---

## Faza 2 - Pierwszy mikro-live

To ma być etap "czy bot potrafi zrobić dodatni, prawdziwy round-trip", a nie "czy umiemy szybko spalić większy kapitał".

### Profil rolloutu

Ustawić jawnie:

- `[execution].execution_mode = "dual"`
- `[trigger].entry_mode = "live_and_shadow"`
- `max_concurrent_positions = 1`
- bardzo mały, ale realny `max_position_size_sol`
- wallet z osobnym, małym fundingiem

Na tym etapie każdy realny BUY powinien mieć równoległy ślad compare-only.

### Zasady pierwszego live

1. Jeden dedykowany host.
2. Jeden dedykowany wallet.
3. Jedna pozycja naraz.
4. Brak ręcznego "podkręcania" configu w trakcie sesji.
5. Operator jest obecny i monitoruje metryki / logi.
6. Każdy restart ma przejść przez recovery check.

### Twarde kill-switch conditions

Natychmiast zatrzymujesz live, jeśli wystąpi którykolwiek z poniższych warunków:

- `eventbus_lag_total` rośnie,
- `gatekeeper_pending_curve_terminal_total{outcome=timed_out}` zaczyna dominować,
- WAL nie jest aktywny, mimo że miał być,
- wallet spada do emergency floor,
- shadow/live divergence staje się duża i powtarzalna,
- pojawia się nieoczekiwany duplicate BUY / powtórny fire na ten sam mint,
- recovery po restarcie nie odtwarza stanu,
- pojawia się nieautoryzowana ścieżka side effectów,
- Jito tip cost zjada sens ekonomiczny pozycji.

### Gate wyjścia z Fazy 2

Dopiero po serii poprawnie zamkniętych mikro-trade'ów można przejść dalej.

Warunek nie brzmi:

- "bot kupił coś prawdziwego"

Warunek brzmi:

- "bot kupił, wyszedł, policzył realny netto wynik, nie zgubił stanu i nie zrobił nic nieautoryzowanego".

---

## Faza 3 - Udowodnienie lub obalenie edge'u

To jest najważniejsza faza z perspektywy sponsorów.

Nie interesuje nas piękny dashboard. Interesuje nas odpowiedź:

> czy po wszystkich kosztach system ma dodatnią wartość oczekiwaną?

### Metryki, które trzeba liczyć bez oszukiwania siebie

#### 1. Net PnL, nie gross PnL

Każdy trade liczony jako:

`wynik netto = wyjście - wejście - fee - Jito tip - slippage - failed execution cost`

Jeśli patrzysz tylko na "czy cena wzrosła po BUY", to jeszcze niczego nie udowodniłeś.

#### 2. Fill quality

Trzeba porównać:

- expected fill,
- shadow fill,
- real fill.

Jeśli shadow wygląda dobrze, a real execution stale przegrywa na opóźnieniu albo tipach, edge jest pozorny.

#### 3. Decision quality

Potrzebne są trzy liczby:

- hit-rate,
- average winner / average loser,
- max drawdown.

Bez tego łatwo mieć "więcej trafień", ale nadal trwale ujemny wynik.

#### 4. Operational loss rate

Osobno licz:

- decyzje poprawne, ale niewysłane,
- decyzje wysłane, ale niewłączone,
- decyzje wykonane z błędnym fill,
- decyzje zabite przez pending/recovery/runtime drift.

To są realne koszty produkcyjne, nie "techniczne detale".

### Uczciwy warunek kontynuacji

Jeżeli po sensownym zbiorze mikro-live trades wynik netto pozostaje ujemny albo niestabilny, trzeba zrobić jedną z dwóch rzeczy:

1. wrócić do researchu selekcji / execution,
2. zatrzymać rollout.

**Nie wolno skalować kapitału tylko dlatego, że system "w końcu działa live".**

---

## Faza 4 - Dopiero potem powolne zwiększanie kapitału

Skalowanie ma sens dopiero, gdy wszystkie poniższe rzeczy są jednocześnie prawdziwe:

1. workspace jest zielony,
2. sekrety są poza repo,
3. safety bulkhead działa w realnym BUY path,
4. WAL/recovery są udowodnione w runtime,
5. live+shadow dają spójne dane,
6. net PnL po kosztach jest dodatni,
7. max drawdown mieści się w akceptowalnym limicie.

Wtedy dopiero:

- przechodzisz z `dual + live_and_shadow` do dalszego `dual`,
- stopniowo zwiększasz `max_position_size_sol`,
- dopiero na końcu rozważasz `trigger.entry_mode = "live"`.

Do tego momentu `shadow` jest twoim ubezpieczeniem poznawczym i nie warto go wyłączać.

---

## Minimalna docelowa architektura operacyjna dla pierwszego live

### Host

- jeden dedykowany host,
- firewall na zewnątrz,
- brak innych ryzykownych procesów na tym samym wallet host,
- metryki wystawione tylko świadomie.

### Proces

Obecnie repo sugeruje manualny `tmux` (`POLECENIA_SKROT.md`). To wystarczy do stagingu, ale do prawdziwego production należy mieć:

- jedną autorytatywną komendę startu,
- service wrapper,
- jawny restart policy,
- jawny stop procedure,
- zapis logów i recovery state.

### Sekrety

- wallet poza repo,
- credentials poza repo,
- osobny funding wallet,
- osobny trading wallet,
- rotacja/revocation provider keys po cleanupie.

### Observability

Minimalny zestaw alertów:

- event bus lag > 0,
- provider stall / circuit breaker open,
- pending curve timeout,
- restore/replay error,
- brak WAL przy oczekiwanej aktywacji,
- spadek salda do floor,
- zbyt wysoki tip/trade ratio,
- rosnąca rozbieżność shadow vs live.

---

## Konkretna sekwencja "co dalej"

Jeśli celem jest ruszyć z tym wreszcie do przodu, kolejność powinna być dokładnie taka:

1. Naprawić czerwony baseline workspace.
2. Wynieść sekrety i wallet poza repo.
3. Ujednoznacznić config execution (`execution_mode` + `entry_mode`).
4. Podpiąć bulkhead safety do kanonicznego BUY path.
5. Domknąć aktywację WAL i potwierdzić ją logami/metrykami.
6. Spisać jeden production preflight + start/stop/restart runbook.
7. Odpalić mainnet `paper + shadow_only`.
8. Przeanalizować shadow economics i operational health.
9. Przejść do `dual + live_and_shadow` z mikro-pozycją.
10. Dopiero po dodatnim wyniku netto zwiększać rozmiar ryzyka.

---

## Najważniejsza zasada na koniec

Pierwszy live nie ma za zadanie "wreszcie zarobić porządne pieniądze".

Pierwszy live ma udowodnić trzy rzeczy:

1. system nie robi głupich, nieautoryzowanych rzeczy pod kapitałem,
2. execution na prawdziwej sieci nie zabija edge'u,
3. po wszystkich kosztach zostaje cokolwiek dodatniego.

Jeżeli te trzy rzeczy zostaną potwierdzone, projekt ma prawo przejść do skalowania.

Jeżeli nie, to największą wygraną będzie szybkie, uczciwe zakończenie złej hipotezy bez dalszego spalania czasu i sponsorów.
