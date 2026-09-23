# ADR-8D: Opóźnienie BUY przez sekwencyjny precheck RPC i błędny czas ceny wejścia

## D0. Zakres

Usunąć potwierdzony mechanizm opóźnienia BUY i zweryfikować Gatekeeper → wynik udanej symulacji względem 300 ms. Poprawić czas ceny wejścia. Zachować TP +50%, SL −25%, konfigurację Gatekeepera, walidacje kont, SSOT i tryb shadow-only. Bez commit/push i ingerencji w zastane zmiany.

## D1. Stan wejściowy

R9: `predator-v11-20260920-tp50-sl25-r9`, PID591024, binarka d314e9ef6c91c0b2b6af1316affaff15a0d2b2d26d9c84ed4a7123c5ce21cef8. Kopie źródeł, wyniki i helper pomiarowy: `/tmp/ghost-buy-latency-20260920-GFAlLw/`.

## D2. Dowód problemu

Snapshot r9 11:21:38 UTC: 98 prób, 77 udanych symulacji. Decyzja → koniec: mediana1165 ms, średnia1246,49 ms, min944, max2431; samo RPC symulacji mediana64 ms. Wszystkie77 wejść mają timestamp decyzji zamiast wyniku. Stary decision_ts powstaje dopiero po części przygotowania, więc powyższe czasy pomijają ten wcześniejszy koszt.

## D3. Przyczyny

`counterfactual_probe_manifest_account_checks` i `counterfactual_probe_missing_required_account` wykonywały getAccountInfo kolejno dla każdego konta. Aktywny routing i precheck powtarzały tę pracę w kolejnych etapach. Dla17 kont z r9 odczyty processed/dataSlice0 trwały308,70/274,82/276,52 ms; pojedynczy getMultipleAccounts14,71/14,71/20,48 ms, z identyczną obecnością kont. Pełny batch17 kont (151720 bajtów odpowiedzi), świeże połączenie:89,09 ms. To dowody transportu, nie pomiar kompletnego BUY.

`prepare_buy_request_with_tip_telemetry_and_amount_lamports` tworzył decision_ts po odczycie podstawowych kont. `shadow_entry_record_from_event` używał decision_ts do ceny obliczonej z tokenów otrzymanych dopiero w wyniku symulacji.

## D4. Minimalna poprawka

Zbiorczy odczyt kont, commitment processed, deduplikacja zachowująca pierwszą rolę i kolejność, maksymalnie100 adresów na żądanie. Zachowane owner/data length/context slot i przypisanie braków. Błąd RPC lub niepełna odpowiedź blokuje precheck. Walidacje kontraktów, routing, wymagane konta i guard przed submit pozostają aktywne.

Rzeczywisty gatekeeper_verdict_at przechodzi do PreparedBuyRequest przed routingiem i dispatch. Przygotowanie bez werdyktu mierzy czas od wejścia do funkcji. Wpis ceny oraz czas otwarcia pozycji shadow wskazują simulation_finished_ts_ms; osobne decision_ts pozostaje w artefakcie. Struktura JSONL zachowana, timing_source jednoznacznie opisuje zmienioną semantykę.

## D5. Weryfikacja przed zmianą

RED: testy liczby RPC i wielkości batcha wykazały getAccountInfo per konto. Test timestampu wykazał10 zamiast16. Logi `precheck-red.log`, `timestamp-red.log`. Test błędu batcha również czerwony na starej implementacji; nowa ma blokować cały błąd bez przypisywania go arbitralnemu kontu.

## D6. Weryfikacja po zmianie

Pierwsza wersja: RED→GREEN precheck4/4 oraz timestamp/handoff. Szerszy zestaw106PASS/8FAIL, baseline103PASS/8FAIL; identyczne osiem błędów. Release8m45s PASS, SHA6864845ff94f3b53347aa4196e0439543f2b8d0ba250022dec54944aba45469c. R10 miał535 i521ms, więc celu nie uznano za osiągnięty. Timestampy2/2zgodne; obie pozycje zamknięte. R11 włączył istniejące logi etapów. Snapshot8udanych BUY: mediana468ms, min443, max681; timestampy8/8zgodne.

Log r11 pokazał mint_account_fetch256ms (kolejny przypadek328ms), około27ms blockhash po odczycie kont i około125ms od przygotowania do symulacji. Stary helper nie logował liczby udanych retry, zatem sam czas256ms nie jest dowodem liczby jego ponowień. Kod miał150ms stałej przerwy po braku mintu.

