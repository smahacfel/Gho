# Gate 0 — jeden run, pięć snapshotów, decyzja czy kontynuować

`ghost_gate0` uruchamia Seera i istniejący materializer sesji. Nie uruchamia OracleRuntime, Triggera, walleta ani submitu. Nie używa tradingowych progów do odsiewu.

## Uruchomienie

Ustaw `GHO_GATE0_GRPC_ENDPOINT` i `GHO_GATE0_GRPC_TOKEN` w środowisku operatora (token tylko jako metadata; nie w URL). Domyślny nagłówek to `x-token`; inny wybiera `--auth-header`.

```sh
cargo build --locked --offline --profile test -p ghost-launcher --bin ghost_gate0
# Najpierw source smoke, z oddzielnym plikiem; to NIE jest dataset Gate 0.
./target/debug/ghost_gate0 --config configs/gate0.toml --output /path/smoke.jsonl --smoke-seconds 60
# Właściwy run: 10h nowych create + do 600s na domknięcie ostatnich tokenów.
./target/debug/ghost_gate0 --config configs/gate0.toml --output /path/gate0.jsonl
python3 scripts/gate0_scan.py /path/gate0.jsonl --output /path/gate0_report.json
```

Przy `CARGO_TARGET_DIR` plik wykonywalny jest w odpowiednim `debug/`, niekoniecznie w lokalnym `target/`.
Wyjścia są create-new; istniejący log lub raport nie zostanie nadpisany. Nie podłączać konfiguracji walleta ani plików holdoutu.

## Źródło i statusy transakcji

Profil `Gate0Observation` używa stałej subskrypcji: globalne filtry krzywych Pump.fun i pul PumpSwap, statyczne konto fee, transakcje i BlockMeta. Lokalna rejestracja kont BCV2/generic służących wykonaniu nie zmienia filtrów źródła. Zwykły `PrimaryGlobal` zachowuje dynamiczne konta. Lokalne watch mapy nadal podlegają cap/TTL.

Profil Gate0 jawnie ustawia `source_commitment = "confirmed"`. To również domyślna wartość nowego pola przy wczytaniu wcześniejszego pliku TOML; ordinary Seer zachowuje swój dotychczasowy default. Primary oraz funding używają tej samej konfiguracji commitment. Wartość jest zapisana w `run_start.config`.

