# ADR-8D — Watcher diagnostyczny: snapshot metryk przed kontrolowanym stopem

**Data:** 2026-07-29
**Status:** ACCEPTED — diagnostic-only
**Zakres:** `scripts/ace_core_one_day_diagnostic_watch.py`

## 1. Problem

Poprzedni run IPC osiągnął `IPC_EGRESS_SATURATED`, lecz watcher otrzymał
nieprawidłowy port Prometheusa. Nie zapisał przez to histogramów potrzebnych
do klasyfikacji, a kontrolowany stop zamknął endpoint metrics.

## 2. Decyzja

Watcher przyjmuje opcjonalny preflight `GET /metrics`. Po wykryciu markera
najpierw pobiera metryki, zapisuje je, wykonuje `flush` i `fsync`, a dopiero
potem wysyła `SIGINT` do launchera.

## 3. Granice

Zmiana nie dotyka binarki `ghost-launcher`, IPC capacity, backpressure,
fail-close, `tokio::select!`, CandidateIntegrity ani konfiguracji Dnia 1.
Manifest diagnostycznego runu nadal wskazuje SHA binarki `de5c684...`.

## 4. Weryfikacja

`scripts/test_ace_core_one_day_diagnostic_watch.py` potwierdza, że snapshot
jest zapisany i `fsync` wywołany przed potencjalnym stopem.
