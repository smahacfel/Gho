# RAPORT P3.7-L1R14 R16-r11 BCV2 Identity-Readiness Smoke

Data: 2026-05-23
HEAD: `d6992a5`
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness.toml`
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness`

## Werdykt

`R16-r11 = PASS-B / CORRECT FAIL-CLOSED`

L1R14 runtime smoke potwierdził najważniejszą semantykę:

- `observed_tx_account_meta` może być źródłem identity dla `bonding_curve_v2`,
- observed identity nie jest już traktowane jako execution readiness,
- `bonding_curve_v2_rpc_load_ready=false` nie daje `builder_required_curve_account_ready=true`,
- `bonding_curve_v2` nie dochodzi już do post-simulation `AccountNotFound`,
- failure rows nie są lifecycle-eligible.

Smoke nie odblokował execution. Wszystkie active shadow BUY rows zakończyły się fail-closed przed symulacją na `execution_account_not_ready:bonding_curve_v2:<pubkey>`.

## Uruchomione raporty

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness.toml \
  --json

python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness.toml \
  --strict \
  --json

python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness/p3_7_mfs_lifecycle_join_key_audit.json \
  --output-md logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness/p3_7_mfs_lifecycle_join_key_audit.md

python3 scripts/v3_p37_l1_reject_diagnostics.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness.toml \
  --json
```

Lifecycle labelery nie zostały uruchomione, ponieważ `shadow_lifecycle.jsonl` zawiera dispatch failure rows oznaczone jako `not_lifecycle_eligible`, nie zamknięte pozycje do labelowania.

## Główne liczby

Replay i diagnostyka:

- `replay_status = full_replay_ok`
- `total_rows = 409`
- `v3_rows = 409`
- `bad_rows = 0`
- `diagnostic_quality.status = PASS`
- `r16_artifact_identity_status = PASS`
- `single_active_hash_status = PASS`

V3 / policy:

- `BUY = 7`
- `REJECT_CORE_FAIL = 20`
- `REJECT_HARD_FAIL = 29`
- `REJECT_IWIM_LOW_CONF = 1`
- `TIMEOUT_PHASE1_INSUFFICIENT = 271`
- `TIMEOUT_PHASE1_NO_DATA = 81`

Probe:

- `probe_selection_rows = 16`
- `probe_transport_rows = 0`
- `probe_entry_rows = 0`
- `probe_lifecycle_rows = 0`
- `probe_skip_rows = 409`
- `bonding_curve_v2_precheck_skipped_before_simulation_rows = 16`
- `bonding_curve_v2_account_not_found_after_simulation_rows = 0`

Active shadow:

- canonical `active_shadow_buys = 7`
- canonical `active_shadow_entries = 7`
- canonical `active_shadow_lifecycle = 7`
- `active_shadow_successful_entry_rows = 0`
- `active_shadow_lifecycle_eligible_rows = 0`
- `active_shadow_runtime_simulation_error_rows = 0`
- `active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows = 21`
- `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows = 0`
- `active_shadow_lifecycle_eligibility_status_counts = {"not_lifecycle_eligible": 21}`

Uwaga: liczba `21` w join-key audit liczy failure evidence przez active shadow transport/entry/lifecycle artefact surfaces; kanoniczne active shadow BUY verdict rows to `7`.

## BCV2 Identity vs Readiness

Active shadow BCV2 diagnostics:

- `bonding_curve_v2_identity_authority_status = authoritative_observed_tx` dla wszystkich active shadow failure artefaktów,
- `bonding_curve_v2_source = observed_tx_account_meta`,
- `bonding_curve_v2_rpc_load_status = missing_on_rpc_precheck`,
- `bonding_curve_v2_rpc_load_ready = false`,
- `builder_required_curve_account_ready = false`,
- `builder_required_curve_account_ready_reason = bonding_curve_v2_observed_meta_missing_on_rpc`.

To jest oczekiwane zachowanie po L1R14. Identity source i execution readiness są rozdzielone.

## Decyzja

`P3.7-L1R14 runtime semantic gate = PASS-B`

R16-r11 potwierdza, że L1R14 naprawił błąd semantyczny: observed BCV2 identity nie awansuje już automatycznie do execution-ready. Jednocześnie route nadal nie jest wykonywalna w tej próbce, bo observed BCV2 jest missing on RPC precheck.

## Następny krok

Nie L2, nie collection, nie Phase B.

Następna decyzja techniczna to route-level handling dla tego stanu:

1. route fallback, jeśli istnieje executable fallback route z kompletnym simulation-load account set,
2. route exclusion, jeśli `routed_exact_sol_in` wymaga BCV2 missing on RPC,
3. parser provenance validation, jeśli nadal podejrzewamy, że `observed_tx_account_meta` wskazuje zły instruction account position/message account index.

Minimalny następny etap:

`P3.7-L1R15 / J3Q - BCV2 Observed Meta Provenance Validation + Route Fallback/Exclusion Decision`

Cel: rozstrzygnąć, czy observed BCV2 missing-on-RPC oznacza prawdziwie nieegzekwowalną route, czy błąd parsera/provenance account meta.
