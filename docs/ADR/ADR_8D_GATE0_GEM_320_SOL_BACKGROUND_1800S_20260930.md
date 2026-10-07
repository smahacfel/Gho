# ADR-8D: Gate0 — próg Gem 320 SOL i run 1800 s w tle

Status: walidacja zakończona; bezpośrednio po tym zapisie start odłączonego runu 1800 s na polecenie użytkownika, 2026-09-30.
Zakres: wyłącznie definicja Gem w obserwatorze Gate0 i uruchomienie na 1800 s.

## 1. Przygotowanie i działania wstępne

Użytkownik polecił zmianę 350→320 SOL, uruchomienie na 1800 s i pozostawienie procesu bez monitorowania do jego powrotu. Sprawdzono dirty tree, aktywny warunek record_reserves, testy i istniejący runner. Poprzednie wyniki 350 SOL pozostają historyczne.

## 2. Skills i routing

Rola główna: Config Rollout Safety Reviewer. Dokument: docs/agents/config-rollout-safety-reviewer.md. Skills: ghost-execution, rust-master, trading-systems (odczytane wcześniej w tym wątku). Rozważono Decision Logging Replay; dodatkowy dokument zbędny przy addytywnym polu run_start. Brak osobnych subagentów.

## 3. Opis problemu — 3W2H

Dotychczasowy stały próg minimum obserwowanego MC po migracji wynosił 350 SOL. Operator ustalił nową definicję 320 SOL. To kryterium etykiety Gem przy ukończeniu 600 s, a nie próg utworzenia snapshotu Phase V.

## 4. Przyczyna zmiany

Bezpośrednie polecenie operatora. Nie jest to estymacja statystyczna ani wynik optymalizacji na GO-D. Brak potrzeby analizy zewnętrznej/RPC.

## 5. Strategia

Najmniejsza zmiana istniejącej stałej definicji: GEM_MIN_MARKET_CAP_SOL=320. Jedno źródło dla porównania całkowitoliczbowego i pola gem_min_market_cap_sol w run_start. Zachowano nierówność minimum >= próg i pamięć wcześniejszego zejścia poniżej progu.

## 6. Wykonane zmiany

- ghost-launcher/src/gate0.rs: próg 320 SOL i jawne pole run_start.
- ghost-launcher/src/gate0/tests.rs: dokładnie 320, 334.2, jeden lamport poniżej 320, późniejszy powrót powyżej progu i granica migration age 3000/3001 ms.
- docs/GATE0.md: aktywna definicja 320 SOL.
- gate0.md i .codex/active-task.md: bieżące polecenie i zasada bezwzględnego braku monitorowania po uruchomieniu.

C/D/E, wiek faz, retencja, endpoint, materializer i execution pozostają niezmienione. Pole run_start jest addytywne. Rollback kodu: przywrócić stałą 350 i odpowiadające testy przed nowym runem; nie zmieniać aktywnego procesu.

## 7. Walidacja

PASS: Gate0 18/18, formatter, git diff --check, release build. Logi: /tmp/gate0_320_tests.log i /tmp/gate0_320_release.log. Self-review zakończony przed uruchomieniem. Smoke 1800 s użyje nowej release binarki, metadanych SHA256 i bezpiecznego przekazania ingest credentials przez środowisko.

## 8. Zabezpieczenia antyregresyjne i self-review

Sprawdzono: dokładne porównanie u128; brak zmiany C/D/E; wcześniejszy spadek poniżej progu nadal wyklucza Gem; wiek migracji i niekompletność nadal obowiązują; brak execution. Nie klasyfikuje się ponownie starych artefaktów. Bez commit/push/PR. Po potwierdzeniu startu nie odczytywać procesu, logów ani artefaktów i nie wprowadzać dalszych zmian do powrotu użytkownika.

Uwaga o formacie: wymagany ADR_8D_SZABLON.md był niedostępny; zachowano osiem sekcji istniejących ADR-8D, jak ustalono w poprzednim etapie.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie był wejściem tego zadania; GO-E nie wykonywano.

Release SHA256: `ba6cdd383dc445246825fcda03fb373644cb965ddd750f53b2b8ea7e25feff83`. Faktyczne potwierdzenie startu/PID: metadane `gate0_320sol_1800s_*.meta.json`. Po potwierdzeniu startu agent kończy pracę bez dalszych kontroli.