Domknięcie optymalizacji: przerwa po braku mintu10ms, transport retry pozostaje150ms; log liczby retry i braków. Blockhash pobierany równolegle z kontami i ponownie sprawdzany przez is_fresh przed budowaniem. Trasa LegacyBuy pomija wyłącznie analizę fallbacku, który dla niej nie istnieje; finalny manifest i wymagane konta nadal sprawdzane, równolegle. Nie dodano cache potwierdzeń kont.

Dodano opcjonalny `trigger.shadow_run.decision_to_buy_deadline_ms` (domyślnieNone, r12=300). Autorytatywny dispatch shadow kontroluje pozostały czas od werdyktu, obejmuje jednym timeoutem wszystkie retry symulacji i blokuje późny wynik. Powód `shadow_execution_deadline_exceeded` jest odrębny od błędu providera. Counterfactual probe oraz równoległy compare-shadow przy wykonaniu live zachowują dotychczasowy kontrakt. Zmniejszenie latency należy oceniać razem z liczbą odrzuceń deadline, nie przez sam rozkład przyjętych pozycji.

Testy końcowe precheck5/5, w tym odświeżenie blockhasha zestarzałego podczas odczytu kont. Deadline2/2: pięć scenariuszy pokrywających przygotowanie, wolne RPC, późny timestamp wyniku, poprawne wejście, wyłączony limit i zwolnienie slotu; drugi test sprawdza stary config. Rozszerzony zestaw132PASS/9FAIL; dokładny baseline126PASS/te same9FAIL. Dodatkowy test pierwszego odczytu mintu null, drugiego obecnego konta i budżetu100ms:PASS, zakończenie70ms. Łącznie133 różnych testówPASS i9zastanychFAIL; nie jest to zielony cały workspace. Dowody: regression-final-{green,baseline}.log, test-final-comparison.json, mint-retry-green.log.

Finalny release PASS8m37s, SHA8feb896632404a3b2c381bbbfdac7d351268992deaab107640cc569f894c8e90. Preflight r12 PASS. R10/R11 zakończone odpowiednio2/2 i11/11closed; wszystkie13 wpisów czasu wejścia zgadzają się z wynikiem symulacji. Cenę identyfikuje również rpc_slot. To weryfikacja czasu, a nie ponowna walidacja całego algorytmu wyceny/PnL.

## D7. Przegląd i granice

Odpowiedź getMultipleAccounts dotyczy jednego kontekstu slotu na batch; null oznacza brak konkretnego konta. Nie ma nowej pamięci podręcznej ani przenoszenia potwierdzenia pomiędzy BUY. Wynik shadow pozostaje symulacją, bez dowodu włączenia transakcji do łańcucha. Timestamp wyniku jest czasem otrzymania odpowiedzi, stan wyceny identyfikuje rpc_slot. Nie odtwarzamy historycznie zmienionej ceny ani timestampów r9.

Przegląd końcowy: stała kolejność/deduplikacja kont, brak niepełnego zip przy błędnej długości odpowiedzi, błędy fail-closed, brak await pod nowym lockiem, równoległość ograniczona do istniejących niezależnych odczytów, zachowany limit świeżości blockhasha, zgodność starego configu, odrębny powód deadline, zwolnienie lease przy błędzie. Zakres własnego diffu porównano z kopiami wejściowymi, bez naruszania zastanych zmian. Bez commit/push.

Poza kodem potwierdzono różnicę dostępności świeżego mintu między detekcją ze strumienia a RPC hosta spectrum-02.simplystaking.xyz. Równoległy getAccountInfo/getMultipleAccounts, processed, na3mintach:

| Mint | Ostatnia odpowiedź null po detekcji | Pierwsza odpowiedź z kontem | context.slot null → konto | RTT |
|---|---:|---:|---|---:|
| A4eC7SYD8T2fztjYuCoGKgey6TfsQrM8ANJx2udFpump |286ms|352ms|448728565→448728566|14–17ms|
| 3eYJu1TKpFk3HFB28fgPWtYhjLJYkDKU6wU3mTYghGX4 |234ms|303ms|448728571→448728572|15–24ms|
| EvBQVy7nnfaamzC84juSh8WeaDRbjeGaHbPw15nLpump |159ms|226ms|448728622→448728623|14–17ms|

