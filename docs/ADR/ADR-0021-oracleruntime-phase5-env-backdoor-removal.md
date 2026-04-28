# ADR-0021: OracleRuntime Phase-5 env backdoor removal

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Po implementacji Fazy 5 aktywna ścieżka produkcyjna w `ghost-launcher/src/main.rs` już poprawnie tworzyła `OracleRuntime` z konfiguracji top-level `[shadow_ledger]` przez `OracleRuntimeConfig::from_shadow_ledger_config(...)`.

Jednocześnie w `ghost-launcher/src/oracle_runtime.rs` pozostawał residual backdoor:

- `OracleRuntimeConfig::from_env()` dalej czytał `GHOST_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS`,
- legacy konstruktory `OracleRuntime::new`, `new_with_rpc`, `new_with_paradox` delegowały do tego env-based pathu,
- oznaczało to, że część testów, przykładów i potencjalnych pobocznych callerów mogła ominąć launcherowy SSOT Fazy 5.

To nie psuło głównej ścieżki runtime, ale naruszało cel Fazy 5 w sensie architektonicznym: polityka freshness nie powinna być sterowana rozproszonymi env override'ami.

## Decision

Usunięto env-based Phase-5 initialization z `OracleRuntime`:

1. skasowano `OracleRuntimeConfig::from_env()` oraz pomocnicze env parse helpers używane wyłącznie do tego celu,
2. legacy konstruktory `OracleRuntime::new`, `new_with_rpc`, `new_with_paradox` zostały przepięte na `OracleRuntimeConfig::default()`,
3. jedynym jawnie wspieranym path dla Phase-5 policy wiring pozostał `OracleRuntime::new_with_config(...)`,
4. launcherowy path w `main.rs` pozostaje kanoniczny i wstrzykuje `OracleRuntimeConfig::from_shadow_ledger_config(&config.shadow_ledger)`.

## Architectural Impact

Zmiana domyka Phase-5 SSOT nie tylko na głównej ścieżce produkcyjnej, ale także na powierzchni inicjalizacji `OracleRuntime`.

Po tej decyzji:

- `[shadow_ledger]` pozostaje jedynym źródłem policy dla aktywnego runtime,
- legacy konstruktory nie mogą już zmienić freshness policy przez env,
- testy i przykłady używające `OracleRuntime::new*()` dostają deterministyczny default config zamiast ukrytego wpływu środowiska.

## Risk Assessment

**Rate:** Low

Ryzyka:

1. część starych testów lub przykładów mogła niejawnie polegać na env override dla freshness,
2. użytkownik lokalnie uruchamiający examples z env mógł oczekiwać starego zachowania.

To ryzyko jest akceptowalne, bo wcześniejsze zachowanie było architektonicznie błędne względem SSOT Fazy 5.

## Consequences

### Positive

- pełniejsze domknięcie Phase-5 policy modelu,
- brak residual env backdoor dla `OracleRuntime`,
- bardziej deterministyczne testy/examples.

### Negative

- examples/tests używające legacy konstruktorów nie mogą już lokalnie stroić enrichment freshness env var bez przejścia na `new_with_config(...)`.

## Alternatives Considered

### 1. Zostawić `from_env()` i tylko nie używać go w `main.rs`

Odrzucone, bo residual backdoor dalej istniałby na publicznej powierzchni API.

### 2. Zostawić `from_env()` jako deprecated helper

Odrzucone, bo nadal utrzymywałoby drugi, błędny model konfiguracji dla tej samej odpowiedzialności.

### 3. Przepiąć legacy konstruktory na `default()` i usunąć env path

Przyjęte, bo to najmniejsza zmiana, która realnie zamyka problem bez ruszania kanonicznego launcher wiring.

## Validation Steps

1. Potwierdzić, że `ghost-launcher/src/main.rs` nadal używa `OracleRuntimeConfig::from_shadow_ledger_config(&config.shadow_ledger)`.
2. Potwierdzić, że `OracleRuntime::new`, `new_with_rpc`, `new_with_paradox` używają `OracleRuntimeConfig::default()`.
3. Potwierdzić, że w `ghost-launcher/src/oracle_runtime.rs` nie istnieje już `OracleRuntimeConfig::from_env()`.
4. Uruchomić test regresyjny sprawdzający, że env override nie wpływa już na legacy konstruktor runtime.