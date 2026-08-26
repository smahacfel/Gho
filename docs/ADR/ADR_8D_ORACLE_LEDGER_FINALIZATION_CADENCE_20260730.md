# ADR-8D — Cadence finalizacji PumpObservationLedger poza szybkim pruningiem

**Data:** 2026-07-30
**Status:** ACCEPTED
**Zakres:** `ghost-launcher/src/components/seer.rs`

## Problem

Diagnostic IPC run wykazał, że `finalize_pump_observation_ledger()` jest
wywoływany razem z szybkim pruningiem około 20 razy na sekundę. W jednej
pętli `tokio::select!` pochłaniał około 57% czasu do saturacji IPC.

## Decyzja

Szybki pruning `SessionPoolTradeBridge` i `SessionAccountUpdateBridge` pozostaje
bez zmiany. Finalizer ledgeru dostaje osobny interval równy aktywnemu
`PumpObservationLedgerConfigV1::correlation_window_ns` (domyślnie 250 ms) i
`MissedTickBehavior::Skip`.

Finalizacja nadal wykonuje się synchronicznie w tej samej pętli `select!` oraz
zachowuje istniejące locki i kolejność emitowania decyzji.

## Granice

Nie dodano taska, `spawn_blocking`, nowej kolejki ani zmian capacity,
backpressure, fail-close, CandidateIntegrity, Gatekeepera, Brain, ACE lub PR2.

## Weryfikacja

Focused test potwierdza mapowanie domyślnego correlation window na cadence
250 ms. Regression run z istniejącą instrumentacją pozostaje warunkiem
operacyjnego uznania poprawki.
