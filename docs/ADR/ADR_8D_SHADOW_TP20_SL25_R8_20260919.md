# ADR-8D: Shadow r8 z TP +20% i SL -25%

## D0. Zakres

Zatrzymać r7 i uruchomić nowy shadow burn-in z niezmienioną konfiguracją poza progami wyjścia: take profit +20% oraz stop loss -25%.

## D1. Stan wejściowy

R7 działał jako `predator-v11-20260919-tp25-r7` z aktywnym guardianem TP +25% i SL -50%. Źródłem progów shadow lifecycle jest sekcja `post_buy_guardian` w dedykowanym pliku brain; zgodne pola adaptera znajdują się w profilu launchera.

## D2. Decyzja

Na bezpośrednie zlecenie użytkownika ustawiono `target_threshold = 20.0` i `stoploss_threshold = 25.0`. Nie zmieniono Gatekeepera, kwoty pozycji, timeoutów, trybu wykonania ani pozostałych zasad lifecycle.

## D3. Implementacja

Zmodyfikowano `configs/rollout/ghost_brain_predator_v11_shadow_burnin_20260916.toml`. Utworzono profil `/tmp/predator-v11-20260919-tp20-sl25-r8.toml`, zmieniając względem r7 wyłącznie namespace oraz zgodne wartości `live_exit_take_profit_pct = 0.20` i `live_exit_stop_loss_pct = 0.25`. Binarka nie została przebudowana.

## D4. Granice bezpieczeństwa

Profil pozostaje `entry_mode = "shadow_only"` oraz `execution_mode = "shadow"`. R7 zatrzymano przez `SIGINT` przed startem r8; nie uruchamiano dwóch runtime równolegle.

## D5. Weryfikacja

Porównanie sparsowanych TOML potwierdziło, że brain różni się od snapshotu r7 wyłącznie dwoma progami, a profil launchera wyłącznie namespace i dwoma zgodnymi progami. Preflight zakończył się kodem 0.

## D6. Wdrożenie

Uruchomiono `predator-v11-20260919-tp20-sl25-r8` w tmux `ghost-predator-v11-tp20-sl25-20260919-r8`, PID 577048. Manifest: `logs/rollout/predator-v11-20260919-tp20-sl25-r8/launch_manifest.json`.

## D7. Kontrola startowa

Proces i sesja tmux są aktywne. Zapisują się `system.log`, `oracle.log`, log launchera oraz health sidecar. Snapshot 2026-09-19 22:53:17 UTC: 0 BUY, 0 aktywnych, 0 zamkniętych, 0 unresolved; 53 sprawdzone linie logu, 0 panic i 0 błędów aliasów.

## D8. Wynik

R8 pozostawiono w tle. Dalszy polling nastąpi dopiero na nowe zlecenie użytkownika.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie stanowi wejścia tej operacji.
