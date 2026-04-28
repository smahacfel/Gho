## Plan: Stabilizacja identity + tx-only + Prometheus w ghost-launcher

Celem jest domknięcie trzech luk w sposób spójny z przepływem danych: (1) naprawa retry/backoff identity-promotion (obecnie może się „zawiesić” po 2 failach), wraz ze stabilnymi testami niezależnymi od env/Lazy; (2) konsekwentne przejście na architekturę **tx-only** przez wyłączenie całej ścieżki AccountUpdate → IPC → GhostEvent → OracleRuntime reconciliation (bez ryzykownego grzebania w subskrypcji gRPC, zgodnie z decyzją); (3) ekspozycja metryk Prometheusa w `ghost-launcher` (rejestracja + `/metrics`), tak by dane faktycznie były scrape’owalne i znaczeniowo poprawne.

**Założenia i invariants (zgodność przepływu danych)**
- Źródłem prawdy o zmianach curve/pool w czasie rzeczywistym są transakcje (`PoolTransaction`) z gRPC tx stream.
- AccountUpdate nie ma już wpływać na ShadowLedger ani na decyzje OracleRuntime (brak korekt, brak „heal”, brak overwrite) — w trybie tx-only.
- Metryki Prometheus w launcherze muszą:
  - być zarejestrowane dokładnie raz w tym samym rejestrze, który jest zbierany przez endpoint `/metrics`.
  - być semantycznie spójne (np. coverage ratio nie może deklarować mianownika `grpc_received`, gdy w kodzie używa `chain_truth`).

---

### Punkt 1: Naprawić identity promotion retry/backoff + testy

**Problem do naprawy (stan obecny)**
- `ghost-launcher/src/oracle_runtime.rs::maybe_promote_observation_identity_from_tx()` ma backoff oparty o `failed_promotion_attempts % 2 == 0`.
- Po 2 porażkach licznik zostaje na `2` i funkcja zawsze early-returnuje, więc nie ma już kolejnych prób (permanentny backoff).
- `max_identity_promotion_retries()` czyta env przez `static Lazy<u8>` tylko raz → testy mogą być wrażliwe na kolejność i env ustawiony wcześniej w procesie.

**Projekt docelowy (wybrany przez Ciebie): exponential backoff czasowy**
Zastępujemy „co drugi tx” logiką backoffem czasowym, deterministycznym i niemogącym utknąć.

**Kroki implementacyjne**
1. Zmienić strukturę stanu identity promotion
   - W `ObservationIdentity` dodać pola umożliwiające backoff czasowy, np.:
     - `failed_promotion_attempts: u8` (zostaje jako budżet),
     - `next_promotion_attempt_ts_ms: u64` (kiedy najwcześniej wolno próbować),
     - opcjonalnie `last_promotion_attempt_ts_ms: u64` (debug/telemetria).
   - Ustalić stałe/backoff schedule (np. w ms): 0, 50, 200, 1000, 5000, 15000… z górnym limitem.

2. Przebudować `maybe_promote_observation_identity_from_tx()`
   - Logika wejściowa bez zmian: jeśli `base_missing == false && dev_missing == false` → false.
   - Budget: jeśli `failed_promotion_attempts >= max_retries` → false (zachować `POOL_IDENTITY_EXHAUSTED_TOTAL` i WARN tylko raz).
   - Backoff czasowy:
     - pobrać `now_ms` (spójne z resztą runtime: `current_time_ms()`),
     - jeśli `now_ms < next_promotion_attempt_ts_ms` → false (ale NIE zmieniać liczników).
   - Próba promocji:
     - jeśli uda się wypełnić choć jedną lukę → sukces:
       - `failed_promotion_attempts = 0`,
       - `next_promotion_attempt_ts_ms = 0` (lub `now_ms`),
       - aktualizacja `first_seen_ts_ms/end_10s_ts_ms` jak dziś,
       - metryki: `POOL_IDENTITY_PROMOTION_TOTAL{result="success"}++`.
     - jeśli nic nie udało się podnieść → porażka:
       - `failed_promotion_attempts += 1`,
       - obliczyć `delay_ms = backoff(failed_promotion_attempts)` i ustawić `next_promotion_attempt_ts_ms = now_ms + delay_ms`,
       - metryki: `POOL_IDENTITY_PROMOTION_TOTAL{result="failure"}++`.

