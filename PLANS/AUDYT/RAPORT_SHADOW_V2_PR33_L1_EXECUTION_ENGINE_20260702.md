# Raport PR33: Shadow V2 L1 Deterministic Execution Engine

Data: 2026-07-02

Status:

```text
PR33_IMPLEMENTATION_READY_FOR_VALIDATION
```

## 1. Cel

PR33 realizuje pierwszy etap po resecie priorytetow Shadow V2:

```text
L0 recorder / manifest / harness != L1 execution simulation
```

Dotychczasowy Shadow V2 potrafil zapisywac canonical events, manifesty,
replay/lifecycle i density rows. PR33 dodaje side-by-side L1 deterministic
execution simulation engine, ktory ma policzyc deterministyczny fill z
dostepnego stanu poola, kwoty, fee, min_out i ordering boundary.

PR33 nie jest runtime burninem, nie jest strategy proof, nie nadaje
research-grade i nie nadaje live-equivalence.

## 2. Zakres wykonany

Dodano centralny modul:

```text
ghost-brain/src/guardian/post_buy/shadow_v2_execution.rs
```

Modul wprowadza:

- `ShadowV2FillEngine`;
- `ShadowV2ExecutionInput`;
- `ShadowV2ExecutionOutcome`;
- `ShadowV2ExecutionSide`;
- `ShadowV2BoundaryKind`;
- `ShadowV2ExecutionLabelGrade`;
- `ShadowV2NoFillReason`;
- `ShadowV2BlockedReason`;
- `ShadowV2DerivedPoolState`.

`ShadowV2FillEngine` uzywa jako canonical formula source:

```text
ghost-core/src/shadow_v2_price.rs
quote_constant_product()
SHADOW_V2_PRICE_FORMULA_VERSION
```

Nie skopiowano formul constant-product do nowego modulu.

## 3. Kontrakt L1

PR33 rozdziela dwa poziomy gotowosci:

```text
execution_simulation_ready
research_provenance_ready
```

Oraz wprowadza grade:

```text
execution_label_grade = DIAGNOSTIC_SIM | RESEARCH_CANDIDATE | LIVE_CONFIRMED
```

W PR33 realnie emitowane sa tylko:

```text
DIAGNOSTIC_SIM
RESEARCH_CANDIDATE
```

`LIVE_CONFIRMED` pozostaje zarezerwowane dla przyszlego L3 i wymaga
live-confirmed calibration dataset.

## 4. Account data hash nie blokuje diagnostic fill

Najwazniejsza zmiana kontraktowa:

```text
brak account_data_hash blokuje research provenance,
ale nie musi blokowac deterministic diagnostic fill
```

Jesli dostepne sa:

- reserves;
- token decimals;
- lamports normalization;
- input amount;
- fee bps;
- min_out / slippage tolerance;
- pool phase;
- causal event_order_key;

to engine moze wyemitowac:

```text
fill_status = FILLED
execution_simulation_ready = true
research_provenance_ready = false
execution_label_grade = DIAGNOSTIC_SIM
provenance_blockers = ["POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME", ...]
```

To jest diagnostic deterministic simulation, nie research-grade proof.

## 5. Slippage i quote/fill divergence

PR33 nie udaje live slippage.

W L1:

```text
slippage_tolerance_bps = configured tolerance
deterministic_price_impact_bps = deterministic formula impact
realized_slippage_bps = null
quote_fill_divergence_bps = null
```

`quote_fill_divergence_bps` nie jest ustawiane na `0`, bo w L1 nie istnieja
dwa niezalezne zrodla: real quote i real fill. `null` oznacza brak pomiaru,
nie zmierzone zero.

## 6. NO_FILL vs BLOCKED_BY_DATA

PR33 rozdziela:

```text
BLOCKED_BY_DATA = nie mozemy policzyc deterministycznego wyniku
NO_FILL = policzylismy formule i transakcja nie przeszlaby wedlug modelu
FILLED = policzylismy formule i transakcja przeszlaby wedlug modelu
```

Dla `NO_FILL_MIN_OUT_NOT_MET` engine zapisuje expected output i min_out, ale
nie zapisuje `fill_price`, bo fill nie zaszedl.

## 7. Integracja ze schema Shadow V2

`ShadowEntryFillV2` i `ShadowExitFillV2` dostaly addytywne pola z defaultami
serde, bez destrukcyjnej zmiany starszego JSON:

