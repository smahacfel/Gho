# ADR-8D: publikacja kompletnego Gate0 i pierwszy run 10 h

## 1. Problem i autoryzacja
Użytkownik polecił opublikować wszystkie prace Gate0 obecnego i wcześniejszych wykonawców jako nowy PR, a następnie uruchomić pierwszy run 10 h. Nie zlecono merge. Szablon /Gho/docs/ADR/ADR_8D_SZABLON.md nie jest dostępny; zachowano osiem sekcji stosowanych przez wcześniejsze ADR.

## 2. Zakres
Obserwator i pięć checkpointów, metryki i jakość, CPV/dev/FSC/DES, potwierdzona migracja PumpSwap, retencja, fail-closed, telemetria, keepalive, statyczna subskrypcja Gate0, testy, skaner i dokumentacja. Wcześniejsza migracja hostname endpointów poza Gate0 pozostaje lokalna. Pliki .env i lokalne notatki agentów nie są publikowane.

## 3. Dowody odbioru
Run gate0-1790803700426: 1800,568 s, exit0, brak reconnectów, resubscribe i lokalnych dropów. 1172 obserwacje, 1885 snapshotów, 4 pełne lifecycle600s, 3 z migracją i initial AMM. Jest to smoke_only, nie przyjęty zbiór badawczy. Szczegóły i znane ostrzeżenia w ADR statycznej subskrypcji.

## 4. Przyczyna dodatkowej niestabilności testu
Powtórzenie testów przed publikacją wykazało, że porównanie ordinary/observation SELL porównuje owner_token_deltas w losowej kolejności dwóch HashMap. Wartości i ownerzy były identyczne, różniła się kolejność tablicy. Poprawka wyłącznie w cfg(test): sortowanie pełnych rekordów przed porównaniem, bez usuwania pól lub wartości.

## 5. Rozwiązanie i kontrakty
Publikacja przez jawną listę plików i wyłączenie zmian endpointów także w plikach mieszanych. Runtime pozostaje observe-only. Próg Gem320SOL, C/D/E, confirmed i guardy bez zmian. Korzystamy z binarki release sprawdzonej w runie1800, SHA256 1ce19530d144998e36f207c26d175cdc5307a27b5c1aca1fd5d66933bfa0ae74. Jedyna późniejsza zmiana Rust dotyczy asercji testowej. Manifest lokalnych źródeł dokumentuje także pozostawione zmiany endpointów, więc hash commita sam nie jest deklarowany jako pełny opis środowiska buildu.

## 6. Uruchomienie
Po commit/push i utworzeniu PR: configs/gate0.toml z admission_ms=36000000, bez --smoke-seconds. Zamknięcie admission po10h, potem naturalny drain do600s. Ten sam endpoint operatora i funding. Osobne JSONL/log/meta; wrapper redaguje token ingestu, zapisuje PID, SHA binarki, źródeł i configu oraz kod zakończenia. Bez automatycznych restartów. Po potwierdzeniu startu brak ingerencji do powrotu użytkownika.

## 7. Weryfikacja
Manifest źródeł przed poprawką testu zgodny z odbiorem1800; SHA binarki niezmienione. Gate0 24/24, transport99/99, metryki183/183 (w tym CPV31), AMM3/3, CLI3/3 i Python7/7 PASS. Parser pierwotnie134/135 z udowodnioną różnicą kolejności; po poprawce i ponownej kompilacji135/135 PASS; dodatkowo20/20 powtórzeń regresji oraz transport99/99 PASS, logi w /tmp/gate0_publish_10h_20260930. Poprzednie fmt/Clippy/release PASS zachowują osobny zakres. Wynik10h nie jest jeszcze znany.

## 8. Self-review i ograniczenia
Przegląd zakresu, profili opt-in, retencji, statusów transakcji, źródeł ceny i nulli, niezmiennych cutoffów oraz admission/drain. Pełny diff jest artefaktem do review w nowym PR. Publikacja nie oznacza merge ani PASS CI. Pozostają udokumentowane ostrzeżenia parsera i granice diagnostycznego runu1800. Osobny manifest i metadane wskazują rzeczywisty build/run.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie jest wejściem tego runu.
