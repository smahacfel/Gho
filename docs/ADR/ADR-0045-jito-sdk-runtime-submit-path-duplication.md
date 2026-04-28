# ADR-0045: Jito SDK Runtime Submit Path Duplication

**Date:** 2026-03-28
**Status:** Proposed
**Author:** Ghost Father

## Context

Pierwszy run `dual-micro-live` wszedł w runtime, ingest gRPC działał, eventy i shadow lane zapisywały się poprawnie, ale live BUY path regularnie kończył się błędem:

- `GATEKEEPER BUY PATH FAILED: Jito bundle submission failed: Jito bundle error: Failed to submit bundle: Request error: error decoding response body: EOF while parsing a value at line 1 column 0`

Dowody z runtime:

- `logs/rollout/dual-micro-live/oracle.log.2026-03-28` zawierał powtarzalne błędy `Jito bundle submission failed`.
- Zliczenie logów wykazało 18 takich błędów w trakcie pojedynczego runu.
- `datasets/events/dual-micro-live/*` zawierały wyłącznie eventy `Candidate`; nie było śladów udanej live egzekucji.
- `logs/shadow_run/dual-micro-live-buys.jsonl` zawierał poprawne wpisy shadow-run dla tych samych kandydatów.

Preflight był zielony, co oznaczało, że kontrakt reachability endpointu Jito w preflight różni się od rzeczywistego kontraktu runtime submission.

Analiza kodu wykazała następujący mismatch:

1. `ghost-launcher/src/components/trigger/component.rs` tworzy runtime client przez:
   - `JitoClient::new(endpoint, BundleConfig::default())`
2. `off-chain/components/trigger/src/jito_client.rs` normalizuje operator-supplied endpoint do:
   - `https://<host>/api/v1/bundles`
3. `jito-sdk-rust-0.3.2` traktuje przekazany `base_url` jako bazę i sam dokleja endpointy RPC:
   - `send_bundle()` używa `"/bundles"`
   - `get_tip_accounts()` używa `"/bundles"`
4. W efekcie runtime submit idzie na:
   - `https://<host>/api/v1/bundles/bundles`
   zamiast na:
   - `https://<host>/api/v1/bundles`

Dowód HTTP z produkcyjnego endpointu Amsterdam:

- `POST https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles`
  - zwraca JSON-RPC body (`429`, `-32097`, `Network congested...`)
- `POST https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles/bundles`
  - zwraca `404` z pustym body

To dokładnie tłumaczy runtime symptom `EOF while parsing a value at line 1 column 0`: SDK próbuje sparsować pustą odpowiedź jako JSON.

## Decision

Za SSOT RCA przyjmuje się, że aktualny repo-side kontrakt Jito ma rozjazd:

- preflight sprawdza reachability poprawnego path `.../api/v1/bundles`,
- runtime submission przez `jito-sdk-rust` trafia na błędny path `.../api/v1/bundles/bundles`.

Kolejny fix musi spełnić wszystkie poniższe warunki:

1. Runtime i preflight muszą używać tego samego kontraktu URL dla operacji Jito.
2. Nie wolno przekazywać do `JitoJsonRpcSDK::new(...)` bazy już zakończonej `.../api/v1/bundles`, jeśli SDK samo dokleja `"/bundles"`.
3. Submit path musi zostać zweryfikowany testem jednostkowym/integracyjnym obejmującym dokładny URL użyty przez SDK.
4. Błędy HTTP z pustym/non-JSON body muszą być klasyfikowane jawnie, z logowaniem statusu i URL, a nie tylko przez końcowe `EOF`.
5. Zielony `getTipAccounts` preflight nie może być traktowany jako dowód poprawności runtime submit path bez wspólnej warstwy kontraktowej.

## Architectural Impact

To ustalenie dotyczy bezpośrednio:

- `off-chain/components/trigger/src/jito_client.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- kontraktu preflight z `ghost-launcher/src/main.rs`
- zewnętrznej zależności `jito-sdk-rust`

Wpływ systemowy:

- preflight dawał fałszywie zielony sygnał dla endpointu, który runtime wykorzystywał inaczej,
- shadow lane pozostawał zdrowy, przez co problem ujawniał się wyłącznie w realnym live BUY path,
- first live run nie może być uznany za poprawny, dopóki runtime submit nie zostanie sprowadzony do tego samego kontraktu co preflight.

## Risk Assessment

**Rate:** High

Ryzyka regresji / produktu:

- każdy kolejny `dual` lub `live` run z publicznym Jito endpointem może przechodzić preflight i jednocześnie failować realny submit,
- operator może błędnie uznać rollout za gotowy na podstawie zielonego preflightu,
- runtime może generować shadow-success przy braku live execution, co tworzy semantyczny drift shadow/live,
- brak poprawnej klasyfikacji pustego body utrudnia szybkie wykrycie błędnego URL kontraktu.

## Consequences

Po tym RCA wiemy dokładnie, że problem nie jest „losową niestabilnością Jito”, lecz deterministycznym rozjazdem kontraktu URL między preflightem a runtime.

To ułatwia następny fix, bo zawęża go do:

- normalizacji/denormalizacji bazy URL przekazywanej do SDK,
- testu końcowego używanego path submit,
- lepszego logowania błędów HTTP/non-JSON.

To jednocześnie utrudnia utrzymanie obecnego stanu, bo nie można już bronić zielonego preflightu jako wystarczającego sygnału gotowości live.

## Alternatives Considered

### 1. Uznać błąd za chwilową niestabilność publicznego endpointu

Odrzucone, ponieważ:

- runtime fail był powtarzalny,
- manualny `sendBundle` na poprawnym path zwracał JSON body,
- tylko błędny path `.../bundles/bundles` dawał pustą odpowiedź zgodną z symptomem `EOF`.

### 2. Uznać, że publiczny endpoint wymaga wyłącznie UUID/autoryzacji

Odrzucone jako główna przyczyna tego incydentu, ponieważ:

- nawet bez UUID poprawny path odpowiadał JSON-em,
- symptom runtime był zgodny z 404 + pustym body, nie z czytelnym JSON-RPC auth error.

### 3. Zmienić jedynie preflight

Odrzucone, ponieważ problem leży w runtime submit path, a nie w samym probe `getTipAccounts`.

## Validation Steps

1. Zmienić kontrakt runtime Jito tak, aby `sendBundle` trafiał na ten sam logiczny endpoint co preflight.
2. Dodać test, który potwierdza finalny URL używany przez SDK dla submit.
3. Dodać test na pusty body / non-JSON response z czytelnym logowaniem statusu i URL.
4. Powtórzyć:
   - preflight `dual-micro-live`,
   - kontrolowany dual run,
   - obserwację `oracle.log` pod kątem `Jito bundle submission failed`.
5. Za warunek zamknięcia uznać co najmniej jedną poprawną live egzekucję bez rozjazdu shadow/live.
