# P3.7-X1 Working Helius Buy/Sell Execution Path Recovery & Parity Audit

Data: 2026-05-25
Tryb: offline audit / repo artifact recovery
Runtime: nie uruchamiano
Smoke: nie uruchamiano

## Final Decision

`WORKING_BUILDER_FOUND_AND_CURRENT_PATH_DIVERGED`

Working buy/sell path istnieje w repo jako realny live path przez Helius Sender,
potwierdzony artefaktami `data/dual_live_090426_1_summary.jsonl` i
`data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl`.

Obecny P3.7 probe/shadow path nie odtwarza tej samej sciezki end-to-end:

- live path konczy sie `LiveTxSender::send_transaction(&request.buy_tx)` i
  Yellowstone/balance confirmation,
- P3.7 probe path konczy sie `simulate_counterfactual_shadow_probe()` ->
  `RpcShadowSimulator::simulate_buy()` na `request.rpc_buy_tx`,
- P3.7 doklada warstwe route resolver / selected fallback handoff, ktora moze
  przelaczyc logiczna trase na `legacy_buy`, ale finalny manifest nadal ma
  `bonding_curve_v2` z obecnego `DirectBuyBuilder`,
- current `DirectBuyBuilder` jest ta sama rodzina buildera co working BUY path,
  ale obecny P3.7 route/simulation boundary nie jest working Helius sender
  boundary i nie zachowuje tej samej executable route semantics.

Nastepny krok:

`P3.7-X2 -- Rebind P3.7 shadow/probe execution to working Helius builder path`

## Scope / Non-Goals Checked

In scope:

- odzyskanie working BUY path,
- odzyskanie working SELL path,
- zmapowanie aktualnego P3.7 PreparedBuyRequest / DirectBuyBuilder /
  shadow-probe path,
- porownanie builder/account manifestow,
- decyzja X1.

Out of scope, zgodnie z taskiem:

- no runtime,
- no smoke,
- no Gatekeeper changes,
- no threshold tuning,
- no V3 selector,
- no P2/live activation,
- no `legacy_buy` patch,
- no BCV2 handoff patch,
- no new route implementation.

## Evidence Base

Kod:

- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/live_tx_sender.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`
- `off-chain/components/trigger/src/direct_buy_builder.rs`
- `off-chain/components/trigger/src/revolver_sell_builder.rs`

Historia git:

- `git log --all --oneline -S'Helius Sender BUY confirmed' ...` wskazuje
  `d0926a2 Initial commit`.
- `git log --all --oneline -S'LiveExit: SELL submitted via Helius Sender' ...`
  wskazuje `d0926a2 Initial commit`.
- `git show d0926a2:off-chain/components/trigger/src/direct_buy_builder.rs`
  pokazuje historyczny `DirectBuyBuilder` z `bonding_curve_v2` w manifiescie
  BUY.

Artefakty:

- `data/dual_live_090426_1_summary.jsonl`
- `data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl`
- `configs/dual-micro-live.toml`
- raporty E5A/E5B/E6/E4R3S pod `PLANS/AUDYT/`

Uwaga: artefakty live potwierdzaja landing/signature/status/amount dla Ghost BUY
i SELL, ale nie zawieraja pelnego raw submitted transaction manifestu dla
wlasnego BUY. Manifest account order/discriminator dla working BUY jest wiec
odzyskany z kodu buildera i historii git, nie z dekodowanego raw tx.

## Working BUY Path Call Graph

Recovered working path:

```text
Gatekeeper BUY
-> OracleRuntime active BUY wrapper
-> execute_gatekeeper_buy_via_trigger_with_fsc_gate()
-> TriggerComponent::prepare_buy_request_with_tip_telemetry()
-> TriggerComponent::create_buy_build_profile()
-> DirectBuyBuilder::build_buy_ix_with_accounts_and_bonding_curve_v2()
-> TriggerComponent::build_buy_transaction_from_profile()
-> PreparedBuyRequest { rpc_buy_tx, buy_tx }
-> TriggerComponent::dispatch_prepared_buy_with_shadow()
-> TriggerComponent::submit_prepared_via_sender()
-> LiveTxSender::send_transaction(&request.buy_tx)
-> TriggerComponent::confirm_sender_buy_attempt()
-> TriggerBuyOutcome::LiveConfirmed
-> apply_trigger_buy_outcome()
-> PostBuySubmitted lane=live source=LiveBuy
```

Code anchors:

- `PreparedBuyRequest` niesie jednoczesnie `rpc_buy_tx` i `buy_tx`:
  `ghost-launcher/src/components/trigger/component.rs:341-374`.
- Live BUY executor jest opisany jako `[Compute Budget, ATA?, Swap, Tip
  Transfer] -> VersionedTransaction -> Helius Sender -> Yellowstone
  Confirmation`: `ghost-launcher/src/components/trigger/component.rs:524-540`.
- live dispatch fail-closed wymaga Helius Sender + Yellowstone:
  `ghost-launcher/src/components/trigger/component.rs:2480-2483`.
- `create_buy_build_profile()` defaultuje do `RoutedExactSolIn` gdy override
  nie poda wariantu i buduje `DirectBuyBuilder`:
  `ghost-launcher/src/components/trigger/component.rs:2653-2733`.
- `build_buy_transaction_from_profile()` buduje zarowno legacy `Transaction`
  do RPC simulation jak i v0 `VersionedTransaction` do Sender:
  `ghost-launcher/src/components/trigger/component.rs:2735-2787`.
- `submit_prepared_via_sender()` wykonuje lokalny AccountStateCore preflight,
  potem wysyla `current_request.buy_tx` przez Sender:
  `ghost-launcher/src/components/trigger/component.rs:3314-3565`.
- `dispatch_prepared_buy_with_shadow()` w live/live_and_shadow konczy sie
  `submit_prepared_via_sender()`:
  `ghost-launcher/src/components/trigger/component.rs:4684-4833`.
- `LiveTxSender::send_transaction()` serializuje `VersionedTransaction` base64
  i wywoluje JSON-RPC `sendTransaction` z `skipPreflight=true` i `maxRetries=0`:
  `ghost-launcher/src/components/live_tx_sender.rs:957-1030`.
- `LiveTxSender::confirm_submission_with_timeout()` subskrybuje Yellowstone
  `transactions_status` po signature:
  `ghost-launcher/src/components/live_tx_sender.rs:1040-1151`.

Working live BUY evidence:

- run `20260409-123806` mial `buy_confirmed_count=7`,
  `sell_confirmed_count=7`, `realized_exit_count=7`:
  `data/dual_live_090426_1_summary.jsonl:1-8`.
- first BUY:
  - mint `8ex24x4XVnFBc8krzif7A1xWm1zvhikGoJXGP3egpump`,
  - signature
    `TsXUKkGaLREDt4inf3jk2JivWEMDs8nVW6b5Rdi7CFVjjyVHTSL7r1aCEUzheGjbYrPCQV8iWnvup7X1VGfNdqC`,
  - submit slot `412071427`,
  - landed slot `412071428`,
  - confirm source `signature_status`,
  - amount `100000` lamports,
  - tip `1000000` lamports,
  - priority fee `2000000` micro-lamports:
  `data/dual_live_090426_1_summary.jsonl:41-63`.
- console log potwierdza `Trigger: live BUY submitted via Helius Sender`:
  `data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl:34252`.
- console log potwierdza `Helius Sender BUY confirmed`, `LIVE BUY LANDED`,
  Gatekeeper BUY i `PostBuySubmitted lane=live source=LiveBuy`:
  `data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl:34555-34561`.

## Working SELL Path Call Graph

Recovered working SELL path:

```text
LiveConfirmed BUY
-> PostBuySubmitted lane=live source=LiveBuy
-> PostBuyRuntime::run()
-> LiveExit session
-> initialize_live_exit_session()
-> EntryPriceExtractor::extract_with_retry()
-> canonical price monitor / stop-loss or take-profit trigger
-> build_full_exit_transaction_with_retry()
-> SellTxBuilder::build_signed_sell_tx_with_token_program_and_priority_tip()
-> SellTxBuilder::build_pump_sell_instruction_with_token_program()
-> VersionedTransaction
-> submit_live_exit_transaction()
-> LiveTxSender::send_transaction(&transaction)
-> confirm_sender_sell_attempt()
-> mark_exit_confirmed()
-> realized exit evidence
```

Code anchors:

- `PostBuyRuntime` live lane doc: persists confirmed BUY metadata, monitors
  canonical price and executes one 100% SELL via Helius Sender:
  `ghost-launcher/src/components/post_buy_runtime.rs:1-27`.
- `LiveSellHandle` owns `live_tx_sender`, payer, RPC client and canonical
  AccountStateCore for live SELL:
  `ghost-launcher/src/components/post_buy_runtime.rs:74-87`.
- live exit function is documented as Sender-only and launcher-owned:
  `ghost-launcher/src/components/post_buy_runtime.rs:2574-2597`.
- `build_full_exit_transaction_with_retry()` computes sell amount/min output,
  tip, fresh blockhash, priority fee and builds `SellTxBuilder` transaction:
  `ghost-launcher/src/components/post_buy_runtime.rs:2694-2904`.
- `submit_live_exit_transaction()` sends through `live.live_tx_sender` and
  confirms terminal exit:
  `ghost-launcher/src/components/post_buy_runtime.rs:2906-3190`.
- `SellTxBuilder::build_signed_sell_tx_with_token_program_and_priority_tip()`
  builds v0 transaction with compute budget, SELL instruction and optional tip:
  `off-chain/components/trigger/src/revolver_sell_builder.rs:203-291`.
- Pump.fun SELL instruction uses 24-byte `SELL_DISCRIMINATOR + amount +
  min_sol_output`, account order global/fee/mint/bonding_curve/associated/user
  etc., and appends `bonding_curve_v2` last:
  `off-chain/components/trigger/src/revolver_sell_builder.rs:359-433`.
- startup builds `LiveTxSender` and `LiveSellHandle` before PostBuyRuntime:
  `ghost-launcher/src/main.rs:400-433`, `ghost-launcher/src/main.rs:850-893`,
  `ghost-launcher/src/main.rs:1838-1897`.

Working live SELL evidence:

- first SELL:
  - pool `BRcRtCUBD361VL9uyG6FoXVsjDByLhXmCmbJ9HkJxFry`,
  - base mint `8ex24x4XVnFBc8krzif7A1xWm1zvhikGoJXGP3egpump`,
  - buy signature same as first BUY,
  - exit signature
    `4be4TqiuocZU1wCYY8NnfHLjG3ST7187UvuR5sTSwSgG7Q5RahKQoWayqVv5Jj6mbDF8cqo5pe9kPG3qMqau6uTg`,
  - landed slot `412071432`,
  - confirm source `balance_delta`,
  - tip `1000000`,
  - priority fee `2000000`:
  `data/dual_live_090426_1_summary.jsonl:64-80`.
- realized exit evidence includes token account, token balance before/after,
  token program, SOL received and `tokens_sold_raw=1545600207`:
  `data/dual_live_090426_1_summary.jsonl:82-110`.
- console log confirms SELL tx build and Helius Sender submit/confirmation:
  `data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl:34748-34750`,
  `data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl:34772-34775`,
  `data/dual-micro-live/dual_live_090426_1_full_console_logs.jsonl:35004-35005`.

## Current P3.7 Probe / Shadow Execution Path

Current P3.7 problem path:

```text
P3.7 Gatekeeper decision / probe selection
-> maybe_handle_p37_shadow_probe_decision()
-> run_p37_shadow_probe_dispatch()
-> p37_shadow_probe_derive_account_override_context_for_pool()
-> p37_shadow_probe_execution_precheck()
-> TriggerComponent::prepare_buy_request_with_decision_ts_and_amount_lamports()
-> PreparedBuyRequest
-> p37_apply_selected_fallback_route_handoff_for_shadow_only()
-> p37_shadow_probe_account_set_diagnostics()
-> p37_selected_route_final_manifest_failure_reason()
-> p37_shadow_probe_wait_for_required_account_readiness()
-> TriggerComponent::simulate_counterfactual_shadow_probe()
-> RpcShadowSimulator::simulate_buy()
-> rpc.simulate_transaction_with_config(&request.rpc_buy_tx, ...)
-> probe transport / shadow entries / probe lifecycle labels
```

Code anchors:

- P3.7 dispatch starts in `maybe_handle_p37_shadow_probe_decision()` and spawns
  `run_p37_shadow_probe_dispatch()`:
  `ghost-launcher/src/oracle_runtime.rs:11181-11280`.
- probe dispatch prepares a buy request using TriggerComponent, then applies
  selected fallback handoff:
  `ghost-launcher/src/oracle_runtime.rs:10787-10900`.
- selected route final manifest failure blocks before simulation:
  `ghost-launcher/src/oracle_runtime.rs:10928-10965`.
- required account readiness gates run before simulation:
  `ghost-launcher/src/oracle_runtime.rs:10968-11023`.
- actual P3.7 simulation call is
  `trigger_component.simulate_counterfactual_shadow_probe(&request)`:
  `ghost-launcher/src/oracle_runtime.rs:11046-11052`.
- `simulate_counterfactual_shadow_probe()` delegates to `shadow_simulator`:
  `ghost-launcher/src/components/trigger/component.rs:4348-4355`.
- `RpcShadowSimulator::simulate_buy()` calls
  `rpc.simulate_transaction_with_config(&request.rpc_buy_tx, ...)`, not Helius
  Sender:
  `ghost-launcher/src/components/trigger/shadow_run.rs:1087-1119`.

Route resolver / fallback details:

- `P37_LEGACY_BUY_FALLBACK_SUPPORT_STATUS` is
  `unsupported_builder_layout_requires_bcv2`:
  `ghost-launcher/src/oracle_runtime.rs:6566-6569`.
- route resolution can attempt fallback `legacy_buy`, but returns
  `no_executable_route_account_set` when primary BCV2 is missing/not load-ready
  and legacy is removed as unsupported:
  `ghost-launcher/src/oracle_runtime.rs:6875-7045`.
- selected fallback handoff rebuilds the request as `LegacyBuy` and clears
  `bonding_curve_v2` in overrides, but the rebuild re-enters the same
  `DirectBuyBuilder` final manifest:
  `ghost-launcher/src/oracle_runtime.rs:13119-13225`.
- active BUY wrapper also applies this selected fallback handoff before
  dispatch/precheck:
  `ghost-launcher/src/oracle_runtime.rs:13329-13545`.

E4/E5/E6 audit chain:

- E4R3S: selected legacy handoff was claimed but validated zero times; final
  selected legacy manifest contained BCV2 in active and probe rows; execution
  unlock stayed NO-GO:
  `PLANS/AUDYT/RAPORT_P3_7_E4R3S_FINAL_SELECTED_ROUTE_MANIFEST_SMOKE_20260525.md:237-280`.
- E5A: current `DirectBuyBuilder` `LegacyBuy` final manifest still uses extended
  account list and contains `bonding_curve_v2` at account index 16:
  `PLANS/AUDYT/RAPORT_P3_7_E5A_DIRECT_BUY_BUILDER_ROUTE_ABI_MANIFEST_AUDIT_20260525.md:40-100`.
- E5B: current `legacy_buy` fallback is closed as unsupported under current
  builder/account-layout support:
  `PLANS/AUDYT/RAPORT_P3_7_E5B_LEGACY_BUY_ROUTE_CLOSURE_20260525.md:5-55`.
- E6: current artifacts have no safe next route target; `legacy_buy` closed,
  `routed_exact_sol_in` blocked by BCV2 readiness, supported executable route
  scope empty:
  `PLANS/AUDYT/RAPORT_P3_7_E6_ROUTE_SUPPORT_NEXT_TARGET_DECISION_20260525.md:5-10`,
  `PLANS/AUDYT/RAPORT_P3_7_E6_ROUTE_SUPPORT_NEXT_TARGET_DECISION_20260525.md:54-76`.

## DirectBuyBuilder Manifest Recovery

Current builder:

```text
variant: RoutedExactSolIn
discriminator: 38 fc 74 08 9e df cd 5f
payload: amount_sol_in:u64 + min_tokens_out:u64 + track_volume:u8
payload_len: 25
accounts_len: 18
account[16]: bonding_curve_v2
account[17]: routed_buyback_fee_recipient
```

```text
variant: LegacyBuy
discriminator: 66 06 3d 12 01 da eb ea
payload: min_tokens_out:u64 + amount_sol_in:u64
payload_len: 24
accounts_len: 18
account[16]: bonding_curve_v2
account[17]: routed_buyback_fee_recipient
```

Code evidence:

- discriminators and enum:
  `off-chain/components/trigger/src/direct_buy_builder.rs:71-98`.
- current comment explicitly says current `global:buy` still uses 24-byte
  legacy payload with newer extended account list:
  `off-chain/components/trigger/src/direct_buy_builder.rs:78-83`.
- builder default variant is `RoutedExactSolIn`:
  `off-chain/components/trigger/src/direct_buy_builder.rs:230-245`.
- current payloads:
  `off-chain/components/trigger/src/direct_buy_builder.rs:281-298`.
- current account metas:
  `off-chain/components/trigger/src/direct_buy_builder.rs:300-327`.

Historical initial builder at `d0926a2`:

```text
variant: RoutedExactSolIn
payload_len: 25
accounts_len: 17
account[16]: bonding_curve_v2
no routed_buyback_fee_recipient account
```

```text
variant: LegacyBuy
payload_len: 25
accounts_len: 17
account[16]: bonding_curve_v2
no routed_buyback_fee_recipient account
```

Evidence:

- initial code already derived `bonding_curve_v2`:
  `git show d0926a2:off-chain/components/trigger/src/direct_buy_builder.rs`
  lines 172-177.
- initial `LegacyBuy` payload was 25 bytes and included a track-volume flag:
  `git show d0926a2:off-chain/components/trigger/src/direct_buy_builder.rs`
  lines 199-217.
- initial account manifest had 17 accounts and account 16 was
  `bonding_curve_v2`:
  `git show d0926a2:off-chain/components/trigger/src/direct_buy_builder.rs`
  lines 219-238.

Conclusion:

There is no separate clean 12-account legacy BUY builder recovered from repo
history. The recovered working BUY builder family is `DirectBuyBuilder`, and it
has been extended/changed over time:

- old initial layout: 17 instruction accounts, BCV2 at index 16,
- current layout: 18 instruction accounts, BCV2 at index 16, buyback recipient
  at index 17,
- current P3.7 transaction-level manifests may show 21 account keys because
  compute budget, ATA and tip instructions add transaction-level accounts beyond
  the Pump.fun buy instruction accounts.

## Manifest Parity Matrix

| Axis | Working Helius BUY/SELL path | Current P3.7 path | Parity verdict |
| --- | --- | --- | --- |
| BUY builder function | `TriggerComponent::create_buy_build_profile()` -> `DirectBuyBuilder` -> v0 `buy_tx` -> Helius Sender | same request builder family, but under P3.7 route resolver / selected fallback handoff | PARTIAL |
| BUY sender boundary | `LiveTxSender::send_transaction(&request.buy_tx)` and confirmation | `RpcShadowSimulator::simulate_buy(&request.rpc_buy_tx)` | DIVERGED |
| BUY route variant | working live path likely default `RoutedExactSolIn` unless overrides supplied; the live artifact does not log Ghost's own `buy_variant` directly | primary `RoutedExactSolIn`; fallback may rebuild as `LegacyBuy`; E5/E6 now remove unsupported legacy fallback | DIVERGED |
| BUY discriminator | current routed discriminator `38fc74089edfcd5f`; current legacy discriminator `66063d1201daebea`; historical builder also had both variants | same constants when builder reached, but selected legacy fallback can be semantically non-executable | PARTIAL |
| BUY payload | historical initial `LegacyBuy` had 25 bytes; current `LegacyBuy` has 24 bytes; current routed has 25 bytes | current builder only; no clean true-legacy ABI | DIVERGED for legacy label |
| BUY instruction account metas | historical initial 17 accounts with BCV2; current 18 accounts with BCV2 + buyback recipient | final P3.7 request still contains BCV2 when selected fallback logically clears it | DIVERGED |
| Account source | live path uses prepared overrides + builder + AccountStateCore preflight and Helius landing proof | P3.7 records DIAG/MFS/route_builder/account_state_core/observed tx sources; BCV2 often ends as builder-only/not load-ready | DIVERGED |
| Payer model | configured live keypair, balance preflight, real signature | shadow/probe can use shadow payer / ephemeral semantics, no live submit | DIVERGED |
| ATA handling | live BUY may include idempotent ATA create; live evidence records real token account and token balance after buy | simulation uses `request.user_ata` account return and allows idempotent ATA exception | PARTIAL |
| User volume / track volume | builder includes user volume accumulator and track flag for routed | same builder, but required-account simulation/readiness differs by route diagnostics | PARTIAL |
| Compute budget | live BUY/SELL prepend compute budget and priority fee; live priority fee can be estimated against Sender path | P3.7 simulates `rpc_buy_tx` with configured RPC simulation settings | PARTIAL |
| Slippage / amount encoding | live first BUY amount `100000` lamports, min tokens persisted in PostBuySubmitted; SELL min output calculated from canonical price | P3.7 probe amount is config override and token param is resolved for simulation route | DIVERGED |
| SELL builder | `SellTxBuilder` via `PostBuyRuntime` live lane | no equivalent SELL execution in P3.7 probe; only probe lifecycle labels if simulated BUY succeeds | DIVERGED |
| Confirmation / lifecycle | BUY signature confirmed, PostBuySubmitted live, SELL built/submitted/confirmed, realized exit | shadow/probe transport/entry/lifecycle logs only; no Helius landing proof | DIVERGED |

## Account Manifest Comparison

Working/current `DirectBuyBuilder` Pump.fun BUY instruction accounts:

```text
0  global
1  fee_recipient
2  mint
3  bonding_curve
4  associated_bonding_curve
5  user_ata
6  payer signer
7  system_program
8  token_program
9  creator_vault
10 event_authority
11 pump_program
12 global_volume_accumulator
13 user_volume_accumulator
14 fee_config
15 fee_program
16 bonding_curve_v2
17 routed_buyback_fee_recipient      # current only; absent in d0926a2 initial builder
```

Working SELL `SellTxBuilder` Pump.fun instruction accounts:

```text
0  global_state
1  pump_fee_recipient
2  mint
3  bonding_curve
4  associated_bonding_curve
5  associated_user
6  user signer
7  system_program
8  creator_vault
9  token_program
10 event_authority
11 pump_program
12 fee_config
13 fee_program
14 user_volume_accumulator           # only if cashback enabled
last bonding_curve_v2
```

Current P3.7 selected route manifest problem:

```text
selected fallback route: legacy_buy
logical override: bonding_curve_v2 = None
final builder manifest: still contains bonding_curve_v2
E4R3S enforcement: selected_route_handoff_mismatch
execution unlock: 0 rows
```

This is not just "missing context". The P3.7 path can build a request through
the same `DirectBuyBuilder` family, but its route resolver can select a route
that the builder cannot physically represent as clean legacy, and its terminal
transport is RPC simulation rather than Helius Sender.

## Golden Evidence Extract

Recovered from repo artifacts:

```text
run_id: 20260409-123806
start_iso: 2026-04-09T12:38:06Z
wallet_pubkey: 9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw
buy_confirmed_count: 7
sell_confirmed_count: 7
realized_exit_count: 7
```

First BUY:

```text
mint: 8ex24x4XVnFBc8krzif7A1xWm1zvhikGoJXGP3egpump
signature: TsXUKkGaLREDt4inf3jk2JivWEMDs8nVW6b5Rdi7CFVjjyVHTSL7r1aCEUzheGjbYrPCQV8iWnvup7X1VGfNdqC
submit_slot: 412071427
landed_slot: 412071428
confirm_source: signature_status
amount_lamports: 100000
tip_lamports: 1000000
priority_fee_micro_lamports: 2000000
sender_status: submitted via Helius Sender, confirmed
```

First SELL:

```text
pool: BRcRtCUBD361VL9uyG6FoXVsjDByLhXmCmbJ9HkJxFry
mint: 8ex24x4XVnFBc8krzif7A1xWm1zvhikGoJXGP3egpump
buy_signature: TsXUKkGaLREDt4inf3jk2JivWEMDs8nVW6b5Rdi7CFVjjyVHTSL7r1aCEUzheGjbYrPCQV8iWnvup7X1VGfNdqC
exit_signature: 4be4TqiuocZU1wCYY8NnfHLjG3ST7187UvuR5sTSwSgG7Q5RahKQoWayqVv5Jj6mbDF8cqo5pe9kPG3qMqau6uTg
submit_slot: 412071398
landed_slot: 412071432
confirm_source: balance_delta
tokens_sold_raw: 1545600207
token_account: 732Qo5b48rNSQKxVYa4LjQ7a1Mn1HwmJd2zCut1jUBT9
token_program: Token-2022
sender_status: submitted via Helius Sender, confirmed full exit
```

Program/discriminator/account-order status:

- Pump.fun program id in current builder: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
  via `DirectBuyBuilder::pump_program_id()`.
- Current routed BUY discriminator: `38fc74089edfcd5f`.
- Current legacy BUY discriminator: `66063d1201daebea`.
- Current SELL discriminator: `33e685a4017f83ad`, recovered from
  `SellTxBuilder` constants/code.
- Account order for own working Ghost BUY/SELL is code-derived, not raw
  artifact-derived, because the artifacts record Sender/landing telemetry but
  not full submitted transaction instruction dumps for Ghost's own signatures.

## Why P3.7 Does Not Reproduce The Working Path

1. Different terminal transport.

   Working path sends `VersionedTransaction` through Helius Sender and confirms
   through Yellowstone/signature-status/balance-delta evidence. P3.7 uses RPC
   simulation on `request.rpc_buy_tx`.

2. Different route arbitration layer.

   Working live path prepares and submits the request under TriggerComponent live
   semantics. P3.7 inserts `p37_shadow_probe_route_resolution_diagnostics()` and
   selected fallback handoff before simulation.

3. `legacy_buy` label is not a clean executable legacy builder.

   Current `DirectBuyBuilder` can select `LegacyBuy` discriminator/payload, but
   final accounts still include `bonding_curve_v2`. E5A/E5B close this as
   unsupported for fallback execution.

4. P3.7 can claim logical fallback but then fail final manifest parity.

   E4R3S proves selected legacy handoff claimed rows, but zero validated rows and
   final manifest still containing BCV2. That safety gate is correct but confirms
   divergence from a working executable sender path.

5. Working SELL path is not part of P3.7 execution parity.

   The recovered working execution asset includes post-buy live SELL through
   `SellTxBuilder` and Helius Sender. P3.7 probe can only produce simulated/probe
   lifecycle labels if BUY simulation succeeds; it does not exercise the
   Sender-backed live exit machinery.

## Acceptance Mapping

1. working buy path call graph albo jawny brak artefaktow:
   PASS -- working BUY path found and mapped.

2. working sell path call graph albo jawny brak artefaktow:
   PASS -- working SELL path found and mapped.

3. current P3.7 execution path call graph:
   PASS -- P3.7 probe/shadow path mapped from selection to RPC simulation and
   lifecycle labels.

4. porownanie builderow i account manifests:
   PASS -- current/historical `DirectBuyBuilder`, current selected route
   manifest, and `SellTxBuilder` manifests compared.

5. decyzja z listy czterech werdyktow:
   PASS -- `WORKING_BUILDER_FOUND_AND_CURRENT_PATH_DIVERGED`.

6. konkretny next step:
   PASS -- `P3.7-X2 -- Rebind P3.7 shadow/probe execution to working Helius
   builder path`.

## Residual Uncertainty

- Repo artifacts prove working live BUY/SELL signatures and Sender landing, but
  do not include full raw transaction manifests for Ghost's own submitted BUY and
  SELL. Account order/discriminator parity is therefore code/history-derived.
- The first live BUY artifact does not explicitly log Ghost's own `buy_variant`.
  Default `RoutedExactSolIn` is inferred from TriggerComponent/DirectBuyBuilder
  code when no override is supplied. This inference should be validated in X2 by
  binding P3.7 to the working builder path and emitting the selected builder
  manifest before simulation.
- The historical initial builder already used BCV2; no clean pre-BCV2 legacy
  buy asset was recovered from repo history.

## Explicit Non-Decisions

- This report does not claim current Pump.fun ABI is fully current.
- This report does not reopen `legacy_buy`.
- This report does not claim BCV2 readiness is solved.
- This report does not authorize P2/live.
- This report does not recommend ABI discovery before X2.

## Final Recommendation

Proceed to:

`P3.7-X2 -- Rebind P3.7 shadow/probe execution to working Helius builder path`

X2 should not implement a new route from zero. It should first make P3.7 use the
working execution asset semantics:

- one canonical prepared buy manifest,
- one builder route actually supported by `DirectBuyBuilder`,
- no logical selected route whose final manifest differs from the selected
  route contract,
- shadow/probe simulation bound to the same transaction manifest that the Helius
  Sender live path would submit,
- explicit diagnostics proving whether divergence is builder/account context or
  only simulation transport.

delegation_trace:
  task_classification: "offline audit / execution path parity"
  routing_performed: true
  primary_specialist: "Solana Execution Path Engineer"
  supporting_specialists_considered:
    - "Ghost Runtime Coordinator"
    - "Decision Logging Replay Analyst"
    - "Gatekeeper Policy Auditor"
    - "Config Rollout Safety Reviewer"
  specialist_docs_loaded:
    - "docs/agents/solana-execution-path-engineer.md"
    - "docs/agents/decision-logging-replay-analyst.md"
    - "docs/agents/ghost-runtime-coordinator.md"
  specialist_docs_not_loaded:
    - name: "Gatekeeper Policy Auditor"
      reason: "Gatekeeper BUY was treated as upstream evidence; no policy behavior was changed."
    - name: "Config Rollout Safety Reviewer"
      reason: "No config/default/threshold change was made; config was inspected only as historical live-run evidence."
    - name: "SSOT Feature Materialization Guardian"
      reason: "MaterializedFeatureSet was checked as a source label in P3.7 diagnostics, but no materialization contract was modified."
  skills_used:
    - "ghost-execution"
    - "solana-pumpfun-architect"
  fast_path_used: false
  contracts_checked:
    - "shadow/live separation"
    - "Helius Sender submit/confirmation boundary"
    - "PreparedBuyRequest rpc_buy_tx vs buy_tx boundary"
    - "DirectBuyBuilder route variant and final account manifest"
    - "selected fallback route final manifest parity"
    - "post-buy live SELL handoff and Sender confirmation"
    - "DecisionLogger/replay evidence boundary as artifact-only, no runtime proof upgrade"
  unresolved_routing_uncertainty:
    - "No raw submitted Ghost transaction manifest was present in repo artifacts; builder manifest was recovered from code/history."