Gate0 nie ma rollbacku niezmiennych snapshotów po odrzuceniu spekulacyjnego forka. Dlatego runy na `processed` nie są równoważne nowemu profilowi: zmienia się moment dostępności zdarzeń i birth. Nie łączyć ich bez jawnego rozdzielenia źródła. `confirmed` nie oznacza `finalized` ani gwarancji braku błędu dostawcy. Yellowstone buforuje zdarzenia do wybranego commitment: [dokumentacja producenta](https://docs.triton.one/project-yellowstone/dragons-mouth-grpc-subscriptions#managing-commitment-levels).

Na polecenie operatora z 2026-10-02 błędy danych nie zatrzymują całego zbieracza. Sprzeczne statusy zapisują `transaction_outcome_conflict` z `handling=quarantine_token`, po czym kończą tylko ten token z `gem:null`. Konflikt nie zasila CPV ani liczników; wcześniejszy wkład do wspólnego CPV oznaczamy jako brak ciągłości metryki. Redostawa nie nadpisuje pierwszego dowodu. Skaner dopuszcza taki zakończony run tylko z jawnym terminalem kwarantanny; wszystkie snapshoty tego tokena wyklucza z grup Gem/non-Gem. Stare nieobsłużone konflikty nadal oznaczają błędny dataset.

## Co zapisujemy

Jeden append-only JSONL: `run_start`, każdy `birth`, pięć `phase` (30/90/180/300/600s), `terminal`, `run_end`; w razie sprzeczności również diagnostyczny `transaction_outcome_conflict`, a przy błędach `runtime_issue`. `run_start.error_policy=quarantine_and_continue` opisuje politykę kontynuacji, `summary.runtime_issues` zawiera liczniki przyczyn.
Każda faza zawiera cały zestaw scalar metrics z TxIntelligence, fingerprintu i Sybil/MFS, dodatkowe aliasy oryginalnej listy, liczności oraz jakość. FTDI/DES legacy i V2 są osobnymi kolumnami. Nie wybrano pięciu metryk kosztem pozostałych.
C/D liczą unikalne skuteczne transakcje; E liczy skuteczne swapy. Thresholdy są opisane w `configs/gate0.toml`. Cisza nie zatrzymuje timerów. Snapshot jest zapisywany przed eviction; późniejsze dane nie zmieniają wcześniejszego rekordu.

## Gem i granice danych

Gem wymaga ukończenia 600s, przejścia C/D/E, rzeczywistej migracji do kanonicznej puli PumpSwap po więcej niż 3000ms oraz minimum MC >=320 SOL. MC jest obserwowane na granicach zdekodowanych swapów i transakcji migracyjnej: rzeczywiste salda vaultów + supply/virtual quote z eventu. Nie jest odczytywane z nieaktualnego bonding curve ani ze slippage limitu. Samo `curve.complete` nie oznacza migracji. Nie deklarujemy pomiaru ciągłego pomiędzy obserwowanymi zmianami.
Zweryfikowany `CreatePoolEvent` kanonicznej AMM jest samodzielnym dowodem migracji, nawet gdy osobny `PoolDetected` jeszcze nie dotarł. Parser Gate0 zachowuje ten stan także przy swapie w tej samej transakcji; obserwator nie liczy inicjalizacji jako swapa, wolumenu ani CPV. Zwykły swap nie zastępuje dowodu CreatePool.
Brak początkowego stanu migracji, supply lub wymaganych danych AMM daje `gem:null`. Błąd metryk/checkpointu albo przekroczenie pojemności cenzuruje dotknięty token. Luka źródła/IPC cenzuruje aktywną kohortę; kolejne obserwacje są przyjmowane do pierwotnego deadline. Zakończenie zadania Seera, zamknięcie IPC albo 30s bez postępu uruchamia ponowne zestawienie transportu, z co najmniej 5s odstępu i bez restartowania procesu/zerowania admission. Każdy restart zapisuje diagnostykę poprzedniego segmentu; końcowe liczniki kolejek opisują ostatni segment, a `source_restarts` liczy restarty. Raport analityczny ma `data_quality=degraded`, jeśli wystąpiły problemy. Kontynuacja procesu nie oznacza ciągłości brakujących danych.

Błąd zapisu pliku nadal jest błędem wykonania: zbieracz nie udaje zapisanych danych przy pełnym/uszkodzonym dysku. Kontrole poprawności konfiguracji pozostają przed startem; Ctrl-C nadal kończy run na żądanie operatora. Brak dowodu w smoke jest zapisywany jako issue zamiast wcześniejszego przerwania; do odbioru źródła trzeba sprawdzić issue, liczniki i dane, a nie tylko exit code.

Znane granice istniejących producentów:
- Profil `Gate0Observation` odbiera skuteczne i nieudane transakcje. `failed_tx_ratio` deduplikuje je po sygnaturze; nieudane próby nie zasilają wolumenu ani C/D/E. Zwykły profil `PrimaryGlobal` nadal filtruje nieudane transakcje.
- FSC potrzebuje działającego full-chain funding i jego historii. `--no-funding` jest tylko diagnostyczne; braki są widoczne w jakości.
- CPV zachowuje globalne pule i własny lookback. `CPV_LOOKBACK_NOT_COVERED` oznacza brak pełnej historii; `CPV_PROGRESS_BEHIND_ANCHOR` — ostatni dowód kompletności nie obejmuje jeszcze końca pomiaru. Późniejszy progress nie uzupełnia wcześniejszego cutoffu. Takie null nie jest zastępowane zerem.
- Metryki nazwane `early`, `3s` i `50tx` zachowują oryginalne krótkie okna. Główne metryki są kumulatywne na cutoffie fazy; burst używa długości tej fazy, nie z góry całych 600s.
- Dotychczasowy DES nie otrzymuje wymyślonych rezerw Pump po migracji. Brak obsługi AMM jest widoczny w jakości. Legacy i V2 nie są zamieniane miejscami.
- `volume_gini` i `top3_volume_pct` opisują wolumen signerów, nie top3 pojedynczych transakcji; pełne nazwy/aliasy są zapisane w rekordach.

## Wynik Gate 0

Raport zawiera liczby create, C/D/E, pięciu faz i Gemów, brakujące etykiety, kwantyle/OVL/Cliff's delta dla wszystkich metryk. Kontrole muszą mieć snapshot tej samej fazy. Phase V i liczniki użyte w bramkach są opisowe, nie stanowią samodzielnej podstawy GO.
Domyślny roboczy filtr kandydatów: co najmniej20 wartości na klasę, OVL<=0.8, |Cliff|>=0.33 w fazieI–IV. Wynik jest wstępnym skanem, nie zwalidowanym modelem. `INSUFFICIENT_DATA` nie oznacza braku sygnału. Parametry są jawnie zapisane w raporcie; nie stroić ich tak, by wymusić GO.

## Testy

```sh
cargo test --locked --offline -p ghost-launcher --lib gate0::tests
cargo test --locked --offline -p seer --lib amm_observation::tests
python3 -m unittest discover -s scripts -p 'test_gate0_scan.py'
```

## Preflight przepustowości przed runem 10 h

Run badawczy nie startuje na binarce dev/test. Zbuduj osobny artefakt release w osobnym CARGO_TARGET_DIR; log buildu musi wskazywać profil release [optimized], a SHA256 binarki ma trafić do manifestu runu.

Najpierw uruchom performance smoke na rzeczywistym źródle. Gate0 raportuje osobno primary/funding ingress (current, capacity, high-water, received, overflow_dropped), IPC egress/downstream backlog, maksymalny lag Seer→konsument, pierwotny run_end.reason oraz niezależny run_end.shutdown_error.

shutdown_error nigdy nie zastępuje pierwotnej przyczyny. Ogólne „local IPC coverage gap” nie jest nazwą przyczyny: typed notice zachowuje provider oraz powód, np. ingress_queue_saturated albo ipc_egress_queue_saturated.

Performance smoke jest PASS dopiero przy zero gap/drop, bez stale progress oraz gdy seria pomiarów pokazuje, że backlog jest odprowadzany zamiast rosnąć przez czas testu. Sam większy buffer nie jest dowodem przepustowości.

Po performance PASS wymagany jest smoke >=600 s, który rzeczywiście przeprowadzi co najmniej jeden przyjęty token przez dostępny lifecycle snapshotów. Przed deklaracją pokrycia migracji trzeba dodatkowo zobaczyć realny terminal z migration_age_ms i post-migration MC. Brak takiego przypadku to brak pokrycia, nie PASS przez założenie. Dopiero po obu smoke PASS można wystartować 10 h.

Braki metryk: `EARLY_TOP3_BUY_VOLUME_ZERO` oznacza brak dodatniego BUY w pierwszych 3 s, nawet gdy później jest wiele transakcji. Błędne rezerwy, brak stanu AMM, supply lub zdarzenia migracji unieważniają bieżącą cenę zamiast przenosić poprzednią. `quality.fields` podaje konkretną przyczynę; późniejsza poprawna cena nie usuwa historycznej niekompletności etykiety Gem.
