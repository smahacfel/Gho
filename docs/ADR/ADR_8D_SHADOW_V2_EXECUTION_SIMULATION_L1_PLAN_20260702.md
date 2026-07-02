# ADR-8D: Shadow V2 Execution Simulation L1 Plan

Data: 2026-07-02

## Status

Accepted for operator review.

```text
ACCEPTANCE_READY_FOR_OPERATOR_REVIEW
```

## Context

Dotychczasowy Shadow V2 dowiozl przede wszystkim L0 logging harness:
canonical events, manifesty, replay/lifecycle, density rows, temporal audits
oraz shutdown/flush. To jest infrastruktura pomiarowa, ale nie pelny silnik
symulacji egzekucji.

Aktualny kod ma juz:

- inert formula library w `ghost-core/src/shadow_v2_price.rs`;
- struktury entry/exit fill w
  `ghost-brain/src/guardian/post_buy/shadow_v2.rs`;
- runtime emission w PostBuyRuntime i post-buy lifecycle;
- offline audits pokazujace, ze fill rows nadal czesto pozostaja
  `BLOCKED_BY_DATA`.

Operator zaakceptowal kierunek resetu: zamrozic L0 jako infrastrukture i
przeniesc priorytet na L1 deterministic execution simulation.

## Decision

Tworzymy formalny plan realizacji PR33-PR36:

```text
PLANS/DO_REALIZACJI/PLAN_SHADOW_V2_EXECUTION_SIMULATION_L1_PR33_PR36_20260702.md
```

Plan ustanawia:

- centralny `ShadowV2FillEngine`;
- `ghost-core/src/shadow_v2_price.rs` jako canonical formula source;
- rozdzielenie `execution_simulation_ready` od
  `research_provenance_ready`;
- `execution_label_grade = DIAGNOSTIC_SIM | RESEARCH_CANDIDATE | LIVE_CONFIRMED`;
- twarde rozroznienie `NO_FILL` od `BLOCKED_BY_DATA`;
- brak realized live slippage w L1;
- brak quote/fill divergence claim w L1;
- wymaganie pelnego executable roundtripu per ten sam `position_id` przed
  PASS dla L1 validation.

## Invariants

Plan nie przyznaje i nie moze przyznawac:

```text
runtime_approval=true
shadow_close_only_approval=true
active_close_approval=true
research_grade=true
live_equivalence=true
strategy_research_unblocked=true
```

Plan nie zmienia:

```text
BUY/REJECT
Gatekeeper policy
selector runtime
TX/Jito/live path
shadow_close_only
active close
R51
```

## Consequences

Po tym planie nastepny poprawny etap to PR33, ale dopiero po jawnej decyzji
operatora o implementacji.

PR33 nie moze zaczynac od kolejnego burnina ani kolejnego report-only PR.
Najpierw musi powstac L1 core execution engine.

Brak `account_data_hash` nie blokuje automatycznie diagnostic deterministic
fill, ale blokuje research provenance:

```text
execution_simulation_ready may be true
research_provenance_ready = false
execution_label_grade = DIAGNOSTIC_SIM
```

## Validation Boundary

PR36 PASS bedzie mozliwy dopiero, gdy wystapi:

```text
complete_executable_roundtrip_positions > 0
entry_fill_FILLED_count > 0
exit_fill_FILLED_count > 0
terminal_truth_with_final_pnl_executable_bps > 0
```

Roundtrip musi byc per ten sam `position_id`.

## Rejected Alternatives

Odrzucono dalsze rozwijanie samego harnessu/audytow jako glownego celu.

Odrzucono traktowanie `account_data_hash` jako warunku samego diagnostic fill,
bo aktualny runtime juz pokazal, ze taka zasada prowadzi do 100%
`BLOCKED_BY_DATA`.

Odrzucono wpisywanie `quote_fill_divergence_bps = 0` w L1, bo brak pomiaru nie
jest zmierzona zerowa dywergencja.

Odrzucono uzywanie `slippage_bps` jako realized live slippage w L1.
