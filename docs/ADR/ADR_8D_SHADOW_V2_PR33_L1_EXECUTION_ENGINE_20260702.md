# ADR-8D: Shadow V2 PR33 L1 Deterministic Execution Engine

Data: 2026-07-02

Status:

```text
ACCEPTED_FOR_IMPLEMENTATION_PR
```

## D1. Problem

Po serii prac nad Shadow V2 logging harness system potrafil zapisywac
canonical evidence, manifesty i derived replay/lifecycle, ale nadal nie mial
wlasciwego jadra symulacji egzekucji.

Poprzednie audyty pokazaly, ze rozwijalismy glownie narzedzia pomiarowe, a nie
silnik, ktory odpowiada na pytania:

- czy simulated entry dostalby fill;
- po jakiej cenie;
- czy min_out zostalby spelniony;
- jaki byl deterministic own impact;
- jaki bylby deterministic exit fill;
- czy terminal executable PnL mozna policzyc bez lookaheadu.

## D2. Decyzja

Dodajemy side-by-side L1 deterministic execution engine:

```text
ghost-brain/src/guardian/post_buy/shadow_v2_execution.rs
```

Engine jest decision-inert i opakowuje canonical formula source:

```text
ghost-core/src/shadow_v2_price.rs
quote_constant_product()
```

`shadow_v2.rs` pozostaje schema/canonical surface, a konstruktory
`ShadowEntryFillV2` i `ShadowExitFillV2` deleguja deterministic fill logic do
`ShadowV2FillEngine`.

## D3. Evidence

PR33 dodaje:

- `ShadowV2FillEngine`;
- `ShadowV2ExecutionInput`;
- `ShadowV2ExecutionOutcome`;
- `ShadowV2ExecutionLabelGrade`;
- typed no-fill reasons;
- typed blocked reasons;
- deterministic derived after-state;
- addytywne pola serde dla entry/exit fill schema.

Kluczowe kontrakty:

```text
execution_simulation_ready != research_provenance_ready
DIAGNOSTIC_SIM != RESEARCH_CANDIDATE != LIVE_CONFIRMED
NO_FILL != BLOCKED_BY_DATA
```

Brak `account_data_hash` jest provenance blockerem, ale nie musi blokowac
diagnostic deterministic fill, jezeli wszystkie dane do formuly sa dostepne.

## D4. Semantyka L1

W L1:

```text
slippage_tolerance_bps = configured tolerance
deterministic_price_impact_bps = formula-derived impact
realized_slippage_bps = null
quote_fill_divergence_bps = null
```

`quote_fill_divergence_bps = null` oznacza brak realnego porownania quote-vs-
fill. Nie wpisujemy `0`, bo `0` oznaczaloby zmierzone zero divergence.

`NO_FILL` jest dozwolone tylko, gdy engine ma komplet danych i formula mowi,
ze fill nie przejdzie, np. `NO_FILL_MIN_OUT_NOT_MET`.

Brak danych zawsze pozostaje:

```text
BLOCKED_BY_DATA
```

## D5. Ograniczenia

PR33 nie dowodzi runtime source dla:

- `ENTRY_BEFORE`;
- pelnego `EXIT_BEFORE`;
- live landing;
- failed/no-fill tx telemetry;
- realized slippage;
- quote/fill divergence;
- live-confirmed calibration.

PR33 nie przyznaje:

```text
research_grade = true
live_equivalence = true
runtime_approval = true
shadow_close_only_approval = true
active_close_approval = true
strategy_research_unblocked = true
```

## D6. Runtime boundary

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- R51.

Nie uruchomiono runtime burnina.

## D7. Rejected alternatives

Odrzucono kopiowanie formul z `ghost-core/src/shadow_v2_price.rs` do nowego
modulu, bo groziloby to rozjazdem reconstruction logic.

Odrzucono dalsze trzymanie fill formulas jako rozproszonych helperow w
`shadow_v2.rs`.

Odrzucono traktowanie braku `account_data_hash` jako automatycznego blockera
samego deterministic diagnostic fill.

Odrzucono udawanie realized live slippage lub quote/fill divergence w L1.

## D8. Konsekwencje

Po PR33 mozna implementowac PR34 tylko pod warunkiem, ze najpierw zostanie
rozstrzygniete realne zrodlo `ENTRY_BEFORE`.

Jesli runtime nie ma deterministycznego source dla entry boundary, PR34 musi
zakonczyc sie:

```text
BLOCKED_ENTRY_BOUNDARY_SOURCE_MISSING
```

PR36 PASS bedzie mozliwy dopiero, gdy istnieje co najmniej jeden pelny
executable roundtrip per ten sam `position_id`:

```text
entry_fill = FILLED
exit_fill = FILLED
terminal final_pnl_executable_bps != null
```
