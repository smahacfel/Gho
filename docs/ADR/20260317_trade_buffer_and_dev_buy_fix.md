# ADR: Refaktoryzacja Buforowania Handlu i Poprawa Precyzji Analizy Dev Buy

## Data: 2026-03-17
## Status: Zaimplementowano

### Kontekst
Wykryto krytyczne problemy w procesie analizy nowych pul (launches) na Pump.fun:
1. **Wyścig zdarzeń (Race Condition):** Zdarzenia handlu (Trade) i wykrycia puli (Create) dla tej samej transakcji atomowej są rozdzielane w potoku przetwarzania. Przy dużym obciążeniu handel mógł wyprzedzać metadane puli, co wymuszało buforowanie (poprzednio 1500ms).
2. **Błąd w kalkulacji SOL:** Parser `dev_buy` błędnie interpretował pierwszy parametr instrukcji `Buy` (ilość tokenów) jako ilość SOL (lamporty), co prowadziło do absurdalnych wartości w logach (miliardy SOL).
3. **Brak identyfikowalności:** Logi analizy rozszerzonej nie zawierały adresu puli (`pool_id`), co uniemożliwiało korelację zdarzeń.

### Decyzje Architektoniczne
1. **Redukcja TTL bufora do 10ms:** Zmieniono `SESSION_POOL_TRADE_BUFFER_TTL` z 1500ms na 10ms. Decyzja ta ma na celu wymuszenie maksymalnej szybkości przetwarzania i minimalizację opóźnień w wejściu w pozycję. 
2. **Wprowadzenie logowania wygaśnięcia:** Dodano ostrzegawcze logi (`warn!`) w przypadku, gdy handel zostanie odrzucony z powodu braku metadanych puli w zadanym oknie 10ms. Pozwoli to na diagnostykę wąskich gardeł w systemie.
3. **Usunięcie heurystyki w parserze swapów:** Zaimplementowano precyzyjne rozróżnianie instrukcji `Buy`/`Sell` dla Pump.fun na podstawie dyskryminatorów, co pozwoliło na poprawne wyciąganie ilości lamportów (drugi parametr instrukcji).
4. **Wzbogacenie logów Enhanced Analysis:** Dodano `pool_id` do logów analizy rozszerzonej w celu zapewnienia pełnego SSOT (Single Source of Truth).

### Konsekwencje i Ryzyka
- **Ryzyko Regresji (Wysokie):** Skrócenie TTL do 10ms może prowadzić do częstszego odrzucania poprawnych trade'ów w przypadku mikro-jittera w systemie operacyjnym lub asynchronicznym runtime (Tokio). Wymaga to ścisłego monitorowania logów "trade buffer EXPIRED".
- **Poprawa SSOT:** Eliminacja błędnych wartości SOL w analizie `dev_buy` przywraca wiarygodność sygnałom Gatekeepera.
- **Brak automatycznego fallbacku RPC dla handlu:** Potwierdzono, że mechanizm RPC Seeder uruchamia się tylko po wykryciu puli. Handel na "nieznanej" puli, której `Create` nigdy nie dotarł, zostanie bezpowrotnie odrzucony po 10ms.
