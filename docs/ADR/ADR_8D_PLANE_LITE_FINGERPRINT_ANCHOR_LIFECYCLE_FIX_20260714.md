# ADR-8D: Plane-Lite — zachowanie fingerprint evidence po późnym anchorze

Status: `IMPLEMENTED / LOCAL VALIDATION PASSED / CI PENDING`

Typ: ADR-8D / Decision Plane / review fix / aktywny prebuy runtime

Data: 2026-07-14

Repo: `smahacfel/Gho`

PR: `#66`

Poziom ryzyka: `MEDIUM` — naprawa zmienia lifecycle kanonicznego fingerprintu,
ale nie zmienia Gatekeeper policy, progów, konfiguracji ani execution mode.

## 1. Problem wykryty w review

Plane-Lite usunął drugi runtime-local `FingerprintAggregator` i przełączył
terminalne ścieżki `BUY`, `REJECT` i `TIMEOUT` na fingerprint należący do
`PoolObservationSession -> TxIntelligenceEngine`.

Sesyjny właściciel nie zachowywał jednak stanu przy późnej aktualizacji
metadanych. `set_dev_identity()` oraz `update_fingerprint_anchor()` tworzyły
nowy, pusty agregator. Dwa kolejne wywołania z obsługi późnego
`NewPoolDetected` usuwały więc każde fingerprint evidence przyjęte przed
poznaniem prawdziwego deva, creation slotu lub t0.

Doprecyzowanie względem review: runtime call site późnego anchora już istniał.
Brakiem nie było samo połączenie `NewPoolDetected` z sesją, lecz destrukcyjna
semantyka dwóch rebuildów bez replayu wcześniejszych eventów.

## 2. Red proof

Najpierw dodano test odtwarzający rzeczywistą kolejność:

```text
eligible BUY + SELL
  -> fingerprint zawiera evidence
  -> late dev identity + creation slot + t0
  -> terminal BUY / REJECT / TIMEOUT
```

Przed poprawką test kończył się błędem: po późnej aktualizacji
`sell_buy_ratio` zmieniał się z `Some(1.0)` na `None`. Test nie został
osłabiony ani zastąpiony po implementacji.

## 3. Decyzja implementacyjna

`TxIntelligenceEngine` zachowuje bounded `VecDeque<FingerprintTxEvent>` dla
wszystkich eventów, które przeszły kanoniczne sesyjne admission, są successful,
non-dust i dają się przekształcić do wejścia fingerprintu.

Historia zachowuje również eventy poza aktualnym oknem. Jest to wymagane,
ponieważ późna korekta t0 może zmienić przynależność eventu do okna.

Każda zmiana deva lub anchora:

1. tworzy nowy agregator z aktualnymi metadanymi;
2. przechodzi po retained events w stabilnej kolejności przyjęcia;
3. ponownie sprawdza `in_window()` względem nowego t0;
4. podmienia agregator dopiero po zakończeniu replayu.

Późny `NewPoolDetected` używa jednego atomowego adaptera:

```text
PoolObservationSession::update_tx_intelligence_pool_identity_and_fingerprint_anchor
  -> TxIntelligenceEngine::update_pool_identity_and_fingerprint_anchor
  -> jeden rebuild + deterministic replay
```

Nie ma przejściowego stanu po rebuildzie identity, ale przed rebuildem anchora.

## 4. Boundedness i degraded semantics

Historia korzysta z istniejącego `tx_key_capacity`; nie dodano nowego configu.
Po przekroczeniu capacity najstarsze replay eventy są usuwane. Bieżący
agregator nadal zawiera wszystkie eventy, dlatego nie jest zastępowany
częściowym replayem. Jeżeli późniejsza zmiana metadanych wymaga rebuilda,
rebuild zostaje pominięty, dotychczasowe evidence pozostaje bez zmian, a
fingerprint staje się jawnie degraded z reason:

`FINGERPRINT_REPLAY_HISTORY_TRUNCATED`

Flaga pominiętego rebuilda jest monotoniczna dla życia sesji. System nie
przedstawia częściowego replayu jako pełnego SSOT; w tym skrajnym przypadku
zachowuje pełny stan według poprzedniego anchora zamiast zastosować późne
metadane kosztem utraty wcześniejszych eventów.

## 5. TxKey timestamp drift

Plane-Lite celowo nie zmienia globalnej semantyki `TxKey`. Znormalizowany
event timestamp pozostaje częścią equality/order, dlatego ta sama signature i
ten sam ordinal z innym timestampem są obecnie dwoma odrębnymi eventami.

Kontrakt został nazwany komentarzem przy admission i osobnym testem. Ewentualna
zmiana identity musi być oddzielną migracją obejmującą wszystkie konsumenty
`TxKey`; nie została ukryta w review fixie fingerprintu.

## 6. CI follow-up: jawny czas fixture i aktualny static guard

Pierwszy run CI po review fixie ujawnił dwa stare test contracts, które nadal
opisywały stan sprzed Plane-Lite:

1. `curve_tx()` w `gatekeeper_policy_tests` tworzył pięć zdarzeń z
   `EventTimeMetadata::default()`, nieważnymi testowymi signature i tym samym
   ordinalem. Base przechodził dzięki przypadkowemu rozdzieleniu wall-clock
   fallbacków podczas późniejszego ingestu; wcześniejsze canonical admission
   ujawniło kolizję. Fixture otrzymał jawny `ingress_wall_ts_ms` równy swojemu
   istniejącemu testowemu timestampowi. Runtime time semantics i Gatekeeper
   policy pozostały bez zmian.
2. `refactor_invariants_tests` nadal szukał starego przypisania
   `early_fingerprint = Some(...)`. Guard został zaostrzony do dokładnie trzech
   przypisań z `session.read().fingerprint_metrics()` i nadal sprawdza, że
   znajdują się wyłącznie w ramionach `REJECT`, `TIMEOUT` i `BUY`.

Porównanie diagnostyczne przed korektą fixture: czysty base `4/4`, current head
`2/4` dla filtra `feature_policy_`. Po jawnej provenance pełne
`gatekeeper_policy_tests` przechodzi `46/46`.

## 7. Zakres zmian

- `ghost-launcher/src/tx_intelligence/engine.rs` — bounded retained events,
  deterministic rebuild/replay, atomic late metadata update i degraded reason;
- `ghost-launcher/src/session/observation.rs` — atomowa fasada właściciela oraz
  jawny kontrakt timestamp drift;
- `ghost-launcher/src/oracle_runtime.rs` — jedno wywołanie sesyjnego ownera dla
  późnego `NewPoolDetected`;
- `ghost-launcher/tests/session_lifecycle_tests.rs` — late-anchor lifecycle dla
  `BUY`/`REJECT`/`TIMEOUT`, duplicate stability i timestamp-drift contract;
- `ghost-launcher/tests/gatekeeper_policy_tests.rs` — jawny ingress-wall time
  dla fixture'ów, które rzeczywiście testują wieloeventową policy;
- `ghost-launcher/tests/refactor_invariants_tests.rs` — static guard wymaga
  dokładnie trzech terminalnych odczytów session-owned fingerprintu;
- ADR Plane-Lite — korekta opisu lifecycle fingerprintu.

## 8. Niezmienione kontrakty

- `MaterializedFeatureSet` i feature formulas;
- Gatekeeper V2/V2.5/V3 policy, verdicts, thresholds i reason codes;
- selector, Type-5 oraz IWIM;
- config i serde schema;
- DecisionLogger/replay schema;
- shadow/live separation, execution i postbuy;
- existing `TxKey` identity semantics.

## 9. Weryfikacja i warunki akceptacji

Red/green proof:

- nowy late-anchor lifecycle test przed implementacją: FAIL — po aktualizacji
  `sell_buy_ratio` wynosiło `None` zamiast `Some(1.0)`;
- ten sam, nieosłabiony test po implementacji: PASS.

Końcowa walidacja lokalna:

- `cargo test -p ghost-launcher --lib tx_intelligence::engine::tests -- --nocapture`
  — `7/7`;
- `cargo test -p ghost-launcher --test session_lifecycle_tests -- --nocapture`
  — `32/32`;
- `cargo test -p ghost-launcher --test gatekeeper_policy_tests` — `46/46`;
- `cargo test -p ghost-launcher --test metric_contracts_pr2a_producers -- --nocapture`
  — `26/26`;
- `cargo test -p ghost-launcher --test gatekeeper_v25_regression` — `42/42`;
- `cargo test -p ghost-launcher --test gatekeeper_v3_tests` — `9/9`;
- `cargo test -p ghost-launcher --test refactor_invariants_tests` — `12/12`;
- `cargo test -p ghost-brain --lib replay_payload` — `5/5`;
- `cargo check -p ghost-launcher` — PASS;
- `cargo fmt --all -- --check` — PASS;
- `git diff --check` — PASS.

`session_lifecycle_tests` potwierdza identyczny fingerprint po duplicate i dla
terminalnych wariantów `BUY`/`REJECT`/`TIMEOUT`, a także jawny kontrakt
timestamp drift. Test modułu silnika potwierdza, że overflow nie uruchamia
częściowego replayu i emituje degraded reason przy próbie rebuilda.

PR pozostaje draftem do czasu zakończenia pełnego wymaganego CI. Znane,
niezależne baseline'y nie są przedstawiane jako regresje ani maskowane
zmianami poza zakresem.

## 10. Rollback

Rollback to revert follow-up commita. Nie ma migracji danych, configu ani
schema. Revert przywraca destrukcyjny rebuild i dlatego jest dopuszczalny tylko
łącznie z wycofaniem Plane-Lite fingerprint cutover albo inną równoważną
naprawą lifecycle.

Uwaga: wskazany przez instrukcję repo szablon `ADR_8D_SZABLON.md` nie występuje
w dostępnym checkoutcie; zachowano format zastosowany w ADR Plane-Lite.