3. Ustabilizować `max_identity_promotion_retries()` dla testów
   - Opcja rekomendowana: dodać funkcję pomocniczą „czyste” źródło max retries:
     - np. `fn max_identity_promotion_retries_from_env() -> u8` oraz `fn max_identity_promotion_retries() -> u8` które w testach można nadpisać feature-flag / dependency injection.
   - Alternatywnie: wyeliminować `static Lazy` i czytać env per wywołanie (mniej wydajne, ale prostsze i test-stable), albo przynajmniej w testach zapewnić izolację (test serial / ustawienie env zanim cokolwiek dotknie funkcji).

4. Testy jednostkowe (muszą obnażać pierwotny bug)
   - Rozszerzyć zestaw testów w `ghost-launcher/src/oracle_runtime.rs`:
     1) **Regression**: „po 2 failach nadal próbuje w przyszłości”
        - zasymulować kolejne wywołania z rosnącym `now_ms` tak, by backoff mijał i kolejne próby inkrementowały licznik aż do max.
     2) **Exhaustion**: test podobny do obecnego, ale sterowany czasem (żeby realnie dobił do max).
     3) **Partial success reset**: base_mint uda się, dev_pubkey nie → licznik resetuje się, a brakujące pole może być uzupełnione później.
   - Jeśli wprowadzisz zależność od czasu, testy nie mogą polegać na realnym zegarze; plan zakłada wstrzyknięcie `now_ms` do funkcji (parametr) albo pomocniczy „clock” w testach.

**Relevant files**
- `/root/Gho/ghost-launcher/src/oracle_runtime.rs` — `ObservationIdentity`, `max_identity_promotion_retries()`, `maybe_promote_observation_identity_from_tx()`, testy „Identity promotion retry/backoff/exhaustion”.
- `/root/Gho/ghost-launcher/src/oracle_metrics.rs` — metryki `POOL_IDENTITY_PROMOTION_TOTAL`, `POOL_IDENTITY_EXHAUSTED_TOTAL` (już istnieją).

**Verification**
1. Uruchomić testy crate `ghost-launcher` i upewnić się, że regresja „freeze po 2 failach” jest złapana.
2. Sprawdzić w logach/metrykach, że:
   - w przypadku śmieciowych tx (brak token_mint i signer) licznik dochodzi do max i potem się zatrzymuje,
   - w przypadku poprawnych tx luki w identity są uzupełniane szybko.

---

### Punkt 2: „tx-only” — najczystsze wyłączenie AccountUpdate path end-to-end

**Decyzja (Twoja):** wyłączamy tylko downstream path (nie subskrypcję accounts w gRPC), aby nie ryzykować, że inne elementy korzystają z pool/global updates.

**Mapa aktualnego przepływu, który ma zostać odcięty**
1. Seer: `handle_account_update()` → `ipc.send_account_update(...)` (`off-chain/components/seer/src/lib.rs` + `off-chain/components/seer/src/ipc.rs`)
2. Ghost-launcher: odbiór `SeerEvent::AccountUpdate` → publikacja `GhostEvent::AccountUpdate` (`ghost-launcher/src/components/seer.rs`)
3. OracleRuntime: match `GhostEvent::AccountUpdate` → `oracle_runtime.process_account_update(...)` (reconciliation) (`ghost-launcher/src/oracle_runtime.rs`)

**Kroki implementacyjne**
1. Wprowadzić pojedynczy SSOT-flag w konfiguracji launchera
   - Nowa flaga w `ghost-launcher/src/config.rs` (np. w `[oracle]` lub `[seer]`, ale SSOT powinna być opisana jako „tx-only disables AccountUpdate reconciliation path”):
     - `account_updates_enabled: bool` z default `false` (bo tx-only).
   - W `config.toml` dodać czytelny komentarz i ustawienie.

2. Odciąć Seer → IPC dla AccountUpdate
   - W `off-chain/components/seer/src/lib.rs::handle_account_update()`:
     - nadal można parsować i logować (opcjonalnie), ale **nie wolno**:
       - pisać do ShadowLedger z AccountUpdate (`store_curve_with_snapshots(..., true)`),
       - wysyłać `ipc.send_account_update(...)`.
   - Najczyściej: w `Seer` dodać flagę `account_updates_enabled` i w `process_event()` na `GeyserEvent::AccountUpdate` zwracać `Ok(true)` bez efektów (lub `Ok(false)` jeśli chcesz nie liczyć jako „handled”).
   - Ważne: nie zostawiać „po cichu” częściowego działania (np. mapping curve→mint) jeśli downstream jest wyłączony, chyba że mapping jest potrzebny do tx-stream (zwykle nie jest, bo tx parser niesie mint).

