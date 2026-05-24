# P3.7-E4S Legacy Buy Account-Set Separation Smoke

Data: 2026-05-24
HEAD: `76e33cf`
Config lokalny: `configs/rollout/shadow-burnin-v3-p37-e4s-legacy-buy-account-set-separation-smoke.toml`
Namespace: `shadow-burnin-v3-p37-e4s-legacy-buy-account-set-separation-smoke`
Runtime: `timeout 20m`, shadow-only, promotion/P2/live off

## Werdykt

`E4S` nie odblokował execution.

Wynik nie jest `PASS-A`, bo nie ma successful entry. Nie jest też czysty `PASS-B`, bo `legacy_buy` jest raportowany jako route-ready dla 5/6 active BUY rows, ale active shadow nadal kończy się precheck failure z primary-route `bonding_curve_v2`.

Najkrótszy werdykt:

```text
strict replay: PASS
diagnostic_quality: PASS
identity/hash: PASS
IWIM overflow panic: 0
post-simulation AccountNotFound: 0
legacy_buy primary BCV2 leak in fallback required set: 0
legacy_buy fallback route ready: 5 / 6
successful entries: 0
lifecycle eligible rows: 0
overall: FAIL / route resolver to execution handoff gap
```

E4 naprawił istotną część account-set separation: `bonding_curve_v2` nie występuje już w `fallback_required_precheck_account_set`, a `user_ata`, `user_volume_accumulator` i ephemeral payer nie są już blocking roles w `legacy_buy_missing_roles`. Nadal jednak runtime zapisuje precheck failure:

```text
no_executable_route_account_set:primary_route_bcv2_missing:bonding_curve_v2:<pubkey>
```

również dla rows, gdzie:

```text
route_resolution_status = fallback_route_ready
selected_route_kind = legacy_buy
legacy_buy_route_ready = true
execution_feasibility_status = executable
```

To oznacza, że resolver potrafi wybrać fallback, ale ścieżka dispatch/precheck/simulation nadal nie konsumuje wybranego fallback route jako terminalnego account setu.

## Run

Komenda:

```bash
timeout 20m env RUST_LOG=info cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-e4s-legacy-buy-account-set-separation-smoke.toml
```

Proces zakończył się kodem `124`, czyli przez oczekiwany limit `timeout 20m`. Po runie nie został aktywny proces `ghost-launcher`.

Artefakty:

```text
buys.jsonl                         6 rows
shadow_entries.jsonl               6 rows
shadow_lifecycle.jsonl             6 rows
probe_selection.jsonl              31 rows
probe_skips.jsonl                  381 rows
seer_runtime_coverage_audit.jsonl  380 rows
decision rows                      381 rows
```

## Replay / Diagnostics

`v3_full_replay_report.py --strict --json`:

```text
replay_status = full_replay_ok
total_rows = 381
v3_rows = 381
bad_rows = 0
```

`v3_shadow_report.py --json`:

```text
status = ok
artifact_freshness.stale_against_config = false
raw_rows = 381
deduped_rows = 381
v3_rows = 381
bad_rows = 0
execution.success_count = 0
execution.outcomes.shadow_unknown_error = 6
execution.outcomes.missing = 375
```

`v3_p37_l1_reject_diagnostics.py --json`:

```text
diagnostic_quality.status = PASS
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
active_shadow_account_not_found_rows = 0
active_shadow_dispatch_failure_rows = 12
active_shadow_precheck_status_counts.precheck_failed = 12
active_shadow_lifecycle_eligible_failure_rows = 0
shadow_payer_strategy = ephemeral
shadow_payer_account_status = ephemeral_not_rpc_required
```

## Active Path

Active BUY rows: `6`.

Route resolver:

```text
route_resolution_status:
  fallback_route_ready = 5
  no_executable_route_account_set = 1

selected_route_kind:
  legacy_buy = 5
  None = 1

selected_route_reason:
  fallback_route_passed_simulation_load_readiness = 5
  no_route_candidate_passed_simulation_load_readiness = 1

fallback_route_ready:
  true = 5
  false = 1

legacy_buy_account_set_status:
  ready = 5
  not_ready = 1

legacy_buy_route_ready:
  true = 5
  false = 1
```

Legacy buy curve:

