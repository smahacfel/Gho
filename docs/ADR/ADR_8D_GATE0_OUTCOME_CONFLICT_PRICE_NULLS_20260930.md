# ADR-8D: Gate0 — konflikt statusów transakcji, nieaktualna cena i nieprecyzyjne przyczyny null

Data: 2026-09-30. Status: IMPLEMENTACJA + TESTY OFFLINE + SOURCE SMOKE PASS; LIVE MIGRATION / LIFECYCLE600s NIEZWERYFIKOWANE.
Repo: `/root/gho_gate0_20260927`, branch `agent/gate0-600s-20260927`, HEAD `38acb83f492dc55b9244f0461c2d6b9e513771e4`. Wyłącznie lokalnie.

Wskazany `/Gho/docs/ADR/ADR_8D_SZABLON.md` jest niedostępny, również w `/root/Gho` i bieżącym repo. Zachowano osiem sekcji istniejących ADR-8D; nie deklarujemy zgodności z nieodczytanym szablonem.

## 1. Przygotowanie i działania wstępne

Użytkownik wrócił po runie 1800 s i polecił zidentyfikować oraz naprawić błędy. Wykorzystano istniejące artefakty `gate0_320sol_1800s_20260930T143906Z` z `/root/gho_gate0_runs_20260927`. Run trwał około 1138 s i zakończył się `conflicting transaction outcome in Gate0`; 608 births i terminali, 989 snapshotów, brak Phase V. Nie zachował sygnatury konfliktowej transakcji. Nie utożsamiono 43 ocenzurowanych terminali z 43 konfliktami.

Przed zmianami zachowano pliki i status w `/tmp/gate0_failure_repair_20260930/before`. Zastane zmiany użytkownika pozostawiono. Nie zmieniano sekretów, endpointu ani niezależnego procesu trenchd.

## 2. Wykorzystane skills i role

Skills: ghost-execution, rust-master, trading-systems, solana-pumpfun-architect. Główna rola: Seer Ingest Event Integrity Specialist. Wsparcie logiczne: Config Rollout Safety Reviewer, Decision Logging Replay Analyst i SSOT Feature Materialization Guardian. Przeczytano właściwe dokumenty specjalistyczne; bez uruchamiania subagentów.

## 3. Opis problemu

1. Gate0 wymaga niezmienności statusu sygnatury, lecz dziedziczył `processed` bez obsługi rollbacku forka. Sprzeczność powodowała globalny stop bez zachowania tożsamości ani obu obserwacji; dodatkowo konfliktowy sukces mógł wcześniej zmutować CPV.
2. Producent fingerprintu zapisał `EARLY_TOP3_BUY_VOLUME_ZERO`, lecz adapter zgubił ten powód w dwóch snapshotach.
3. Braki `market_cap_sol` i `price_change_ratio` miały ogólną przyczynę. Wszystkie sześć braków z runu dotyczy tokena `FWRx7U4G3yg76x2Xs1s84BZjPdAxk625nHhU4xaDpump`, ze znanymi `missing_amm_state` i `missing_migration_create`. Przegląd ujawnił dodatkowo zachowywanie poprzedniej ceny po nieprawidłowych rezerwach.
4. Parser pomijał stan inicjalizacji AMM przy obecności swapa w tym samym TX; Gate0 nie wykorzystywało samodzielnego zweryfikowanego CreatePoolEvent bez oddzielnego PoolDetected. Dwie nowe regresje odtworzyły te ścieżki.
5. CPV podawało tylko zbiorcze `CPV_ROLLING_STATE_UNAVAILABLE`, niewystarczające do rozróżnienia historii, cutoffu i watermarku.

## 4. Przyczyny i granice dowodu

Błędy adaptera i zachowania nieaktualnej ceny odtworzono trzema testami RED. Brak obsługi zmienności źródła processed wynika z kodu konfiguracji i obserwatora. Nie udowodniono, że konkretny historyczny konflikt spowodował fork: pierwotny JSONL nie zawiera pary obserwacji.

Pomocnicza sonda odczytała bezpośrednie protobufy Yellowstone `processed`, bez Seera i RPC backfillu: 600 s, 607775 transakcji, zero nieprawidłowych sygnatur, brak konfliktów i redostaw w zachowanym oknie sondy (150 s, do 250000 sygnatur). Zakończenie `DEADLINE_EXCEEDED` było zaplanowanym limitem czasu. To nie jest dowód nieistnienia historycznego konfliktu. Artefakty: `/tmp/gate0_failure_repair_20260930/raw_outcome_probe_summary.json`, `raw_probe.log`, `probe_outcomes.py`.

