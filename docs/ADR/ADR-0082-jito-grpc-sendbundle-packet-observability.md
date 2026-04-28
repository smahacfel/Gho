# ADR-0082: Jito gRPC SendBundle packet observability

**Date:** 2026-04-07
**Status:** Accepted
**Author:** Ghost Father

## Context

Po wdrożeniu nagłówka `x-jito-auth` dla Jito gRPC potrzebna była dokładna widoczność tego, jak wygląda `SendBundleRequest` tuż przed wywołaniem `SearcherServiceClient::send_bundle(...)`.

Szczególnie istotne było potwierdzenie dwóch rzeczy:

1. ile pakietów (`bundle.packets`) faktycznie wychodzi w pojedynczym gRPC submit,
2. jakie są rozmiary bajtowe poszczególnych pakietów po zdekodowaniu base64 i przed wysyłką.

Bez tej obserwowalności trudno rozróżnić problem auth/transport od problemu kształtu bundle albo niepoprawnej serializacji pojedynczego tx.

## Decision

Dodano tymczasowy log debug w `off-chain/components/trigger/src/jito_client.rs` bezpośrednio po zbudowaniu `searcher::SendBundleRequest` i przed wywołaniem `send_bundle(...)`.

Log raportuje:

- `endpoint`
- `packet_count`
- `packet_sizes`

Nie zmienia to struktury requestu ani ścieżki transportowej; rozszerza jedynie obserwowalność runtime.

## Architectural Impact

Zmiana nie wpływa na format bundle, routing, auth metadata ani retry policy. Dotyka wyłącznie warstwy obserwowalności gRPC submit path.

To wzmacnia forensic visibility dla live diagnostyki Jito bundle transport bez zmiany kontraktów danych.

## Risk Assessment

**Risk:** Low

Ryzyko regresji funkcjonalnej jest niskie, bo zmiana ogranicza się do logowania na poziomie `debug`.

Potencjalnym kosztem jest tylko większy wolumen logów przy aktywnym poziomie debug dla ścieżki submitu.

## Consequences

Co staje się łatwiejsze:

- szybkie potwierdzenie, czy bundle niesie oczekiwaną liczbę tx,
- odróżnienie problemu z auth od problemu z payloadem,
- korelacja `sendBundle ACK/Rejected/Invalid` z realnym rozmiarem i liczbą pakietów.

Co staje się trudniejsze:

- nic istotnego architektonicznie; jedynym kosztem jest dodatkowy debug noise przy wysokiej szczegółowości logowania.

## Alternatives Considered

### 1. Brak dodatkowego logowania

Odrzucone, bo utrudniałoby diagnostykę problemów `SendBundleRequest` po stronie gRPC.

### 2. Logowanie pełnych bajtów `packet.data`

Odrzucone, bo byłoby zbyt ciężkie, mało czytelne i niepotrzebnie zwiększałoby wolumen logów.

### 3. Dodanie osobnego, trwałego telemetry eventu

Odrzucone na tym etapie jako nadmiarowe wobec prostego logu debug wymaganego do bieżącej diagnostyki.

## Validation Steps

1. Uruchomić ścieżkę bundle submit z logowaniem `debug`.
2. Zweryfikować obecność wpisu `"gRPC SendBundleRequest before submit"`.
3. Potwierdzić, że `packet_count` odpowiada liczbie tx w bundle.
4. Potwierdzić, że `packet_sizes` odpowiadają długościom zdekodowanych payloadów tx.