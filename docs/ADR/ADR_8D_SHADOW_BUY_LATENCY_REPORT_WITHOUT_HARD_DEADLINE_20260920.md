# ADR-8D: Raport czasu BUY bez twardego deadline 300 ms

## D0. Dyspozycja

Zachować optymalizacje i poprawiony timestamp wejścia. Wyłączyć twardy deadline300ms, aby jego przekroczenie nie blokowało symulacji. Zapisywać czas od decyzji Gatekeepera do udanego BUY dla każdego wejścia.

## D1. Stan wejściowy

R12 działa z opcjonalnym decision_to_buy_deadline_ms=300. Istnieją timestampy decyzji i zakończenia symulacji, lecz shadow_entries nie ma pojedynczego pola ich różnicy. Kopia wejściowego źródła i dowody w /tmp/ghost-shadow-latency-report-20260920/.

## D2. Przyczyna

Jawny deadline z poprzedniego zadania odrzuca spóźnione wejścia. Użytkownik wybiera pomiar zamiast tego ograniczenia. Nie zmieniamy przyczyn opóźnienia ani wcześniejszych optymalizacji.

## D3. Kontrakt

Nowe opcjonalne pole `decision_to_buy_ms` w shadow_entries.jsonl oznacza czas od decision_ts_ms do simulation_finished_ts_ms dla udanej symulacji. Obejmuje IWIM, przygotowanie, precheck oraz symulację RPC. Nie oznacza czasu włączenia transakcji do łańcucha.

## D4. Zmiana

ShadowEntryRecord otrzymuje serde(default) i skip_serializing_if dla nowego Option<u64>. Udany event wylicza checked_sub; błąd lub odwrócone timestampy nie dostają fałszywego pomiaru. Request bez wyniku oraz harness bez symulacji pozostawiają None. Schema1 i istniejące pola zachowane; starsze wpisy są nadal czytelne.

Profil r13 usuwa decision_to_buy_deadline_ms, co zgodnie z istniejącym configiem oznacza None. Pozostałe wartości identyczne z r12 poza namespace. Kod opcjonalnego limitu pozostaje dostępny, lecz jest wyłączony w aktywnym profilu. TP+50%, SL−25%, IWIM, Gatekeeper i shadow-only zachowane.

## D5. Testy

Istniejący test trwałego zapisu canonical shadow entry rozszerzony o wartość nowego pola oraz deserializację starszego JSON bez niego. Istniejące testy deadline pokrywają None dopuszczające wynik600ms oraz domyślną konfigurację bez limitu.

## D6. Wynik weryfikacji

Test trwałego zapisu i zgodności starego JSON: 1 PASS. Testy opcjonalnego deadline i domyślnej konfiguracji: 2 PASS, w tym dopuszczenie wyniku600ms przy None. Logi entry-test.log oraz deadline-off-tests.log w katalogu dowodów. Build release PASS8m36s, preflight exit0. Porównanie konfiguracji PASS: tylko namespace i usunięcie deadline; pozostałe wartości identyczne. Hash brain i plików optymalizacji zgodny z poprzednim wdrożeniem.

Kontrola runtime od13:12 do13:16:59UTC:0BUY,0prób symulacji,0pozycji,0odrzuceń deadline,0panic/alias w próbce. Strumień dostarcza świeże dane;41decyzji Gatekeepera w odczycie13:15, ostatnia13:15:28, świeży log13:15:51. Nowe pole potwierdzone w teście trwałego zapisu, ale nie ma jeszcze rzeczywistego wejścia r13, na którym można je sprawdzić. Nie zastępujemy tej granicy syntetycznym wpisem do plików runa. Dowód r13-status.json.

## D7. Przegląd

Zmiana addytywna, bez ingerencji w wycenę, decyzję, canonical timestamps i optymalizacje. Nowe pole jest diagnostyczne. Własny diff porównywany z kopią wejściową, zastane zmiany zachowane. Bez commit/push.

## D8. Wdrożenie

R12 zatrzymano SIGINT po sprawdzeniu2registered/2closed/0active/0unresolved. Uruchomiono `predator-v11-20260920-latency-report-r13` o13:12UTC, PID655382, tmux `ghost-predator-v11-latency-report-20260920-r13`. Profil /tmp/predator-v11-20260920-latency-report-r13.toml, skrypt /tmp/launch-predator-v11-20260920-latency-report-r13.sh. SHA działającej binarki cc9ab2078328922a1cc5037a9f18068d7acb411e7db8c6690e03b2ac81846a61. Manifest i brain snapshot w logs/rollout/predator-v11-20260920-latency-report-r13/. Run pozostawiono w tle, po odpowiedzi bez dalszego pollingu/restartów bez nowej dyspozycji.

Szablon /Gho/docs/ADR/ADR_8D_SZABLON.md niedostępny; zachowano układ D0–D8 istniejących ADR.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie jest wejściem tej zmiany runtime.
