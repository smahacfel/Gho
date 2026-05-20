# RAPORT P3.7-J3H Probe Execution-Account Eligibility

Date: 2026-05-20

Status:

```text
P3.7-J3H account readiness audit: PASS
R15-r6 runtime smoke: NOT_READY_DIAGNOSED if no probe entries were generated
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f/decisions`

## Summary

```text
selected_probe_rows = 30
pre_scan_precheck_skip_rows = 26
audited_probe_rows = 56
diagnosed_selected_probe_rows = 30
exact_decision_v3_join_rows = 56
missing_account_roles = {'bonding_curve_v2': 29, 'creator_vault': 1, 'none': 26}
classifications = {'execution_account_not_ready': 30, 'missing_execution_route_identity': 26}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `93d6ae3bc7` | `bonding_curve_v2` | `execution_account_not_ready` | `7tWqyVwY9HV8LpUueuLxrD8z2vuGCLC4FtjtxsD4jXXQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7tWqyVwY9HV8LpUueuLxrD8z2vuGCLC4FtjtxsD4jXXQ` |
| `a57cafd996` | `bonding_curve_v2` | `execution_account_not_ready` | `3p97wpqGN29MwNMVfRYWKmURH775x79Y3Wdu6A5y4jgX` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3p97wpqGN29MwNMVfRYWKmURH775x79Y3Wdu6A5y4jgX` |
| `8ea6151cec` | `bonding_curve_v2` | `execution_account_not_ready` | `JDZEHvh2MgeAwtTpFCiGF9mKUXuPT2NAipiCDAhFf8F1` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:JDZEHvh2MgeAwtTpFCiGF9mKUXuPT2NAipiCDAhFf8F1` |
| `f95ba0734f` | `bonding_curve_v2` | `execution_account_not_ready` | `2izeb1vpsYD3UFtUjuQv3uMbNDUs8ipvX9oYbcJjLzaw` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2izeb1vpsYD3UFtUjuQv3uMbNDUs8ipvX9oYbcJjLzaw` |
| `538582b3d1` | `bonding_curve_v2` | `execution_account_not_ready` | `FbpMM2nDAZE3UBjPBsxFNXp3FwyTYaMy2vuxWcU5h7vv` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FbpMM2nDAZE3UBjPBsxFNXp3FwyTYaMy2vuxWcU5h7vv` |
| `d129d97898` | `bonding_curve_v2` | `execution_account_not_ready` | `CK5tdybp56JLxJMwpzqRsze5oqt79uqz2H1MwUDZfTxH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CK5tdybp56JLxJMwpzqRsze5oqt79uqz2H1MwUDZfTxH` |
| `b8afcf3bbd` | `bonding_curve_v2` | `execution_account_not_ready` | `Co25e8csiUoGxqmEdqrPJhFUPpKQo2Eo5okSbhMhfY9v` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Co25e8csiUoGxqmEdqrPJhFUPpKQo2Eo5okSbhMhfY9v` |
| `2df1fa6a02` | `bonding_curve_v2` | `execution_account_not_ready` | `2fM7ykvJgJd5YgSqZPDka6NmqHRmbxe16YYPtaDWTDyJ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2fM7ykvJgJd5YgSqZPDka6NmqHRmbxe16YYPtaDWTDyJ` |
| `3b1450594d` | `bonding_curve_v2` | `execution_account_not_ready` | `EDkxS8n1W6sVAX4Dxxsd9kJA6j2ctkxJBqLQVUCA24Tt` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EDkxS8n1W6sVAX4Dxxsd9kJA6j2ctkxJBqLQVUCA24Tt` |
| `a5a1a37f06` | `bonding_curve_v2` | `execution_account_not_ready` | `FhfWxibJrL6PLPuAQthZqmFyRyAeSqCSFRnTe9emKM5J` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FhfWxibJrL6PLPuAQthZqmFyRyAeSqCSFRnTe9emKM5J` |
| `a6b1ff6908` | `bonding_curve_v2` | `execution_account_not_ready` | `3iPZwNG5UjV25jBtpb3wqoR1cAqF4ewEoDAifqRwtxW1` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3iPZwNG5UjV25jBtpb3wqoR1cAqF4ewEoDAifqRwtxW1` |
| `8c0b656f93` | `bonding_curve_v2` | `execution_account_not_ready` | `2SfFNJcb44MG79FaCt2SApqtnds8BMEaQYX6ienR9Cpq` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2SfFNJcb44MG79FaCt2SApqtnds8BMEaQYX6ienR9Cpq` |
| `631b154043` | `bonding_curve_v2` | `execution_account_not_ready` | `GstyvsroVAqsPmupjbs9SgmmHDWAe5b2Wiet4ADw9nK7` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GstyvsroVAqsPmupjbs9SgmmHDWAe5b2Wiet4ADw9nK7` |
| `e9e882e0a8` | `creator_vault` | `execution_account_not_ready` | `CXfvNnyi4k4pCU2asV2RsgYHJa5N1pzzFghHx8BKMUB4` | `exact` | 0 | `execution_account_not_ready:creator_vault:CXfvNnyi4k4pCU2asV2RsgYHJa5N1pzzFghHx8BKMUB4` |
| `60731d7e43` | `bonding_curve_v2` | `execution_account_not_ready` | `DUBamDV6eJqXUmYxgbDEe3oLri85sS2gAzu3M5HgefbX` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DUBamDV6eJqXUmYxgbDEe3oLri85sS2gAzu3M5HgefbX` |
| `c2c185b5ed` | `bonding_curve_v2` | `execution_account_not_ready` | `9gnqxCyNQz85RMBtnW5p1gSaMsAP5HTfMAtiQhaWUQrj` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9gnqxCyNQz85RMBtnW5p1gSaMsAP5HTfMAtiQhaWUQrj` |
| `9fa19f0a5e` | `bonding_curve_v2` | `execution_account_not_ready` | `FtbezCz5FzoHJMHgMts2F6WQXcCWyHPv9EFmXKvQdQrC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FtbezCz5FzoHJMHgMts2F6WQXcCWyHPv9EFmXKvQdQrC` |
| `ea16cb13d5` | `bonding_curve_v2` | `execution_account_not_ready` | `BYRZ33vN2cUZPGTsn4FXySLSa6jfPjqMQp68eWT3jsBA` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BYRZ33vN2cUZPGTsn4FXySLSa6jfPjqMQp68eWT3jsBA` |
| `cf3f2d0f1d` | `bonding_curve_v2` | `execution_account_not_ready` | `75ipheDKP8ti8A2Zocd3UTWeEmncjeYcgsapv2pJtzg1` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:75ipheDKP8ti8A2Zocd3UTWeEmncjeYcgsapv2pJtzg1` |
| `018979762e` | `bonding_curve_v2` | `execution_account_not_ready` | `CSHcFVZ8PKTX84Ni1MTA6gC2ur3vLFXGJohcFzCtYXd3` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CSHcFVZ8PKTX84Ni1MTA6gC2ur3vLFXGJohcFzCtYXd3` |
| `a31b34503b` | `bonding_curve_v2` | `execution_account_not_ready` | `C1bakEWweDstBLXYBhX6scR8Srxv1RTTvAcqrKAbBQGW` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:C1bakEWweDstBLXYBhX6scR8Srxv1RTTvAcqrKAbBQGW` |
| `584f71cd66` | `bonding_curve_v2` | `execution_account_not_ready` | `GAYtyhgP1MJNccJCgr5KszBi1WbaTFdhkPSG35qXtzgW` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GAYtyhgP1MJNccJCgr5KszBi1WbaTFdhkPSG35qXtzgW` |
| `279d09c34c` | `bonding_curve_v2` | `execution_account_not_ready` | `BmrVceDDq4LMQ7Vez3SVADvg8iXLRYXgHDJpNc8Ff1Hp` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BmrVceDDq4LMQ7Vez3SVADvg8iXLRYXgHDJpNc8Ff1Hp` |
| `e45695dbfd` | `bonding_curve_v2` | `execution_account_not_ready` | `BAHrnk7D1dmaD1S66vRx422FNWouauCaNNxWWZoYFUbJ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BAHrnk7D1dmaD1S66vRx422FNWouauCaNNxWWZoYFUbJ` |
| `52b363fd02` | `bonding_curve_v2` | `execution_account_not_ready` | `5jaS6hGSeZiXmfodsE4a7Dp1bYLP9PS5nQwq4NDWScb5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5jaS6hGSeZiXmfodsE4a7Dp1bYLP9PS5nQwq4NDWScb5` |
| `b6cbad2f31` | `bonding_curve_v2` | `execution_account_not_ready` | `DGnAQYBCDVarAXo6BnMaQcURpiaj4ZQ3WTZtutZqaMBH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DGnAQYBCDVarAXo6BnMaQcURpiaj4ZQ3WTZtutZqaMBH` |
| `068c9a4db0` | `bonding_curve_v2` | `execution_account_not_ready` | `3k6MNeULPBmjErNDWSBNVNXAKf97HizXGNwa8NAJMHi5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3k6MNeULPBmjErNDWSBNVNXAKf97HizXGNwa8NAJMHi5` |
| `400ae285bb` | `bonding_curve_v2` | `execution_account_not_ready` | `HzaY7AxtY5Jva663rD2De9icmWMkymCrGprHcDXq34fJ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HzaY7AxtY5Jva663rD2De9icmWMkymCrGprHcDXq34fJ` |
| `586f33addf` | `bonding_curve_v2` | `execution_account_not_ready` | `uKTRx1oaYc7d47arqUEiaz9S1cwV7wujvsGVhh6jMgW` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:uKTRx1oaYc7d47arqUEiaz9S1cwV7wujvsGVhh6jMgW` |
| `bcea08b044` | `bonding_curve_v2` | `execution_account_not_ready` | `Emrdr2CzBKybyWdeVB8tMwMzAX3gLXEWS2J93FbZA1q2` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Emrdr2CzBKybyWdeVB8tMwMzAX3gLXEWS2J93FbZA1q2` |
| `136939a4cf` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ccde1564db` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `533a3f41d5` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c45f7aec24` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a868d3a0c3` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `27a84ee362` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b770d057e0` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d62984d7b2` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `9c366c8cd4` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `652a4c893b` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e56ec29c55` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e12243de1c` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `762dbb75a4` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `79fd151355` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ef5cc2a47c` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `dc11066e64` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `236779c661` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `2ec58eec79` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6809be9cee` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `71a32c17c0` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `fed54c4639` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6453986595` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `fe0ea4bac9` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `19dbfc7e87` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6ac45d4928` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `902be29fd8` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |

## Interpretation

The R15-r6 selected probes no longer fail on payer, user-volume, or generic
`transaction_account` handling. They fail on strict routed execution accounts.

For all selected probes, the missing pubkey was present in the prepared
transaction account set and was checked by required-account precheck. The
selected decision rows had V3/MFS snapshots, curve data marked known/ready,
and clean account/curve evidence, but the snapshots do not materialize
`bonding_curve_v2` or `creator_vault` as explicit execution-account fields.

The current classification is therefore:

- runtime state: `override_present_but_account_missing_on_rpc`, because the
  prepared request had a concrete required pubkey and processed RPC/precheck
  did not find the account;
- dataset contract gap: the strict account identities are not explicit V3/MFS
  fields, so future probe eligibility cannot be audited from MFS alone.

## Decision

Do not bypass required-account precheck. Do not start collection.

Recommended next repair path:

```text
P3.7-J3I Probe Execution-Account Materialization Or Eligibility Narrowing
```

J3I should decide whether to add explicit decision-time-safe materialization
for `bonding_curve_v2` and route-specific `creator_vault`, or narrow probe
eligibility to rows where these strict execution accounts are already known
and ready. If the accounts are known but absent on RPC at processed
commitment, the row should remain classified as
`execution_account_not_ready` rather than dispatched.

R15-r6 should only run after a concrete eligibility/materialization fix. It
should not increase probe limits and should not weaken strict precheck.
