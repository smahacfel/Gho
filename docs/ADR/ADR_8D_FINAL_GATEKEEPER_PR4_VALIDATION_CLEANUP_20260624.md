# ADR-8D: Final Gatekeeper PR4 Validation Cleanup

Data: 2026-06-24

Status: accepted

Zakres: final validation cleanup after Gatekeeper PR1-PR4 plan

## Problem

Finalna walidacja po PR4 wykazala dwa czerwone punkty niezalezne od zmian progow i decyzji Gatekeepera:

- `session_lifecycle_tests` padal na `filtered_transfer_does_not_unlock_fsc_when_stream_is_only_health_ready`, bo health-only filtered Pump funding lane mogl ustawic rolling FSC state jako warmed.
- `cargo test -p ghost-core -- --nocapture` padal na doctescie w `ghost-core/src/shadow_ledger/mod.rs`, bo przyklad nadal uzywal starej sygnatury `simulate_buy(..., u64)` zamiast aktualnego `simulate_buy(..., Option<u64>)`.

## Decyzja

Zrobic minimalny cleanup walidacyjny:

1. Dodac lokalny predicate `funding_transfer_can_feed_capture_index()`.
2. Pozwalac na warming rolling FSC capture-index tylko dla:
   - `full_chain_coverage = true`,
   - `NlnProgramStreams`, ktory ma osobny capture-only kontrakt testowy.
3. Nie traktowac `GrpcGlobalStreamFiltered` ani `FundingLanePumpFiltered` jako dowodu usable rolling state.
4. Zaktualizowac doctest `simulate_buy()` do aktualnej sygnatury API.

## Uzasadnienie

`stream_available=true` moze znaczyc health-ready transport, ale nie oznacza jeszcze, ze rolling FSC index ma uzywalne decision-time evidence. Dla filtered Pump lane brak capture-eligible transferu powinien materializowac `FSC_ROLLING_STATE_UNAVAILABLE`, a nie przechodzic do `FSC_INSUFFICIENT_KNOWN_SOURCES`.

Jednoczesnie nie wolno zablokowac istniejacego capture-only kontraktu dla `NlnProgramStreams`, gdzie filtered observations sa dopuszczone jako diagnostyczne FSC evidence po jawnym ustawieniu stream availability.

## Zmiany

- `ghost-launcher/src/tx_intelligence/funding_source.rs`
  - dodano `funding_transfer_can_feed_capture_index()`,
  - health-only filtered lanes zachowuja lane provenance, ale nie ustawiaja `saw_transfer`,
  - dodano regresje `pump_filtered_transfer_does_not_warm_capture_index_when_stream_is_health_ready`.
- `ghost-core/src/shadow_ledger/mod.rs`
  - doctest `simulate_buy()` uzywa `Some(1000)`.

## Non-goals

- Brak zmian Gatekeeper thresholds.
- Brak zmian BUY/REJECT semantics.
- Brak zmian JSONL schema.
- Brak live enablement.
- Brak zmian w alpha/XGBoost/31100 runtime.
- Brak refaktoru FSC v2 lub DecisionLogger.

## Walidacja

Do wykonania po patchu:

- `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test session_lifecycle_tests -- --nocapture`
- `cargo test -p ghost-core -- --nocapture`
- `git diff --check`
- finalna macierz Gatekeeper PR1-PR4 wskazana w planie.

## Ryzyka

- Zmiana jest na granicy FSC availability, dlatego najwieksze ryzyko dotyczy przypadkowego zablokowania capture-only evidence. Ograniczono je przez pozostawienie `NlnProgramStreams` jako capture-eligible i regresje istniejacego testu `capture_transfer_warms_index_when_stream_is_explicitly_available`.
- Health-only filtered Pump lane nadal moze byc widoczny w provenance/lane diagnostics, ale nie jest materializowany jako usable rolling FSC state.