3. Odciąć Ghost-launcher komponent Seer: nie publikować `GhostEvent::AccountUpdate`
   - W `ghost-launcher/src/components/seer.rs`: przy odbiorze `SeerEvent::AccountUpdate`:
     - w tx-only: ignorować (opcjonalnie licznik/debug log rate-limited),
     - w trybie legacy (gdybyś kiedyś włączył): zachować dotychczasowe mapowanie.

4. Odciąć OracleRuntime: brak reconciliation
   - W `ghost-launcher/src/oracle_runtime.rs` w głównej pętli `start_oracle_runtime_task()`:
     - match arm dla `GhostEvent::AccountUpdate`:
       - w tx-only: no-op + ewentualnie metryka „account_update_ignored_total” (opcjonalnie),
       - docelowo: usunąć wywołania `process_account_update()` z runtime w trybie tx-only.
   - Rozważyć usunięcie / feature-flag dla `OracleRuntime::process_account_update` jeśli ma być martwe; minimum: gwarancja, że w tx-only nie jest wywoływane.

5. Porządki i spójność komentarzy
   - W `off-chain/components/seer/src/grpc_connection.rs` komentarze BUG-3 oraz te o „pending_curve_updates jest sole owner” trzeba dopasować do nowej rzeczywistości:
     - jeśli AccountUpdate downstream jest wyłączony, te komentarze stają się mylące.
   - W testach seera, które zakładają refresh ShadowLedger przez AccountUpdate, dodać gating (feature/flag) albo zaktualizować oczekiwania.

**Relevant files**
- `/root/Gho/off-chain/components/seer/src/lib.rs` — `handle_account_update()`, `process_event()`.
- `/root/Gho/off-chain/components/seer/src/ipc.rs` — `send_account_update()` oraz typ `SeerEvent::AccountUpdate` (może pozostać dla kompatybilności).
- `/root/Gho/ghost-launcher/src/components/seer.rs` — odbiór `SeerEvent` i publikacja `GhostEvent`.
- `/root/Gho/ghost-launcher/src/oracle_runtime.rs` — match `GhostEvent::AccountUpdate`, ewentualnie `process_account_update` użycia.
- `/root/Gho/off-chain/components/seer/src/grpc_connection.rs` — komentarze i ewentualne logi „BUG-3 fix” (bez zmiany subskrypcji na tym etapie).

**Verification**
1. Testy integracyjne: uruchomić pipeline w trybie tx-only i potwierdzić, że:
   - ShadowLedger rośnie wyłącznie od tx events (commit_history/append_live),
   - żadne logi „CURVE_UPDATED source=account_update” nie występują (lub są jawnie oznaczone jako ignored),
   - `seer_account_updates_reconciliation_forwarded_total` nie rośnie.
2. Wykonać szybki „shadow run”/smoke: detekcja pooli + tx ingest działa bez AccountUpdate.

---

### Punkt 3: Metryki Prometheus: rejestracja + ekspozycja

**Stan obecny**
- `ghost-launcher/src/oracle_metrics.rs` definiuje metryki i `register_oracle_metrics(registry: &Registry)`.
- Brak call-site `register_oracle_metrics`.
- `ghost-launcher` nie wystawia `/metrics` (brak własnego serwera).
- W `config.toml` istnieje `seer.metrics_port = 9090` (w praktyce w launcherze można go użyć jako port procesu, ale lepiej dodać jawny `launcher.metrics_port`).

**Rekomendowana architektura** (zgodnie z Twoją decyzją: „Odpowiedz 1”)
- Uruchomić osobny serwer `/metrics` w procesie `ghost-launcher` (wzorowany na `seer/src/main.rs` lub `ghost-brain/src/metrics_server.rs`).
- Użyć **default global registry** (najprostsze): endpoint robi `prometheus::gather()`.
- `register_oracle_metrics()` rejestruje do **tego samego** default registry (czyli poprzez `prometheus::default_registry()` jeśli dostępne w 0.13; w przeciwnym razie: trzymać `static REGISTRY: Lazy<Registry>` i używać `REGISTRY.gather()` w serwerze).

