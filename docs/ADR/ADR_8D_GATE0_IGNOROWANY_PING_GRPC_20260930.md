# ADR-8D: Gate0 — ignorowany Ping serwera Yellowstone i zagłodzenie obsługi keepalive

Data: 2026-09-30. Status: poprawka lokalna; regresja RED→GREEN; source smoke360s PASS; pełny lifecycle600s nadal niezweryfikowany.
Wskazany /Gho/docs/ADR/ADR_8D_SZABLON.md nie był dostępny w środowisku; zastosowano osiem sekcji istniejącego ADR-8D projektu.

## 1. Przygotowanie i działania wstępne

Użytkownik zażądał naprawy kodu po runie gate0-1790791158156. Run kończy się po180.6s zamiast1800s z primary progress unavailable/stale. Ten komunikat jest skutkiem guardu; nie dowodzi awarii providera. Próg320SOL, confirmed, C/D/E, guard10s, limity i observe-only pozostają bez zmian. Stan sprzed zmiany: /tmp/gate0_keepalive_repair_20260930/before/.

## 2. Wykorzystane skills i role

Seer Ingest Event Integrity Specialist; ghost-execution, rust-master, solana-pumpfun-architect. Załadowano dokument specjalisty ingest. Bez subagentów. Routing obejmuje sterowanie transportem i integralność zdarzeń; bez zmian materializacji lub polityki.

## 3. Opis problemu

stream_loop rozpoznawała Ping serwera i tylko go odfiltrowywała. Obsługa odpowiedzi polegała na timerze10s, umieszczonym za stream.next w biased select. Pod ciągłym napływem gotowych danych niższe gałęzie mogły być opóźniane. Gate0 dodatkowo wycinało INFO transportu, pozostawiając ogólny komunikat guardu bez liczników keepalive.

## 4. Przyczyny i granice dowodu

Test lokalnego serwera gRPC uruchamia rzeczywisty YellowstoneConnector i stream_loop. Serwer odbiera pierwszy ping timera, następnie wysyła dwa własne Ping przed kolejnym terminem timera; wymaga oddzielnych odpowiedzi z rosnącymi ID i pustymi filtrami. Stary kod nie odpowiada — RED. Poprawiony odpowiada dla Gate0Observation i FundingLaneFullChain — GREEN.

Zgodnie z oficjalnym protokołem ID nadaje klient; pusty SubscribeUpdatePing nie uzasadnia pomijania odpowiedzi. Źródło: https://github.com/rpcpool/yellowstone-grpc — opis SubscribeRequest.ping i odpowiedzi na Ping serwera.

Nie ustalono z zapisu poprzedniego runu, czy dokładnie ta wada spowodowała historyczne zatrzymanie. Brak logów ping/pong/health uniemożliwia taki wniosek. Nie przypisujemy przyczyny providerowi i nie deklarujemy zamknięcia incydentu na podstawie samego testu.

## 5. Rozwiązanie

Odpowiadać na serwerowy Ping bez czekania na timer, z kolejnym ID klienta. Dla biased select obsłużyć gotowy watchdog i timer keepalive przed odbiorem kolejnej wiadomości. Zaległe terminy timera pominąć zamiast tworzyć serię pingów. Zachować logi zdrowia transportu z licznikami wysłanych pingów i odebranych pongów.

## 6. Przeprowadzone akcje naprawcze

- off-chain/components/seer/src/grpc_connection.rs: odpowiedź na Ping; kolejność gałęzi; Skip dla timera; dodatkowe pola istniejącego logu INFO.
- off-chain/components/seer/src/grpc_keepalive_tests.rs: lokalny serwer gRPC, regresja obu profili, sprawdzanie filtrów/ID/zamykania i oddzielenia transportu od kolejki danych.
- ghost-launcher/src/bin/ghost_gate0.rs: filtr warn,seer::grpc_connection=info, bez zmiany publicznego CLI i bez ujawniania uwierzytelnienia.

Nie dodano reconnectu przez nieudokumentowaną lukę, RPC backfillu, fałszywego postępu z Ping ani wydłużenia guardu świeżości. Historyczne taśmy GO-D nie zostały zmienione.

## 7. Walidacja działań naprawczych

RED: nowa regresja upada na starym kliencie (brak odpowiedzi Gate0Observation). GREEN: grpc_connection98/98, w tym dwa profile w nowej regresji. fmt i diff-check PASS. Logi /tmp/gate0_keepalive_repair_20260930/. CLI3/3 i Clippy seer --lib --no-deps PASS (istniejące ostrzeżenia pozostają). Release PASS.

Próba release360s 19:16:15–19:22:16 UTC: exit0, reason=smoke_only, shutdown_error=null. Binarka6b4b325b1be30803dfce47fc52b7424caace8a226844991b96e39757bef045e6. Run gate0-1790795775836. Primary502165/funding592160 odebranych zdarzeń;616804 zdarzeń konsumenta; brak dropów, pending expiry, reconnectów i błędów postępu. Ostatnie health obu połączeń: ping70/pong70; recon=1 oznacza początkowe zestawienie, nie reconnect. Lag maks47ms, high-water635/593 wobec16384. 210births/terminali,312snapshotów, fazy192/68/50/2/0,4migracje+4initial; brak pełnego600s. Audyt60pól: zero naruszeń, developer210/210; CPV26clean/7insufficient/279unavailable. Zostało126 ostrzeżeń GATE0_TRADE_CANDIDATE_PARSE_MISS — nie przypisujemy im przyczyny bez dowodu ani nie oznaczamy ich jako naprawione. Tej próbki nie akceptujemy jako pełnego datasetu Gate0. Poprawka keepalive ma regresję RED→GREEN i lokalną walidację360s; dokładna przyczyna historycznego przerwania180s nadal nie została dowiedziona.

Artefakty: `/root/gho_gate0_runs_20260927/gate0_repaired_confirmed_360s_20260930T191615Z.jsonl` i pliki towarzyszące. Manifest źródeł i hash binarki zgodne po próbie. Proces zakończony, nie uruchomiono kolejnego1800s/10h.

## 8. Zabezpieczenia antyregresyjne i self-review

Przejrzano zakres względem kopii plików sprzed tej zmiany: tylko sterowanie transportem, logowanie i test. Ping/Pong nadal nie są materializowane jako dane rynku; request Ping nie zmienia subskrypcji; ID wspólne dla odpowiedzi i timera. Brak zmian progów, retencji, auth, RPC lub live execution. Wszystkie zmiany LOCAL ONLY; bez commit/push/PR.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie było wejściem zadania.
