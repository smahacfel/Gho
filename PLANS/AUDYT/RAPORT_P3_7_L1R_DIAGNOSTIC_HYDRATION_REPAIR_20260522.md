# RAPORT P3.7-L1R Diagnostic Hydration Repair

Status: `CODE-LEVEL REPAIR / R16-r2 NEXT GATE`

## Verdict

L1R naprawia dwie blokady z R16 standard/soft-PDD r1:

```text
diagnostic_quality.status = FAIL
r16_artifact_identity_status = FAIL
```

Zakres jest diagnostyczny. Nie zmieniono progow polityki, configu J4C baseline,
root `ghost_brain_config.toml`, P2/live ani Phase B.

## R16-r1 Blockers

R16-r1 pokazal pozytywny sygnal runtime:

```text
strict replay = full_replay_ok
probe lifecycle labels zawieraly dirty_good
active shadow lifecycle labels zawieraly dirty_good
```

Nie mogl jednak sluzyc jako pelny diagnostic PASS, bo najwazniejsze pola
PDD/whale nie byly wypelnione:

```text
pdd_entry_drift_anchor_coverage_pct = 0.0
pdd_spike_ratio_quality_coverage_pct = 0.0
whale_single_max_pct_coverage_pct = 0.0
```

Druga blokada to brudna tozsamosc artefaktow:

```text
one active shadow_lifecycle shadow_dispatch failure row missing run/session/brain/policy hash
single_active_hash_status = FAIL
```

## Code Changes

### Materialized PDD Hydration

`ghost-launcher/src/components/gatekeeper_policy.rs` uzupelnia teraz
materialized PDD diagnostics emitowane z tej samej sciezki policy/evaluation,
ktora buduje R16 decision rows.

Dodane pola:

```text
pdd_entry_drift_elapsed_ms
pdd_entry_drift_anchor_price
pdd_entry_drift_current_price
pdd_entry_drift_anchor_ts_ms
pdd_entry_drift_current_ts_ms
pdd_entry_drift_elapsed_max_pct
pdd_entry_drift_effective_max_pct
pdd_entry_drift_threshold_source
pdd_spike_ratio
pdd_spike_ratio_quality
pdd_spike_recent_rate
pdd_spike_earlier_rate
pdd_whale_single_max_pct
```

Entry drift zachowuje dotychczasowa hierarchie zrodel:

```text
checkpoint price_change_from_first_checkpoint_pct
fallback account_features.price_change_since_t0_pct
```

Anchor price jest wyprowadzany tylko wtedy, gdy `current_price` i drift sa
skonczone oraz dodatnie. Jesli timestampy albo ceny sa niekompletne, row jest
jawnie zdegradowany przez `pdd_entry_drift_threshold_source`, np.
`fallback_no_anchor` albo `invalid_timestamp_order`.

### Spike Ratio Quality

Materialized sequence diagnostics emituja:

```text
ok
earlier_rate_zero
insufficient_earlier_window
insufficient_recent_window
unavailable
```

Gdy `earlier_rate = 0`, `pdd_spike_ratio = null`; logger nie serializuje
nieskonczonosci.

### Whale Single Max

Path B materialized features nie maja pelnej signer-level atrybucji wolumenu.
L1R wypelnia `pdd_whale_single_max_pct` jako decision-time-safe proxy:

```text
max_tx_sol / total_volume_sol * 100
```

To jest diagnostyka R16 dla raportowania pokrycia whale/PDD, nie zamiennik
bogatszej atrybucji signer-level.

### Shadow Failure Identity

`ghost-launcher/src/oracle_runtime.rs` dodaje helper failure-context, ktory
wstrzykuje `ExecutionJoinMetadata` do `TriggerDispatchFailureContext`.

Obejmuje to failure rows emitowane przed normalnym shadow dispatch/entry, np.
prepare/preflight failure. Failure rows nie powinny juz zrywac kontraktu:

```text
run_id
session_id
rollout_namespace
brain_config_hash
v3_policy_config_hash
ab_record_id, gdy dostepny
```

## Tests

Targeted Rust:

```text
cargo test -p ghost-launcher --lib materialized_pdd -- --nocapture
PASS: 2/2

cargo test -p ghost-launcher --lib shadow_dispatch_failure_context_inherits_execution_join_metadata -- --nocapture
PASS: 1/1
```

Python diagnostics:

```text
python3 -m py_compile scripts/v3_p37_l1_reject_diagnostics.py
PASS

python3 -m unittest scripts/test_v3_p37_l1_reject_diagnostics.py -v
PASS: 3/3
```

Broader validation:

```text
rustfmt --edition 2021 --check ghost-launcher/src/components/gatekeeper_policy.rs ghost-launcher/src/oracle_runtime.rs
PASS

cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
PASS: 47/47

cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
PASS: 8/8

git diff --check
PASS
```

## Next Gate

Run R16-r2 with the same policy bundle and a fresh namespace:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2.toml
```

R16-r2 acceptance:

```text
strict replay = full_replay_ok
diagnostic_quality.status = PASS
pdd_entry_drift_anchor_coverage_pct >= 95%
pdd_spike_ratio_quality_coverage_pct >= 95%
whale_single_max_pct_coverage_pct >= 95%
gatekeeper_first_or_terminal_gate_coverage_pct = 100%
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
active BUY/probe lifecycle labels reported separately
custom_2006 classified, not unknown
```

Only after R16-r2 diagnostic PASS can `P3.7-L2 Policy Axis Ablation` start.

## Holds

```text
No ablation yet
No Phase B
No P2/live
No threshold tuning
No baseline J4C config edit
No root ghost_brain_config.toml edit
```
