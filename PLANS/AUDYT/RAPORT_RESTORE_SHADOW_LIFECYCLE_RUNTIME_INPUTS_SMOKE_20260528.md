# RAPORT RESTORE SHADOW LIFECYCLE RUNTIME INPUTS SMOKE 2026-05-28

## Status

```text
CONTRACT RESTORE: PASS
CONFIG / TRANSPORT URL BLOCKER: FIXED
BCV2 / Custom(6062) SHADOW SIMULATION BLOCKER: FIXED
P37 LEGACY PRECHECK BLOCKER: FIXED
DIAG_ACCOUNT_UPDATE_RELAY TRUTH: PASS
GATEKEEPER BUY CONTEXT RECOVERY: PASS
RUNTIME RESTORE: PASS
```

Ten raport zamyka etap `RESTORE-SHADOW-LIFECYCLE-RUNTIME-INPUTS`.

Phase 1 przywrocila kontrakt reportera. Ten etap przywrocil runtime inputy potrzebne do realnego lifecycle proof:

```text
Yellowstone / Seer ingest
-> Gatekeeper BUY context
-> shadow transport buys
-> shadow_entries
-> shadow_lifecycle
-> DIAG_ACCOUNT_UPDATE_RELAY truth
-> shadow_onchain_lifecycle_report
-> compact raportneu-style outcome
```

Weryfikacja koncowa wygenerowala realny full lifecycle row z aktualnego `/root/Gho`, nie tylko syntetyczny fixture i nie tylko `0 rows`.

## Zakres

In scope:

- legacy pump.fun buy account layout dla shadow simulation;
- propagacja observed legacy buy remaining accounts z Seer do OracleRuntime i Trigger;
- P37 counterfactual precheck dla observed legacy buy remaining accounts;
- DirectBuyBuilder legacy remaining accounts;
- guard IWIM RPC URL przed placeholderami/aliasami;
- helper wyboru aktualnego Gatekeeper BUY logu w `shadow_run_report.py`;
- aktywny rollout config `configs/rollout/shadow-burnin.toml`, bo endpointy i payer strategy sa warunkiem powtarzalnosci smoke.

Out of scope:

- BCV2 rescue / BCV2 readiness redesign;
- X8/X9 terminal closure;
- Gatekeeper redesign;
- scoring / V3 policy changes;
- live Sender / Helius Sender;
- builder redesign;
- nowy labeler;
- zmiany w `shadow_onchain_lifecycle_report2.py`.

## Root Cause

Pierwotny runtime failure nie byl brakiem labelera ani problemem reportera.

Glowny blad runtime byl w execution account shape dla legacy pump.fun buy:

```text
shadow simulation error:
  n_error_account_role="bonding_curve_v2"
  simulation_error / Custom(6062)
```

Przyczyny:

1. Seer parser i dalszy runtime mylily observed legacy optional buy accounts z BCV2 / routed layout.
2. Legacy buy path nie przenosil dwoch required remaining accounts potrzebnych przez aktualny on-chain program layout.
3. P37 counterfactual precheck traktowal observed legacy remaining accounts jako wymagane do RPC precheck, zamiast uznac je za konta dostarczone przez observed buy tx shape.
4. `shadow_run_report.py` mogl wybrac stary plaski `gatekeeper_v2_buys.jsonl`, zamiast aktualnego logu z routed rollout scope.
5. Aktywny config mial stare endpoint placeholders i provider-unfriendly full-chain funding lane; bez poprawionych `GHOST_*` env varow, `funding_lane_mode = "disabled"` i `payer_strategy = "configured"` smoke nie jest powtarzalny.
6. IWIM mogl probowac zbudowac `RpcClient` z aliasu lub placeholdera URL, co w runtime dawalo `relative URL without a base`.

## Zmienione Pliki

Runtime/code:

```text
off-chain/components/seer/src/types.rs
off-chain/components/seer/src/binary_parser.rs
ghost-launcher/src/events.rs
ghost-launcher/src/components/seer.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/src/components/trigger/component.rs
ghost-launcher/src/components/iwim_veto.rs
off-chain/components/trigger/src/direct_buy_builder.rs
off-chain/components/trigger/src/lib.rs
scripts/shadow_run_report.py
```

Config:

```text
configs/rollout/shadow-burnin.toml
```

Report:

```text
PLANS/AUDYT/RAPORT_RESTORE_SHADOW_LIFECYCLE_RUNTIME_INPUTS_SMOKE_20260528.md
```

## Config Warunek Runtime Restore

`configs/rollout/shadow-burnin.toml` jest czescia runtime restore, bo bez tych zmian smoke nie ma stabilnych wejsc:

