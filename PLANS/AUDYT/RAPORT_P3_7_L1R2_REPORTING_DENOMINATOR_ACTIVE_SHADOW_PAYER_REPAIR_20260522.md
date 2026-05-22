# RAPORT P3.7-L1R2 Reporting Denominator + Active Shadow Payer Repair

Status: `CODE-LEVEL REPAIR / R16-r3 NEXT GATE`

## Decyzja

R16-r2 zostal zatrzymany jako `PARTIAL DIAGNOSTIC`. Run spelnil funkcje
diagnostyczna: potwierdzil, ze L1R hydratuje PDD drift dla realnie
ewaluowanych rows, ale ujawnil dwa blokery przed uczciwym R16 diagnostic PASS:

```text
1. Skrypt reject diagnostics liczyl zly denominator dla PDD drift.
2. Active shadow BUY path uzywal configured payer i padal na payer AccountNotFound.
```

L1R2 naprawia tylko raportowanie i active-shadow payer semantics dla nastepnego
R16-r3. Nie zmienia progow, policy, probe amount, IWIM, P2 ani live.

## R16-r2 Stopped Artifact

Aktualny zatrzymany artefakt R16-r2 po zatrzymaniu runu:

```text
decision_rows = 843
BUY verdict rows = 7
probe_selection_rows = 72
probe_transport_rows = 4
probe_entry_rows = 3
probe_lifecycle_rows = 4
active_shadow_buys rows = 7
active_shadow_lifecycle rows = 7
active_shadow_entries rows = 0
lifecycle_labels rows = 0
```

Active shadow lifecycle rows sa dispatch failure rows, nie zamkniete lifecycle
labels:

```text
dispatch_status = failed
simulation_outcome = failed
err = Failed to fetch payer account: AccountNotFound
payer_pubkey = 9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw
```

Identity stamping dziala:

```text
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
```

## Denominator Repair

Poprawna semantyka:

```text
pdd_drift_evaluated_rows = rows where pdd_entry_drift_pct is present
```

`pdd_entry_drift_threshold_source` moze byc obecny jako default/fallback context
na rows, gdzie entry drift nie byl realnie ewaluowany. Takie rows nie moga
wchodzic do denominatora pokrycia anchor/current/elapsed.

Po poprawce na zatrzymanym R16-r2:

```text
pdd_drift_evaluated_rows = 453
pdd_drift_anchor_hydrated_rows = 453
pdd_drift_anchor_coverage_pct_among_evaluated = 100.0
pdd_drift_threshold_source_rows = 843
pdd_drift_threshold_source_only_rows = 390
diagnostic_quality.status = PASS
pdd_entry_drift_anchor_coverage_pct = 100.0
spike_ratio_coverage_pct = 100.0
spike_ratio_quality_coverage_pct = 100.0
whale_single_max_pct_coverage_pct = 100.0
gatekeeper_first_or_terminal_gate_coverage_pct = 100.0
```

To potwierdza, ze L1R runtime hydration dziala dla realnie ewaluowanych PDD
drift rows. Wczesniejszy FAIL byl bledem denominatora raportowego.

## Active Shadow Payer Repair

Skrypt `scripts/v3_p37_l1_reject_diagnostics.py` raportuje teraz:

```text
shadow_payer_strategy
shadow_payer_pubkey
shadow_payer_account_status
shadow_payer_account_error
shadow_payer_account_not_found_rows
shadow_payer_account_not_found_pubkey_counts
```

Na R16-r2 raport pokazuje:

```text
shadow_payer_strategy = configured
shadow_payer_account_status = rpc_missing
shadow_payer_pubkey = 9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw
shadow_payer_account_not_found_rows = 14
```

Nastepny run uzywa osobnego configu:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3.toml
```

W R16-r3 ustawiono:

```text
[trigger.shadow_run]
payer_strategy = "ephemeral"
```

To jest tylko diagnostyczny active-shadow payer mode dla R16. Baseline J4C,
root config, progi i active policy pozostaja nietkniete.

## Zmienione pliki

```text
scripts/v3_p37_l1_reject_diagnostics.py
scripts/test_v3_p37_l1_reject_diagnostics.py
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3.toml
PLANS/PLAN_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_PLANE_20260519.md
PLANS/AUDYT/RAPORT_P3_7_L1R2_REPORTING_DENOMINATOR_ACTIVE_SHADOW_PAYER_REPAIR_20260522.md
```

## Walidacja

Uruchomione:

```bash
python3 -m py_compile scripts/v3_p37_l1_reject_diagnostics.py
python3 -m unittest scripts/test_v3_p37_l1_reject_diagnostics.py -v
python3 scripts/v3_p37_l1_reject_diagnostics.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2.toml \
  --output-jsonl /tmp/p3_7_l1r2_r16_r2_per_reject_diagnostics.jsonl \
  --summary-json /tmp/p3_7_l1r2_r16_r2_summary.json \
  --summary-md /tmp/p3_7_l1r2_r16_r2_summary.md \
  --json
```

Wynik:

```text
py_compile = PASS
unit tests = 5/5 PASS
R16-r2 stopped artifact denominator validation = PASS
```

## R16-r3 Gate

R16-r3 jest nastepnym gate'em. Nie startowac L2 ablation przed R16-r3.

R16-r3 acceptance:

```text
strict replay = full_replay_ok
diagnostic_quality.status = PASS
pdd_drift_anchor_coverage_pct_among_evaluated >= 95%
spike_ratio_quality_coverage_pct >= 95%
whale_single_max_pct_coverage_pct >= 95%
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
active shadow BUY path does not fail on configured-payer AccountNotFound
BUY/probe lifecycle labels reported separately
custom_2006 classified, not unknown
```

## Non-Goals

```text
no threshold tuning
no L2 ablation yet
no Phase B
no P2/live
no IWIM change
no probe amount change
no root ghost_brain_config.toml edit
no baseline J4C config edit
```

