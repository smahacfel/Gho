# RAPORT P3.7-X9F — Total-Order Fix + Repeat X9-S

Data: 2026-05-27  
Namespace smoke: `shadow-burnin-v3-p37-x9f-total-order-repeat-smoke`  
Stan testowany:

```text
origin/main
+ a45fa8e fix: make grpc exact-watch ordering deterministic
+ uncommitted X9 terminal closure diff
```

Konfiguracja smoke: `/tmp/gho-x9f-smoke/shadow-burnin-v3-p37-x9f-total-order-repeat-smoke.toml`  
Preflight log: `/tmp/gho-x9f-smoke/x9f_preflight.log`  
Runtime console: `/tmp/gho-x9f-smoke/x9f_runtime_console.log`  
Audit JSON: `/tmp/gho-x9f-smoke/x9f_audit.json`  
Audit MD: `/tmp/gho-x9f-smoke/x9f_audit.md`  
Unique BCV2 JSON: `/tmp/gho-x9f-smoke/x9f_unique_bcv2.json`

## Werdykt

```text
X9F comparator fix: PASS
X9-S repeat transport stability: PASS
X9-S executable subset verdict: B — executable subset pusty
R18: NO-GO
P2/live/Sender: NO-GO
Gatekeeper/scoring/threshold tuning: NO-GO
legacy_buy/fallback: NO-GO
```

Decyzja operacyjna:

```text
current shadow/probe route universe has no executable subset
```

Po naprawie comparatora gRPC exact-watch transport nie powtórzył poprzedniego panic/stall failure mode. X9 terminal route closure działała, ale po wycięciu non-loadable BCV2 route universe nie pojawił się executable subset.

## X9F Comparator Fix

Commit:

```text
a45fa8e fix: make grpc exact-watch ordering deterministic
```

Zakres commita:

```text
off-chain/components/seer/src/grpc_connection.rs
```

Zmiana:

- `snapshot_exact_watch_accounts(...)` snapshotuje `(pubkey, touch_rank)` przed sortowaniem.
- `snapshot_set_by_recency(...)` snapshotuje `(pubkey, touch_rank)` przed sortowaniem.
- Comparator sortuje już tylko lokalne tuple: `rank desc`, potem `pubkey asc`.
- Comparator nie czyta `DashMap` / `last_touch` w trakcie `sort_by`.

Testy regresyjne:

- `account_registry_recency_sort_uses_total_order_for_ties`
- `primary_global_exact_snapshot_survives_concurrent_bcv2_retouch`

Walidacja przed commitem:

```text
cargo check -p seer
cargo test -p seer grpc_connection -- --nocapture
cargo fmt --check
git diff --check -- off-chain/components/seer/src/grpc_connection.rs
```

Wynik:

```text
PASS
```

`cargo test -p seer grpc_connection -- --nocapture`:

```text
81 passed; 0 failed
```

## Warunki Smoke

Preflight:

```text
[ok] execution_profile: execution_mode=Shadow, entry_mode=shadow_only
[ok] transport.grpc: source_mode=grpc endpoint=yellowstone-solana-mainnet.core.chainstack.com:443
[ok] trigger.balance: 0.047172000 SOL >= 0.007200000 SOL reserve+trade budget
[ok] preflight: all runtime checks passed
```

Konfiguracja:

```text
execution_mode = Shadow
entry_mode = shadow_only
p37_execution_builder_mode = working_builder_parity
bcv2_terminal_route_closure_enabled = true
no live Sender
no R18
no Gatekeeper/scoring changes
no legacy/fallback
```

Runtime status:

```text
timeout wrapper status = 124
```

To jest oczekiwane dla pełnego 30-min smoke zakończonego przez `timeout 1800s`.

## Transport Stability

Runtime console:

```text
log lines = 697784
transport_failure_markers = 0
```

Nie wystąpiły:

```text
user-provided comparison function does not correctly implement a total order = 0
Ghost/Pump transport ... all workers exited = 0
WATCHDOG FATAL = 0
WATCHDOG WARN = 0
```

Wniosek:

```text
Transport stability blocker z poprzedniego X9-S został usunięty dla tego 30-min smoke.
```

## BCV2 / Evidence / Exact-Watch

Audit:

```text
bcv2_exact_watch_registered_rows = 18901
bcv2_exact_watch_in_subscribe_request_rows = 10304
bcv2_resubscribe_sent_rows = 10217
bcv2_account_update_received_rows = 211
bcv2_rpc_hydration_ready_rows = 0
bcv2_rpc_hydration_missing_rows = 11546
```

Working-builder BCV2 join:

