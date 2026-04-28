# SKILL: Kompletny workflow filtrowania i parsowania zdarzeń Pump.fun ze strumienia gRPC Yellowstone

## Cel
Automatyzacja odbioru, filtrowania, dekodowania i parsowania wszystkich zdarzeń związanych z Pump.fun pochodzących ze strumienia gRPC Yellowstone. Skill jest uniwersalny – do wykorzystania w różnych projektach korzystających z gRPC i Pump.fun.

---

## Workflow krok po kroku

1. **Połącz się ze strumieniem gRPC Yellowstone**
   - Skonfiguruj klienta gRPC z odpowiednim endpointem Yellowstone.
   - Upewnij się, że masz wymagane uprawnienia i certyfikaty (jeśli wymagane).

2. **Odbieraj zdarzenia w czasie rzeczywistym**
   - Subskrybuj odpowiedni stream (np. bloków, transakcji lub logów).
   - Zaimplementuj obsługę reconnectów i backoff na wypadek utraty połączenia.

3. **Filtruj zdarzenia związane z Pump.fun**
   - Zidentyfikuj unikalne cechy zdarzeń Pump.fun (np. adresy kontraktów, typy instrukcji, specyficzne pola).
   - Odrzucaj zdarzenia niepowiązane (np. po adresie, typie eventu, tagach).

4. **Dekoduj payload zdarzenia**
   - Użyj odpowiednich protobufów lub schematów do dekodowania binarnego payloadu.
   - Waliduj poprawność dekodowania (checksumy, typy pól, wersje).

5. **Parsuj i mapuj dane do modelu domenowego**
   - Zamapuj zdekodowane dane na struktury domenowe (np. trade, pool, event).
   - Uzupełnij brakujące pola na podstawie kontekstu (np. timestamp, block number).

6. **Obsłuż edge-case'y i błędy**
   - Loguj i raportuj nieparsowalne zdarzenia.
   - Implementuj retry/backoff dla błędów sieciowych i dekodowania.

7. **Przekaż przetworzone zdarzenia dalej**
   - Zapisz do bazy, wyślij do kolejki, lub przekaż do dalszego pipeline'u analitycznego. JEŚLI JEST TAKA MOŻLIWOŚĆ TO PRZEKAZUJ DALEJ JEDYNIE TE EVENTY, KTÓRE ODNOSZĄ SIĘ DO POOLS, KTÓRE WYKRYTO! W TRAKCIE PRACY NASZEGO BOTA. WYKRYTO = ZAREJESTROWANO EVENT TYPU InitializedPool (utworzono nową pool pump.fun).

---

## Decyzje i kryteria jakości
- Czy filtracja nie odrzuca żadnych istotnych zdarzeń Pump.fun?
- Czy dekodowanie jest zgodne z aktualnym schematem protobuf?
- Czy obsługa reconnectów i retry jest odporna na typowe awarie?
- Czy logowanie błędów pozwala na szybkie wykrycie problemów?

---

## Przykładowe prompt do użycia
- "Odfiltruj i sparsuj wszystkie zdarzenia Pump.fun z Yellowstone gRPC stream."
- "Zaimplementuj reconnect i retry dla klienta gRPC odbierającego zdarzenia Pump.fun."
- "Zmapuj zdekodowane eventy Pump.fun na strukturę domenową TradeEvent."

---

## Propozycje dalszych customizacji
- Skill do automatycznego testowania poprawności dekodowania eventów.
- Skill do monitorowania i alertowania na wypadek utraty zdarzeń.
- Skill do generowania statystyk na podstawie przetworzonych eventów.
