# RAPORT P3.7-L1R16 / R16-r13 Executable Route Resolver Smoke

Data: 2026-05-23

## Werdykt

R16-r13: PASS-B / correct fail-closed.

L1R16 route resolver dziala diagnostycznie:

- primary route `routed_exact_sol_in` z missing BCV2 nie jest wybierana jako executable,
- fallback `legacy_buy` jest jawnie probowany,
- fallback nie jest cicho traktowany jako sukces, jesli nie ma kompletnego executable account set,
- brak executable route konczy sie `no_executable_route_account_set`,
- BCV2 nie dochodzi do post-simulation `AccountNotFound`,
- failure rows nie sa lifecycle-eligible.

Execution unlock: NO.

L2 / collection / Phase B / P2 / live / threshold tuning: HOLD / NO-GO.

## Run

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver.toml`

Namespace:

`shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver`

Run zostal uruchomiony w `tmux`, a nastepnie zatrzymany po uzyskaniu wystarczajacych dowodow diagnostycznych.

## Raporty

Wygenerowane raporty robocze:

- `/tmp/r16-r13-shadow-report.json`
- `/tmp/r16-r13-full-replay.json`
- `/tmp/r16-r13-join-key-audit.json`
- `/tmp/r16-r13-join-key-audit.md`
- `/tmp/r16-r13-l1-reject-diagnostics.json`

## Kluczowe liczby

Strict replay:

- status: `full_replay_ok`
- total_rows: 67
- v3_rows: 67
- bad_rows: 0

L1 reject diagnostics:

- diagnostic_quality.status: `PASS`
- gatekeeper_first_or_terminal_gate_coverage_pct: 100.0
- pdd_drift_evaluated_rows: 34
- pdd_drift_anchor_coverage_pct_among_evaluated: 100.0
- spike_ratio_quality_coverage_pct: 100.0
- whale_single_max_pct_coverage_pct: 100.0

Probe artifacts:

- probe_selection rows: 2
- probe_transport rows: 0
- probe_entry rows: 0
- probe_lifecycle rows: 0
- probe skip reason counts:
  - `creator_vault_source_not_authoritative`: 52
  - `probe_execution_precheck_failed`: 8
  - `verdict_type_not_in_sample_scope`: 5
  - `no_executable_route_account_set`: 2

Active shadow artifacts:

- active BUY/shadow transport rows: 3
- active shadow entry rows: 3
- active shadow lifecycle rows: 3
- active shadow successful entry rows: 0
- active shadow lifecycle eligible rows: 0
- active shadow runtime simulation error rows: 0
- active shadow precheck failed rows: 9

The audit counts active shadow failures across transport/entry/lifecycle artifacts, therefore 3 unique active shadow dispatches appear as 9 failure rows across the three artifact surfaces.

## Route Resolver Evidence

Active shadow route resolver counters:

- `active_shadow_route_resolution_status_counts`: `{"no_executable_route_account_set": 9}`
- `active_shadow_primary_route_bcv2_missing_rows`: 9
- `active_shadow_route_fallback_attempted_rows`: 9
- `active_shadow_route_fallback_success_rows`: 0
- `active_shadow_route_fallback_failed_rows`: 9
- `active_shadow_no_executable_route_account_set_rows`: 9
- `active_shadow_selected_route_kind_counts`: `{}`

Probe route resolver counters:

- `route_resolution_status_counts`: `{"no_executable_route_account_set": 2}`
- `primary_route_bcv2_missing_rows`: 2
- `route_fallback_attempted_rows`: 2
- `route_fallback_success_rows`: 0
- `route_fallback_failed_rows`: 2
- `no_executable_route_account_set_rows`: 2
- `selected_route_kind_counts`: `{}`

BCV2 diagnostics in active shadow:

- `active_shadow_observed_bcv2_rows`: 9
- `active_shadow_observed_bcv2_route_compatible_rows`: 9
- `active_shadow_bonding_curve_v2_source_counts`: `{"observed_tx_account_meta": 9}`
- `active_shadow_bonding_curve_v2_identity_authority_status_counts`: `{"authoritative_observed_tx": 9}`
- `active_shadow_bonding_curve_v2_rpc_load_status_counts`: `{"missing_on_rpc_precheck": 9}`
- `active_shadow_bonding_curve_v2_rpc_load_ready_counts`: `{"false": 9}`
- `active_shadow_builder_required_curve_account_ready_counts`: `{"false": 9}`
- `active_shadow_builder_required_curve_account_ready_reason_counts`: `{"bonding_curve_v2_observed_meta_missing_on_rpc": 9}`

BCV2 AccountNotFound:

- `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows`: 0
- `active_shadow_account_not_found_unattributed_rows`: 0
- post-simulation BCV2 AccountNotFound: 0

## Interpretation

R16-r13 potwierdza kontrakt L1R16:

1. Primary route `routed_exact_sol_in` nie jest executable, gdy BCV2 jest missing on RPC.
2. Fallback jest audytowalny, ale w tej probce nie jest executable.
3. Runtime nie symuluje nieegzekwowalnej route i nie produkuje slepego AccountNotFound.
4. Brak successful entries oznacza, ze route universe nadal nie daje lifecycle labels.

To jest diagnostyczny sukces fail-closed, ale nie execution unlock.

## Obecny Problem

Obecny blocker nie jest juz BCV2 attribution/provenance/readiness. To jest zamkniete.

Obecny blocker:

`no_executable_route_account_set`

Dla probki R16-r13 primary route odpada przez missing BCV2, a fallback `legacy_buy` nie ma kompletnego executable account set.

## Nastepny Krok

Nie L2 i nie progi.

Nastepny sensowny etap:

P3.7-L1R17 / J3S - Executable Route Universe Decision

Zakres:

- ustalic, czy `legacy_buy` fallback da sie zbudowac bez tego samego missing BCV2,
- albo jawnie oznaczyc `routed_exact_sol_in`/BCV2 route class jako non-executable under current shadow simulation,
- albo ograniczyc R16/L2 tylko do rows z `route_resolution_status in ["primary_route_ready", "fallback_route_ready"]`.

Do czasu tej decyzji:

- collection HOLD,
- L2 ablation HOLD,
- Phase B / P2 / live NO-GO.
