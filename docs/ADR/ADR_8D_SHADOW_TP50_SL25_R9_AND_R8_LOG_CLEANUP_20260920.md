# ADR-8D: Shadow r9 z TP +50%, SL -25% i czyszczeniem logów r8

## D0. Zakres

Zatrzymać r8, usunąć jego logi `system.log*` i log decyzji Oracle, ustawić take profit +50% przy pozostawieniu stop loss -25%, a następnie uruchomić nowy shadow run.

## D1. Stan wejściowy

R8 działał jako `predator-v11-20260919-tp20-sl25-r8`, PID 577048, z TP +20% i SL -25%. Profil zapisuje log Oracle pod nazwą `oracle.log`, a nie `oracle_decision.log`.

## D2. Zatrzymanie i czyszczenie

R8 otrzymał `SIGINT`; przed usunięciem logów potwierdzono zakończenie procesu oraz sesji tmux. Usunięto cztery pliki `system.log*` i `oracle.log*` z katalogu r8, łącznie 1 079 089 983 bajty. Zachowano `tmux-launcher.log`, JSONL decyzji, pozycje, lifecycle i PnL.

## D3. Zmiana konfiguracji

W `configs/rollout/ghost_brain_predator_v11_shadow_burnin_20260916.toml` zmieniono wyłącznie `post_buy_guardian.target_threshold` z 20.0 na 50.0. `stoploss_threshold = 25.0` pozostał bez zmian. Profil r9 ustawia zgodnie `live_exit_take_profit_pct = 0.50` i `live_exit_stop_loss_pct = 0.25`.

## D4. Granice

Nie zmieniono Gatekeepera, rozmiaru pozycji, timeoutów ani pozostałych zasad lifecycle. Profil zachowuje `entry_mode = "shadow_only"` i `execution_mode = "shadow"`.

## D5. Weryfikacja

Porównanie sparsowanych TOML potwierdziło, że brain różni się od snapshotu r8 tylko TP, a launcher tylko namespace i TP. Preflight zakończył się kodem 0. Binarka nie została przebudowana.

## D6. Wdrożenie

Uruchomiono `predator-v11-20260920-tp50-sl25-r9` w tmux `ghost-predator-v11-tp50-sl25-20260920-r9`, PID 591024. Manifest: `logs/rollout/predator-v11-20260920-tp50-sl25-r9/launch_manifest.json`.

## D7. Kontrola startowa

Snapshot 2026-09-20 09:08:02 UTC: 1 BUY, 1 `shadow_simulated`, 1 aktywna pozycja, 0 zamkniętych i 0 unresolved. Writer wykonał 12 zapisów przy 0 błędów i 0 dropów. W próbce logu nie było panic ani błędów aliasów.

## D8. Wynik

R9 pozostawiono w tle. Dalszy polling nastąpi dopiero na nowe zlecenie użytkownika.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie stanowi wejścia tej operacji.
