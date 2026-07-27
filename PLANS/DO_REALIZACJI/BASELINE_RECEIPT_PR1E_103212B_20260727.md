# BASELINE RECEIPT PR1E — `103212b` — 2026-07-27

Status: `FROZEN CHANGE SET 0 / PR1E COMPARISON AUTHORITY`  
Repozytorium: `smahacfel/Gho`  
Branch roboczy: `agent/ingest-state-quote-boundary-pr1e-20260727`  
Baseline:
`103212b16bfc059db367e1ceb3c7d00fd307d6c5`  
Rodzice merge:
`a7a7bf194033331a2a59cad89f6ce255b82c7635`
oraz
`a982157f499313eb8f9b42326e67d495ace6224d`  
Opis: `Merge pull request #85 from
smahacfel/agent/ingest-observation-ledger-pr1d-20260725`

## 1. Granica dowodu

Receipt został wykonany po `git fetch origin --prune`, na czystym checkoutcie
dokładnego merge commita PR1D. Przed pierwszym zapisem do repo:

```text
git status --short                         = empty
git rev-parse HEAD                         = 103212b16bfc059db367e1ceb3c7d00fd307d6c5
git rev-parse origin/main                  = 103212b16bfc059db367e1ceb3c7d00fd307d6c5
git merge-base HEAD origin/main            = 103212b16bfc059db367e1ceb3c7d00fd307d6c5
```

Pełne stdout/stderr każdej bramki znajduje się w osobnym pliku `/tmp`.
Zbiorczy receipt:

```text
/tmp/pr1e_baseline_103212b_gate_summary.tsv
sha256 = 2b97ec7afb27489496ba3d55b94b3d9d710644ebfedd5e2620301b6097181111
```

Brak czerwonej bramki jest przedstawiany jako PASS. Podpis baseline nie jest
waiverem dla nowych albo zmienionych failure signatures.

## 2. Hash wejść

```text
Cargo.lock
3362b5fcd6cb305407906a9dbcd6a9094398ae1c7e6830d34ee4fb5c30c7a7c3

config.toml
eecc6462eaa98325a899bc4de19fb5fba7387f74ac96e3c994126379fb60e737

ghost-brain/ghost_brain_config.toml
81419c88b9a2e79589e02674899535f98ab47c227aef91baa81891d53c78e032
```

## 3. Macierz bramek

| Bramka | Czas | Exit | Status | Log SHA-256 |
| --- | ---: | ---: | --- | --- |
| `cargo fmt --all --check` | 5 s | 0 | PASS | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `git diff --check` | 0 s | 0 | PASS | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo test -p ghost-core` | 13 s | 101 | EXPECTED BASELINE FAILURE | `a3807a16445b6a7f8b33345c011a621cf9ddb13aba26eab86cf761e4fa09b13d` |
| `cargo test -p seer --no-fail-fast` | 25 s | 101 | EXPECTED BASELINE FAILURE | `ead56d0c94846395b2e8a9605f8fb186bf33b5307ad2053c5d8160fb2895f721` |
| `cargo test -p trigger` | 19 s | 101 | EXPECTED BASELINE FAILURE | `ae96c69dbffed4c0d8fcbed46c93121ce21240c90f0ab73520fef3c2a356f842` |
| `cargo test -p ghost-launcher` | 59 s | 101 | EXPECTED BASELINE FAILURE | `c4bc557dd0c4e2ef6e30a52d72eb77767dc0a75de6a7795016a7d022bd516a39` |
| `cargo test --workspace --no-fail-fast` | 83 s | 101 | EXPECTED BASELINE FAILURE | `90ea91de4d04f0923d52abc74ebb5eb1b46d0db6722efdb83ff94df07a8cf625` |
| `cargo build --release --workspace` | 626 s | 0 | PASS | `3948c54fd4f1e65fc83e7e298d88c84fab4b08e1c2a76d78709119ffc1983c69` |

## 4. Zamrożone failure signatures

### 4.1. `ghost-core`

```text
ghost-core/tests/pr1_contracts_foundations.rs
foundational_types_serialize_and_deserialize_roundtrip
deserialize account update: InvalidTagEncoding(104)
3 passed; 1 failed
```

### 4.2. `seer`

`seer --lib`:

```text
469 passed; 12 failed; 2 ignored

