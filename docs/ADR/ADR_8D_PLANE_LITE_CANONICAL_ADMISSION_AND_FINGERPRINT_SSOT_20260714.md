# ADR-8D: Plane-Lite — kanoniczne admission i jeden właściciel fingerprintu

Status: `IMPLEMENTED / LOCAL VALIDATION PASSED / CI PENDING`

Typ: ADR-8D / Decision Plane / aktywny prebuy runtime

Data: 2026-07-14

Repo: `smahacfel/Gho`

Branch: `main`

Base SHA: `aca107afc14b3122899af2afa2091ccfb400f498`

Poziom ryzyka: `MEDIUM` — zmiana porządkuje populację zdarzeń widzianą przez
reduktory prebuy, ale nie zmienia polityki, progów, konfiguracji ani trybu
wykonania.

## 1. Problem

Aktywny prebuy runtime miał dwa źródła rozjazdu danych:

1. sesyjny dedup następował dopiero po wejściu zdarzenia do części reducerów;
2. `pool_observation_task` utrzymywał drugi `FingerprintAggregator` równolegle
   do kanonicznego agregatora należącego do `TxIntelligenceEngine`.

W konsekwencji duplicate, dust albo failed transaction mogły być widziane
przez różne komponenty według innych populacji, a terminalny
`early_fingerprint` pochodził z runtime-local agregatora zamiast z właściciela
stanu sesji.

## 2. Decyzja

Wprowadzono bezpośredni cutover bez flagi i bez okresu dual-compute:

```text
PoolTransaction
  -> PoolObservationSession::admit_transaction
  -> wyłącznie admitted event
  -> TxIntelligence / Gatekeeper / CPV / retained history
  -> session-owned fingerprint
  -> terminal GatekeeperAssessment
```

`PoolObservationSession` jest jedyną pierwszą bramką admission dla zdarzeń
transakcyjnych trafiających do Decision Plane. Klucz nadal pochodzi z
`GatekeeperBuffer::tx_key_for`, dzięki czemu zachowana jest bieżąca semantyka
tożsamości zdarzenia.

Duplicate kończy przetwarzanie przed wszystkimi reducerami. Jego event-time
może jedynie przesunąć zegar deadline, aby powtórzona dostawa nie blokowała
zamknięcia obserwacji. Sesje `Decided` i `Closed` są immutable: każde późne
zdarzenie jest pełnym no-op.

## 3. Kontrakt populacji zdarzeń

| Klasa zdarzenia | Admission | TxIntel/Gatekeeper attempt | Trade/history | CPV | Fingerprint | FlipV2 eligible |
| --- | --- | --- | --- | --- | --- | --- |
| unique, successful, non-dust | tak | tak | tak | tak | tak | według istniejących kryteriów |
| exact duplicate | nie | nie | nie | nie | nie | nie |
| ta sama signature, inny `event_ordinal` | osobne zdarzenie | tak | tak, jeśli eligible | tak, jeśli eligible | tak, jeśli eligible | według istniejących kryteriów |
| unique dust | tak, tylko klasyfikacja | tylko istniejący dust diagnostic | nie | nie | nie | nie |
| unique failed non-dust | tak | tak, zgodnie z V2 baseline | retained attempt history | nie | nie | nie |
| event po `Decided`/`Closed` | nie | nie | nie | nie | nie | nie |

Nie zmieniono celowo istniejącej semantyki V2, według której failed non-dust
pozostaje częścią attempt/core counts. Plane-Lite usuwa jedynie możliwość, aby
failed transaction stał się pozytywnym fingerprint/CPV/Flip evidence.

## 4. Fingerprint SSOT

Usunięto runtime-local `FingerprintAggregator`, jego adapter
`pool_tx_to_fingerprint_event`, reinitializację i finalizację w
`pool_observation_task`.

Jedynym właścicielem fingerprintu w aktywnym launcherze jest teraz:

```text
PoolObservationSession
  -> TxIntelligenceEngine
  -> fingerprint_agg
```

Terminalne warianty `BUY`, `REJECT` i `TIMEOUT` ustawiają
`assessment.early_fingerprint` przez `session.fingerprint_metrics()`.

Późny `NewPoolDetected` aktualizuje dev identity i anchor jednym wywołaniem
sesyjnego właściciela. Rebuild agregatora deterministycznie odtwarza wcześniej
przyjęte, successful, non-dust i fingerprint-convertible eventy z bounded
historii. Dzięki temu późna korekta creation slot/t0/deva może przeliczyć
metryki, ale nie może po cichu wyzerować wcześniejszego evidence. Przepełnienie
historii blokuje częściowy rebuild, zachowuje kompletny bieżący agregator i
jest jawnie raportowane jako `FINGERPRINT_REPLAY_HISTORY_TRUNCATED`.

Szczegóły review fixu i jego red/green proof opisuje
`ADR_8D_PLANE_LITE_FINGERPRINT_ANCHOR_LIFECYCLE_FIX_20260714.md`.

