# P3.7-L1R21 L2 Input Manifest / Dataset Contract Audit

## Verdict

- manifest_status: `pass`
- final_decision: `GO_L2_INPUT_MANIFEST_LOCKED`

## Locked Contract

- allowed_runs: `J4C`, `R16-r1`
- blocked_runs: `R16-r3..R16-r13` and every unknown/unlisted run
- failure_mode: denominator or namespace mismatch is a hard fail
- this is not scoring, threshold tuning, policy promotion, P2/live, or full R16 route-universe approval

## Expected Vs Actual

| field | expected | actual |
| --- | ---: | ---: |
| `buy_quality_denominator_rows` | 85 | 85 |
| `buy_quality_dirty_good` | 4 | 4 |
| `dirty_good_rate` | 0.0471 | 0.0471 |
| `excluded_non_executable_rows` | 3956 | 3956 |
| `excluded_unsupported_route_rows` | 11 | 11 |

## Denominator Gap

- executable_eligible_rows: `87`
- buy_quality_denominator_rows: `85`
- eligible_not_in_buy_quality_denominator: `2`
- meaning: execution eligible does not equal buy-quality label eligible

## Allowed Run Manifests

| namespace | decisions | route_exec | route_non_exec | lifecycle_labels | buy_denominator | bad | dirty_good | good | feature_join_exec_labels | artifacts |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | 2344 | 44 | 1205 | 42 | 42 | 42 | 0 | 0 | 42 | 14 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | 1978 | 43 | 1075 | 43 | 43 | 39 | 4 | 0 | 39 | 14 |

## Input Artifacts

