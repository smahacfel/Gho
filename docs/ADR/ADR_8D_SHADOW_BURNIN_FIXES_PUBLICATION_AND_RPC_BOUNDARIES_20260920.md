# ADR-8D: Publikacja poprawek shadow burn-in i granice RPC/gRPC

## D0. Dyspozycja

Zacommitować i opublikować jako nowy PR dotychczasowe poprawki tej sesji. Wyjaśnić na podstawie kodu, czy obecny odbiór danych i BUY zależą wyłącznie od gRPC. Audyt nie upoważnia do niejawnego wyłączenia RPC ani zmiany działającego runa.

## D1. Baza i izolacja

Baza e5122fa1fa0321f249905b1e3aada936d36ba5a3, gałąź agent/pump-research-go-d-frozen-authority, PR93. Nowy PR jest zależny od tej gałęzi. Osobny worktree /root/Gho_shadow_burnin_pr_20260920 i osobny zapisywalny CARGO_TARGET_DIR. Zastane zmiany badań/capture, AGENTS i dokumentacji agentów pozostają poza PR.

## D2. Zakres

Pomiar gRPC→przekazanie kandydata i opcjonalny budżet admission; niezależność tożsamości mint/pool i parser CPI PumpSwap; punktowe potwierdzenie świeżości wyjścia shadow; batche RPC i równoległość przygotowania BUY; poprawiony timestamp wejścia; decision_to_buy_ms; opcjonalny deadline z domyślną wartością None. Końcowy profil ma TP50/SL25 i wyłączony deadline BUY. Dołączono bieżący brain config oraz ADR operacyjne sesji.

## D3. Odtworzenie zmian

Zmiany przeniesiono względem kopii sprzed poszczególnych zadań. We współdzielonych plikach zastosowano tylko należące do nich fragmenty diffu. Kompilacja wykryła dwa wymagane konstruktory SeerConfig oraz dwa wywołania testowe wymagające dopasowania do bazowej wersji; zostały uzupełnione.

Baza zawiera include_bytes! do nieśledzonego fixture `ghost-core/tests/fixtures/pump_observation_ledger_v1/pump_observation_differential_corpus_v1.jsonl`. Dołączono istniejący plik bez zmian, aby kompilacja po pobraniu PR nie zależała od lokalnego nieśledzonego pliku. 33 wiersze,75123 bajty,SHA256082531eb8c1d2c32524c5b1272790f3d56e0a9d2c172b6ba2d0be7209a85bb4e. Nie jest to nowa taśma ani modyfikacja danych historycznych.

## D4. Ustalenia RPC/gRPC

Aktywny profil r13 wskazuje source_mode=grpc, commitment=processed. Nie ustawia grpc_manual_backfill_enabled, którego domyślna wartość to true. GrpcConnection uruchamia osobny worker uzupełniający luki przez getSignaturesForAddress/getTransaction i oznacza wynik grpc_backfill. Backfill nie otwiera nowych sesji CandidateAdmission.

Parser Seera otrzymuje endpoint RPC i uruchamia asynchroniczny worker hydracji BCV2 przez getAccountInfo. Zapisuje execution-account evidence; nie jest to zwykły canonical update z gRPC. Osobny worker nie oznacza szeregowego oczekiwania odbiornika gRPC na każdy taki odczyt.

Po werdykcie Gatekeepera IWIM pobiera historię dewelopera przez RPC. Trigger odczytuje mint,payer,ATA,blockhash i sprawdza wymagane konta; dispatch czeka na wymagane wyniki. RpcShadowSimulator wykonuje simulateTransaction. Te operacje uczestniczą w czasie decyzja→wynik BUY. Potwierdzenie świeżości wyjścia jest osobnym, wcześniej zatwierdzonym przez użytkownika wyjątkiem: asynchroniczny odczyt konkretnej krzywej przy StaleSnapshot, bez zmiany canonical timestamps lub inactivity.

Wniosek: bieżący system nie spełnia literalnej zasady „wszystkie dane on-chain tylko gRPC, bez RPC”. Nie przypisujemy pomiaru opóźnienia BUY do odbioru pakietów gRPC. Usunięcie zależności od RPC wymaga osobnego określenia źródeł danych, kompletności streamu i sposobu symulacji; nie jest częścią publikacji tych poprawek.

## D5. Weryfikacja

Kompilacja biblioteki testowej launchera PASS. 44 testy candidate_integrity/precheck/deadline/zapisu wejścia PASS;9testów quote_confirmation PASS;5testów parsera,tożsamości i czasu przekazania kandydata Seera PASS. Razem58PASS/0FAIL. Biblioteki testowe ghost-brain i Seer skompilowane w oddzielnym target-lean (5m35s). Kontrola staged diff bez błędów whitespace. Pełnego zestawu workspace nie uruchamiano. Dowody /tmp/ghost-shadow-pr-20260920/. Wcześniejsze testy i runtime opisują poszczególne ADR; ich wynik nie jest automatycznie wynikiem wydzielonego PR.

Podczas oddzielnej kompilacji testów ghost-brain stwierdzono brak wolnego miejsca. Przerwano własny build i usunięto wyłącznie własny target/cache w /tmp/ghost-shadow-pr-20260920/target. Dalsza walidacja używa target-lean, CARGO_PROFILE_DEV_DEBUG=0,CARGO_PROFILE_TEST_DEBUG=0,CARGO_INCREMENTAL=0. R13 pozostał uruchomiony; punktowa kontrola wykazała świeży log, writes_succeeded3408,writes_failed0,queue_full_drops0, bez ENOSPC w końcowym fragmencie logu. To kontrola ograniczona, nie dowód pełnej ciągłości każdego zapisu w chwili zapełnienia dysku.

## D6. Przegląd

Przegląd zakończony: porównanie własnych fragmentów z kopiami wejściowymi, kontrola nowych plików i zależności, addytywność pól JSON, wyłączony deadline w profilu i brak literalnych sekretów w konfiguracji. Nie publikujemy surowych logów runów ani plików .env. Stage obejmuje jawną listę37plików, zapisaną w allowlist.json.

## D7. Publikacja

Commit/push/nowy PR jawnie zatwierdzone przez użytkownika. Gałąź publikacji fix/shadow-burnin-latency-and-lifecycle, baza agent/pump-research-go-d-frozen-authority. Nie wykonujemy merge. Numer PR i zgodność lokalnego/zdalnego SHA są potwierdzane metadanymi GitHub i zapisywane w checkpointcie operatora; nie utożsamiamy lokalnych testów z wynikiem CI.

## D8. Stan runtime

R13 pozostaje w dotychczasowym tmux. Nie wykonano restartu, zmiany progów ani wyłączenia RPC na podstawie samego pytania o architekturę.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie jest wejściem eksperymentu w tej publikacji; zachowano istniejący fixture bez zmian.