**Kroki implementacyjne**
1. Dodać komponent/sekcję konfiguracyjną dla metryk procesu launchera
   - W `ghost-launcher/src/config.rs`: `MetricsConfig { enabled: bool, bind: String, port: u16 }`.
   - Migracja: jeśli nie chcesz zmieniać config teraz, użyj `seer.metrics_port` jako port procesu (ale to trzeba jasno opisać w config docs).

2. Dodać minimalny serwer HTTP `/metrics` w `ghost-launcher`
   - Nowy moduł np. `ghost-launcher/src/metrics_server.rs` (kopiuj wzorzec z `off-chain/components/seer/src/main.rs::start_metrics_server`).
   - Obsługa tylko `GET /metrics` (opcjonalnie `/healthz`).
   - Zbieranie:
     - wariant global: `prometheus::gather()` + `TextEncoder`.
     - wariant custom registry: `registry.gather()`.

3. Jednorazowa rejestracja metryk oracle przy starcie
   - W `ghost-launcher/src/main.rs` po inicjalizacji logowania/config:
     - wywołać `ghost_launcher::oracle_metrics::register_oracle_metrics(&registry)`.
   - Zapewnić, że rejestracja jest wykonana tylko raz (w praktyce: startup main jest single-shot).

4. Naprawić semantykę coverage gauge (żeby Prometheus nie kłamał)
   - W `ghost-launcher/src/oracle_metrics.rs`:
     - albo zmienić komentarz + nazwę gauge na „vs chain_truth”,
     - albo zmienić obliczenie na `shadow_ledger_total / grpc_received` (wymaga dostępu do liczb z `PipelineCoverageSnapshot`).
   - W tx-only, jeśli `chain_truth` bywa 0, to gauge nie może stale wisieć na 0% bez wyjaśnienia.

5. (Opcjonalne, ale ważne) Ujednolicić story „prometheus vs metrics crate”
   - `ghost-core` pipeline instrumentacja używa crate `metrics`, a nie `prometheus`.
   - Jeśli chcesz te dane w Prometheusie, potrzebujesz exportera/recorder dla `metrics` (np. `metrics-exporter-prometheus`) albo przenieść kluczowe wskaźniki do `prometheus`.
   - To nie jest konieczne do domknięcia Twoich gauge w `oracle_metrics.rs`, ale warto odnotować jako follow-up.

**Relevant files**
- `/root/Gho/ghost-launcher/src/oracle_metrics.rs` — rejestracja i semantyka gauge.
- `/root/Gho/ghost-launcher/src/main.rs` — miejsce startu serwera i rejestracji.
- `/root/Gho/ghost-launcher/src/config.rs` i `/root/Gho/config.toml` — port/bind dla `/metrics`.
- Referencje wzorców: `/root/Gho/off-chain/components/seer/src/main.rs` (prosty serwer), `/root/Gho/ghost-brain/src/metrics_server.rs` (custom registry).

**Verification**
1. Smoke: uruchomić `ghost-launcher` i sprawdzić, że `curl http://127.0.0.1:<port>/metrics` zwraca tekst w formacie Prometheus.
2. Sprawdzić obecność serii:
   - `pool_identity_promotion_attempts_total{result="success"|"failure"}`
   - `pool_identity_exhausted_total`
   - `shadow_ledger_committed_pools`, `shadow_ledger_total_snapshots`
3. Sprawdzić, że coverage gauge ma spójny opis i nie stoi permanentnie na 0 przez mianownik==0.

---

## Decyzje
- Tx-only: wyłączamy downstream AccountUpdate path end-to-end (Seer→IPC→Launcher→OracleRuntime), bez ryzykownej zmiany subskrypcji gRPC na tym etapie.
- Identity backoff: przechodzimy na exponential backoff czasowy, testy sterują czasem (brak zależności od realnego zegara).
- Prometheus: osobny `/metrics` server w `ghost-launcher` (nie w GUI), z rejestracją metryk na starcie.

## Further considerations
1. Jeśli kiedyś zechcesz wrócić do AccountUpdate jako safety-net, najczyściej utrzymać to jako osobny feature flag („reconciliation_enabled”), a nie implicit behavior.
2. Warto dodać metrykę/licznik „account_update_ignored_total” w tx-only, żeby wykryć przypadkowe źródła AccountUpdate i potwierdzić, że odcięcie działa.
