# RAPORT P3.7-J3J Probe Execution-Account Readiness Coverage

Date: 2026-05-21
Namespace: `shadow-burnin-v3-r27-all-decision-counterfactual-30-30`

Status:

```text
P3.7-J3 execution-account readiness audit: PASS
bounded_wait_recommendation: not_justified_account_never_observed
recommended_next_stage: account_coverage_or_route_identity_investigation
runtime smoke status must be read from the paired smoke/join-key report
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r27-all-decision-counterfactual-30-30.toml`
- probe_selection: `logs/shadow_run/shadow-burnin-v3-r27-all-decision-counterfactual-30-30/probe_selection.jsonl`
- probe_skips: `logs/shadow_run/shadow-burnin-v3-r27-all-decision-counterfactual-30-30/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-r27-all-decision-counterfactual-30-30/decisions`

## Summary

```text
selected_probe_rows = 42
pre_scan_precheck_skip_rows = 146
audited_probe_rows = 188
diagnosed_selected_probe_rows = 0
exact_decision_v3_join_rows = 188
missing_account_roles = {'none': 188}
classifications = {'unknown': 42, 'missing_execution_route_identity': 146}
readiness_latency_classes = {}
wait_would_help_within_1500_ms = 0
recommended_next_stage = account_coverage_or_route_identity_investigation
```

## Readiness Latency

```text
audited_missing_account_rows = 0
observed_before_decision = 0
observed_between_decision_and_probe_selected = 0
observed_after_probe_selected = 0
never_observed_in_run = 0
ready_within_500_ms = 0
ready_within_1000_ms = 0
ready_within_1500_ms = 0
ready_within_3000_ms = 0
```

## Per-Probe Diagnosis

| probe | role | classification | latency class | ready after selected ms | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | ---: | --- | --- | ---: | --- |
| `fafffe9720` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5a6876401c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d86d25ce95` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `bb00810353` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d57c6eb375` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `deb78450c1` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e09d5bd15c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ed31adcdd7` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `b926dfebd6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `aa7b8c6e06` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `2f3650efc4` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f6f097415c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `98abb1a130` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `587738fa8a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e3a170be32` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a72159b8ad` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `26f781a74b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e6e85f1de8` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c023f72b09` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `73c2d5eeaa` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a14c6413c4` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `48f52d6e50` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `4910889536` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0251f29718` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `424b40bcb2` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `1de6068dba` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `eb247b3345` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `b1bbca82be` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `fc31bdb81c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `af7e6e39b7` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `898d2bc1e2` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9c310bb067` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `aa54159c36` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `06998b5145` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `38a61b8627` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `no_executable_route_account_set:primary_route_bcv2_missing:bonding_curve_v2:FH7WTAkK56uAv9hobd695f9wh3gW88gLukV2u79z5CDp` |
| `bed4ae85b6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c825287913` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a16651dd05` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `14d3375156` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `eae3e2fa78` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `df9bad4308` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `cc2aeaff4e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `2d8008743f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e951993126` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `dfa98a7eff` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `5f906a6b4d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ffc358f210` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `15a6e4954d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `5f4b230f38` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1bfe4a7115` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7304ca986e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `27129b68ac` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `2a450e548b` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `63c4ea4ff5` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `8d7c5b02e2` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `9e8f9c4f2f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `cfaa02eebe` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `9c55d43668` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `3b885a78f0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a81d8e8fa8` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `0be6b1218f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `12cd1b9653` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `da47c7f7fd` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a67df9e8ca` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b83e42b3c0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e34f556b91` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7f61a42b11` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `f306284cfb` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c376aab4bf` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `3c855ac1ae` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `3cadca9166` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `da446171f9` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e0ab8006ae` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `196dccf0d0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `0e16b79d2e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `45c5a7b5fd` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c839c34533` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b301cc4d0e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `0b4fc0332e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ecdd17cb3e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a2c0056c14` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `17a5ca67a5` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7fbc9eb5f8` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `af0a054eaa` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `93e976ed59` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c3e196158f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `11af233a0d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `f45946a076` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c60a17bd1a` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d05f677f00` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `193729068d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `bc363d3938` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6251e43e85` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1b5c4ee43f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `edf3f33521` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `db7cb3f40a` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `aef776fc07` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `fb0f531b67` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `28c1be5e93` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `471f0146c8` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `578084226b` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d4c9aaf8d7` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `2f257b79c5` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a9cec039b8` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `760264c728` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b68d9d522d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `4c84d3b035` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `bbcc9736c2` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `22370ec922` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ddcaeb2273` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `392f086a00` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `71bac416dc` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d231adbdcc` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `bbf91cb6ca` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1a008ccc61` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `bed16e831b` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b409742f48` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b830954732` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `38845ced7a` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `8e0b6dd126` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d610d0976c` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c82b73e1c0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `2b8faa28d4` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `28e53ccbcc` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7c720e2da3` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `f622c350ff` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `274489aef9` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `cb4a2e85af` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ccfced88aa` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c21c645c3d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `8620c7bf6f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `803b9a7288` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `19bf8c2d2b` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1bb27e8afe` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `5b98d268e3` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `138ad8ea55` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6031a63cc7` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e3e0f7e137` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `95e94acdc7` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `3b393c032f` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e919870648` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a70605d183` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `48b4cfbf36` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `98cf8f91a6` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7c5fff485d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e0bced60d0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `f0b8676286` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `46354ca7ea` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `eb3b67c21b` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `586a7c77a9` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1bbe6b59c6` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6d93085d7e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c71bc00e7d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `cef5d46219` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `ae48e28724` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `87b7339549` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `acfc56713e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7e441bcf23` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `198d27336c` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `2a8eec8342` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `3db5ce7f33` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `70109b75f7` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `97ab3e06f6` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `6cb8b48b67` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a7aade2dfa` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `8eb5950e6e` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `9c3e5bc3e0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a9ec4c99c8` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `0c03828b04` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `c1db8562e4` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `5532ee8dee` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `855b9f8174` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `820a49de72` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `aa254e27cc` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1b56379285` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `399d8cbf98` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `50785d782c` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e581672713` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `3f9e5ba98a` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `43fad5aa98` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `987a585804` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `e21c9289a3` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1f9cd5fc73` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1ddbd1fec7` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d26ce17bd5` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `148936634a` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `7f4ad003d3` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `480f50d770` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |

## Interpretation

This report is an offline probe-readiness audit. It classifies selected
counterfactual probes and pre-scan skips by exact decision/V3 join status,
required-account role, and explicit precheck reason.

Rows classified as `unknown` in this report are selected probes that were
not stopped by execution-account precheck. They must be interpreted with
the paired probe transport/entry and simulation-error reports.

## Decision

Do not bypass required-account precheck. Do not use this report alone to
start collection.

If `execution_account_not_ready` dominates and no probe transport/entry rows
exist, the next step is account-readiness/materialization work. If transport
and entry rows exist, classify any simulation errors before scaling.

For J3J, bounded wait is justified only when missing execution accounts
are usually first observed after probe selection within the configured
wait window. If accounts are already observed before selection, the
problem is route/materialization coverage rather than runtime latency.
