# ADR-8D: Naprawa routingu RPC dla probe ATA w triggerze shadow

Status: Zrealizowano
Typ: Naprawa błędu
Data: 2026-06-16
Repo/branch: /root/Gho
Commit/PR: pending
Zakres: Jedna ścieżka pre-submit ATA probe w TriggerComponent
Dotknięte moduły/pliki:
- ghost-launcher/src/components/trigger/component.rs
Powiązane runy/logi/raporty:
- R31 runtime.log: dominujące błędy 429 z `ATA pre-submit fast probe` na `solana-mainnet.g.alchemy.com`
Poziom ryzyka: Niski (zmiana routingu wewnątrz istniejącej ścieżki)

## 1. Przygotowanie i działania wstępne

Plan początkowy:
Przepiąć odczyt salda tokenów podczas pre-submit ATA probe na klienta RPC przekazywanego do funkcji probe (a nie zawsze primary RPC), aby usunąć niezamierzony fallback do Alchemy przy konfiguracji shadow-only.

Rzeczywisty przebieg:
Zmieniono sygnaturę `fetch_token_account_balance` i wszystkie wewnętrzne wywołania w ścieżkach pre-submit tak, aby korzystały z przekazanego klienta.

Odchylenia od planu:
Brak.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
- ghost-execution
- solana-pumpfun-architect

Powód użycia:
Diagnoza ścieżki runtime/trigger i rozdział ścieżek RPC w przygotowaniu transakcji.

Zakres użycia:
Inspekcja `component.rs`, potwierdzenie miejsca źródłowego 429 i zakres minimalnych zmian.

Wynik:
Lokalizacja błędu i minimalny zestaw edycji.

Ograniczenia:
Nie uruchamiano testów runtime/rebuild, zgodnie z bieżącym żądaniem implementacji.

## 3. Opis problemu — 3W2H

What:
`getTokenAccountBalance` był wykonywany przez `primary_rpc_client` niezależnie od tego, że pre-submit miał używać `preparation_rpc()`.

Where:
`ghost-launcher/src/components/trigger/component.rs` w metodach `fetch_token_account_balance`, `probe_user_ata_pre_submit`, `probe_user_ata_pre_submit_legacy`.

Why it matters:
Powodowało to nieplanowane obciążenie Alchemy i 429 po stronie Alchemy mimo NLN jako shadow RPC.

How observed:
W logach R31 pojawiały się wielokrotne wpisy `ATA pre-submit fast probe failed ... 429 ... solana-mainnet.g.alchemy.com/...`.

How many / scale:
W logu uruchomienia R31 odnotowano setki przypadków 429 powiązanych z tym kodem.

Evidence:
Runtime log R31 + kod ścieżki pre-submit.

## 4. Przyczyna źródłowa

Root cause:
Niejawny hardcoded access do primary klienta przy pobieraniu salda ATA.

Mechanizm błędu:
`fetch_token_account_balance` używał wewnętrznie `self.primary_rpc_client`, ignorując wcześniej przekazywany `rpc`.

Miejsce:
`component.rs` w funkcji `fetch_token_account_balance`.

Skutek:
Rate-limitowe błędy Alchemy i fallback do semantics legacy, obniżające stabilność przygotowania zakupu.

Dowód:
Logi `getTokenAccountBalance failed ... 429 Too Many Requests` z URL Alchemy podczas pre-submit.

Odrzucone hipotezy:
Zmiana samej konfiguracji URL bez naprawy kodu ścieżki `fetch_token_account_balance` nie usuwała źródłowości błędu.

## 5. Strategia naprawy

Przyjęta strategia:
Przekazanie klienta RPC do funkcji pobierającej saldo i użycie tego samego klienta w pre-submit probe; utrzymanie potwierdzeń post-submit na dotychczasowym primary.

Zakres ingerencji:
- `fetch_token_account_balance(rpc, ata)`
- `probe_user_ata_pre_submit(..., rpc)` i legacy path używające tego helpera
- `confirm_sender_buy_attempt` używa jawnie `primary_rpc_client` dla zachowania dotychczasowej semantyki potwierdzania

Czego nie zmieniano:
Logika fallback, progi, metryki, konfiguracja runa/endpointów.

Ryzyka:
Niewielkie ryzyko zmiany źródła RPC w tych ścieżkach przy bardzo nietypowych konfiguracjach testów.

Odrzucone alternatywy:
Pełna przebudowa routingową przez zmianę konfiguracji `preparation_rpc` i/lub wyłączanie Alchemy globalnie.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `ghost-launcher/src/components/trigger/component.rs`
- Co zmieniono: zmiana sygnatury `fetch_token_account_balance` na przyjmującą jawny `rpc` i zastąpienie wywołań na to pole.
- Dlaczego: usunięcie stałego odwołania do primary w ścieżce probe.
- Efekt: pre-submit balance probe respektuje klienta przekazanego z kontekstu przygotowania.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Przegląd diff | `git diff -- /root/Gho/ghost-launcher/src/components/trigger/component.rs` | Oczekiwany zakres zmian | PASS | Tylko funkcja balance fetch i 3 wywołania |
| Runtime proof | Uruchomienie rerunu na nowej konfiguracji | do wykonania | PENDING | Uruchomić i obserwować spadek 429 Alchemy w `ATA pre-submit fast probe` |

Wniosek walidacyjny:
Zmiana implementacyjna jest spójna logicznie i ograniczona do jednej ścieżki.

Ograniczenia walidacji:
Brak testów uruchomionych w tej turze.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Minimalizm zmian
- Co zabezpiecza: brak regresji funkcjonalnej poza routingiem RPC
- Kiedy się aktywuje: zawsze na ścieżce pre-submit ATA probe
- Jak przetestowano: poprzez zachowanie semantyki fallbacku i brak zmian decyzji/progów
- Co pozostaje poza zakresem: zachowanie runtime post-submit

Guardrail 2:
- Typ: Idempotentna zmiana sygnatury
- Co zabezpiecza: brak utraty danych przez przypadkowe korzystanie z innego klienta
- Kiedy się aktywuje: każdorazowe wywołanie `probe_user_ata_pre_submit` i potwierdzenie
- Jak przetestowano: lokalny przegląd użyć helpera
- Co pozostaje poza zakresem: pełna walidacja produkcyjna

## Otwarte ryzyka / follow-up

- Uruchomić ponownie R31 i sprawdzić, czy dominujące 429 Alchemy znikają z sekcji pre-submit fast probe.