| namespace | role | path | rows | sha256 |
| --- | --- | --- | ---: | --- |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `config` | `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1.toml` | n/a | `76ea2b6ab49c855d4c00f65f15e96348015c433460aee1fb3f6e02686c30b24b` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `decision` | `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_buys.jsonl` | 2 | `7dd5c12ddfee037fe77243b83d3943e09c1acbb6e019dc3fb0e57ef72047d9c8` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `decision` | `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl` | 2075 | `21fc00ab878749235d2880b16ce63475039865a2227ed6f5403f9dbe27545641` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `decision` | `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl` | 267 | `421f360718e6db54999c63f5fd0b5d187f5776ee08f7c685d12d684289a5424d` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `probe_entry` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_shadow_entries.jsonl` | 50 | `d1fa4c503e427f7c157b42683717d9ccce2aa6c89dfebf684c36294de21a650d` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `probe_lifecycle` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_shadow_lifecycle.jsonl` | 84 | `fe38b800fb8cda1c0b00b2e33fba6e695817559d90f7b8ac0f81505c9b02414b` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `probe_selection` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_selection.jsonl` | 151 | `5564d86f3b1f3cb7564a8828b68bc7b315fdbb54f69250b16457a47ec14f63c5` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `probe_skip` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_skips.jsonl` | 2025 | `e21b3a1235df2fd84cfa29cc75778b8ecb7a756c9128f91b83b8b4733f8796a0` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `probe_transport` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/probe_transport.jsonl` | 50 | `4ba6ebce56c1396fc3b4f3de7a1dcf7375218889eeba9381f407c3b385d8ef57` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `shadow_entry` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/shadow_entries.jsonl` | 2 | `a8f449eb04da841976fb2ad2b50f84221f5777c8353fbb7be4eea044229f2fd6` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `shadow_lifecycle` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/shadow_lifecycle.jsonl` | 6 | `c90e5fc43d659207f6c2b659cde8d375d5c4c25f0740248221f42d41e79df027` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `shadow_transport` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/buys.jsonl` | 2 | `76899b50b335d9f9bd10576321fffc782d71677a854920997c8751690189fa64` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `lifecycle_label_file` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/p3_7_probe_shadow_lifecycle_labels.jsonl` | 42 | `f0f985aeee28147ff0df30374bc4e970f619f2e0d13b66295858bde329c1bce1` |
| `shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1` | `feature_availability_file` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1/p3_7_probe_shadow_lifecycle_feature_availability.json` | n/a | `1384c2b7030c2821f7f9b7f0176cda47fb909c231c2ea7dbe284ff8b56938033` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `config` | `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml` | n/a | `d02372a2b6c5fb3c1fbcca26fbd2bbc097accece96c2185017ce6e2befc9beb8` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `decision` | `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_buys.jsonl` | 6 | `87c7caba8cf3f689ac08d987f470843a104c61e3de9564af178a8c4bf18386b6` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `decision` | `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/decisions/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v2.2/legacy_live/00b3d576e6ddfaefe5f738ef016d91e644fe3c67269a7cb058b29e4c75a2087d/gatekeeper_v2_decisions.jsonl` | 1972 | `bbf3a0ee8a06c3d86327c5e35282d5ab09ed12d00e5a9f7d1d4b7bbcbc4f592b` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `probe_entry` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_shadow_entries.jsonl` | 50 | `354067c6c0052141cea039ead489e0c478fc2b24ee4ca6a26e2520f40a88f955` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `probe_lifecycle` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_shadow_lifecycle.jsonl` | 78 | `9de435a683fb2409115364b7a6a75c476db20f562f3217988ee64254485a7b98` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `probe_selection` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_selection.jsonl` | 163 | `a68de911074bcb3bb570b33923c8a3a91465d9af254cddecb0dbe41060f510ac` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `probe_skip` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_skips.jsonl` | 1922 | `da5b07eded60eef76bf71be31ce2a6b53aaf996b3d07b2bc145cfcccd984a418` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `probe_transport` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_transport.jsonl` | 50 | `4a59084ff5b8353c5abdf9bd4b33bd866177bcd2b156b74ea33f78308ffa02e8` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `shadow_entry` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/shadow_entries.jsonl` | 5 | `af0f5a97b218d7d1c2a9d04de846d37daac8cbcdb1e6dc7a9b9dafb14ddf6663` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `shadow_lifecycle` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/shadow_lifecycle.jsonl` | 14 | `a25d33af29e015fa3d6eb398ef239b606c55e96417f43e95f57d728f5530e0f4` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `shadow_transport` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/buys.jsonl` | 6 | `440e73ee7b50946725c1ec55981b1c114da13d6b114bea61dd427dc7e9d36d99` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `lifecycle_label_file` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_p3_7_shadow_lifecycle_labels.jsonl` | 39 | `751d3c4a291f9890f7b33732412edbfa53a97cdea4239b88770fe16ba4a2b2cb` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `lifecycle_label_file` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/active_p3_7_shadow_lifecycle_labels.jsonl` | 4 | `807decbb046134387c9e2bc4ce201f95457a8790df5b5d877e8ac9409695e782` |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1` | `feature_availability_file` | `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_shadow_lifecycle_feature_availability.json` | n/a | `d8b278fcc89c716d7f682d819fb6e932aa113ef3cbab026e92c3869dfb25f5ea` |

## Blocked Run Classes

| namespace | class | decisions | route_non_exec | unsupported_route | buy_denominator |
| --- | --- | ---: | ---: | ---: | ---: |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2` | `excluded_no_buy_quality_denominator` | 850 | 554 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r3` | `hard_blocked_unsupported_route_universe` | 451 | 275 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r4-account-attribution` | `hard_blocked_unsupported_route_universe` | 427 | 300 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r5-candidate-narrowing` | `hard_blocked_unsupported_route_universe` | 641 | 452 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r6-bcv2-contract` | `hard_blocked_unsupported_route_universe` | 934 | 568 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r7-active-shadow-attribution` | `hard_blocked_unsupported_route_universe` | 1159 | 673 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r8-active-shadow-report-attribution` | `hard_blocked_unsupported_route_universe` | 223 | 134 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck` | `hard_blocked_unsupported_route_universe` | 81 | 57 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r10-route-bcv2-source` | `hard_blocked_unsupported_route_universe` | 455 | 332 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r11-bcv2-readiness` | `hard_blocked_unsupported_route_universe` | 416 | 263 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r12-bcv2-provenance` | `hard_blocked_unsupported_route_universe` | 399 | 285 | 0 | 0 |
| `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver` | `hard_blocked_unsupported_route_universe` | 70 | 63 | 11 | 0 |

## Hard Fail Conditions

- `unknown_or_unlisted_run_in_l2_input`
- `denominator_mismatch`
- `dirty_good_count_mismatch`
- `blocked_namespace_in_l2_input`
- `non_executable_route_rows_in_buy_quality_denominator`