Pierwsze zapytania5–7ms po lokalnej detekcji; następne pary50ms po zakończeniu poprzednich. Są to przedziały obserwacji, nie dokładne chwile powstania kont. Obie metody dawały zgodny wynik obecności. Nie orzekamy awarii providera ani opóźnienia o kilkadziesiąt bloków. Potwierdzamy opóźnienie dostępności stanu mimo szybkich odpowiedzi HTTP. Raport do providera: `/tmp/ghost-buy-latency-20260920-GFAlLw/provider_rpc_freshness.md`, pełne odczyty: fresh-mint-rpc-probe.json.

W rzeczywistym BUY r12: cztery odpowiedzi null, pełny odczyt mintu po190ms; w drugim BUY14odpowiedzi null i416ms. Te wartości obejmują RPC i lokalne10ms przerwy między ponowieniami. Pierwszy przypadek dodatkowo wymagał przebudowy routed_exact_sol_in→legacy_buy. Nie przypisujemy całości tych czasów samej sieci.

Końcowy odczyt 12:30:17 UTC obejmuje siedem prób. Dodatkowy przypadek C5eVVDnn21jUc2VA5mZP5FWYcEJfwA7rNVRvMNXKpump ma zapisane `iwim_latency_ms=218`, `iwim_fetch_status=OK`; mint_account_fetch_ms=60. Kod wykonuje IWIM po werdykcie Gatekeepera, przed przygotowaniem BUY. Zatem opóźniona dostępność mintu nie wyjaśnia wszystkich przekroczeń: pozostaje również szeregowy koszt obowiązującej bramki IWIM. Nie wyłączano jej ani nie zmieniano polityki. Inny przypadek ma payer_account_fetch_ms=2690 i mint_account_fetch_ms=2723; sam ten log nie rozstrzyga przyczyny opóźnienia odczytów. Pełne zestawienie: `r12-final-seven-attempts.json` oraz `r12-preparation-breakdown.json` w katalogu dowodów.

## D8. Stan końcowy

Zmiany wdrożone; kryterium rzeczywiście udanego BUY≤300ms nie zostało osiągnięte w sprawdzonej próbce. Nie zastępujemy tego wniosku samym działaniem limitu. Końcowy snapshot12:30:17UTC:7próbBUY,0udanych symulacji/pozycji,7execution_deadline_exceeded. Dlatego r12 nie ma PnL pozycji. Zapisano decyzje, failure records i lifecycle dispatch. Wcześniejszy snapshot health12:24:30UTC:0panic/alias errors w próbce, writer0errors/0drops/0writes.

Deadline nie anuluje całego przygotowania po300ms: kontroluje dopuszczenie i wynik symulacji. Zapisy błędów siedmiu prób powstały po334,493,3000,353,347,510,396ms od werdyktu. Nie wolno przedstawiać tego mechanizmu jako limitu czasu wszystkich operacji ani jako udanego przyspieszenia BUY. To istotna granica obecnego wdrożenia.

Aktywny run `predator-v11-20260920-buy-latency-r12`, PID651413, tmux `ghost-predator-v11-buy-latency-20260920-r12`, profil `/tmp/predator-v11-20260920-buy-latency-r12.toml`. Proces pozostawiono w tle z budżetem300ms. TP50/SL25, brainSHA5683db1094822aeac12fdb20bf547c9ecbe220c6f08e75483e2fbfe440f4ad0c bez zmian. PID/exeSHA potwierdzone. Launch manifest w katalogu runa; źródła i hash: candidate-final-source-sha256.json. Rollback binarki r9 zachowany w katalogu dowodów.

Dalsze domknięcie celu wymaga rozstrzygnięcia dostępności aktualnego stanu RPC, uwzględnienia szeregowego IWIM oraz ponownego pomiaru udanych prób; nie wolno omijać brakujących danych ani przedstawiać siedmiu odrzuceń jako skutecznego przyspieszenia BUY. Raport dla providera zawiera konkretne minty, sloty i pomiary. Zgodnie z wcześniejszą dyspozycją użytkownika dotyczącą potwierdzonej przeszkody zewnętrznej przekazujemy liczby do kontaktu z providerem. Po odpowiedzi nie wykonywać dalszych polling/restartów bez nowej dyspozycji.

Brak szablonu pod wskazanym `/Gho/docs/ADR/ADR_8D_SZABLON.md`; zachowano układ D0–D8 istniejących ADR repozytorium.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie jest wejściem tej naprawy runtime.
