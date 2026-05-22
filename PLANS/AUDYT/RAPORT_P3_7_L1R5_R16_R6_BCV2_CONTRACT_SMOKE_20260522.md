# RAPORT P3.7-L1R5 — R16-r6 BondingCurveV2 Contract Smoke

Data: 2026-05-22

Status:

```text
R16-r6 bcv2 contract smoke: DIAGNOSTIC PASS / PROBE EXECUTION BLOCKED
L1R5 bonding_curve_v2 precheck contract: RUNTIME VALIDATED
L2 / ablation: HOLD
collection: HOLD
Phase B: HOLD
P2/live/threshold tuning: NO-GO
```

## Cel

R16-r6 sprawdzał, czy po L1R5 `bonding_curve_v2` obecny w transaction account
metas jest traktowany jako simulation-load required przez precheck.

Poprawny wynik diagnostyczny:

```text
missing bonding_curve_v2 -> probe_skipped przed simulate_buy
nie -> counterfactual_shadow_probe_simulation_error / AccountNotFound
```

## Wyniki

Runtime zakończył się timeoutem procesu:

```text
EXIT:124
```

Artefakty wystarczyły do oceny kontraktu.

Replay:

```text
replay_status = full_replay_ok
v3_rows = 922
bad_rows = 0
```

R16 diagnostics:

```text
diagnostic_quality.status = PASS
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
brain_config_hash = b41923673eacd484bd2178c6c7eb6782c5d90a9755f12ad68f1e625a0b658388
v3_policy_config_hash = 55416d4c7ef23a0aaea0c5b3bb4da0abc6564ce7059049c49a8bf80b07170fdc
```

Probe plane:

```text
probe_selection_rows = 39
probe_selection exact decision/V3 join = 39/39
probe_transport_rows = 0
probe_entry_rows = 0
probe_lifecycle_rows = 0
probe_skips_rows = 922
```

L1R5 contract counters:

```text
account_not_found_rows = 0
account_not_found_unattributed_rows = 0
simulation_required_account_not_in_precheck_rows = 0
simulation_account_meta_missing_on_rpc_rows = 0
bonding_curve_v2_account_not_found_after_simulation_rows = 0
bonding_curve_v2_precheck_skipped_before_simulation_rows = 37
```

Skip distribution:

```text
creator_vault_source_not_authoritative = 529
probe_execution_precheck_failed = 201
verdict_type_not_in_sample_scope = 153
execution_account_not_ready = 39
```

Readiness role distribution:

```text
creator_vault = 531
bonding_curve_v2 = 37
```

Active shadow path:

```text
BUY verdict rows = 12
active_shadow_entries = 12
active_shadow_lifecycle = 12
dispatch_status = failed on all active shadow lifecycle rows
simulation_outcome = failed on all active shadow lifecycle rows
err = AccountNotFound on all active shadow lifecycle rows
good_or_dirty_good_label_rows = 0
```

## Interpretacja

L1R5 naprawił konkretną niespójność kontraktu:

```text
precheck mówił: bonding_curve_v2 optional
simulation tx mówił: bonding_curve_v2 jest account meta
RPC simulation kończyła: AccountNotFound
```

Po R16-r6 ta klasa nie przechodzi już do symulacji:

```text
bonding_curve_v2_account_not_found_after_simulation_rows = 0
bonding_curve_v2_precheck_skipped_before_simulation_rows = 37
```

To jest poprawny fail-closed outcome: pipeline przestaje udawać execution-ready,
gdy konto ładowane przez symulację nie jest dostępne.

## Co nadal blokuje

Probe execution jest nadal zablokowany, ale z inną, jawną przyczyną:

```text
creator_vault_source_not_authoritative = 529
missing_execution_route_identity = 201
bonding_curve_v2 precheck skip = 37
```

Active shadow BUY path nadal kończy się `AccountNotFound`, mimo że R16 raportuje
`shadow_payer_strategy = ephemeral` i `shadow_payer_account_status =
ephemeral_not_rpc_required`. Ten problem dotyczy active shadow dispatch path, a
nie kontraktu probe `bonding_curve_v2` naprawianego w L1R5.

## Decyzja

```text
P3.7-L1R5 runtime gate: PASS diagnostyczny dla bonding_curve_v2 contract
Probe transport/entry/lifecycle: BLOCKED
Active shadow BUY lifecycle labels: BLOCKED przez AccountNotFound
Next: active shadow dispatch AccountNotFound attribution/precheck contract
Alternative next for probe-only path: creator_vault authority / route identity repair
```

Nie ma podstaw do:

```text
L2 ablation
collection
Phase B
P2/live
threshold tuning
```