```text
working_builder_bcv2_unique_pubkeys = 233
working_builder_bcv2_registered_unique_pubkeys = 233
working_builder_bcv2_included_unique_pubkeys = 227
working_builder_bcv2_hydration_missing_unique_pubkeys = 233
working_builder_bcv2_hydration_ready_unique_pubkeys = 0
```

X8D-style unique BCV2 buckets:

```text
same_pubkey_update_but_not_execution_ready = 50
included_rpc_missing_no_same_update = 177
registered_not_included_rpc_missing = 6
dropped_over_cap = 6
```

Interpretacja:

- `RpcReady` nadal nie pojawił się dla working-builder BCV2.
- AccountUpdate remains diagnostic-only.
- Terminal closure była właściwym zachowaniem dla non-loadable BCV2 routes.
- Capacity dalej istnieje jako poboczny problem, ale nie zmienia decyzji X9-S, bo executable subset nadal jest pusty.

## Terminal Route Closure

Console markers:

```text
bcv2_not_persistent_or_not_loadable occurrences = 219
P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED rows = 233
```

Audit:

```text
bcv2_terminal_route_exclusion_rows = 214
bcv2_terminal_route_exclusion_unique_pubkeys = 214
execution_feasibility_reject_bcv2_not_persistent_rows = 214
buy_quality_denominator_excluded_bcv2_rows = 214
lifecycle_denominator_excluded_bcv2_rows = 214
```

Active-shadow prefixed audit:

```text
active_shadow_bcv2_terminal_route_exclusion_rows = 15
active_shadow_bcv2_terminal_route_exclusion_unique_pubkeys = 5
active_shadow_execution_feasibility_reject_bcv2_not_persistent_rows = 15
active_shadow_buy_quality_denominator_excluded_bcv2_rows = 15
active_shadow_lifecycle_denominator_excluded_bcv2_rows = 15
```

Execution feasibility:

```text
route_executable_rows = 0
route_non_executable_rows = 251
execution_feasibility_reject_rows = 251
probe_execution_feasibility_status_counts = {
  "not_executable_route": 233,
  "unknown": 113
}
active_shadow_execution_feasibility_status_counts = {
  "not_executable_route": 18
}
```

Execution feasibility reasons:

```text
active_shadow_execution_feasibility_reason_counts = {
  "bcv2_not_persistent_or_not_loadable": 15,
  "no_executable_route_account_set": 3
}
```

## Acceptance A/B

Acceptance A wymaga:

```text
route_executable_rows > 0
successful_probe_entry_rows > 0 albo active_shadow_successful_entry_rows > 0
lifecycle_eligible_rows > 0
```

Wynik:

```text
route_executable_rows = 0
successful_probe_entry_rows = 0
active_shadow_successful_entry_rows = 0
lifecycle_eligible_rows = 0
```

Acceptance A:

```text
FAIL
```

Acceptance B wymaga:

```text
bcv2_terminal_route_exclusion_rows > 0
route_executable_rows = 0
successful_probe_entry_rows = 0
lifecycle_eligible_rows = 0
```

Wynik:

```text
bcv2_terminal_route_exclusion_rows = 214
route_executable_rows = 0
successful_probe_entry_rows = 0
lifecycle_eligible_rows = 0
```

Acceptance B:

```text
PASS
```

Uwaga: audit pokazał też `shadow_entry_rows = 6` i `shadow_lifecycle_rows = 6`, ale nie spełniają one acceptance A, ponieważ:

```text
active_shadow_successful_entry_rows = 0
lifecycle_eligible_rows = 0
route_executable_rows = 0
```

## Safety Invariants

```text
legacy_buy_route_attempted_rows = 0
legacy_fallback_attempted_rows = 0
selected_route_handoff_mismatch_rows = 0
post_simulation_account_not_found_rows = 0
bonding_curve_v2_account_not_found_after_simulation_rows = 0
send_transaction markers = 0
LiveTxSender::send_transaction markers = 0
SUBMITTED markers = 0
```

Nie było live/Sender path, R18, Gatekeeper/scoring changes ani legacy/fallback revival.

## Decyzja Końcowa

```text
X9-S repeat = B
current shadow/probe route universe has no executable subset
```

Nie ma podstaw do dalszego ratowania BCV2 readiness ani do prób promowania `AccountUpdateReceived` jako execution-ready. Stabilny 30-min smoke po naprawie transportu potwierdził, że po terminalnym wycięciu non-loadable BCV2 nie zostaje executable subset w obecnym route universe.

Następny etap powinien wrócić do:

```text
X1 / historyczny Helius execution contract rebuild
```

To powinno być osobnym kontraktem wykonawczym, a nie dalszą naprawą BCV2.

