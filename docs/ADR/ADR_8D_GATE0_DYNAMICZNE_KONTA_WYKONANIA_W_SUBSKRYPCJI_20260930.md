# ADR-8D: Gate0 — dynamiczne konta wykonania zmieniały subskrypcję obserwatora

Data:2026-09-30. Status:implementacja, regresja RED→GREEN i techniczny odbiór1800s PASS. Wskazany /Gho/docs/ADR/ADR_8D_SZABLON.md pozostaje niedostępny; zastosowano osiem sekcji poprzednich ADR-8D.

## 1. Przygotowanie i działania wstępne

Bezpośrednie polecenie naprawy po runie gate0-1790796656455. Zapisany czas586.9s/1800s, primary progress unavailable/stale, poprawny keepalive116/116. Kopie źródeł i status przed zmianą w /tmp/gate0_static_subscription_repair_20260930/before/.

## 2. Wykorzystane skills i role

Główny Seer Ingest Event Integrity Specialist, pomocniczy Config Rollout Safety Reviewer; przeczytano oba dokumenty. Skills ghost-execution,rust-master,solana-pumpfun-architect. Bez subagentów. Zakres: aktywny opt-in obserwator, bez live execution.

## 3. Opis problemu

Gate0Observation dziedziczył uses_registry_filters=true i dynamiczne exact-account BCV2/generic z PrimaryGlobal. Parser odkrywa i rejestruje konta wykonania; ich zmiany powodowały resubscribe co5s na health_tick. Gate0 ignoruje ExecutionAccountEvidence, a on_account przyjmuje tylko właściwe kanoniczne konta Pump.fun. Te dane były już objęte stałymi filtrami globalnymi. Niepotrzebna zależność zmieniała aktywną subskrypcję źródłową obserwatora.

## 4. Przyczyny i granice dowodu

Regresja wywołuje produkcyjny builder i maybe_send_resubscribe po zmianach wszystkich kategorii lokalnego rejestru. Stary kod wysyła nowe requesty — RED. Poprawiony nie wysyła ich, a cały request jest niezmienny — GREEN. Dane historycznego runu dowodzą częstych resubscribe, lecz nie dowodzą, że to wyłączna przyczyna historycznej przerwy. Nie przypisujemy awarii providerowi; związek z brakiem danych wymaga walidacji1800s.

## 5. Rozwiązanie

Odseparować Gate0Observation od dynamicznych kont wykonania. Zachować globalne filtry Pump.fun/PumpSwap i static fee account, failed/successful transactions, BlockMeta i confirmed. Lokalne watch mapy nadal mają cap/TTL i sprzątanie. PrimaryGlobal zachowuje dotychczasowe zachowanie.

## 6. Przeprowadzone akcje naprawcze

- grpc_connection.rs: tylko PrimaryGlobal używa dynamicznych filtrów; Gate0 zachowuje globalne filtry kont i nie pobiera dynamicznych exact-account. Builder i fingerprint zgodne. Log SUBSCRIBE_SENT opisuje statyczny kontrakt Gate0.
- grpc_keepalive_tests.rs: regresja driftu subskrypcji dla health_tick/notify/tick; weryfikacja danych źródłowych i zachowania ordinary profilu.
- Nie zmieniono guard10s, progu320SOL,C/D/E,fail-closed,endpointu ani credentials.

## 7. Walidacja działań naprawczych

Nowa regresja RED; suite transport99/99 GREEN. Logi, źródła i manifest /tmp/gate0_static_subscription_repair_20260930. Clippy seer --lib --no-deps, formatter i diff-check PASS. Release PASS.

Pełne1800s PASS: start21:28:20UTC, koniec21:58:20.994441UTC, czas odrun_id1800.568s, exit0, reason=smoke_only, shutdown_error=null. Run gate0-1790803700426. Binarka1ce19530d144998e36f207c26d175cdc5307a27b5c1aca1fd5d66933bfa0ae74. Zero resubscribe, reconnectów, lokalnych dropów, pending mapping expiry i konfliktów statusów. Primary1844208/funding2320096 odebranych zdarzeń; konsument2491086. High-water1361/554 wobec16384, maksymalny lag266ms, końcowe kolejki puste.

1172births/terminali;1885snapshotów; fazy1155/376/342/8/4. TerminaleC765,D186,E149,completed4,smoke_only68. Cztery pełne obserwacje600s, trzy z migracją i initial AMM; łącznie14migracji i14initial. Wszystkie wymagania technicznego CLI smoke>=600s przeszły. Audyt60pól: zero naruszeń, known_creator1172/1172, identyczna populacja birth/terminal, brak powtórzonych snapshotów, poprawne prefiksy faz, newline-complete26362779b. CPV1133clean/361insufficient/391unavailable.

Gem0 ma konkretne przyczyny: jeden completed bez migracji, dwa z migracją w wieku3ms/7ms (warunek wymaga>3000ms), ostatni z migracją220344ms i minimumMC266.3246SOL<320. Jeden z wczesnych przypadków ma także historyczny missing_amm_state. Nie zmieniono żadnego progu.

Ograniczenia: log nadal zawiera1388 GATE0_TRADE_CANDIDATE_PARSE_MISS i jeden Missing pong. Po ostrzeżeniu ping125 o21:38:50 oba liczniki osiągnęły126/126 do21:38:55, bez przerwy danych, reconnectu lub fail guardu. Nie oznaczać ostrzeżeń parsera jako naprawionych. Ten run jest diagnostycznym smoke_only;68aktywnych obserwacji zamknięto na limicie. Skaner datasetu odrzuca go zgodnie z kontraktem jako smoke_only, więc accepted=false dla badania, przy source_smoke_pass=true i full_lifecycle_verified=true. To nie jest wynik badania ekonomicznego ani dowód stabilności10h. Nie dowodzi osobno wyłącznej przyczyny wszystkich historycznych przerw.

Artefakty: `/root/gho_gate0_runs_20260927/gate0_static_confirmed_1800s_20260930T212820Z.jsonl` i .log/.meta.json/.audit.json/.audit.md.

## 8. Zabezpieczenia antyregresyjne i self-review

Przegląd względem kopii przed zmianą: zakres do opt-in Gate0; konto krzywej i cena pozostają pod tą samą authority; brak wypełniania luk, reconnectu z udawaniem ciągłości lub wydłużenia timeoutów. Test sprawdza pełną równość requestu oraz zachowanie dynamicznych kont dla PrimaryGlobal. Końcowy self-review i kontrola manifestu: kod i binarka nie zmieniły się podczas runu. Nie ma aktywnego Gate0; nowego runu nie uruchomiono. LOCAL ONLY, bez commit/push/PR.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie było wejściem zadania.
