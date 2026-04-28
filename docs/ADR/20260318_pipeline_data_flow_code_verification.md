# ADR: Weryfikacja kodu produkcyjnego Pipeline'u Danych z dokumentacją

**Data:** 2026-03-18
**Status:** Zaakceptowane
**Kontekst:** Weryfikacja zgodności dokumentu `20260318_production_pipeline_data_flow.md` z rzeczywistym kodem źródłowym pipeline'u danych systemu Ghost. Zbadano poszczególne warstwy od połączenia gRPC, przez parser binarny, po ostateczne decyzje Gatekeepera.

## Ustalenia i weryfikacja

1. **Warstwa 1 i 2 (Transport i Parser)**:
   - Zbadano pliki `grpc_connection.rs` i `binary_parser.rs`. Architektura opisana w dokumentacji (np. `DualLaneChannel` z backpressure, `DelayedAccountQueue`) precyzyjnie zgadza się ze stanem faktycznym w kodzie. Ponadto `binary_parser.rs` rzeczywiście obsługuje eventy PumpSwap, jak opisano.

2. **Warstwa 3 (Seer Orchestration)**:
   - Pętla Seera poprawnie buforuje trades oczekujące na powiązanie pool-mint. Potwierdzono TTL dla tego bufora wynoszący 300s (`PENDING_TRADE_TTL`), jak wskazano w dokumencie.

3. **Warstwa 4 (IPC i Event Bus)**:
   - Odkryto dodatkowy szczegół, który należało uściślić w dokumentacji. Oprócz ogólnego bufora w Seerze (300s), mostek Sesji (`SessionPoolTradeBridge`) w komponencie ghost-launcher posiada optymalizację w postaci `SESSION_POOL_TRADE_BUFFER_TTL` ustawionego zaledwie na 10ms. Ma to kluczowe znaczenie w optymalizacji narzutu zapisu dla pierwszych trade'ów względem detekcji nowej puli.

4. **Warstwa 5-7 (Decyzje, Gatekeeper, Oracle)**:
   - Potwierdzono, iż Gatekeeper V2 prawidłowo korzysta z `apply_trade_strict`, które uwzględnia matematykę `k-invariant` oraz odejmuje sztywną opłatę protokołu Pump.fun w wysokości 1%. 
   - W systemie w pliku `oracle_runtime.rs` potwierdzono wspomniany krytyczny błąd: `dev_buy_sol` or `has_dev_buy` są nadawane na `0.0` / `false` co powoduje lukę w scoring engine (wycięcie transakcji deweloperskich z kalkulacji).

## Wpływ Architektoniczny i Konsekwencje

Nie ma tu drastycznych nieścisłości względem Single Source of Truth, na jakich bazuje dokument. Dokonana weryfikacja zapewnia wysoki stopień pewności operacyjnej. Należy jednak mieć na szczególnej uwadze rozbieżności z hardkodowaniem braku "dev buy", co może zakłócać działanie fazy oceny na środowisku live. Ten problem będzie wymagać niezależnego poprawienia i może doprowadzić do zmian na wielu kontraktach strumieniowania. Oprócz tego bufor 10ms w launcherze stanowi optymalizację wpływającą na opóźnienia, kluczową dla szybkiego strzału w nowe poole (snipe).