```text
funding_lane_mode: full_chain -> disabled
seer.grpc_endpoint: ${CHAINSTACK_GRPC_ENDPOINT} -> ${GHOST_SEER_GRPC_ENDPOINT}
seer.grpc_x_token: ${CHAINSTACK_GRPC_TOKEN} -> ${GHOST_SEER_GRPC_X_TOKEN}
seer.rpc_endpoint: ${CHAINSTACK_RPC_URL} -> ${GHOST_SEER_RPC_ENDPOINT}
trigger.rpc_url: ${CHAINSTACK_RPC_URL} -> ${GHOST_TRIGGER_RPC_URL}
trigger.shadow_run.shadow_rpc_url: ${CHAINSTACK_RPC_URL} -> ${GHOST_TRIGGER_SHADOW_RPC_URL}
trigger.shadow_run.payer_strategy: ephemeral -> configured
```

`entry_mode` pozostaje:

```text
shadow_only
```

To nie wlacza live Sendera.

## Najwazniejsze Zmiany Techniczne

### Seer parser / event model

Przywrocono jawne przenoszenie observed legacy buy remaining accounts:

```text
TradeEvent.buy_remaining_accounts: Vec<Pubkey>
PoolTransaction.buy_remaining_accounts: Vec<String>
```

Parser legacy buy capture:

```text
fixed accounts: 16
observed legacy remaining accounts: exactly account indices 16 and 17 when present
```

Legacy account index 16 nie jest juz traktowany jako `bonding_curve_v2`.

### OracleRuntime / Trigger handoff

OracleRuntime przenosi `buy_remaining_accounts` do `BuyAccountOverrides`.

Trigger waliduje liczbe remaining accounts:

```text
allowed: 0 albo 2
legacy index 16: buyback_fee_recipient
legacy index 17: buyback_quote_account
```

Routed path zachowuje osobna semantyke:

```text
routed index 16: bonding_curve_v2
routed index 17: buyback_fee_recipient
```

### DirectBuyBuilder

Dodano i uzyto buildera, ktory sklada legacy buy z observed remaining accounts:

```text
build_buy_ix_with_accounts_and_remaining
```

Legacy buy path dopina observed remaining accounts do instrukcji.

### P37 precheck

Observed legacy buy remaining accounts nie blokuja juz precheck:

```text
buyback_fee_recipient
buyback_quote_account
```

Sklasyfikowano je jako:

```text
observed_legacy_buy_remaining_account
non_fatal
legacy_buy_remaining_account_not_precheck_required
```

### IWIM URL guard

IWIM odrzuca przed `RpcClient`:

```text
empty URL
${...} placeholders
aliases: primary / fallback / runtime
non-absolute non-http(s) URLs
```

Efekt runtime:

```text
relative URL without a base: 0
```

### Gatekeeper BUY context recovery

`scripts/shadow_run_report.py` nie wybiera juz slepo starego plaskiego:

```text
decision_dir / gatekeeper_v2_buys.jsonl
```

Gdy istnieja routed rollout candidates, wybiera nowszy preferowany log rekursywnie.

Efekt w full lifecycle row:

```text
timing.gatekeeper_buy_context_found = true
```

## Preflight

Command:

```bash
cargo run -p ghost-launcher --bin ghost-launcher -- \
  --config configs/rollout/shadow-burnin.toml \
  --preflight
```

Wynik:

```text
PASS
execution_mode: Shadow
entry_mode: shadow_only
seer.grpc_endpoint: Chainstack Geyser OK
trigger.rpc_url getVersion: OK
trigger wallet balance: OK
entry/shadow/lifecycle dirs: writable
metrics port: free
```

## Smoke Evidence

Finalny smoke z poprawionym runtime path:

```bash
timeout 600s cargo run -p ghost-launcher --bin ghost-launcher -- \
  --config configs/rollout/shadow-burnin.toml
```

Run zakonczyl sie przez `timeout 124`, zgodnie z kontrolowanym smoke window.

Kluczowe markery:

```text
ResourceExhausted = 0
relative URL without a base = 0
Custom(6062) = 0
custom program error: 0x17ae = 0
buy_remaining_account_count=2 = observed
PostBuyGuardian: shadow simple threshold exit executed = observed
DIAG_ACCOUNT_UPDATE_RELAY = observed
```

Artefakty runtime po smoke:

```text
logs/shadow_run/shadow-burnin-v3-p1-buys.jsonl: >0 rows
logs/shadow_run/shadow-burnin-v3-p1/shadow_entries.jsonl: >0 rows
logs/shadow_run/shadow-burnin-v3-p1/shadow_lifecycle.jsonl: >0 rows
logs/rollout/shadow-burnin-v3-p1/system.log*: DIAG_ACCOUNT_UPDATE_RELAY >0
```

Ostatnia lokalna inspekcja artefaktow pokazala:

```text
logs/shadow_run/shadow-burnin-v3-p1-buys.jsonl: 32 rows
logs/shadow_run/shadow-burnin-v3-p1/shadow_entries.jsonl: 32 rows
logs/shadow_run/shadow-burnin-v3-p1/shadow_lifecycle.jsonl: 48 rows
```