pumpportal_connection::tests::test_buy_emits_only_tx
pumpportal_connection::tests::test_create_emits_both_events
pumpportal_connection::tests::test_create_pool_detected_before_tx
pumpportal_connection::tests::test_create_to_pool_transaction
pumpportal_connection::tests::test_price_derived_from_reserves
pumpportal_connection::tests::test_price_none_when_no_reserves
pumpportal_connection::tests::test_price_none_when_zero_tokens
pumpportal_connection::tests::test_pumpportal_no_slot
pumpportal_connection::tests::test_sell_emits_only_tx
pumpportal_connection::tests::test_sol_amount_to_lamports_precision
tests::test_create_sets_curve_mapping
tests::test_wal_records_raw_and_parsed_synthetic_trade
```

`seer --test source_router`:

```text
test_geyser_mode_parses_raw_events
Binary parser SHOULD be invoked for raw events in Geyser mode
13 passed; 1 failed
```

### 4.3. `trigger`

```text
jito_client::tests::test_get_bundle_status_by_uuid_uses_submit_endpoint_host_for_polling
ConfigError("Jito status polling requires status_uuid but none is configured")

jito_client::tests::test_confirm_bundle_submission_rejected_bundle_keeps_tip_signature_offchain
ConfigError("Jito status polling requires status_uuid but none is configured")

transaction_builder::tests::test_presigned_transaction_size_validation
assertion failed: presigned.size_bytes < 700

335 passed; 3 failed
```

### 4.4. `ghost-launcher`

Test build zatrzymuje historyczny test/example-only `E0063`:

```text
ghost-launcher/examples/oracle_validation_comprehensive.rs:356
PoolTransaction
missing fields complete, creator_vault, real_sol_reserves and 3 other fields
```

### 4.5. Workspace

`--no-fail-fast` ujawnia tę samą historyczną klasę testowych fixture’ów:

```text
ghost-launcher/tests/time_contract_bridge.rs:8
TradeEvent
missing fields complete, real_sol_reserves, real_token_reserves and 2 other fields

ghost-launcher/tests/time_contract_bridge.rs:63
PoolTransaction
missing fields complete, real_sol_reserves, real_token_reserves and 2 other fields
```

W PR1E failure może zostać zaklasyfikowany jako baseline wyłącznie wtedy, gdy
zachowuje ten sam kod błędu, typ, missing-field set i istniejący na baseline
callsite. Inny target lub inny zestaw pól wymaga osobnego dowodu źródłowego.

## 5. Post-build executable inventory

Poniższe hashe opisują wykonawczy inventory wspólnego `target/release` po
zielonym baseline buildzie. Nie są przedstawiane jako niezależny dowód
reprodukowalności każdego artefaktu:

```text
c449028577017e3ecbe3a12fd1031c9d28bb0736f21396b7450b9b1775531a5d  e2e-test-runner
ad6f475d6a07170dff0c9db194bbec79efb83c3887d21b8a7355b9f7a51c7e4c  ghost-brain-backtest-qedd
4257f5aa92304515c6252c029bdf905361d6d118cedd16d231a1df59cf93a3be  ghost-brain-calibrate-qedd
57bb586eee82fe25cc88ded3e532e530ef8e934e9ab6b8ed22de2cc68fb59b3c  ghost-e2e-runner
f51ee76e3a00433b596be95c3a9822568fb09f04f668ae9d876c7d28b3439232  ghost-gui
e0ee4230901002efdcdd9e4d26237d3973a24f59f714cf1a1e74d37738747d27  ghost-launcher
e59cf8ee6c40e500efa8266dbba0f38b3e4bb91f432f3faef2435fb09a6894ad  metric_contract_audit
0a5f12bffe0160cf5a106682f498fa6aba234f8a89ec531fec94ecce06e5b0ae  perf-test
971d7b4114e7ea1f8e1ee39de92275842de82cb3dcc42fd805cf98c7407329b3  pumpfun_collector
84b4ca832f5cfde848535591713dd2b2942eebc45bb349a98a0ffe65ccf7af22  replay_equivalence_proof
1a1062014596420ec78cc0cfd612c36e0750491233237100a3ca5fbfc9b93008  seer
567e35e2a02b073cbf340d046f6b9e6f043bfbedc5fd60b6cb12f8124fa6c081  trigger
c8a3fe24b0bc558c5860286bd84a94d54ab30dd79c1de5e0673b9844c1774208  v3_replay
```

## 6. PR1E comparison rule

Finalny PR1E:

- nie może wprowadzić nowej failure signature;
- musi utrzymać zielony release workspace build;
- musi wykonać dedykowany PR1E corpus i fault injection;
- musi przejść formalny parent-versus-branch performance protocol;
- nie dziedziczy performance waivera PR1D;
- nie może naprawiać powyższych historycznych fixture’ów w ramach PR1E;
- musi zachować exact baseline output dla wszystkich untouched primary
  records.