- `execution_simulation_ready`;
- `research_provenance_ready`;
- `execution_label_grade`;
- `provenance_ready`;
- `provenance_blockers`;
- `blocked_reasons`;
- `no_fill_reason`;
- `fail_reason`;
- `expected_output_raw`;
- `output_amount_raw`;
- `slippage_tolerance_bps`;
- `deterministic_price_impact_bps`;
- `realized_slippage_bps`;
- `quote_fill_divergence_bps`;
- `pool_state_after_source`;
- `execution_model_version`.

Existing `from_static_buy_model()` i `from_static_sell_model()` deleguja do
`ShadowV2FillEngine`, zamiast trzymac rozproszone helpery formuly w
`shadow_v2.rs`.

## 8. Granice zachowane

PR33 nie zmienia:

```text
BUY/REJECT
Gatekeeper policy
selector runtime
TX/Jito/live path
shadow_close_only
active close
runtime approval
research-grade approval
live-equivalence approval
R51
```

PR33 nie uruchamia runtime burnina i nie stage'uje raw JSONL/log/runtime
scope/local config.

## 9. Co PR33 realnie wypelnia

Przy kompletnych danych engine moze wyliczyc:

- `fill_status`;
- `fill_price`;
- `fill_price_source`;
- `fill_amount_sol`;
- `fill_amount_tokens`;
- `expected_output_raw`;
- `output_amount_raw`;
- `min_out_raw`;
- `fee_bps`;
- `own_impact_bps`;
- `deterministic_price_impact_bps`;
- deterministic derived `pool_state_after`;
- `execution_model_version`;
- `reconstruction_status`;
- `quality`;
- `limitations`.

## 10. Co nadal pozostaje niedostepne albo nieudowodnione

PR33 nie dowodzi jeszcze, ze runtime ma realny `ENTRY_BEFORE` source.

PR33 nie dowodzi jeszcze, ze runtime ma realny `EXIT_BEFORE` source dla
kazdego exit.

PR33 nie nadaje:

```text
realized_slippage_bps
quote_fill_divergence_bps
live landing telemetry
failed tx telemetry
live-confirmed fills
research-grade provenance
live-equivalence
```

## 11. Test summary

Wymagane intencje testowe PR33:

- buy fill moze byc `RESEARCH_CANDIDATE`, gdy provenance jest kompletne;
- buy fill moze byc `DIAGNOSTIC_SIM`, gdy brakuje `account_data_hash`;
- sell fill moze byc `DIAGNOSTIC_SIM`, gdy brakuje `account_data_hash`;
- missing pool state blokuje deterministic fill;
- min_out failure daje `NO_FILL`, bez fake `fill_price`;
- future/same-boundary pool state blokuje fill;
- L1 nie wypelnia `realized_slippage_bps`;
- L1 nie wypelnia `quote_fill_divergence_bps`;
- existing Shadow V2 test surface nadal przechodzi.

## 12. Review fixes

Po review PR33 poprawiono dwa kontraktowe ryzyka:

1. `research_provenance_ready` jest teraz oparte o pelny canonical
   `PoolStateSampleV2::research_blockers()`, a nie o reczny subset checkow.
   Blockery research provenance nie blokuja diagnostic deterministic fill, ale
   uniemozliwiaja `RESEARCH_CANDIDATE`.
2. `ShadowExitFillV2::modeled_failure()` nie oznacza juz modeled no-fill jako
   L1 execution-ready i nie przypisuje arbitralnego
   `NO_FILL_MIN_OUT_NOT_MET` bez formuly oraz bez `min_out` evidence.

Dodany test potwierdza, ze brak `observed_slot`, pusta signature, zerowy
observed wall time, `PoolStateSource::Unknown` i
`PoolStateSource::ShadowLedgerDiagnostic` nadal moga dac diagnostic fill, ale
nie moga dac research provenance.

## 13. Wniosek

PR33 jest gotowy jako implementation PR dla L1 core engine.

Nastepny poprawny krok po merge PR33:

```text
PR34: rozstrzygnac, czy runtime ma deterministyczne zrodlo ENTRY_BEFORE.
```

Jesli `ENTRY_BEFORE` nie istnieje, PR34 musi zakonczyc sie:

```text
BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
```

Nie wolno uruchamiac kolejnego validation burnina tylko po to, aby ponownie
potwierdzic brak entry boundary source.
