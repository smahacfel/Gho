# MANIFEST P3.7 - Baseline Dataset R10/R11/R13

Data: 20260518

Status: **REGISTERED / TRUTH LAYER INPUT / NO P2 / NO LIVE**

## Executive summary

Ten manifest rejestruje historyczne artefakty R10/R11/R13 jako immutable baseline dla P3.7 Phase A. Nie jest to candidate run, nie jest to zgoda na P2 i nie jest to progowa kalibracja obecnej rodziny V3.

- JSON manifest: `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_dataset_manifest.json`
- Generated at: `2026-05-18T14:09:19Z`
- Git HEAD: `258cb56` / `258cb56ce4bb9e90f613a7079fda53eb678ce19f`
- FSC pozostaje zde-scopeowany zgodnie z ADR-0130.
- Combined-only evidence jest pomocnicze; temporal split pozostaje wymagany w dalszych etapach P3.7.

## Source reports

| Artefact | Exists | SHA-256 | Scope |
| --- | --- | --- | --- |
| `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | R10/R11/R13 combined calibration and embedded baseline replay |
| `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json` | True | `28d4f1273077322afed608d16ca4f72c0d09c4ae540e7e091ae1e628135984a1` | P3.6 feature separation index |
| `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_recent_r11_r13_calibration_report.json` | True | `91310018d6279239274f9ac50124eed2e7f1d84ea57369727bfae388782af5fa` | R11/R13 recent-only calibration |

## Run summary

| Run | Rows | Labels | Replay | Known | Good | Bad | Neutral | Unknown | Protective ratio | Precision | Policy hash |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| R10 | 150 | 150 | full_replay_ok | 136 | 25 | 42 | 69 | 14 | 1.680000 | 0.626866 | `9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c` |
| R11 | 447 | 447 | full_replay_ok | 387 | 91 | 84 | 212 | 60 | 0.923077 | 0.480000 | `9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c` |
| R13 | 2733 | 2733 | full_replay_ok | 2439 | 536 | 556 | 1347 | 294 | 1.037313 | 0.509158 | `d4dd574ae99fe0b2c9edda48caab9b9d756969949a7d820a4b93b5b2f3b4c1cd` |

## Artifact registry

### R10

- Rollout namespace: `shadow-burnin-v3-p32-replay-r10-primary-only`
- Config hash: `eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581`
- Policy hash: `9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c`
- Snapshot hash unique count: `150`
- Replay payload schema versions: `1`

| Artifact | Path | Exists | SHA-256 | Lines | Notes |
| --- | --- | --- | --- | ---: | --- |
| calibration_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | 459041 | combined calibration report |
| config | `configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml` | True | `79f3c91fc75bf7200052b1880902024b9ffdd23e7dec622ee457ab53b68fb768` | 93 |  |
| decision_log | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/shadow-burnin-v3-p32-replay-r10-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl` | True | `80ed1bd8ca34f346879b230e64d520e7d4d212f8b07222a5d8a8a0294a744880` | 150 |  |
| decision_log_dir | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions` | True | `missing` |  | decision log root directory; directory path is hashed through contained files separately |
| feature_separation_index | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json` | True | `28d4f1273077322afed608d16ca4f72c0d09c4ae540e7e091ae1e628135984a1` | 89 |  |
| feature_separation_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/good_vs_bad_r10/comparison_summary.json` | True | `118a6d3b5bc5a1a968aa8799538fcbf0f7045ef2dbd20425357c223e8d516cdf` | 1260 |  |
| label_v1 | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl` | True | `5b3c73f13dd161f00669373ed9b77ac2d7117d5226a725276b0aba959b93a128` | 150 |  |
| recent_calibration_report | `missing` | False | `missing` |  | R10 excluded from recent-only R11/R13 report |
| replay_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | 459041 | per-run strict replay summary embedded in combined report |
| shadow_entry_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r10-primary-only/shadow_entries.jsonl` | False | `missing` |  | shadow entry log path missing or file absent |
| shadow_lifecycle_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r10-primary-only/shadow_lifecycle.jsonl` | False | `missing` |  | shadow lifecycle log path missing or file absent |
| threshold_hits | `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_5_pool_threshold_hits_20260516T201245Z.jsonl` | True | `bdf01b16e18cc0a34edadddb99db3196a4d82619397daf3812ef513d1c94f4d3` | 150 |  |

### R11

- Rollout namespace: `shadow-burnin-v3-p32-replay-r11-primary-only`
- Config hash: `eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581`
- Policy hash: `9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c`
- Snapshot hash unique count: `447`
- Replay payload schema versions: `1`

