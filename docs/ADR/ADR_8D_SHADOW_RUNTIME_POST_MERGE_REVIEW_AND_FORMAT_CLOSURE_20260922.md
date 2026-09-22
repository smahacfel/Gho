# ADR-8D: Domknięcie przeglądu zmian shadow runtime po scaleniu

## D0. Dyspozycja

Przeanalizować ostatnie zmiany shadow runtime od punktowego potwierdzenia świeżości wyjścia RPC, ustalić ich stan publikacji i opublikować w jednym PR wyłącznie nadal nieopublikowane, uzasadnione poprawki.

## D1. Baza i izolacja

Przegląd wykonano w osobnym worktree `/tmp/gho-shadow-consolidated-review` na bazie `fbed59d`, czyli aktualnego `origin/agent/pump-research-go-d-frozen-authority` po scaleniu PR #95 i #96. Brudny główny checkout oraz zmiany badawcze i operacyjne spoza zakresu pozostają nietknięte.

## D2. Stan publikacji

PR #95 zawiera naprawę punktowego potwierdzenia świeżości wyjścia, poprawkę retry po ticku 498-499 ms, zachowanie tożsamości poola oraz pomiar opóźnienia BUY. PR #96 zawiera canonical creator, ponowną wycenę legacy BUY z najnowszego `AccountStateCore` przed dispatch oraz konfigurowalne `min_sell_count`. Oba PR są już scalone do bazy.

Jedyną późniejszą lokalną zmianą kodu było tymczasowe podniesienie `DIAG_ACCOUNT_UPDATE_RELAY` z `debug` do `info`. Zmiana emitowałaby rekord dla każdego AccountUpdate, także replayowanego, dlatego została odrzucona jako regresja hot path i cofnięta przed publikacją.

## D3. Potwierdzenie wyjścia RPC

Ścieżka jest aktywna wyłącznie w monitorze `shadow`. Powstaje dopiero dla istniejącego wyjścia z `StaleSnapshot`, działa w ograniczonym budżecie recovery i nie zastępuje canonical state. Odpowiedź jest wiązana z pozycją, rewizją, ilością, PDA krzywej, ownerem, discriminatorem, slotem oraz pełnym hashem bajtów. Rozbieżność, timeout, zmieniona pozycja albo zmieniony canonical snapshot blokują użycie potwierdzenia.

## D4. Pozostałe kontrakty

Canonical creator pochodzi z konta Pump należącego do programu Pump, przechodzi addytywnie przez transport i jest zachowywany przez compatibility updates bez drugiego źródła authority. Legacy BUY jest ponownie wyceniany z bieżącego `AccountStateCore` bezpośrednio przed dispatch i fail-closed przy zmianie danych. `min_sell_count` ma zgodny wstecznie domyślny próg `0` i uczestniczy we wszystkich właściwych wariantach Phase 1. `decision_to_buy_ms` jest addytywnym, opcjonalnym polem zapisu shadow entry.

## D5. Usterki wykryte w przeglądzie

Aktualny remote base nie przechodził `cargo fmt --all -- --check`; różnice dotyczyły pięciu plików objętych przeglądanym zakresem. Ponadto komentarz `shadow_market_refresh_rpc_url` błędnie opisywał endpoint jako używany wyłącznie przez okresowy stale-market refresh, mimo że zasila on również punktowe potwierdzenie starej wyceny wyjścia.

## D6. Naprawa

Zastosowano wyłącznie `cargo fmt` do wskazanych plików oraz poprawiono komentarz pola konfiguracji. Zmiana nie modyfikuje logiki, progów, formatu danych, canonical timestamps, inactivity, Gatekeepera ani granicy shadow/live.

## D7. Weryfikacja i przegląd

Testy punktowego potwierdzenia wyjścia: 9 PASS. Testy canonical creator w Seer i `AccountStateCore`, re-quote przed dispatch, `min_sell_count`, zgodności starego TOML, zapisu `decision_to_buy_ms` oraz JSONL shadow: 7 PASS. Końcowe `cargo fmt --all -- --check`, `git diff --check` oraz ponowiony test lifecycle/`decision_to_buy_ms`: PASS. Przegląd diffu potwierdza brak zmian semantycznych poza korektą dokumentacji pola RPC.

## D8. Publikacja i runtime

Follow-up jest publikowany jako jeden nowy PR na bazę `agent/pump-research-go-d-frozen-authority`. Nie restartuje, nie uruchamia i nie modyfikuje żadnego runa. Nie zawiera logów runtime, konfiguracji progów ani danych historycznych.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie był wejściem tej naprawy.