Kontrakt commitment zweryfikowano w [dokumentacji Yellowstone](https://docs.triton.one/project-yellowstone/dragons-mouth-grpc-subscriptions#managing-commitment-levels): confirmed/finalized buforują powiadomienia do wybranego poziomu; processed wymaga osobnego rozstrzygania późniejszych statusów slotów. `confirmed` nie jest `finalized` ani gwarancją nieomylności providera.

## 5. Rozwiązanie

Jawnie użyć confirmed w obserwatorze bez rollbacku. Zachować fatalny guard sprzeczności oraz pierwszy kompletny dowód konfliktu. Brak poprawnych danych ceny ma dawać null z konkretną przyczyną. CPV zachowuje obecną definicję i cutoffy; dodatkowe reason codes wyjaśniają niedostępność bez imputacji lub lookahead.

## 6. Przeprowadzone akcje naprawcze

- `gate0.rs`, `ghost_gate0.rs`, `configs/gate0.toml`: serde-default `source_commitment=confirmed`, przekazanie do Seera i zapis w run_start.config. Primary i funding korzystają z tego samego commitment. Domyślna konfiguracja zwykłego Seera bez zmian.
- `gate0.rs`: zamiast samych zbiorów sygnatur ograniczone istniejącym budżetem mapy zachowują pierwszy status, slot, tx_index, received_ms i error_code. Guard działa przed CPV. `transaction_outcome_conflict` zawiera oba dowody. Nie wybieramy zwycięskiego statusu i nie kontynuujemy po sprzeczności.
- `gate0.rs`: unieważnienie bieżącej ceny po nieprawidłowych rezerwach lub niekompletnym stanie AMM; historyczne label_reasons pozostają.
- `metrics.rs`: zachowanie EARLY_TOP3 przyczyn i statusu insufficient_sample dla braku dodatniego wczesnego BUY; precyzyjne przyczyny brakującej ceny i price ratio.
- `cross_pool_velocity.rs`: dodatkowe kody przyczyn, m.in. CPV_LOOKBACK_NOT_COVERED i CPV_PROGRESS_BEHIND_ANCHOR; logiczny warunek ready i wartości metryk bez zmiany.
- `gate0_scan.py`: jawne odrzucenie rekordu konfliktu, również przy fałszywie czystym run_end.
- `binary_parser.rs`: tylko opt-in Gate0 zachowuje stan inicjalizacji przy explicit swap w tym samym TX. Surowy CreatePoolEvent nadal musi przejść dopasowanie programu, puli, minta, WSOL i decimals.
- `gate0.rs`: kanoniczny, successful, zweryfikowany CreatePoolEvent może ustanowić migrację przed osobnym PoolDetected; nie zwiększa C/D/E, wolumenu, retencji ani CPV. Redostawa/późniejszy PoolDetected nie zmienia czasu migracji. Zwykły swap nie ustanawia migracji.
- Testy, docs/GATE0.md, gate0.md, checkpoint i ten ADR.

Nie zmieniono C/D/E, progu Gem 320 SOL, faz ani limitów retencji. Moment dostępności danych przy confirmed różni się od processed; runy muszą być rozróżniane przy analizie. Nie używano GO-D jako danych wejściowych.

## 7. Walidacja działań naprawczych

RED: 18 poprzednich testów Gate0 PASS, trzy nowe FAIL. Pierwszy GREEN: Gate0 22/22, CPV 31/31, CLI 3/3, analizator Python 7/7. Po poprawce migracji uruchomiono finalną walidację. Parser: 135/135 PASS. Regresje obejmują obie kolejności konfliktu, brak mutacji CPV/liczników przy sprzeczności, zachowanie pierwszego dowodu, niewypełnianie historycznego cutoffu późniejszym progress oraz stare pliki konfiguracji.

Logi i manifest źródeł: `/tmp/gate0_failure_repair_20260930/`. Wynik kompilacji release, Clippy i nadzorowanego smoke zostanie dopisany po ich zakończeniu.

## 8. Zabezpieczenia antyregresyjne i self-review

Sprawdzono SSOT materializera, niezmienność snapshotów, rozdzielenie źródła/wykonania, deduplikację successful i failed oraz fail-closed analizatora. Dodatkowy rekord nie powoduje akceptacji uszkodzonego datasetu. Ceny po błędnym stanie nie korzystają ze starej wartości. Nie ma naprawiania historycznych snapshotów późniejszymi danymi.

Brak Phase V nie jest sam w sobie błędem kodu: progi C/D/E mogą wyeliminować wszystkie tokeny. Nie zmieniano tych kryteriów, aby uzyskać PASS. Pełny odbiór lifecycle i gotowość do 10 h wymagają odrębnego dowodu. Sam smoke po zmianach nie identyfikuje brakującej pary z historycznego konfliktu.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

### Pośredni smoke 360 s i dalsza korekta migracji

Binarka `77e08ff1471cc45c4b3f367a97caf326028d9a3e722d00916ba3629a2f5e1e3b` (przed korektą migracji): `/root/gho_gate0_runs_20260927/gate0_repaired_confirmed_360s_20260930T160135Z.*`. Exit0, reason=smoke_only, shutdown_error=null, 224 births/terminals,312 snapshotów, fazy207/64/41/0/0,2 migracje+initial. Bez naruszeń60pól,224znanych creatorów,0 genericnulls. Primary368168/funding511378,0drop/expiry,lag32ms,high-water1029/556 przy capacity16384. CPV40clean/11insufficient/261source unavailable; wszystkie niedostępne mają CPV_LOOKBACK_NOT_COVERED,5 również watermark za anchorem. Stały epoch1 i początek ciągłości1790784095764. Po305s pozostałe16source-unavailable nadal ma za wczesny anchor dla pełnego lookbacku, a nie reset ciągłości.

Audyt wykazał 3 snapshoty z missing_migration_create dla dwóch mintów. To skłoniło do opisanych powyżej dwóch dodatkowych poprawek, odtworzonych RED. Nie stwierdzono, że każdy brak migracji w historycznym runie ma tę samą przyczynę. Ten smoke nie weryfikuje późniejszej korekty parsera ani Gate0 i nie jest końcowym dowodem binarki. Manifest tej rewizji zachowano jako source_manifest_before_migration_fix.json.

### Końcowa walidacja po poprawce migracji

Gate0 **24/24**, parser **135/135**, CPV **31/31**, CLI **3/3**, Python **7/7** PASS. Osobno powtórzony test kolejności CreatePool→swap PASS. `cargo fmt --check`, `git diff --check`, Clippy --no-deps i release build PASS; zastane ostrzeżenia kompilatora nie były rozszerzane w tym zadaniu.

Finalna binarka: `27c4638ccb4efc8972cac0e0ba3aaecbe3ca0d6fa8014e4120a366c727ff7302`. Source SHA256: `6fa2d475dbaa13ebb17fe7998cc9650a1d938dd60cd366e65823022b5be4bfe5`. Config SHA256: `ac55f1072e3a2e5eb6f023ef8000eb45ada1866e849d4a63a389b97d690f10e2`. Manifest źródeł identyczny przed buildem i po smoke.

Finalny smoke60s: `/root/gho_gate0_runs_20260927/gate0_repaired_confirmed_60s_20260930T162216Z.*`. Exit0, reason=smoke_only, shutdown_error=null.38births/38terminali,17snapshotów,0naruszeń60pól,0ogólnych powodów braku wartości,38known creators. Primary34842/funding74968,zero drop/expiry,lag9ms,high-water267/437. Nie zaobserwowano migracji, więc nie deklarujemy potwierdzenia poprawki migracji na nowym przypadku live. Nie było PhaseV ani completed600s. Źródło i proces PASS, pełny Gate0 nadal NOT READY do10h.

Raporty60pól `.audit.md` i `.audit.json`; zbiorcza walidacja `/tmp/gate0_failure_repair_20260930/validation_summary.json`. Metadane obu smoke jawnie oznaczono `accepted=false` dla datasetu badawczego, `source_smoke_pass=true`; nie mylić ze zgodą na GO strategii.

Końcowy self-review wykonanego diffu: sprawdzono opt-in parsera, autorytet CreatePoolEvent, canonical pool/WSOL, znany sukces, zachowanie czasu pierwszej migracji, brak inicjalizacji w C/D/E/CPV, pierwsze dowody sprzeczności przed mutacją i null po nieprawidłowym stanie. Zachowano Gem320 SOL, pozostałe progi i zastane zmiany. Bez commit/push/PR/merge, bez zmian .env i endpointu. Po smoke brak aktywnego ghost_gate0. Nie uruchomiono kolejnego1800s ani10h.