## 5. Celowo niezmieniony zakres

Ta implementacja nie zmienia i nie ogranicza rozwoju Decision Plane:

- Gatekeeper V2 policy, kolejności faz, progów i reason codes;
- Gatekeeper V2.5, DOW, TAS, PDD i APS;
- Gatekeeper V3 ani jego shadow/replay;
- Type-5 ani planu jego późniejszej integracji z V3;
- selectora;
- IWIM — pozostaje całkowicie nietknięty zgodnie z przyjętym zakresem;
- `MaterializedFeatureSet` i jego schema;
- DecisionLogger i replay schema;
- configu oraz defaults;
- postbuy, shadow execution, live execution i execution handoff.

Nie utworzono nowego event modelu, feature store, snapshotu, frameworka
authority ani rozbudowanego systemu scope IDs.

## 6. Zmienione pliki

- `ghost-launcher/src/session/observation.rs` — pierwsza sesyjna bramka
  admission, dedup i terminal no-op;
- `ghost-launcher/src/tx_intelligence/engine.rs` — fingerprint dopiero po
  lokalnej walidacji unique/non-dust oraz tylko dla successful, bounded replay
  przy zmianie anchora/deva;
- `ghost-launcher/src/oracle_runtime.rs` — usunięcie drugiego agregatora,
  terminalny odczyt fingerprintu z sesji oraz atomowe przekazanie późnego
  pool identity/anchora do właściciela sesyjnego;
- `ghost-launcher/tests/session_lifecycle_tests.rs` — kontrakty duplicate,
  ordinal/timestamp identity, dust, failed, late-after-terminal oraz zachowanie
  fingerprint evidence po późnej aktualizacji metadanych;
- `ghost-launcher/tests/gatekeeper_policy_tests.rs` — jawna time provenance
  wieloeventowego fixture po wprowadzeniu wcześniejszego admission;
- `ghost-launcher/tests/refactor_invariants_tests.rs` — guard trzech
  terminalnych odczytów session-owned fingerprintu.

## 7. Weryfikacja

Zaliczone:

- `cargo test -p ghost-launcher --test session_lifecycle_tests -- --nocapture`
  — `32/32`;
- `cargo test -p ghost-launcher --lib tx_intelligence::engine::tests -- --nocapture`
  — `7/7`;
- `cargo test -p ghost-launcher --test gatekeeper_policy_tests` — `46/46`;
- `cargo test -p ghost-launcher --test metric_contracts_pr2a_producers -- --nocapture`
  — `26/26`;
- `cargo test -p ghost-launcher --test gatekeeper_v25_regression`
  — `42/42`;
- `cargo test -p ghost-launcher --test gatekeeper_v3_tests`
  — `9/9`;
- `cargo test -p ghost-launcher --test refactor_invariants_tests` — `12/12`;
- `cargo test -p ghost-brain --lib replay_payload` — `5/5`;
- `cargo check -p ghost-launcher` — PASS;
- `cargo fmt --all -- --check` — PASS;
- `git diff --check` — PASS.

Rozpoznane baseline'y, odtworzone albo istniejące przed zmianą:

- `tx_intelligence_tests`: `9/10`; znany PR2B mismatch
  `recent buy/sell snapshot` w `session_tx_buffer_is_bounded`;
- pięć testów `cutover_feature_driven_terminal_verdict`: `2/5`; dokładnie te
  same trzy awarie odtworzono na czystym worktree z base SHA — dwa stare
  fixture'y `timing.count_ratio` i jedno stare oczekiwanie `PendingCurve`;
- `gatekeeper_v2_pipeline_integration`: dwa pierwsze testy przechodzą, następnie
  bazowy `test_full_pipeline_reject` kończy proces stack overflow; ten sam wynik
  odtworzono na czystym worktree z base SHA.

Żadnego z wymienionych baseline'ów nie maskowano. Jedyna korekta fixture'a
dotyczyła jawnego `EventTimeMetadata` w zielonej na base suite
`gatekeeper_policy_tests`; nie zmieniono jej danych ekonomicznych, oczekiwanego
verdictu ani Gatekeeper policy.

## 8. Rollback i następny krok

Rollback jest atomowy na poziomie commitów Plane-Lite i review fixów. Nie ma
migracji danych, configu ani schema.

Po akceptacji Plane-Lite następnym niezależnym krokiem może być odchudzona
integracja Type-5 z V3: zachować V2.5/V3 jako rozwijane challengery, dostarczyć
V3 tylko nowe decision-time evidence i zmierzyć incremental edge przed każdą
promocją. Plane-Lite nie przesądza wyniku tej walidacji i nie usuwa żadnego z
tych komponentów.

Uwaga: wskazany przez instrukcję repo szablon `ADR_8D_SZABLON.md` nie występuje
w dostępnym checkoutcie; zachowano stosowany lokalnie format dokumentów ADR-8D.