```text
legacy_buy_curve_authority_readiness_status:
  authoritative_and_load_ready = 6

legacy_buy_curve_rpc_load_status:
  present_on_rpc_precheck = 6

legacy_buy_curve_rpc_load_ready:
  true = 6
```

This confirms the E3 convergence remained healthy.

## Account-Set Separation Counters

Counters from active `shadow_entries.jsonl`:

```text
legacy_buy_primary_bcv2_leak_rows = 0

legacy_buy_missing_creatable_user_ata_rows = 1 raw fallback_missing surface, 0 blocking legacy_missing surface
legacy_buy_missing_creatable_user_volume_accumulator_rows = 1 raw fallback_missing surface, 0 blocking legacy_missing surface
legacy_buy_missing_ephemeral_payer_rows = 1 raw fallback_missing surface, 0 blocking legacy_missing surface

legacy_buy_non_blocking_missing_creatable_rows = 6
legacy_buy_non_blocking_ephemeral_payer_rows = 6

legacy_buy_blocking_missing_required_rows = 1
legacy_buy_blocking_missing_required_role_counts:
  creator_vault = 1

legacy_buy_fallback_account_set_ready_rows = 5
legacy_buy_route_ready_after_account_set_separation_rows = 5
legacy_buy_route_ready_rows = 5
fallback_route_success_rows = 5 route-resolver success rows
```

Important nuance: the single non-ready fallback row still has raw `fallback_missing_roles`:

```text
creator_vault
payer_pubkey
user_ata
user_volume_accumulator
```

but `legacy_buy_missing_roles` narrows that to:

```text
creator_vault
```

So E4 did remove creatable/ephemeral roles from the blocking legacy surface. The remaining real route blocker for the one not-ready row is `creator_vault`.

## Execution / Lifecycle

Execution result:

```text
successful_entry_rows = 0
active_shadow_successful_entry_rows = 0
successful_probe_entry_rows = 0
lifecycle_eligible_rows = 0
post_simulation_account_not_found_rows = 0
iwim_overflow_panic_rows = 0
```

Contradictory handoff rows:

```text
route_ready_but_precheck_failed_rows = 5
fallback_ready_but_primary_bcv2_error_rows = 5
```

All 5 fallback-ready rows still end as:

```text
active_shadow_precheck_status = precheck_failed
execution_outcome = shadow_unknown_error
precheck_failure_reason = no_executable_route_account_set:primary_route_bcv2_missing:bonding_curve_v2:<pubkey>
```

This is the key failure. The fallback route is ready by resolver diagnostics, but the execution/precheck failure remains tied to primary `routed_exact_sol_in` BCV2.

## Probe Path

Probe artifacts:

```text
probe_selection_rows = 31
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
```

Probe skips:

```text
creator_vault_source_not_authoritative = 258
verdict_type_not_in_sample_scope = 74
execution_account_not_ready = 30
probe_execution_precheck_failed = 18
no_executable_route_account_set = 1
```

Probe side still does not produce transport/entry. The dominant blocker is `creator_vault_source_not_authoritative`, not curve convergence.

## Runtime Health

Log scan:

```text
attempt to subtract with overflow = 0
thread panicked = 0
panicked at = 0
IWIM overflow = 0
```

S1 held for this smoke. No IWIM overflow panic returned.

## Decision

`E4S = FAIL / route resolver to execution handoff gap`.

What is closed:

```text
legacy_buy curve convergence
primary BCV2 leak in fallback required/precheck set
creatable user_ata/user_volume_accumulator as blocking legacy missing roles
ephemeral payer as blocking legacy missing role
IWIM overflow during this smoke
post-simulation AccountNotFound
```

What is not closed:

```text
selected fallback route is not actually consumed by active shadow precheck/execution
fallback-ready rows still fail as primary_route_bcv2_missing
no successful entries
no lifecycle eligible rows
```

Next valid step is a narrow E4R repair, not a new route target and not Gatekeeper work:

```text
P3.7-E4R -- selected fallback route execution handoff repair
```

E4R should ensure that when:

```text
route_resolution_status = fallback_route_ready
selected_route_kind = legacy_buy
legacy_buy_route_ready = true
```

the active shadow dispatch/precheck/simulation path uses the selected `legacy_buy` prepared request/account set and does not return a stale primary-route BCV2 failure.

No L2D2, threshold tuning, Phase B/P2/live, V3 selector, or snapshot work is justified from this run.
