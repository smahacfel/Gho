# ADR-8D: Shadow V2 Exit L1 Diagnostic Burnin PR38-B

Data: 2026-07-03

Status:

```text
ACCEPTED_AS_DIAGNOSTIC_L1_EXIT_EVIDENCE_ROUNDTRIP_BLOCKED
```

## D1. Problem

Po PR38-B trzeba było sprawdzić, czy runtime potrafi w realnym shadow flow użyć `EXIT_BEFORE` pool-state sample z lifecycle evidence path i zasilić nim PR33 `ShadowV2FillEngine` w trybie SELL.

Wcześniejszy PR34-C potwierdził diagnostic L1 BUY entry fill. Nadal brakowało dowodu, że exit-side L1 engine produkuje `shadow_exit_fill_v2` z `fill_status=FILLED`, oraz brakowało dowodu complete executable roundtrip PnL.

## D2. Decyzja

Wykonano jeden operator-approved validation/fidelity burnin:

```text
run_id = shadow-burnin-v2-exit-l1-diagnostic-pr38b-r1
scope = reports/selector/shadow-v2-exit-l1-diagnostic-pr38b-r1
main_head = 1f3db1022c901d20ed145049d1623559a41c8de4
PR39_merge_commit = 1f3db1022c901d20ed145049d1623559a41c8de4
shutdown = SIGINT_CLEAN
```

Finalny verdict:

```text
PR38_EXIT_L1_DIAGNOSTIC_BURNIN_PASS_ROUNDTRIP_BLOCKED
```

Decyzja jest wąska: zaakceptowano diagnostic L1 SELL exit fill evidence. Nie zaakceptowano complete executable roundtrip proof, bo terminal truth nie ma `final_pnl_executable_bps`.

## D3. Evidence

Runtime/shutdown:

- pre-run manifest: PASS
- pre-run strict audit: PASS
- validation burnin plan audit: PASS / `FIDELITY_ONLY`
- legacy downgrade audit: PASS
- runtime post-run manifest: PASS
- post-run strict audit: PASS
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`
- `PostBuyRuntime shut down successfully`
- `All components shut down successfully`
- `Ghost Launcher shutdown complete`
- SIGTERM/SIGKILL: false
- forced component abort: false

Canonical evidence:

| Metric | Value |
|---|---:|
| accepted shadow handoffs | 129 |
| entry fills | 129 |
| exit attempts | 129 |
| exit pool state before refs | 129 |
| exit token amount raw engine precondition count | 129 |
| exit fills | 129 |
| exit fill `BLOCKED_BY_DATA` | 0 |
| exit `execution_simulation_ready=true` | 129 |
| exit `execution_label_grade=DIAGNOSTIC_SIM` | 129 |
| exit `execution_label_grade=RESEARCH_CANDIDATE` | 0 |
| exit `execution_label_grade=LIVE_CONFIRMED` | 0 |
| terminal truth rows | 129 |
| terminal truth with executable PnL | 0 |
| complete executable roundtrip positions | 0 |

Offline cross-checks:

| Audit | Verdict |
|---|---|
| entry reconstruction readiness | `PASS_ENTRY_RECONSTRUCTION_READY` |
| exit reconstruction readiness | `PASS_EXIT_RECONSTRUCTION_READY` |
| replay/lifecycle reconciliation | `PASS_REPLAY_LIFECYCLE_RECONCILED` |
| manifest/retention | `PASS_MANIFEST_RETENTION_AUDIT` |
| temporal/no-lookahead | `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS` |
| path density horizon | `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS` |

## D4. Limitations

1. `research_provenance_ready=true` wystąpiło 0 razy dla exit fills.
2. `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME` wystąpiło 129 razy dla exit fill provenance.
3. Chain-order ma explicit `UNKNOWN` components, więc temporal audit pozostaje `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`.
4. `BLOCKED_ORDERING_AMBIGUITY` jest obecny jako provenance/limitation label dla exit fills, ale w tym burninie nie spowodował `fill_status=BLOCKED_BY_DATA`.
5. `exit_token_amount_raw` jest wymaganym precondition sell engine i był spełniony dla 129 fills, ale raw input amount nie jest serializowany jako osobne durable field w `shadow_exit_fill_v2` (`exit_token_amount_raw_persisted_field_count = 0`).
6. Terminal executable PnL nie został zapisany: `terminal_truth_with_final_pnl_executable_bps_count = 0`.
7. Density remains `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS`.
8. Brak live-confirmed exit fills, realized slippage, quote/fill divergence, failed/no-fill tx telemetry i live calibration.

## D5. Rejected alternatives

Odrzucono:

- traktowanie diagnostic `FILLED` jako live sell fill;
- traktowanie `entry_FILLED_exit_FILLED_same_position_count` jako complete executable roundtrip proof bez terminal executable PnL;
- wyliczanie executable PnL z mark price albo terminal mark PnL;
- traktowanie braku `account_data_hash` i explicit UNKNOWN chain-order jako research-grade provenance;
- nadawanie runtime approval, shadow_close_only approval albo active close approval;
- commitowanie raw JSONL, logów, runtime scope albo lokalnych TOML.

## D6. Runtime boundary

Nie zmieniono i nie wolno wywodzić z tego ADR zmiany:

```text
BUY/REJECT = unchanged
Gatekeeper policy = unchanged
selector runtime = unchanged
TX/Jito/live path = unchanged
shadow_close_only = disabled / not approved
active close = disabled / not approved
R51 = untouched
```

## D7. Consequences

1. PR38-B ma runtime evidence, że `EXIT_BEFORE` lifecycle pool sample może zasilić L1 sell engine i wygenerować diagnostic `shadow_exit_fill_v2 FILLED`.
2. Exit-side L1 diagnostic simulation jest gotowa do dalszego validation tylko jako `DIAGNOSTIC_SIM`.
3. Complete executable roundtrip proof pozostaje zablokowany, dopóki `shadow_terminal_truth_v2.final_pnl_executable_bps` nie będzie wiązany z tym samym `position_id` i parą `entry_fill=FILLED` + `exit_fill=FILLED`.
4. Research-grade pozostaje zablokowany przez provenance/ordering/density limitations.
5. Następny etap powinien być wąski: terminal executable PnL wiring/audit albo dedicated executable roundtrip audit, bez edge/research-grade interpretation.

## D8. Final flags

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
edge_proven = false
```
