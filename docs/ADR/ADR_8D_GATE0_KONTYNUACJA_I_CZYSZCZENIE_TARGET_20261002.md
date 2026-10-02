# ADR-8D: Gate0 — kontynuacja runu po błędach danych i czyszczenie nieaktywnych targetów

## 1. Przygotowanie i działania wstępne
Polecenie operatora z 2026-10-02: wyłączyć guardy zatrzymujące cały run przy problemach danych i posprzątać targety projektów nieaktywnych od co najmniej trzech dni. Sprawdzono HEAD803e1a5, dirty worktree, brak aktywnego Gate0, metadane i końcówkę ostatniego runu. Nie zmieniano zastanych endpointów ani sekretów. Wskazanego szablonu `/Gho/docs/ADR/ADR_8D_SZABLON.md` ani odpowiednika w `/root/Gho` nie ma; użyto ośmiu sekcji sąsiednich ADR-8D Gate0.

## 2. Wykorzystane skills i role
Ghost execution, Rust master, Solana pumpfun architect. Główna rola Oracle Session Runtime Engineer, pomocnicze Seer Ingest Event Integrity Specialist i Decision Logging Replay Analyst. Przeczytano odpowiednie dokumenty. Praca bez osobnych subagentów. Zakres obejmuje wyłącznie obserwator Gate0, jego CLI, skaner i dokumentację; zwykły runtime handlowy nie otrzymuje polityki kontynuacji.

## 3. Opis problemu
Run gate0-1790842067965 zakończył się po około5317s, exit1. Zapisany konflikt: ta sama sygnatura, najpierw failed w slocie452249038 / tx_index1117, następnie success w slocie452249034 / tx_index91, odstęp odbioru290ms. Nie było dev_volume_ratio range, dropów ani wygasłych pending mappings. Dotychczasowy `bail!` przenosił lokalny konflikt do całego procesu. Analogicznie propagowały się limity, błędy materializacji, źródła i watchdogów.

## 4. Przyczyny i granice dowodu
Potwierdzony mechanizm zatrzymania jest w `check_transaction_outcome` i propagacji Result w CLI. Nie ustalano źródłowej przyczyny dwóch statusów przez RPC ani nie zakładano winy sieci. Celem jest zmiana polityki awarii na wyraźne polecenie operatora, nie zamiana niepewnych danych w poprawne dane. Brak miejsca/uszkodzenie zapisu i błędna konfiguracja nie mogą być przedstawiane jako działający zapis. C/D/E nadal kończą pojedyncze obserwacje zgodnie z definicją badania.

## 5. Rozwiązanie
Gate0 zapisuje `error_policy=quarantine_and_continue`. Konflikt cenzuruje wyłącznie token (`gem:null`), zachowuje oba dowody i oznacza utratę ciągłości wspólnego CPV. Błędy materializacji cenzurują dotknięty token i nie blokują następnych checkpointów. Granice Result handlerów przechwytują błędy danych/limitów; błędy IO pozostają jawne. Luki źródła/IPC cenzurują aktywną kohortę, ale admission trwa do pierwotnego deadline. CLI zastępuje przerwania za stale progress, pending expiry i koniec źródła zdarzeniami runtime_issue. Po zakończeniu Seera/zamknięciu IPC/30s bez postępu tworzy nowe źródło w tym samym procesie; próby oddziela co najmniej5s i ogranicza deadline admission. Zamykanie starego transportu jest ograniczone czasowo. Błędy zamykania po deadline są zapisane jako issue, nie udają czystego zamknięcia.

Skaner zachowuje odrzucenie starego nieobsłużonego konfliktu. Nowy konflikt wymaga jawnej polityki i terminala kwarantanny; wszystkie snapshoty takiego tokena są wyłączone z porównania Gem/non-Gem. Raport zawiera runtime_issues i data_quality=degraded. Ciągłość procesu nie jest dowodem ciągłości źródła. Końcowe queue counters odnoszą się do ostatniego segmentu po reconnect; wcześniejsze są zapisane przy source_restart.

## 6. Przeprowadzone akcje naprawcze
Zmiany: gate0.rs, gate0/tests.rs, bin/ghost_gate0.rs, gate0_scan.py i testy, komentarz configs/gate0.toml, docs/GATE0.md, gate0.md. Bez zmian progów, Gem320, confirmed i execution_enabled=false.

Czyszczenie: wyłącznie rozpoznane katalogi Cargo o nazwie target, po sprawdzeniu mtime/ctime wszystkich plików projektu i aktywnych cwd/exe/fd/maps. atime nie stanowi dowodu aktywnej pracy, bo jest zmieniany również przez ogólne skany i rozwiązywanie symlinków. Usunięto14targetów; odzyskano24.35GiB według wolnej przestrzeni systemu plików. Lista i pomiary: `/tmp/gate0_continuity_20261002/cleanup_result.json` i cleanup_report.md. Źródła, Git, konfiguracje i logi pozostawione. `/root/tmp` nie istnieje; `/tmp` nie objęto usuwaniem bez rozstrzygnięcia ścieżki.

## 7. Walidacja działań naprawczych
Gate031/31, CLI3/3, Python8/8 PASS; rustfmt/diff-check i Clippy --no-deps PASS (istniejące ostrzeżenia). Release [optimized] PASS. Artefakty `/tmp/gate0_continuity_20261002/`. Test logicznych10h wstrzyknął po150konfliktów, awarii snapshotu i luk źródła; kolejne150obserwacji zakończyło się normalnie. To czas symulowany, nie10h czasu rzeczywistego.

Dwie próby rzeczywistej binarki na lokalnym źródle: odmowa TCP oraz serwer przyjmujący połączenie bez danych. Obie osiągnęły35s admission (35.03s procesu), wykonały1restart źródła, zachowały1run_start/run_end i zakończyły się exit0/reasonnull/shutdownnull. Skaner oznaczył obie degraded/INSUFFICIENT_DATA. Pierwsza próba12s nie obejmowała30sgrace, więc nie była dowodem restartu; zastąpiono ją próbami35s. Historyczny przerwany run jest nadal odrzucany przez skaner.

Release preflight60s `gate0-1790928282659`: exit0,28births/terminals,9snapshotów,0runtime_issues,0reconnect,0drops/expiry,maxlag30ms,shutdownnull,reason=smoke_only. Hash kandydata `bdc35043ea96039660bae7279299ea9a10542baeb7aedb3a099bc93dad9ee668`; końcowy powtórzony build ma identyczny SHA256 jak ta kopia. To odbiór źródła, nie dowód10h.

## 8. Zabezpieczenia antyregresyjne i self-review
Sprawdzamy: kwarantanna nie staje się negative label, błędny token nie blokuje kolejnych tokenów, snapshoty nie są przepisywane, limity pamięci są nadal ograniczone, brak busy-loop zamkniętych kanałów, reconnect nie resetuje czasu, błędy zapisu nie są maskowane, skaner nie dopuszcza nieobsłużonego konfliktu. Restarty source nie uruchamiają wykonania transakcji. Odbiór pełnych10h wymaga osobnego końcowego artefaktu runu.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie jest wejściem tej zmiany.
