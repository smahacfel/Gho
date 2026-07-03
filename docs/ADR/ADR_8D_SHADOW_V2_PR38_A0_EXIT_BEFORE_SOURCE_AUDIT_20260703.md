# ADR-8D: Shadow V2 PR38-A0 EXIT_BEFORE Source Audit

Data: 2026-07-03

## Status

Accepted as read-only audit candidate.

Final verdict:

```text
EXIT_BOUNDARY_SOURCE_PRESENT
```

## D1. Problem

Po PR33 + PR34-A/B/C + PR37 entry-side Shadow V2 potrafi generowac diagnostic
L1 BUY entry fill z upstream `ENTRY_BEFORE` boundary payload.

Exit side nadal nie ma udowodnionego executable sell fill. Bez exit fill nie ma
pelnego executable roundtrip PnL.

PR38-A0 ma odpowiedziec na waskie pytanie:

```text
Czy obecny runtime ma deterministyczne, no-lookahead zrodlo EXIT_BEFORE,
ktore moze zasilic ShadowV2FillEngine w trybie SELL?
```

## D2. Decyzja

Akceptujemy jako primary source:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::shadow_v2_exit_pool_state_sample_from_lifecycle()
```

Ta funkcja buduje `PoolStateSampleV2` z `CanonicalPoolState` dostepnego przez
`MonitoringEngine::current_canonical_state(base_mint)`. Jest wywolywana w
exit/lifecycle evidence path przed emisja `ShadowExitFillV2`.

Akceptacja dotyczy tylko diagnostic L1 simulation. Brak `account_data_hash` i
niepelny chain-order degraduja wynik do `DIAGNOSTIC_SIM` i blokuja
research-grade provenance.

## D3. Evidence

Kodowa sciezka:

- `append_shadow_v2_lifecycle_record()` emituje exit-side V2 evidence;
- `shadow_v2_exit_pool_state_sample_from_lifecycle()` tworzy
  `pool_state_sample_v2`;
- `shadow_v2_exit_fill_from_lifecycle()` aktualnie linkuje sample, ale nadal
  wywoluje `blocked_with_pool_state(...)`;
- `ShadowExitFillV2::from_static_sell_model(...)` oraz
  `ShadowV2FillEngine::simulate(... Sell ...)` juz istnieja po PR33, lecz nie
  sa jeszcze podlaczone do lifecycle exit path.

Dowod raportowy z PR18E:

```text
shadow_exit_fill_v2 rows = 185
with pool_state_before = 185
exit fill BLOCKED_BY_DATA = 185
terminal final_pnl_executable_bps = 0
```

To potwierdza, ze exit pool-state-before link moze byc obecny, ale executable
sell fill nadal nie byl liczony.

## D4. Candidate Matrix

Macierz znajduje sie w:

```text
reports/selector/shadow_v2_pr38_a0_exit_before_candidate_matrix.csv
```

Wynik:

| Verdict | Count |
|---|---:|
| `PRESENT` | 4 |
| `MISSING` | 3 |
| `AMBIGUOUS` | 2 |
| `REJECTED` | 5 |

Primary `PRESENT`:

```text
exit_account_state_core_lifecycle_boundary
```

## D5. Accepted Boundaries

PR38-B moze uzyc `PoolStateSampleV2` utworzonego przez
`shadow_v2_exit_pool_state_sample_from_lifecycle()` jako `EXIT_BEFORE`, ale
musi zachowac:

- `TemporalClass::PostEntry` dla sample;
- `ClockDomain::StreamObservedMs`;
- explicit `UNKNOWN` dla brakujacych chain-order components;
- `account_data_hash = None`;
- `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME` jako provenance blocker;
- `research_provenance_ready=false`, jezeli `PoolStateSampleV2::research_blockers()`
  nie jest puste.

## D6. Rejected Alternatives

Odrzucamy jako `EXIT_BEFORE`:

- `TerminalTruthV2`;
- `final_pnl_mark_bps` albo terminal final PnL;
- derived replay/lifecycle jako canonical input;
- `ShadowExitPathReplay` selected point bez causal `pool_state_sample_v2`;
- `ShadowPathSampleV2::from_legacy_lifecycle_mark(...)` jako pool state source;
- ShadowLedger / replay-only synthetic output;
- dowolny late `PostBuyRuntime.account_state_core` read bez historycznego
  snapshotu zwiazanego z exit boundary;
- OracleRuntime price context jako exit boundary source;
- real live sell telemetry jako shadow diagnostic source.

## D7. Required PR38-B Contract

PR38-B ma byc minimalnym shadow-only implementation PR:

1. Uzyc `shadow_v2_exit_pool_state_sample_from_lifecycle()` jako source
   `EXIT_BEFORE`.
2. Gdy `pool_state_before` i `record.exit_token_amount_raw` istnieja, zbudowac
   `ShadowExitFillModelConfig::bonding_curve(...)`.
3. Wywolac `ShadowExitFillV2::from_static_sell_model(...)`.
4. Nie wpisywac fake `min_out_raw`, fake `account_data_hash`, fake realized
   slippage ani fake quote/fill divergence.
5. Zachowac causal blockers:

```text
EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS
EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY
EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY
```

6. Nie ustawic `final_pnl_executable_bps`, jezeli exit fill nie jest `FILLED`.
7. Nie konsumowac Shadow V2 przez BUY/REJECT, Gatekeeper, selector ani
   TX/Jito/live path.

## D8. Consequences

PR38-A0 nie nadaje:

```text
runtime_approval = true
shadow_close_only_approval = true
active_close_approval = true
research_grade = true
live_equivalence = true
strategy_research_unblocked = true
executable_roundtrip_proven = true
```

Po PR38-A0 mozna rozpoczac PR38-B tylko jako shadow-only diagnostic L1 exit
implementation. Validation burnin po PR38-B wymaga osobnej zgody operatora.

## D9. Runtime Boundary

PR38-A0 nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- R51;
- approval flags.

Nie uruchomiono burnina ani validation runu.