## Reporter Proof

Command:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --output /tmp/restore_shadow_lifecycle_report.jsonl \
  --outcome-summary-output /tmp/restore_raportneu.json
```

Reporter output:

```text
scope_start_ms=1779944708873
session_run_id=launcher-1779944708873
rows_written=1
close_truth_coverage=1/1
```

Full lifecycle row:

```text
candidate_id=97h3mGmZT1bqN6HoPDzCpAMyLjViunenJCw9yMTTpump_eN9pNt6ECqEQ5DM9C5TuMHPC9ieo7U7aAvZnVbDUDST_1779945003583
close_reason=Target
truth_status=resolved
truth_source=canonical_account_state_snapshot
timing.gatekeeper_buy_context_found=true
entry_execution_ts_ms=1779945006813
close_ts_ms=1779945155918
position_duration_ms=149104
shadow.final_pnl_pct=52.94417142857143
exit_fills_len=1
```

Compact projection:

```text
/tmp/restore_raportneu.json: non-empty list, 1 row
```

## Verification

Targeted checks:

```text
python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py scripts/shadow_run_report.py: PASS
cargo fmt --check: PASS
cargo test -p seer enrich_trade_optional_accounts --lib: PASS, 4 tests
cargo test -p trigger direct_buy_builder --lib: PASS, 21 tests
cargo test -p ghost-launcher selected_legacy_buy --lib: PASS, 9 tests
cargo test -p ghost-launcher p37_route_resolver --lib: PASS, 5 tests
cargo test -p ghost-launcher p37_counterfactual_probe_required_accounts --lib: PASS, 6 tests
cargo test -p ghost-launcher p37_legacy_buy --lib: PASS, 6 tests
cargo test -p ghost-launcher configured_rpc_url --lib: PASS, 2 tests
cargo test -p ghost-launcher --bin ghost-launcher test_tracked_shadow_burnin_config_uses_primary_only_funding_mode: PASS, 1 test
cargo run -p ghost-launcher --bin ghost-launcher -- --config configs/rollout/shadow-burnin.toml --preflight: PASS
git diff --check: PASS
```

Uwaga: cargo nadal emituje istniejace warningi w kilku crates. Nie blokowaly testow.

## Acceptance

Acceptance dla runtime restore:

```text
shadow_transport_log: PASS
shadow_entries: PASS
shadow_lifecycle: PASS
DIAG_ACCOUNT_UPDATE_RELAY: PASS
Gatekeeper BUY context: PASS
shadow_onchain_lifecycle_report rows > 0: PASS
compact raportneu-style output non-empty: PASS
minimum 1 row with close_reason/final_pnl_pct/fills: PASS
truth_status=resolved for at least 1 row: PASS
```

Finalny werdykt:

```text
RESTORE-SHADOW-LIFECYCLE-RUNTIME-INPUTS: PASS
```

## Commit Scope

Commit powinien byc allowlist-only:

```text
configs/rollout/shadow-burnin.toml
off-chain/components/seer/src/types.rs
off-chain/components/seer/src/binary_parser.rs
ghost-launcher/src/events.rs
ghost-launcher/src/components/seer.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/src/components/trigger/component.rs
ghost-launcher/src/components/iwim_veto.rs
off-chain/components/trigger/src/direct_buy_builder.rs
off-chain/components/trigger/src/lib.rs
scripts/shadow_run_report.py
PLANS/AUDYT/RAPORT_RESTORE_SHADOW_LIFECYCLE_RUNTIME_INPUTS_SMOKE_20260528.md
```

Nie commitowac:

```text
ghost-brain/ghost_brain_config.toml
ghost-launcher/src/components/gatekeeper.rs
ghost-launcher/src/components/oracle_pipeline.rs
ghost-launcher/src/components/seer_stress_tests.rs
ghost-launcher/src/components/snapshot_listener.rs
ghost-launcher/src/config.rs
ghost-launcher/src/main.rs
ghost-launcher/src/tx_intelligence/*
off-chain/components/seer/src/ipc.rs
off-chain/components/seer/src/lib.rs
off-chain/components/seer/src/pumpportal_connection.rs
scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
scripts/v3_p37_mfs_lifecycle_join_key_audit.py
PLANS/AUDYT/RAPORT_P3_7_X9*
```

## Next Step

Nie robic teraz redesignu.

Nastepny etap to dluzszy shadow burnin bez nowych zmian, z celem datasetowym:

```text
N >= 100 lifecycle rows
truth_status = resolved
timing.gatekeeper_buy_context_found = true
shadow.final_pnl_pct present
close_reason in {Target, StopLoss, TimeStop}
```

Potem:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --output /tmp/shadow_onchain_lifecycle_report_batch.jsonl \
  --outcome-summary-output /tmp/raportneu_batch.json
```

Dopiero po batchu lifecycle rows wracac do baseline selektora / labelera / analizy Gatekeepera.
