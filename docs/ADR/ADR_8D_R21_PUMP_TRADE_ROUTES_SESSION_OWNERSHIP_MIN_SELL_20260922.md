# ADR-8D: r21 — brak tras BUY/SELL, przedwczesne usuwanie aktywnych kandydatów i niespójne min_sell_count

## D0. Dyspozycja i zakres

Użytkownik zlecił naprawę ustalonych przyczyn utraty kandydatów i braku BUY w r21. Zakres obejmuje parser Pump, własność rekordu CandidateIntegrity podczas sesji Oracle oraz spójność Phase 1 z `min_sell_count`. Nie zmieniono konfiguracji progów ani stanu uruchomionego procesu. To przygotowanie kodu do niezależnego review, nie dowód efektu na nowym runie.

Wskazany szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` oraz jego odpowiedniki pod `/root/Gho` i lokalnym `docs/ADR` nie były dostępne. Zachowano układ D0–D8 istniejących raportów ADR-8D w repozytorium.

## D1. Izolacja i baza

Kod: `/tmp/gho-r21-repair`, branch `fix/r21-ingest-phase1-integrity`.
Baza: `265969c2ee591e69263813ae8cf7f183bc9edf7a`.
Porównanie bazowe: `/tmp/gho-r21-base`, osobny target `/tmp/r21-base-target`.
Target naprawy: `/tmp/r21-target`; logi walidacji: `/tmp/r21-validation`.

Główny checkout `/root/Gho_ingest` zawiera niepowiązane zmiany. Nie przeniesiono ich do tej naprawy ani nie zastąpiono jej kopią brudnego checkoutu. Zmiany nie zostały zacommitowane, wypchnięte ani wdrożone.

## D2. Potwierdzone przyczyny

1. Parser nie przypisywał BuyV2/SellV2 poprawnych tras instrukcji, a SELL legacy nie otrzymywał `legacy_sell`. V2 ma inny układ kont niż legacy: mint 1, curve 10, user 13 i base_token_program 3. W legacy SELL token_program jest pod indeksem 9, nie 8.
2. Kod sprzątający traktował rekord PreMfs bez pending receipt jak kandydata bez sesji. PreMfs obejmuje również aktywne okno Oracle. Usunięty rekord skutkował `CandidateMissing` w czasie ewaluacji.
3. Runtime deadline sprawdzał wymagane sprzedaże, ale `build_assessment_from_features` pomijał `min_sell_count`. W rezultacie reason code i core1 mogły przeczyć warunkowi, który faktycznie zatrzymał decyzję.
4. Test rozszerzający odtworzenie parsera wykazał przypisywanie metadanych z innego wywołania dla tego samego mintu/poola: BUY V2 otrzymywał `legacy_buy`. Naprawiono wiązanie z outer index i ścieżką CPI.

5. Porównanie JSON ujawniło, że dotychczasowa projekcja `instruction_limit` używała przepływu SOL. Rozpoznanie SELL aktywowało tę niezgodną projekcję także dla nowych tras; dodano oddzielny literalny limit calldata bez zmiany wolumenów.

## D3. Dowody wejściowe

Dwie udane transakcje z diagnozy r21 znajdują się jako fixture w `off-chain/components/seer/tests/fixtures/r21/`. README zawiera signature, slot i SHA-256 oryginalnej odpowiedzi RPC. Instrukcje, salda i logi są rzeczywiste; provider/provenance/hash/tx_index otoczki Geyser są jawnie syntetyczne. Test nie dowodzi kompletności ani opóźnienia strumienia gRPC.

Kontrakt ABI sprawdzono w [oficjalnym IDL Pump](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/idl/pump.json). Wyciąg `pump_trade_idl_excerpt.json` zawiera układ kont, discriminators, args, datę odczytu i SHA-256 pełnego IDL. Nie traktujemy tego pliku jako authority transakcyjnej.

## D4. Implementacja

- Layout instrukcji wspólnie steruje rozpoznaniem trasy, identyfikacją puli i doborem opcjonalnych kont. V2 wymaga długości payload 24 bajty, pełnego zestawu kont oraz quote WSOL; obca waluta nie jest interpretowana jako SOL.
- SELL otrzymuje trasę mimo zachowanej historycznej nazwy pola `buy_variant`. Bez migracji istniejącego schematu.
- Instrukcja będąca właścicielem zdarzenia jest ustalana z istniejącej provenance. Nie wybieramy metadanych z sąsiedniej instrukcji tylko dlatego, że ma ten sam mint i pool.
- `claim_oracle_session` następuje przed spawnem zadania i rozliczeniem CREATE. Wymaga autentycznego, pending receipt InitializePool i otwartego admission. Bounded zbiór właścicieli chroni rekord przed sprzątaniem pre-session. Terminalny cleanup nadal rozlicza receipts i zwalnia własność.
- `TradeEvent.instruction_limit` jest addytywnym polem `Option` z serde default i pomijaniem None. Nowy parser pobiera limit z właściwej instrukcji; primary observation i kontrola wrappera używają tego samego pola. Zero jest poprawnym literalnym minimum SELL. Dawne payloady bez pola zachowują projekcję kompatybilności; nie migrowano historycznych danych. Dodatkowe jednolinijkowe zmiany konstruktorów ustawiają `None`.
- Własność nie nadaje Ready. Konflikt/incomplete coverage pozostaje typed failure i późniejsze Ready go nie kasuje.
- Jeden `phase1_passes` czyta tx/unique/buy/sell z MFS zarówno dla assessment, jak i deadline. JSONL dostaje addytywnie `sell_count` i `min_sell_count` z `serde(default)`; liczby sprzedaży nie odtwarzamy jako total-minus-buy.

## D5. Kontrakty i granice

Zachowane: MFS jako SSOT, ścisła kontrola primary observation, typed failures, ochrona terminalnego cleanup, bounded registry i shadow/live separation. Brak nowych RPC, retries, await pod lockiem, zmiany okna, progów, TP/SL albo timestampów.

To naprawa rozpoznania obserwowanych BuyV2/SellV2 oraz legacy SELL. Nie stanowi deklaracji obsługi każdej instrukcji dostępnej w IDL ani nadania SELL V2 uprawnień wykonania transakcji. Istniejący osobny execution gate pozostaje w mocy.

## D6. Walidacja

### Testy naprawy

- `cargo test --offline -p seer --test r21_trade_routes`: **8 PASS**, w tym rzeczywiste Create+BuyV2, zimny SellV2, legacy BUY/SELL, niepełny payload, obcy quote, dwa wywołania V2, mieszane warianty rodzeństwo CPI, oddzielenie limitu od przepływu i zgodność serde.
- `cargo test --offline -p ghost-launcher --lib r21_`: **5 PASS**. Obejmuje oba porządki CREATE ACK/failure, autentyczność receipt, terminalny cleanup i zwolnienie właściciela, pełne przejście parser→primary boundary→ledger/registry Ready, sprzeczność strony, min_sell_count oraz zgodność starego JSONL.
- CandidateIntegrity: **37 PASS**. Gatekeeper policy: **33 PASS**.
- Test jawnej rewizji digestu parsera R21: **1 PASS**.
- Końcowe `cargo check --offline -p ghost-launcher --bin ghost-launcher`: **PASS**, z istniejącymi ostrzeżeniami.
- `cargo check --offline -p ghost-launcher -p seer --tests`: **FAIL** — dwa istniejące konstruktory `PoolTransaction` w `oracle_transaction_gathering` i `cpv_successful_buy_contract_tests` nie mają pól rezerw i complete. Nie dotyczy nowego pola TradeEvent. Porównanie bazowe jest dołączone; nie naprawiano tych testów poza zakresem zadania.
- `git diff --check`: **PASS**. Formatowano zmienione fragmenty i nowy test, bez formatowania całego workspace.

RED przed naprawą: 2/2 testy parsera odrzucały brakujące warianty; test Phase1 wykazywał `phase1_passed=true` przy SELL0/3. Dodatkowy test mieszanych wywołań odtworzył `legacy_buy` zamiast `buy_v2`; po poprawce przechodzi. Logi RED/GREEN zachowano w pakiecie.

### Szersze testy i porównanie bazowe

Pełne sprawdzone zestawy nie są zielone. Nie przedstawiamy ich jako PASS:

| Zestaw | Baza | Naprawa | Rozstrzygnięcie |
|---|---:|---:|---|
| Launcher / Seer-wrapper | 80 PASS / 5 FAIL | 81 PASS / 5 FAIL | Te same 5 nazw i przyczyn FAIL; dodatkowy test naprawy PASS |
| Runtime / feature_driven | 4 PASS / 4 FAIL | 4 PASS / 4 FAIL | Te same FAIL, m.in. timing.count_ratio |
| Terminal cleanup | 2 PASS / 1 FAIL | 2 PASS / 1 FAIL | Ten sam test FAIL; zawiera się też w Seer-wrapper |
| Seer lib, sandbox — finalny kod | 579 PASS / 22 FAIL / 3 ignored | 580 PASS / 21 FAIL / 3 ignored | Wszystkie końcowe FAIL występują też na bazie; test hasha R21 PASS |

Test IPC `test_trade_with_known_mint_registers_optimistic_mapping_and_forwards` także zawiódł na bazie w przebiegu sandbox; osobno na naprawie przeszedł. Nie uznano go za dowód nowej regresji. Po ustaleniu różnic zaktualizowano oczekiwany digest; jego test oraz końcowy przebieg całego Seer lib są w logach. W pomocniczym szeregowym przebiegu poza sandboxem potwierdzono 13 istniejących FAIL na bazie. To porównanie poprzedza finalne dodanie literalnego limitu; końcowy pełny przebieg wykonano w sandboxie.

Logger: **39 PASS / 1 FAIL**. Jedyny FAIL `test_selector_shadow_score_filters_non_finite_feature_values` odtworzono na czystej bazie z identyczną asercją `gk_vector_price_first`. Nie jest wynikiem dodania sell_count/min_sell_count.

### Jawna rewizja oczekiwanych digestów

Nie zmieniono surowych danych wejściowych historycznego harnessu. Porównano JSON przed/po na tych samych pięciu fixtures. Zmiany SELL obejmują `buy_variant` i `token_program`; pełny snapshot obejmuje ponadto jawny `instruction_limit` i korektę `claims.instruction_limit` dla BUY/SELL. V1 jawnie pomija nowe pole, zachowując historyczny kształt JSON; V2 zawiera nowe pole i poprawne literalne wartości.

Harness ma syntetyczny indeks9 `[10;32]`; nie przedstawiamy tej fixture jako poprawnej transakcji mainnet. Nowy test legacy layout korzysta z poprawnego token program pod indeksem9.

| Projekcja | Historyczny hash z bazy | Rewizja R21 |
|---|---|---|
| V1 | `549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202` | `302df87c36e23b3b3dd7189828aba9ded44582304e768450427d7694b33f7459` |
| V2 | `02136d691e399dace85b112cc5b6d50c79323a2f24adcb3e7569ac68b40654a6` | `581d8bc83036054f7e24dce1e23dce01d0edbd31dae4a08d95eb34d5005fd1fc` |

Dokładny porównany JSON: `parser-semantic-diff.json` w pakiecie review. Tymczasowe wydruki służące porównaniu zostały usunięte z obu drzew.

## D7. Self-review

Self-review objął cały diff produkcyjny i testowy: granicę parser→primary observation, zgodność layoutów z IDL, właściciela metadanych, pending receipts, dwa porządki ACK, zachowanie typed failure, zwolnienie własności, MFS jako źródło countów i zgodność JSONL. Sprawdzono jedyne produkcyjne miejsce spawnu observation task oraz wszystkie wywołania retire_resolved_record. Nie stwierdzono nowej niezamierzonej regresji w zweryfikowanym zakresie. Niezależny review Pro pozostaje odrębnym krokiem.

**Dodatkowa naprawa wynikająca z self-review:** `max_sol_cost` / `min_sol_output` zachowują historyczną rolę przepływu SOL. Literalny limit jest oddzielony w nowym opcjonalnym polu i pochodzi z bajtów instrukcji. Fixture SELL z limitem20 mln ma teraz limit20 mln, bez zastępowania nim przepływu100 mln. Osobny test zmienia literalny limit przy niezmienionym zdarzeniu CPI i potwierdza niezmienność przepływu oraz zachowanie minimum0. Starsze zapisane payloady bez pola wciąż korzystają z dawnej projekcji; nie należy traktować tej zgodności wstecznej jako retroaktywnego poprawienia ich dowodów.

Brak nowego runa oznacza brak deklaracji o poprawionym odsetku BUY, PnL, pełnym pokryciu wszystkich tokenów i opóźnieniu gRPC.

## D8. Handoff i runtime

Pakiet do review: `/tmp/r21-review/`, z patchem względem wskazanej bazy, manifestem SHA-256, wynikami i logami. Autorytatywny hash patcha znajduje się w zewnętrznym manifeście pakietu, aby uniknąć samoodnoszącego się hasha w tym dokumencie.

Zmiany pozostają lokalnie w izolowanym worktree. Progi i uruchomione procesy nie były zmieniane. Nie utworzono PR ani nie dokonano wdrożenia.

```yaml
delegation_trace:
  task_classification: "naprawa ingest, cyklu sesji i warunku Gatekeepera"
  routing_performed: true
  primary_specialist: "seer-ingest-event-integrity-specialist (rola logiczna)"
  supporting_specialists_considered: [oracle-session-runtime-engineer, gatekeeper-policy-auditor, decision-logging-replay-analyst]
  specialist_docs_loaded: [seer-ingest-event-integrity-specialist, oracle-session-runtime-engineer, gatekeeper-policy-auditor]
  specialist_docs_not_loaded:
    - name: solana-execution-path-engineer
      reason: "brak zmiany buildera, submit i uprawnień execution"
  skills_used: [ghost-execution, rust-master, solana-pumpfun-architect]
  fast_path_used: false
  contracts_checked: [MFS_SSOT, primary_observation, session_lifecycle, typed_verdicts, bounded_retention, JSONL_compatibility, shadow_live_separation]
  unresolved_routing_uncertainty: []
```

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie był wejściem tej naprawy.
