# ADR-8D: Nowy shadow r7 z TP +25%, SL -50% i usunięciem starych logów tekstowych

## D0. Zakres

Użytkownik zlecił nowy run z tym samym configiem, obniżenie take profit z +50% do +25%, zachowanie stop loss -50% oraz usunięcie starych logów typu system.log i oracle_decisions.log. Użyto lokalnego układu D0–D8; wskazany szablon ADR nie był dostępny w poprzedniej weryfikacji repo.

## D1. Stan wejściowy

Aktywny r6: PID565326, profil /tmp/predator-v11-20260919-candidate-identity-r6.toml. Brain: configs/rollout/ghost_brain_predator_v11_shadow_burnin_20260916.toml. Aktywny guardian miał target_threshold=50.0 i stoploss_threshold=50.0. Kod EffectiveExitPolicyV1Config dzieli te wartości przez 100; niezależne historyczne pola exit_strategy nie stanowią tej polityki.

## D2. Przyczyna zmiany

Zmiana strategii na wyraźne zlecenie użytkownika. Nie jest naprawą błędu ani kalibracją wykonaną przez agenta.

## D3. Rozwiązanie

W tym samym pliku brain zmieniono wyłącznie post_buy_guardian.target_threshold na25.0. stoploss_threshold=50.0 pozostał bez zmian. Nowy profil /tmp/predator-v11-20260919-tp25-r7.toml zmienia namespace i spójnie ustawia trigger.live_exit_take_profit_pct=0.25; trigger.live_exit_stop_loss_pct=0.50 bez zmian. Pozostałe wartości identyczne z r6. Binarka runtime nie jest przebudowywana.

## D4. Zachowanie danych

Przed zmianą zapisano poprzednią konfigurację brain w logs/rollout/predator-v11-20260919-candidate-identity-r6/brain_config_snapshot.toml. Usuwanie ograniczono do zwykłych plików system.log*, oracle.log* i oracle_decisions.log* w starych katalogach predator-v11. Nazwa używana przez dotychczasowe profile to oracle.log. Zachowano JSONL decyzji, wejść, lifecycle, PnL oraz pozostałe artefakty. Lista usuniętych plików i ich rozmiarów: /tmp/ghost-tp25-20260919/deleted-logs.json.

## D5. Weryfikacja

Porównanie sparsowanych TOML potwierdziło jedną zmianę wartości w brain oraz jedną zmianę TP w launcherze, poza namespace. SL pozostaje50% w obu konfiguracjach. Weryfikacja runtime: preflight oraz krótka obserwacja nowego procesu i zapisu artefaktów; bez zmian kodu Rust i bez dodatkowych testów implementacji.

## D6. Wdrożenie

R6 otrzymał SIGINT dopiero po stwierdzeniu braku aktywnych pozycji. Nowy run: predator-v11-20260919-tp25-r7, tmux ghost-predator-v11-tp25-20260919-r7. Preflight zakończony kodem 0. Start 2026-09-19 o 18:40:15 UTC, PID 571753. Manifest zawierający SHA binarki i konfiguracji: logs/rollout/predator-v11-20260919-tp25-r7/launch_manifest.json. Potwierdzono entry_mode=shadow_only i execution_mode=shadow.

## D7. Przegląd

Sprawdzono aktywne źródło TP/SL, shadow-only, zachowanie wszystkich pozostałych wartości oraz selektywną listę usuwanych plików. Dane pozycji/PnL nie są objęte usuwaniem. Bez commit/push.

## D8. Wynik

Uruchomiony w tmux. Snapshot 18:40:49 UTC: 1 BUY, 1 shadow_simulated, 1 aktywna pozycja, 0 zamkniętych i 0 unresolved. Zapisują się decyzje, wejścia, lifecycle i obserwacje; writer: 41 zapisów, 0 błędów, 0 dropów. Brak panic i błędów aliasów w próbce startowej. PnL zamkniętych pozycji wynosi na tym etapie 0 SOL; nie było jeszcze zamknięcia, więc nie potwierdzono nowego terminalnego rekordu PnL. Usunięto 16 starych logów tekstowych, łącznie 3 803 149 726 bajtów; ponowny skan nie znalazł pozostałych wskazanych logów starych runów. Nowy run pozostawiony w tle bez dalszego polling do następnej dyspozycji użytkownika.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie stanowi wejścia tej operacji.