| Artifact | Path | Exists | SHA-256 | Lines | Notes |
| --- | --- | --- | --- | ---: | --- |
| calibration_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | 459041 | combined calibration report |
| config | `configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml` | True | `c6f30178a2a9b0da9cae2e2d35bc5f8e001deacb03cc9e7ea07bb29adca89f66` | 93 |  |
| decision_log | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/shadow-burnin-v3-p32-replay-r11-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl` | True | `bb2a23d8fe4c7bd877afcdd42edb4ba1d62b51cd7208ff5e4de35847173e5de0` | 447 |  |
| decision_log_dir | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions` | True | `missing` |  | decision log root directory; directory path is hashed through contained files separately |
| feature_separation_index | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json` | True | `28d4f1273077322afed608d16ca4f72c0d09c4ae540e7e091ae1e628135984a1` | 89 |  |
| feature_separation_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/good_vs_bad_r11/comparison_summary.json` | True | `97043eda36547015f11853b4680d80f931620b30c0311ab3dc80a18c550c6d33` | 1261 |  |
| label_v1 | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_gatekeeper_plus40_labels.jsonl` | True | `cfae786d960389ad178bd29c9a6cdcdd88436297b58731456141c831cc26a133` | 447 |  |
| recent_calibration_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_recent_r11_r13_calibration_report.json` | True | `91310018d6279239274f9ac50124eed2e7f1d84ea57369727bfae388782af5fa` | 437161 |  |
| replay_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | 459041 | per-run strict replay summary embedded in combined report |
| shadow_entry_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r11-primary-only/shadow_entries.jsonl` | False | `missing` |  | shadow entry log path missing or file absent |
| shadow_lifecycle_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r11-primary-only/shadow_lifecycle.jsonl` | False | `missing` |  | shadow lifecycle log path missing or file absent |
| threshold_hits | `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_5_pool_threshold_hits_20260516T231727Z.jsonl` | True | `dbe009ebd0be1141c31e5ad3ef42339e85ffcaba6a52542a632519ba400f3d00` | 447 |  |

### R13

- Rollout namespace: `shadow-burnin-v3-p36-sample-r13-primary-only`
- Config hash: `eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581`
- Policy hash: `d4dd574ae99fe0b2c9edda48caab9b9d756969949a7d820a4b93b5b2f3b4c1cd`
- Snapshot hash unique count: `2733`
- Replay payload schema versions: `1`

| Artifact | Path | Exists | SHA-256 | Lines | Notes |
| --- | --- | --- | --- | ---: | --- |
| calibration_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | 459041 | combined calibration report |
| config | `configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml` | True | `8580e8d658dd79f83c2752b70bb1a675c615737bbf9219148e445788c8c87f5d` | 94 |  |
| decision_log | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/shadow-burnin-v3-p36-sample-r13-primary-only/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl` | True | `da650121ea84801adf6e73b9954824f9484fa5101aeb47ee886fcc8a269480e0` | 2733 |  |
| decision_log_dir | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions` | True | `missing` |  | decision log root directory; directory path is hashed through contained files separately |
| feature_separation_index | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/feature_separation_index.json` | True | `28d4f1273077322afed608d16ca4f72c0d09c4ae540e7e091ae1e628135984a1` | 89 |  |
| feature_separation_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/feature_separation_p36_r10_r11_r13/good_vs_bad_r13/comparison_summary.json` | True | `bc6695168721b846ebb436df144c9c0d2fe62389f06cbc590ce2d1639b28e9bc` | 1260 |  |
| label_v1 | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_6_r13_gatekeeper_plus40_labels.jsonl` | True | `f7bc25fe1f221f03413c877046ed8e903abe38905938896ec06bc7fd562a6b5b` | 2733 |  |
| recent_calibration_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_recent_r11_r13_calibration_report.json` | True | `91310018d6279239274f9ac50124eed2e7f1d84ea57369727bfae388782af5fa` | 437161 |  |
| replay_report | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_6_combined_r10_r11_r13_calibration_report.json` | True | `835e0a012aa963e85b8facd54d4fb31775462dc1e242a864dce40b7ab2fa1a0a` | 459041 | per-run strict replay summary embedded in combined report |
| shadow_entry_log | `logs/shadow_run/shadow-burnin-v3-p36-sample-r13-primary-only/shadow_entries.jsonl` | True | `43e6914efd0dd8627a7e7d945374a344f5b4eaf8edd9171235391f207b868a2e` | 1 |  |
| shadow_lifecycle_log | `logs/shadow_run/shadow-burnin-v3-p36-sample-r13-primary-only/shadow_lifecycle.jsonl` | True | `73f2c5487915a6a80cf235501d3857dbeb16dc7e3cebd78b793cd1e7f42d2c05` | 1 |  |
| threshold_hits | `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_6_r13_pool_threshold_hits_20260518T105304Z.jsonl` | True | `d3df515208d524ffec34a0e0245b955d1646fd93d2344efa7fe365a0524d3de1` | 2733 |  |

## Governance and immutability

- R10/R11/R13 sa historycznymi artefaktami i nie moga byc przepisywane przez P3.7.
- R13 jest sample expansion, nie candidate run.
- Ten manifest nie dowodzi edge; rejestruje tylko wejscia do dalszego truth layer.
- Outcome Label v2 i execution feasibility join musza dzialac addytywnie obok label v1.
- Threshold recommendations z legacy analyzer pozostaja appendix-only.
- P3.7 candidate wymaga temporal split i pozniejszego pre-registered holdout.

## Missing or scoped artifacts

| Run | Artifact | Path | Reason |
| --- | --- | --- | --- |
| R10 | recent_calibration_report | `missing` | R10 excluded from recent-only R11/R13 report |
| R10 | shadow_entry_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r10-primary-only/shadow_entries.jsonl` | shadow entry log path missing or file absent |
| R10 | shadow_lifecycle_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r10-primary-only/shadow_lifecycle.jsonl` | shadow lifecycle log path missing or file absent |
| R11 | shadow_entry_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r11-primary-only/shadow_entries.jsonl` | shadow entry log path missing or file absent |
| R11 | shadow_lifecycle_log | `logs/shadow_run/shadow-burnin-v3-p32-replay-r11-primary-only/shadow_lifecycle.jsonl` | shadow lifecycle log path missing or file absent |

## Next step

P3.7.2 Outcome Label v2. Nie przechodzic do feature prototype, dopoki label v2 i execution feasibility join nie rozdziela market outcome od executable opportunity.
