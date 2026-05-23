# RAPORT P3.7-L1R16 / J3R — Executable Route Resolver

Status: CODE-LEVEL IMPLEMENTED, RUNTIME SMOKE PENDING

Data: 2026-05-23

## Problem

R16-r12 domknął diagnostykę BCV2 jako PASS-B: observed route account meta może
być źródłem identity, ale nie oznacza execution readiness. Dla badanej klasy
rows `bonding_curve_v2` był route-compatible, ale `missing_on_rpc_precheck`, więc
system poprawnie fail-closował przed `simulate_buy` i nie generował już
post-simulation `AccountNotFound`.

Pozostały problem nie jest już w provenance, prechecku, AccountNotFound, PDD ani
progach. Problem brzmi: wybrana route nie ma executable simulation-load account
set.

## Zmiany

L1R16 dodaje jawny, deterministyczny resolver route dla probe i active shadow:

- `primary_route_ready` wybiera route pierwotną;
- brak readiness na route pierwotnej ocenia fallback candidate;
- brak executable fallbacku kończy się pre-simulation
  `no_executable_route_account_set`;
- failure rows pozostają `not_lifecycle_eligible`;
- fallback nie może zajść bez pól audytowych.

Nowe pola emitowane addytywnie:

```text
route_resolution_status
selected_route_kind
selected_route_reason
primary_route_kind
primary_route_ready
primary_route_not_ready_reason
fallback_route_kind
fallback_route_attempted
fallback_route_ready
fallback_route_not_ready_reason
no_executable_route_account_set_reason
```

Te pola są propagowane przez probe selection/skip/transport/entry oraz active
shadow diagnostics/entry rows.

## Semantyka BCV2

Jeżeli primary `routed_exact_sol_in` wymaga `bonding_curve_v2`, a to konto nie
jest RPC-load-ready, resolver zwraca:

```text
route_resolution_status = no_executable_route_account_set
primary_route_ready = false
primary_route_not_ready_reason = bonding_curve_v2_observed_meta_missing_on_rpc
fallback_route_kind = legacy_buy
fallback_route_attempted = true
fallback_route_ready = false
no_executable_route_account_set_reason =
  primary_route_bcv2_missing:bonding_curve_v2:<pubkey>
```

W tej sytuacji probe skip używa:

```text
probe_skip_reason = no_executable_route_account_set
```

Active shadow używa:

```text
active_shadow_precheck_status = precheck_failed
simulation_error_category = active_shadow_precheck_failed
active_shadow_lifecycle_eligibility_status = not_lifecycle_eligible
```

## Ważne ograniczenie

Obecny `DirectBuyBuilder` buduje `LegacyBuy` z aktualną rozszerzoną listą kont
Pump.fun, czyli nadal z `bonding_curve_v2`. L1R16 nie udaje, że taki fallback
jest executable. Resolver może policzyć fallback candidate, ale jeśli wymaga tego
samego missing simulation-load account, klasyfikuje go jako not ready.

To oznacza, że pierwszy poprawny runtime wynik może być PASS-B:

```text
fallback_route_attempted_rows > 0
fallback_route_success_rows = 0
no_executable_route_account_set_rows > 0
AccountNotFound after simulation = 0
```

To jest sukces diagnostyczny, nie execution unlock.

## Audyt

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` raportuje teraz:

```text
route_resolution_status_counts
selected_route_kind_counts
primary_route_bcv2_missing_rows
route_fallback_attempted_rows
route_fallback_success_rows
route_fallback_failed_rows
no_executable_route_account_set_rows

active_shadow_route_resolution_status_counts
active_shadow_selected_route_kind_counts
active_shadow_primary_route_bcv2_missing_rows
active_shadow_route_fallback_attempted_rows
active_shadow_route_fallback_success_rows
active_shadow_route_fallback_failed_rows
active_shadow_no_executable_route_account_set_rows
```

## Testy Code-Level

Uruchomione:

```text
cargo test -p ghost-launcher --lib p37_route_resolver -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Wynik:

```text
Rust targeted route resolver tests: PASS
Python join-key/audit tests: PASS, 21 tests
```

## Następny Gate

Następny krok to mały smoke:

```text
R16-r13 executable-route-resolver smoke
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver.toml
```

Acceptance:

- strict replay `full_replay_ok`;
- diagnostic quality PASS;
- identity/hash contract PASS;
- post-simulation BCV2 `AccountNotFound = 0`;
- route resolver fields populated;
- fallback attempts explicit;
- successful entry if fallback is executable, otherwise explicit
  `no_executable_route_account_set`;
- no live/P2/Phase B/threshold/IWIM changes.

L2 ablation, collection, Phase B, P2/live and threshold tuning remain HOLD until
route execution feasibility is either unlocked or explicitly scoped to an
executable route universe.
