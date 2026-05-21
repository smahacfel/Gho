# RAPORT P3.7-J3J Probe Execution-Account Readiness Coverage

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2`

Status:

```text
P3.7-J3 execution-account readiness audit: PASS
bounded_wait_recommendation: not_primary_fix_route_or_materialization_gap
recommended_next_stage: account_coverage_or_route_identity_investigation
runtime smoke status must be read from the paired smoke/join-key report
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2/decisions`

## Summary

```text
selected_probe_rows = 514
pre_scan_precheck_skip_rows = 10
audited_probe_rows = 524
diagnosed_selected_probe_rows = 386
exact_decision_v3_join_rows = 524
missing_account_roles = {'none': 138, 'bonding_curve': 385, 'payer_pubkey': 1}
classifications = {'unknown': 129, 'missing_legacy_bonding_curve': 385, 'missing_execution_route_identity': 10}
readiness_latency_classes = {'observed_before_decision': 385, 'never_observed_in_run': 1}
wait_would_help_within_1500_ms = 0
recommended_next_stage = account_coverage_or_route_identity_investigation
```

## Readiness Latency

```text
audited_missing_account_rows = 386
observed_before_decision = 385
observed_between_decision_and_probe_selected = 0
observed_after_probe_selected = 0
never_observed_in_run = 1
ready_within_500_ms = 385
ready_within_1000_ms = 385
ready_within_1500_ms = 385
ready_within_3000_ms = 385
```

## Per-Probe Diagnosis

| probe | role | classification | latency class | ready after selected ms | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | ---: | --- | --- | ---: | --- |
| `30846bd652` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `b5b05e7663` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `7857c3f741` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `207bc6820d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `de224af2a4` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ff25e5c899` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9ec2c18919` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `50533ab82d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ca29c86682` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 879 | `ADGCJpwm7yzfdb9dEW12WLoWjDeiPdxBtLdW4Tza3fie` | `exact` | 21 | `missing_bonding_curve` |
| `d2f5dda6bb` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4138 | `DQE626WuK8u5apAnMk6KZdYaxCa7k4cuNSWaehvnMXGA` | `exact` | 5 | `missing_bonding_curve` |
| `9d317dfbcc` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 649 | `H7ngXzzrJvGxwBbNkRz2heca2pDVXUJznVCZKkfUfzWP` | `exact` | 39 | `missing_bonding_curve` |
| `24643f4c06` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 55 | `9MaaiJ5Tv64L5Dc5zv9jtp7MFW6ZYpcaBGVZmdL4uexm` | `exact` | 54 | `missing_bonding_curve` |
| `f5e882cc47` | `payer_pubkey` | `unknown` | `never_observed_in_run` |  | `9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw` | `exact` | 0 | `missing_required_account:payer_pubkey:9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw` |
| `4faa7f2fd8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 241 | `8xxs9PDH4CQovoQDzpwpMBmH6cj4wHbD6AnWhwqWTdrB` | `exact` | 36 | `missing_bonding_curve` |
| `9b642d2fe8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 204 | `8ScEzSCtZe2vbEPsTHKKPQ2TEoyjUyuf9b3pLoj4MEh5` | `exact` | 231 | `missing_bonding_curve` |
| `cb22f1b9a3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `DTYcv7rqWkVAVbUbWHTZyZWqDrpY6Wb8481E4uBXcVif` | `exact` | 10 | `missing_bonding_curve` |
| `ac7b557935` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `wcgVKjtuyiPzWw2onwiYD3mzZED8nRB6Jn9ZQ7Ddupr` | `exact` | 2 | `missing_bonding_curve` |
| `f38064ff68` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 114 | `7chsGF4GndiVpcojtq5HhdMMtbH4qzPyPNw18KpU9ctg` | `exact` | 18 | `missing_bonding_curve` |
| `821335ac15` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 526 | `AJLfENBsVKgFa9SzFwddPC81fWqB9P33tuiHDyifM9uJ` | `exact` | 19 | `missing_bonding_curve` |
| `c307b2b43b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 510 | `7y8jjhomyJceUvPxCuaapjdccSVdZaZXzdPfspFUCpNM` | `exact` | 508 | `missing_bonding_curve` |
| `322f7a7fdf` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `9kQvmJX3QQ7BN2CgwvX3rQqpbFDXCcfkAtHZydK5hctp` | `exact` | 6 | `missing_bonding_curve` |
| `23ac7ffc67` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 440 | `God1vNHsbMXgrarhJm5PKPHyEha5nnfreG1EEYMPjgSd` | `exact` | 27 | `missing_bonding_curve` |
| `471d9742b0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 28881 | `8yGDv4aFMWmFpCaKM34umVVcnZ1tuGNDzxSD8MJ2Emg9` | `exact` | 13 | `missing_bonding_curve` |
| `b94246e4ff` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 5325 | `32mSEpFTMuEiJcSyMFhMkAbEEU7b4piBmbj2HMFapwL5` | `exact` | 16 | `missing_bonding_curve` |
| `479dc1af9a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 208 | `DukBnRGhbUwsGBH4bgAttmkmGbr51g4QXREE58pkKG5b` | `exact` | 8 | `missing_bonding_curve` |
| `93cc2452d1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 59459 | `HQaeK7wLGHjXDNQHgyFBa49bEa8u5UGFhyfMTHUffBb7` | `exact` | 7 | `missing_bonding_curve` |
| `6be7c2f24f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 6339 | `AZoLsSXTFqMBQzYbC7Tbb8JFruQf7Vn96LxeGVFba24e` | `exact` | 12 | `missing_bonding_curve` |
| `81011bf615` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `b9c9d9924b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `59a9a06f58` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `11658c326f` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a8fb6c12ae` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3096b2492c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3b20145009` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `613daae8c6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 36382 | `4RbqjgwvDmBPhbULAXwkkLPrgrkziPtdp24bPhewkod4` | `exact` | 4 | `missing_bonding_curve` |
| `471a4f9aa1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 481 | `AAe5FPBosox3pa13hJ3LYbZWYuGU6EKqyBPoLwAVbP9a` | `exact` | 36 | `missing_bonding_curve` |
| `ecad70718c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 987 | `9P7apckamU1hZVV9tNVFhMGDW5ttDwhioZVDyX2aDjdK` | `exact` | 21 | `missing_bonding_curve` |
| `c5d7140731` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4886 | `4rHG7bu1Z9nbgMbRpBVFGxJuQ9iSwUWzGrXAAQsHFM4d` | `exact` | 4 | `missing_bonding_curve` |
| `8dbc228e1d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 253 | `GU79gQLp51jG1bDcDTcKAH5bPy3pzD3iw3cib1UqVjQv` | `exact` | 50 | `missing_bonding_curve` |
| `cd850f643c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 117 | `6pmJb9DrZj6zEKGDQWNzRDxXgPgBGRKLsjM9EWdGjMmb` | `exact` | 178 | `missing_bonding_curve` |
| `c77fb2cf66` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `5ht6miHFAhpyKTUyFpUkPdeqxLg6Gje4Epus6oNcRMGZ` | `exact` | 11 | `missing_bonding_curve` |
| `4879e61e0c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 137 | `9QwN9sTTbqMZwXd33aWMUKVBGxFabvdy1rZ23Knymawr` | `exact` | 158 | `missing_bonding_curve` |
| `8d32e229ef` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 122719 | `G6tV5PYB4gApiMfPdJ9ZWF9mqbyhsnRHAFtnFS3gK5L` | `exact` | 6 | `missing_bonding_curve` |
| `2b009f2f47` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1015 | `AvGnu3Qb1afkrAFxjFJUPj11JHjLDw1jgJiovwYtUrBz` | `exact` | 18 | `missing_bonding_curve` |
| `083b5b24ae` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `8bpvYhSEFrintatrocESHEsyxnc5EeP5Q7VH92B6rnpV` | `exact` | 28 | `missing_bonding_curve` |
| `d59cd80e43` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `3dJtUib98fQYLzeLMCCZUCRS2eVfdc6xw1Syhg5h5HgN` | `exact` | 8 | `missing_bonding_curve` |
| `3491f9fe42` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 245 | `Fbkgn2BDvHequqAyYzgYnJN2qs5ZgU1BzdpyepCMbHC1` | `exact` | 31 | `missing_bonding_curve` |
| `5888bc0777` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 12103 | `HvbtMeV2zqkocqWEko82LfYETSCcpxCs1DyB4NF2ZnSE` | `exact` | 56 | `missing_bonding_curve` |
| `67d3afafd4` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `DgcF3LENmSpbcvqL2sZtmPf5PTxWPx3RexDQorRiCYa4` | `exact` | 2 | `missing_bonding_curve` |
| `bc12e04295` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3655 | `DXxdM4UPAX9HLF5HeFibgcWFbxm3zYKnkZHoCMw5Ckry` | `exact` | 18 | `missing_bonding_curve` |
| `024e22185b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5dbe4156d8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 13373 | `F2Xzq5TTd4vNpecebPJ36pc5K4ypaGRudrHqVtENgTCP` | `exact` | 16 | `missing_bonding_curve` |
| `8e80b22267` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 639 | `CujxpdrSY7Mg3v12wmryF7EaR5Rnjq4KqzEMopDnGp9A` | `exact` | 78 | `missing_bonding_curve` |
| `ab1330363c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 222 | `6HooZdyM9SvwY4iSTjEEmow5rmvKQZuhgTFjTSFVjf5P` | `exact` | 1453 | `missing_bonding_curve` |
| `ccbbecc816` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3776 | `6i63vi8fLCDJ4k53LqTmdi9KPRKvhUFN8JVjcUHB15DB` | `exact` | 7 | `missing_bonding_curve` |
| `b9fa05c230` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d97162497b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `8cd90ae06b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `fc8aa00628` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `bdf5e411b3` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a99b3a169d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6fbc8e7d44` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e92db7c952` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `cbb6620298` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `26665c7610` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5387cd700b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ff6856947d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0c82639f40` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1207 | `AmpPpbp6fpTRc3CXwmXqqFDcyEn6J5LCJERNX91f9Jze` | `exact` | 7 | `missing_bonding_curve` |
| `a5767cc963` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 45635 | `DtzH9h6HG8M9Fac4YgTPCnahik21HagLxjx3ov5Q3V9o` | `exact` | 3 | `missing_bonding_curve` |
| `11b6bf39bb` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 45836 | `CNXbBiBwabbfuieaGz5rUwBbe9oBFWXjnW683TSAkF88` | `exact` | 3 | `missing_bonding_curve` |
| `bd84dc92ee` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 45780 | `61kXp6wcbprTtxJb4BpSZvpjyTT4uG1iggL6Y4oK94wJ` | `exact` | 3 | `missing_bonding_curve` |
| `4c136142fb` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 702 | `7Qg4s6uKE5b3oCHizFtmFHYaHntfuqJzHwsMtFGAm5Q` | `exact` | 18 | `missing_bonding_curve` |
| `1de5e570c7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 160 | `Be4NTF3smMRDTvSpUTx66JHaxq1KcSY65Z3VD1ziixei` | `exact` | 42 | `missing_bonding_curve` |
| `ad6a9c7b71` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 260 | `EYnTbzhVtb5oVWvFYyWRMLZuMgXHZePR5yLgufhyz7x` | `exact` | 24 | `missing_bonding_curve` |
| `c5afe92ca2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 744 | `4KzTWytkC7qLEyTiWkUwzpqyj9QxJ6bJ97HNY4MPStiX` | `exact` | 33 | `missing_bonding_curve` |
| `8f6cfcc0b3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 17209 | `ATiivAWdpz8Sg8JxRNZaJskGAAA25ZUMfxjtax2bzsRk` | `exact` | 11 | `missing_bonding_curve` |
| `0e87f548f2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 41529 | `J6i8vJ9m3DGpssrQ4ShdENN2g2whj4nXYSxEx4CJ5AgH` | `exact` | 3 | `missing_bonding_curve` |
| `4e5edc404c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 41660 | `GMLwWMqVCBXja9kTTzXagRrznFahcEwtoB584dv3a86b` | `exact` | 3 | `missing_bonding_curve` |
| `527cf94ab6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 169268 | `JDKQL46yDFrr1VZhTbKoUEBE4m2sF2sTeA8oJRA6aYpZ` | `exact` | 3 | `missing_bonding_curve` |
| `c485578387` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 137 | `5VFYTDZ6TKbJizaouLfnfevYogbUfnwQYHeA26TqLQ6E` | `exact` | 54 | `missing_bonding_curve` |
| `0a15bf50a5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 42476 | `F5yrB2gXUtQmdJugKeMJUBPY89i136fh6EqWSFvTaGbq` | `exact` | 3 | `missing_bonding_curve` |
| `d324a2ae2d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10149 | `2DE7TRRb9DQHNVFEGboo145BDzo7QjPXpEfoN1YzWQYm` | `exact` | 15 | `missing_bonding_curve` |
| `912b67eafe` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 39242 | `DVaiA5SDP8fB4Hz1dft8i29p2NBKiV42TeLuvd915fN8` | `exact` | 14 | `missing_bonding_curve` |
| `06ac12166d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 964 | `Fu9bhPkyeKQ3859DPL7QutmXCq4TaLPv9F3TxkW1opVN` | `exact` | 499 | `missing_bonding_curve` |
| `32797ccb68` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1831 | `9RzbTt4ZhbiEzXHtFrAJUUawTuu9srHMXkVqFtR1bYLr` | `exact` | 8 | `missing_bonding_curve` |
| `5bc64276e6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4885 | `HdzXiRJMXBS9emiLEquRJbkREZUBzaepRWBd1AvANvV9` | `exact` | 33 | `missing_bonding_curve` |
| `16a6523dbb` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 245 | `7k3fp27j9mRetYfAuneqpYbL7wYHfzHV2V2DFJr4N6en` | `exact` | 50 | `missing_bonding_curve` |
| `77d977153a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 332 | `5cSgqNby7DPZriLsZGLkfBxMYBuNwFEguyRhWqywtxjk` | `exact` | 31 | `missing_bonding_curve` |
| `c35dfa8710` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 46 | `FtGmvneqkxoWCS5qUUuqRLwsmzzicWpVUC2vJxcfEnDh` | `exact` | 80 | `missing_bonding_curve` |
| `4e824a908c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 6319 | `HpSFm4oUXhyJiyxcyUQwry3N5fwpwkvPSTsjRsZvbKZL` | `exact` | 24 | `missing_bonding_curve` |
| `ba9ace0a3c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4348 | `7cB18RDMuQ54U2FhiLTRjP1UQ8EfqNrMKxDRbx3guHgL` | `exact` | 16 | `missing_bonding_curve` |
| `1088cb2427` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1273 | `27MWSSzcUdRq8qFnL31M7cNZn4wtxenXWD5ujHt4Paaa` | `exact` | 36 | `missing_bonding_curve` |
| `8be81309be` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 226 | `Ca1CfFL7bFjd1Gmb1A6bdSA4pJetBVxeJVARf8S4g1DK` | `exact` | 21 | `missing_bonding_curve` |
| `ce2dc71d6b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 11461 | `FjuVmWmjqrE5shJkwrpXgX7hNoc1s9THytf1K5er4GGo` | `exact` | 12 | `missing_bonding_curve` |
| `d5ae8fe282` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `8od4X9mdxeVLVniZ4EGL5Ntv46prbiMoxP4gaRhgtkzh` | `exact` | 4 | `missing_bonding_curve` |
| `c299648bfe` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 24627 | `D8qzbQSARSgMrmBu5d5kYvg5xFqtyrjHn9KEcTmntEJN` | `exact` | 7 | `missing_bonding_curve` |
| `3eb178d867` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 12021 | `FMfX6zbiFNmwXyj6jTWtcHJ9sZxBCB4xkeGnNwELToGL` | `exact` | 2 | `missing_bonding_curve` |
| `27940c6d2b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 260422 | `BzT9TmysH3g9EqojzVgcfR7kPYy9M8ybHk4BBZKKQCVE` | `exact` | 4 | `missing_bonding_curve` |
| `d249f632a7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 61038 | `5rJTen5QWjrrYyLrqH6R8nDWbV7ns1C8jVF5hBjm93Wr` | `exact` | 3 | `missing_bonding_curve` |
| `5c9c6dcfc1` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `387bb8fc66` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `bc4cf56342` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 64062 | `2sFxspruVJ5YeSqs1JFBWPFLJrLVZSHrAWqtFyUrK9xY` | `exact` | 5 | `missing_bonding_curve` |
| `14f6e1dccb` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `12f2345131` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `80539f5e1a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 607 | `B6XB2RTvpn17XWNjz2jcT4jATyZ3D6V7cJveET4FCXrM` | `exact` | 56 | `missing_bonding_curve` |
| `4bc0285b68` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 328 | `7qdppePGz7MpMtRV5mWAXnF6nHRqk6kuk6RuJAkYkUjx` | `exact` | 10 | `missing_bonding_curve` |
| `c1ca0ef817` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 607 | `6zdapEDEsnbUyebgKoUWLbKiuwyEo6vsBodvQJJErYFC` | `exact` | 48 | `missing_bonding_curve` |
| `dbdd3c989f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 8543 | `5q6wdYt1kvixno5V2A56Q5gD5F87r5C7chtznrLoCk91` | `exact` | 25 | `missing_bonding_curve` |
| `1303fac462` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50673 | `FtM7E9wppgqxhJ4i6gkzy4xxfKVGsiG6zpXVkzRC4URs` | `exact` | 3 | `missing_bonding_curve` |
| `9e42c01b84` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50680 | `GZa1S4exBR6p4cPbZoSUbS9mRwTHovayrZLnZcFd7omZ` | `exact` | 3 | `missing_bonding_curve` |
| `d516a7423d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0bdcd718a5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `H4TkgLD45BRpeX1wtkmXHJn4nuxVu1jWPaf3YrCThGgy` | `exact` | 2 | `missing_bonding_curve` |
| `343bc88d81` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50477 | `3aTRfjbTE6anFruMmDQsJmeWugeAU7caZ2oUeKFVjxvm` | `exact` | 3 | `missing_bonding_curve` |
| `245146a44e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `17e413f46b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `20cc16848e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `52aa21b06c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51009 | `xsxmsuey7UWXD9scuar7Vsq9KrnAh9DrmLfd4CdxRRc` | `exact` | 3 | `missing_bonding_curve` |
| `167f3fce43` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50580 | `D11mH1oW7kNNJ1aLBaGLy56Z6c3dgefn5uZarx3Q8yPr` | `exact` | 3 | `missing_bonding_curve` |
| `bccff954a7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 685 | `36DV99CuuFnrMcDGYvbAxAbdnTFyynaPcCoe7NT8w69b` | `exact` | 50 | `missing_bonding_curve` |
| `e9d71f38aa` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `2R4PmcX1usMaWkbmd5rTH7Umshn1Q5jjQGvNfSLdRZW4` | `exact` | 7 | `missing_bonding_curve` |
| `4fa15f9399` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 556 | `2UuksVSGzEvgzaDaDrM2YwewuEknaNSuC2NucsiAcNyn` | `exact` | 60 | `missing_bonding_curve` |
| `0d86b38594` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50392 | `86JK71nujc2ibczapMqKNzNY4Tqqnx26ab2DXwwG7Yxd` | `exact` | 3 | `missing_bonding_curve` |
| `091f90d60e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50643 | `H3syw6Pu7qW4kUVPLbQ2UcdSmzYGSYGhBGtTxkfT2ymg` | `exact` | 4 | `missing_bonding_curve` |
| `5982572e8f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51116 | `6txmUFkREpqbMq4pTi2bhXdGBP9urabZ1v47RTftXz4g` | `exact` | 8 | `missing_bonding_curve` |
| `d74fd55ab1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 116950 | `BXjZ6xhBm6hu7YfaxLRdp7WbmWokuY6JrdoaCeETVjLr` | `exact` | 4 | `missing_bonding_curve` |
| `356d253bf2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50586 | `3Us1meonTJGuiDS4PRyphHHGqdCr4ThMRYfDctSb4gdh` | `exact` | 3 | `missing_bonding_curve` |
| `28980c6dc8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50583 | `GNPV4oJ8NcxbbvLyLMTMRMBJxc9xpqQZKUjZBHDTUdhT` | `exact` | 3 | `missing_bonding_curve` |
| `ab3196dc13` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4466 | `75gyuj9Xf3btHsFe8xZX23RCwG8KxbRxwZE9LwEJ2w4K` | `exact` | 36 | `missing_bonding_curve` |
| `2068089104` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10618 | `9PVxTEwitsDfoddtCbZ7spumoM4wTQdAC3H4K2j8bBXF` | `exact` | 6 | `missing_bonding_curve` |
| `e9367d7a63` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2070 | `4Wt8bD8GbtNxJ7JM78FrkUFENBUxKDnjh5NiBnqMRtRb` | `exact` | 156 | `missing_bonding_curve` |
| `9a9e7a99f3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3188 | `22RCTLg1J8o7P2aP6RokvyihADFW9WfuPHGhAUnUqKXo` | `exact` | 114 | `missing_bonding_curve` |
| `e2b230b869` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `5fuFz5YkGzmGWbV9nN2ueKy7fDydG4za8psutPjyL2Ts` | `exact` | 2 | `missing_bonding_curve` |
| `68adc5eac9` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 430 | `3X4s5An6hgWeJFFnaD2wdVoGiqdRejPdvxK8sj26fWC7` | `exact` | 40 | `missing_bonding_curve` |
| `5f937f358c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 53568 | `6xyyJjJmPC9nF4vGN428w2nQ3W3UiZtaQDzVu6VWKzcj` | `exact` | 3 | `missing_bonding_curve` |
| `1f1cbe2a27` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50549 | `8qxiPevXTdCpHzqbDYwcn5mYidwcBm15TN8G4EJnK2qh` | `exact` | 6 | `missing_bonding_curve` |
| `300871ca28` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 52200 | `7KrqoaTPZHUREbv9QjGbTNSrCRq5KoTkc4fEuedEWrDr` | `exact` | 3 | `missing_bonding_curve` |
| `7ee3638e34` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 964 | `9UfBtbJDATQoKpp1bWEdSGNp4KdzL3SeWZCwkxEBb2Pt` | `exact` | 37 | `missing_bonding_curve` |
| `341a8c4bf3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2570 | `G2Hwvb6txjJC8hPvQiaUawyz8adjruRBgChtZRkZAJUE` | `exact` | 72 | `missing_bonding_curve` |
| `80dab06166` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 48197 | `8j1bZMLzqEeTZWQaVYTWnMM3cGkFWps93V386eEZeF1u` | `exact` | 8 | `missing_bonding_curve` |
| `e91cda8223` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 76925 | `CZA4aMTo9CQBBqiGD88xp2taQo3ArLNmqNbvyX871pX1` | `exact` | 3 | `missing_bonding_curve` |
| `560385a923` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f1b6acbe13` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e56e7bdbc6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `eedf195488` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ab6003ea00` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `04e1451dc1` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c859d9763e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `28a4b12b8d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `76cfb6af43` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0cae923815` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `24d16a2977` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 131908 | `2sX5vZC9THVEZEYCTC7cWbuxevpDYsWd6b5qaGgx8m9w` | `exact` | 7 | `missing_bonding_curve` |
| `1b85dbf1de` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 179 | `2m9j3rGVVWcn9Vg6NcWpzEsYXfrmDUcBCq1f2uf42Zjq` | `exact` | 67 | `missing_bonding_curve` |
| `88dc35f7c7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `46kxcPrVMqUMHpqoK5N5FSddF8jnagqd6omPAZphZEn3` | `exact` | 2 | `missing_bonding_curve` |
| `482386df3b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1498 | `EhRvjpU23wKtgZxFd31khDCtigXchyJ7P5KK8ykBhvVQ` | `exact` | 13 | `missing_bonding_curve` |
| `46994c49c4` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `EcJKv2r7hya5CWrZXphQkPVsgquJH8vhH1cYPhAN8EA7` | `exact` | 2 | `missing_bonding_curve` |
| `6b7c802605` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `At2emKqp8PzKQocztbNjSaYfrkmvnTFp6HnvngMqbuCR` | `exact` | 4 | `missing_bonding_curve` |
| `b12c0c8429` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2570 | `BT6nV7xqPH2Vap359AeL8JYmBReMt5iaoG1rRwkofExQ` | `exact` | 8 | `missing_bonding_curve` |
| `8efd76e867` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1662 | `61R19HqUgKxg9QP78jNUomwTnMe4WYf8mfZGjJSL7mL` | `exact` | 9 | `missing_bonding_curve` |
| `29fde34a11` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `2XKjS5Xtu68AxEi8BMScZS9LDpTFFaGht3RXPGunobPJ` | `exact` | 10 | `missing_bonding_curve` |
| `8a1898cf5a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 479 | `H4CBtTPsnhHsYJG4vdsboV44pThKUFowJQnbHkAsLCo5` | `exact` | 312 | `missing_bonding_curve` |
| `8b971fbaf9` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 166 | `5HyBSCCvtNLAsHYc42mww3B53zpXXdu7NZsZEnshaKX7` | `exact` | 11 | `missing_bonding_curve` |
| `49f469cd61` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 31072 | `7Wqo2c8KijgfBvnKhTcvro6CgAQQywzzSQx4NfXHdnaf` | `exact` | 6 | `missing_bonding_curve` |
| `fa0361c543` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1996 | `AQ6ZBc6cvPG1EjEowgQBL4M5o8MHoLZGtPqLyc3ncENS` | `exact` | 27 | `missing_bonding_curve` |
| `f8262ca3f9` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 42681 | `FS7F59Kynpt2eFH6fFREnfYkPoySZXgZHvj7YMSzVvRw` | `exact` | 4 | `missing_bonding_curve` |
| `ab9d76baf7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 162434 | `6eSd998fX23AtGpnUsUBpt8iXKbsisD2p9atLExGsv6T` | `exact` | 3 | `missing_bonding_curve` |
| `5eb46d23a8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 224 | `9jocWue5hdMucKbWZqjkkMC3w44jnjztXo2enK7W1tnx` | `exact` | 643 | `missing_bonding_curve` |
| `6ce3a0a6b0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 452 | `Hu41eypwSqq5SJsQyiy1AvdFANZTtKmDxsGYFLjRg8Z2` | `exact` | 44 | `missing_bonding_curve` |
| `90071b934a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1851 | `87xqBqTpnBVV1AzD7HkhLhnZwbaTYKNVQwqR5K1wm3H1` | `exact` | 33 | `missing_bonding_curve` |
| `d43994048d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 611 | `zjMTqkJzGEAgsdmf7TGNbf1EUdW6B5u19P332i5YRis` | `exact` | 692 | `missing_bonding_curve` |
| `808ca554b7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 33602 | `GLtbziQugCD6jqCMuGTYXTNezMSHEBbSYBfK2PkLYbri` | `exact` | 7 | `missing_bonding_curve` |
| `90f027bf20` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50928 | `EYZqGEPg8GuXoxdzRqtio5o2AguqpMd6iuXR8Eom5FG7` | `exact` | 14 | `missing_bonding_curve` |
| `54c3ac000f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3961 | `EVMf55auEN3H1owJNfPS65qoRHbxHUuw63mncpUoPU5v` | `exact` | 6 | `missing_bonding_curve` |
| `5d0177c7ae` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 78 | `EbQCQpxvzTHoTFa3SES1MZDPmkA1BF9Lsh6AgGULJtPh` | `exact` | 27 | `missing_bonding_curve` |
| `b863d7d372` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `55339b7fe8` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c55b29675b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0ec9396283` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `eb45b0e2ae` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `7e349effba` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `2380b8af3a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0023c9b340` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 52 | `7MuQNyuwMCrXi7hmytTxLk5Wp7Znk6zC2jmxctzeUYNJ` | `exact` | 32 | `missing_bonding_curve` |
| `f9cba41d5d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1157 | `FNRC2vTdLLJjJ1NE9uS7uvEUN8R9h9uZL8FgVNyebnjg` | `exact` | 19 | `missing_bonding_curve` |
| `6d37625af2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 656 | `6vYPEijUEAYrEsteFvbL4xfKv3rxGTnHXp6nfmYBqLVW` | `exact` | 47 | `missing_bonding_curve` |
| `861e547929` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 642 | `GwWwGjajBaCywfFGVsrPYzqjeMKAJfn3ezkSrJJFcTxF` | `exact` | 53 | `missing_bonding_curve` |
| `bf404b82b0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 120787 | `GpVT6g4421Kva2VWz5TjvizcAs6dEDMEkam8r14mhgFB` | `exact` | 10 | `missing_bonding_curve` |
| `a791ae8b88` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 49537 | `xPhgsTZxu26ruEN1ThmicNigrw5qy3F7NbJ3mVK59FW` | `exact` | 3 | `missing_bonding_curve` |
| `7ef08ee6af` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 13135 | `4CgYG3osTcPiFkvcAeSPMfS3vEWZhsK2cinGzzCpEo5o` | `exact` | 37 | `missing_bonding_curve` |
| `ab053ec04a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 35311 | `8nwniaakVPG1ka3KxtxVM3jRmLubBZM2hH8bprKDcy53` | `exact` | 5 | `missing_bonding_curve` |
| `ae9b58ecc2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 248542 | `Ck2Axte1BUp5irn1ydPAhJgAACxLVc9z7svg5GheGyCh` | `exact` | 5 | `missing_bonding_curve` |
| `a7e2573da5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 600 | `FGoJLPe6fyRaENFjYRbztGzKjRfbiLXZu73Cfv3VsPaa` | `exact` | 31 | `missing_bonding_curve` |
| `a0be88c473` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 56291 | `GC7sjG5WJ3SVDbSQ7yQ6vsQapfs7nu7yN5Gdbz9FPLB8` | `exact` | 2 | `missing_bonding_curve` |
| `ee41ac2d68` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `CSviGEsDggMvmp6A2fRxb97gnhLrvErf2VLMnFq9HAE5` | `exact` | 11 | `missing_bonding_curve` |
| `cf5422b093` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 384 | `73NG25wDiqDGJFwHxCwPLCbvYroijwvjyUxhav3rmKnd` | `exact` | 62 | `missing_bonding_curve` |
| `ba2f0162eb` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 662 | `62ipiJZWzPyAiw3eQHrHPu2GdSXDmuehbsgV1yMyEJDi` | `exact` | 43 | `missing_bonding_curve` |
| `af52f7c9b4` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `FJk58xftwkWDE1a9EvcQBPaNF3kZtLBZQFBhiV248qtk` | `exact` | 2 | `missing_bonding_curve` |
| `7b49611ae2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51925 | `9JSTsPVU3U89R6sdWCHXKuhqoTtyQ3dDjaNDrjXpmEU1` | `exact` | 3 | `missing_bonding_curve` |
| `b9148ed53a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 312 | `HqC4pWYcZZWRjqZE7aWCPRoSxxPh7NuQs8jd4YfxNsMZ` | `exact` | 69 | `missing_bonding_curve` |
| `739f63af02` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a48127a96d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 53064 | `4R4g9vpR8njt8QfNwDG5B2hU4NyaXroaxGa4cer59Pmr` | `exact` | 4 | `missing_bonding_curve` |
| `c3170d729f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 14124 | `FFGiNcyqWj6V3mhzEtPhQWuj9MmqSG3M1eeQmqVp8gMm` | `exact` | 4 | `missing_bonding_curve` |
| `3c5d973b33` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 102 | `4dCHXvpUJUjMbm5fjC9aQzeJtinUq8dGP7Vr8xR6tHDX` | `exact` | 47 | `missing_bonding_curve` |
| `ef29f68cf4` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 821 | `2EqNHU5KLkRh5KBTc36mjW9wWSrb5TZnf4uXwiG83NZx` | `exact` | 38 | `missing_bonding_curve` |
| `201ccdb2c8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1053 | `GiwL5LU75xbz18cg3Q6KJFft11wUf7Dfz5KAeckYbGi9` | `exact` | 31 | `missing_bonding_curve` |
| `9701151279` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 816 | `89Rd3KB4o2Jvkm9QyA1ABwq5e383e8g97eMSm7qsYb26` | `exact` | 548 | `missing_bonding_curve` |
| `c3f649005e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d58ca242b8` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `358e115fcc` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d2d3eb2200` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `85370c1ec9` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a5aefaf6cc` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `035a35b1b8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50915 | `1cQ22zWpTrMe9dAWRoTMMYwco9MtJZPgY1VTC5cm1AV` | `exact` | 4 | `missing_bonding_curve` |
| `f29da31901` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1526 | `9aCx5AU1cuhk434WSZ69ezf414JVe1poHz17z2z5x526` | `exact` | 65 | `missing_bonding_curve` |
| `2cc20feaf0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51353 | `87zRhNX3mPCwD9vsqNc24rw8hC7gUanJww1LJszWkMq6` | `exact` | 18 | `missing_bonding_curve` |
| `4c7fa692aa` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3098 | `A3nLJJcqyRpTLvaYjwVwVd6vjwGktwMNDK893HHVdTVa` | `exact` | 14 | `missing_bonding_curve` |
| `8c29025582` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 24845 | `8yQ7EyGnJoRAK6tTWe4crTST9MGgYAKT8j6yyEKPYYc2` | `exact` | 3 | `missing_bonding_curve` |
| `3b4d3fcd30` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2221 | `BSzeXKPfZr8L7c6PaLm6pP9adkNvLWYGBos8SumnMw8V` | `exact` | 13 | `missing_bonding_curve` |
| `cf498b8f67` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51365 | `A74gUmmNU85HhHPjp9qMaqMMxwpVhZd6hAGhYVCF7gCf` | `exact` | 4 | `missing_bonding_curve` |
| `c1237a21dd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50826 | `4i3Hm9CVcEsDJsAKweEudjZsP2TDGEHa2H32KRPXPbLD` | `exact` | 3 | `missing_bonding_curve` |
| `da3fbb617c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 120 | `7JqHMBVzZk6u9qR11Tmc2fBv7sQEDJTNW8vcMQHB7K5v` | `exact` | 611 | `missing_bonding_curve` |
| `0695d86151` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 670 | `BXKTfD25Jqk26yzzWK5KsnHUtSq7vJTbcPRos5NrJMim` | `exact` | 26 | `missing_bonding_curve` |
| `0bfed99e16` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50111 | `RL6G7H8ZUzvVGWJjk4b9iZm8hmfBK6FMF3ZXxKSs4rC` | `exact` | 6 | `missing_bonding_curve` |
| `a4e5bcb3c7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 19 | `GshcscpRxubBVy4JbZRNucUEp3Jxi6F9xYMsZcCECSkt` | `exact` | 28 | `missing_bonding_curve` |
| `12e2c4f8ae` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 275 | `5E8Ns9pBatLzWuNdLtS1nipKTPrDmtMmg7qTpZEnTBLd` | `exact` | 59 | `missing_bonding_curve` |
| `5d97a222ce` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 19036 | `HeyXoSe86CkfAysX2pvgUnXobapjXgtatad4XLtbZa7D` | `exact` | 61 | `missing_bonding_curve` |
| `b632888416` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 36223 | `D4keeGtzJC9qiYfkra5F49rssDABnhodMVEs4gnHGjbV` | `exact` | 36 | `missing_bonding_curve` |
| `c36d9dbdc6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 39834 | `5boqPF1yG7PCP4WWVNMo7rQFmwuy1EhydR9mA4StaV3h` | `exact` | 12 | `missing_bonding_curve` |
| `592fd5a866` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 314 | `4vsNaHidc8yKEb1eeNcNpQuS1ejVcTk71FmoVkSrs1gg` | `exact` | 15 | `missing_bonding_curve` |
| `271b321406` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 326 | `4U3GyCqD3mQdbYhsqNhBRymRujADz56mHBnjP4PcPs6t` | `exact` | 7 | `missing_bonding_curve` |
| `9b10eac49d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `8026439776` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 56168 | `3fmtCjbgpLECxLMHvb54W7PYqGPN5BM683vyFyZjGqe8` | `exact` | 4 | `missing_bonding_curve` |
| `070d15bca0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3482 | `DAX2DQqAmzY67JxGyc1muqzBy9VAPSobZRSPKTEqSgNB` | `exact` | 61 | `missing_bonding_curve` |
| `ac0fe49888` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 392 | `9kfJwUGRhuLhkD61hSd5ZKsTiEXM7Sw5s2PnbGo5tAeb` | `exact` | 33 | `missing_bonding_curve` |
| `e339854c93` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5da9f0e92e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 6619 | `HzrBR4Su6hWFaLCXjrRVm5H5gNS18NrFgMvCfxgtHLAs` | `exact` | 10 | `missing_bonding_curve` |
| `3728b59784` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 237 | `FU4tyTJTFhPEZ3nZw7ga7QKN73d2PT67xNJVBsW8E8XC` | `exact` | 121 | `missing_bonding_curve` |
| `31d3b115a3` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `85b42afac7` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `00c9438849` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `67be686522` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1132 | `2XXK5YbCf9UtmBC7qDzpoCGdYUkUb9B4RdPrVFGsfzEt` | `exact` | 30 | `missing_bonding_curve` |
| `419ad13fe6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0e11d81ae5` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `dd156111a7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 781 | `DAD5yJ9iiGVpLYrB8NEftjvAd8ybM62Rck9zENQDbQAy` | `exact` | 20 | `missing_bonding_curve` |
| `00618e4243` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1715 | `DDAH5svJ7htC1ds7NHtUxSmFkqwCrmnhi6qZQ2xyBa9X` | `exact` | 55 | `missing_bonding_curve` |
| `6129bf02d0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 432 | `6aV1s34bYAFhUM18RKeoEiAANAqVFhAJaxkrEx9C6RVe` | `exact` | 190 | `missing_bonding_curve` |
| `ae3a693c77` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4278 | `CPEEX5RRZjBcWMeNFoETkvCkVv2rmy6gS7CGThKv93mr` | `exact` | 36 | `missing_bonding_curve` |
| `ea4d6f21bd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2206 | `E8xw8XuqpTKpJ44ihTfGijL1n4HA5RvMKPdJPfqb69M4` | `exact` | 6 | `missing_bonding_curve` |
| `35095a2e08` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 261 | `xUppAp32Uib6vZdvEn7M5gUx3FPHTMePhwvY5sdxS1Z` | `exact` | 11 | `missing_bonding_curve` |
| `43f44bb271` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 411 | `Cim7DqfQztsSAheYP3ocZYEKHNof1fp7NMAW1BgTAiyo` | `exact` | 12 | `missing_bonding_curve` |
| `a3ea8474fe` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1444 | `E4MuV1DSLqMgqfCSqXnGaU33fRkcbwDCZgPE3YFjh4y9` | `exact` | 15 | `missing_bonding_curve` |
| `1c37bdca32` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 710 | `8cAkkig4LgX1LYzMp4o9neP5uePKMmwRnuJighnXpCi6` | `exact` | 31 | `missing_bonding_curve` |
| `71ca4ad394` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2866 | `Cf1TdPNfg64iyeeeDyD2P3JmBCWdaUhioVau6wxswj33` | `exact` | 23 | `missing_bonding_curve` |
| `856603f321` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `46850a037a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 23423 | `8NHMYtnwMjL28TsuXtxV3aen8YxwH6weUV3dJc8KZz5X` | `exact` | 10 | `missing_bonding_curve` |
| `df97557bdd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 7485 | `GfLykYtrJN6CJAuBhNmaNwA2wsZSq18swFKz6VC7Cz9K` | `exact` | 14 | `missing_bonding_curve` |
| `a0f0d87604` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 869 | `EL86f8qFtKTau2b69XxSJNkTYCUf36pUE8ET89UDXvzx` | `exact` | 54 | `missing_bonding_curve` |
| `d90dc9477f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 149 | `DXrpTM1YQp1V6mwzyGhjeefXPhH8E99xoxnFfcXmE2Ar` | `exact` | 96 | `missing_bonding_curve` |
| `e23ee74431` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 886 | `9gECymFXZRAsqeGpLTwNS6k9MN6uj1T9TUL89awAASC2` | `exact` | 55 | `missing_bonding_curve` |
| `3f82e87eae` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 9003 | `4sk5qdd7LujUKT1MpfxKa811QY2aEWovffRb3QsA537m` | `exact` | 8 | `missing_bonding_curve` |
| `07fac6e1e0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 143672 | `DzQ36cno4T21dRkQJQeXgaA8idDtrNMPMzUmYoiRXG1x` | `exact` | 2 | `missing_bonding_curve` |
| `94f9db36e2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 5222 | `8ueBXLk6H7nzNvExGb9UHHgz1mqU1ApU4wGwFgEBufbB` | `exact` | 57 | `missing_bonding_curve` |
| `9592527fe0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `Fnkk7QFwtstLT493uNw837566XyKeVY97sAypdaB5rDA` | `exact` | 12 | `missing_bonding_curve` |
| `d27ef608a2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 31 | `HKRTWi4zjDeGyPBWrZLiNuKm2t7f3YngmbHksEVw7aqh` | `exact` | 317 | `missing_bonding_curve` |
| `b5757c650d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 139 | `C136GqJDtCiabugPmxaTtMJMapLf1i2SA3JKRpyYyewj` | `exact` | 148 | `missing_bonding_curve` |
| `addfa8ba6b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `7e3096fe4f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2580 | `AsZz9E7WRkw8LN6dCjz1JeTQytV5ryG3tNFVomxRKDhP` | `exact` | 19 | `missing_bonding_curve` |
| `53d18124d8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 20764 | `ERq5pdtNrJBb5VbSX2R7LqySa3b4ys9iWSxXtKBHgRKF` | `exact` | 14 | `missing_bonding_curve` |
| `9d43cfbec5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 366 | `53tmKuxHvRnmSK3haTA9C4RW1M5o34BkGNoUPTg5wheQ` | `exact` | 92 | `missing_bonding_curve` |
| `07380c2af7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 128 | `GXNCWhiqZVkdUXynozQFnGXsH2gia4v9tTuo3GZTKW6Q` | `exact` | 2187 | `missing_bonding_curve` |
| `073254aa11` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 869 | `B8493NsjbBkzGgJKY685ThvKwJPDoo9VwvLvrRSpos5A` | `exact` | 21 | `missing_bonding_curve` |
| `366e799268` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ca6226a030` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f7d6659581` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `7fa241c079` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `aab6489707` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `cb673a4da2` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ceb47ef1ed` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 55456 | `G1o94tMUJtH3AFqiwuhTiE4XQad8wGqpoDPKPLwgfa3o` | `exact` | 3 | `missing_bonding_curve` |
| `b11d408c57` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 55448 | `AA6DWNVtgyL6YyStWZ6YcaPQw1wCcv95vUDwGpZzri6u` | `exact` | 3 | `missing_bonding_curve` |
| `9125321798` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 52797 | `5C41AZaAw1fgiDP3oxdzcqa9M3ZoNVwn2weDt7kZ3mGj` | `exact` | 3 | `missing_bonding_curve` |
| `24f39b95c8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 718 | `5buWiASXBWXUPGW9Q2YZ9dsf762sCWVw7iRAMcRodfre` | `exact` | 42 | `missing_bonding_curve` |
| `d02251171d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `DVMERWLvyLo9kicTPNCN6ef4PhSAvPTkCXkLoNpBLXHE` | `exact` | 17 | `missing_bonding_curve` |
| `d69e0c43eb` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2147 | `Hc4h71wYTiX3eE7cACNus15my5Af3gFwyt4CwqzAiM6B` | `exact` | 9 | `missing_bonding_curve` |
| `a7010e9266` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 36460 | `8jo2miMZ9KA5HD5fSNUC3aR3n9Sm6uTRm2gGjCRbmRLe` | `exact` | 6 | `missing_bonding_curve` |
| `d0c037bd37` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 364 | `8VdV4Brb5o9DW5vPD8dMzKNCPu8bTd4JxwvkAM8Pkxam` | `exact` | 152 | `missing_bonding_curve` |
| `66a308b3e6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50444 | `7RfbzfR2f88AQXh6d2S9FarowiMNyXU1B5X6tM6SK2CA` | `exact` | 3 | `missing_bonding_curve` |
| `f76710a867` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 804 | `8VUKzT4vKKPc4cdnkFGb7bo6wSs6TpDGcYmpwWPhiK62` | `exact` | 61 | `missing_bonding_curve` |
| `7618cc55bd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50946 | `9sj3z8kobsnLaZxXbuNjg367hXr6D71MRdK9h1LsoTJG` | `exact` | 3 | `missing_bonding_curve` |
| `ef58308668` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2650 | `37BcXm4GZNbMmqqMTQ81iJjAfQrdGGG38EzyJ63WiFJY` | `exact` | 14 | `missing_bonding_curve` |
| `4a551b7415` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1212 | `D8FkFCiaLaB8ebaJ6mCoQG1WBEzsw5n7NAVRRNfnXJxj` | `exact` | 155 | `missing_bonding_curve` |
| `e2a5fdc6c6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `5PXgQM63VAJJcQhVHfu3A7uhNL73ifjLXM4zxaxbB5v3` | `exact` | 2 | `missing_bonding_curve` |
| `5da6fda83f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50856 | `6nH8y9cPVuyfQY5peF72kf8e6RnhKSjDDiprLhtuah6K` | `exact` | 3 | `missing_bonding_curve` |
| `8f340fe871` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50849 | `DwyBospXzmviDxboeAwHqJUEY3MdtfvR1Rg2cnk2MHTd` | `exact` | 3 | `missing_bonding_curve` |
| `6e3a5d6152` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 57003 | `4ztZsaMoQPyuEwDxtvGTpRxoM9YZD2ixgGotc9JbUrLZ` | `exact` | 4 | `missing_bonding_curve` |
| `b75d09420a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 56378 | `H86zemBbX3rxJuTBbTn43cnZitKp8JWfTG592d9zkFTQ` | `exact` | 3 | `missing_bonding_curve` |
| `a5a310dcc8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 196170 | `DdP2CSg7tPEhDZjLbKkHBHuu3kGkJ6NNzAnMKzoxGnTa` | `exact` | 3 | `missing_bonding_curve` |
| `e1b6d8fe51` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 55 | `4xXVBaH23thMxJg9W3EiSZ1EE5chNVz12K9fJswkqoWC` | `exact` | 72 | `missing_bonding_curve` |
| `4032e817dd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50366 | `4GZUpMEV3pR6s4wsR8iJsgwV4fL2yhuVKveeEjmPToHK` | `exact` | 4 | `missing_bonding_curve` |
| `9758a1ee93` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50834 | `7vBARADc7wiw8nuq5NMgV2UPCWae78x2FU91WjxAu2wJ` | `exact` | 3 | `missing_bonding_curve` |
| `fde2bc7528` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50835 | `BKP9WamJrJ5MsPrBkBSq7DDqgkJ3J5h8MQjBS6Mf53iG` | `exact` | 3 | `missing_bonding_curve` |
| `38b54979da` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `141cd9f0c1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51258 | `GCiMWNb26mTdYGHrPcrcA3WDTZskVSBnczGqM2PMWUQy` | `exact` | 15 | `missing_bonding_curve` |
| `c21bb065bd` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9ef22a0b85` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 127689 | `59Af8FeQeJ99LbURiWNqjrPXkpXpNPJuujWFF56w7zUL` | `exact` | 9 | `missing_bonding_curve` |
| `f2e3d1d717` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 138294 | `CRiBLc6G4tHWaCeGSy6e3PeDdZkYc4qHSCzT4549NDEo` | `exact` | 4 | `missing_bonding_curve` |
| `0d506a69be` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 124 | `1aTtjnnjmEZ5PmmN15sGjrTmHHAGwUuDuTsa7F6b7iu` | `exact` | 53 | `missing_bonding_curve` |
| `01cac129f6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 55433 | `ARprG1fWnywQ5pAtHDe1AVwdEtfxczTiXo3eVvgWXiKG` | `exact` | 3 | `missing_bonding_curve` |
| `fddd3731a0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 14551 | `Hayf187CK6c8PW6qnNo52wpwYq2dYaS1gEvdx5FFaQrN` | `exact` | 12 | `missing_bonding_curve` |
| `89dbcce081` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3be61d7b9c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 594 | `BB42A2YTNJbrsqHmxYAiJXjQijy9LfjsYcXScraJtvKs` | `exact` | 133 | `missing_bonding_curve` |
| `5976df0b5e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50369 | `4rrZsRLWqXGvD6XDEfFHWiCZ8E6feB3vJmSm5NdEu1Ve` | `exact` | 3 | `missing_bonding_curve` |
| `50189c4603` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 91606 | `B9EvyDJN1ooxt3B93CdxhWwtTv87BKn5U3mNybs8x3Pn` | `exact` | 2 | `missing_bonding_curve` |
| `938759de57` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `86i6LajEpAfdbxTLUr3FT2cK3DYU4pumCPmFdMX8rWv1` | `exact` | 4 | `missing_bonding_curve` |
| `3270b65cd0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 50467 | `64H15qhtoMQoowNmPCMM5vicLCGoM6zHPYM6FS45iUW8` | `exact` | 3 | `missing_bonding_curve` |
| `1062eac77a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 54423 | `4mVSJ1UAgKhbRyUDCLmSJLeuwjVxdPitTatgiFYmHvrM` | `exact` | 4 | `missing_bonding_curve` |
| `dd217e525a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51520 | `9L8X3CDJHuEQAoEonCmuv4Md195fxgYCVq23UBmGHo2m` | `exact` | 3 | `missing_bonding_curve` |
| `8455e2e14e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `155e5ad547` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6766c4442c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3938 | `GVUL8vFWS5KwdsdT7bb94RhDmhSAMo7KBFtRe2vxxY67` | `exact` | 4 | `missing_bonding_curve` |
| `4929060c51` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 211 | `B6hi9U5QuKMdU9QEQrUVFM7efgkJvChEa8w835QCPFzq` | `exact` | 148 | `missing_bonding_curve` |
| `6d0fea9d1d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `2d344c029c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e812017b97` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f6be87d530` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 14610 | `6QW8DnLVf8xYzFEY6FJJZEuAeiNbk2mUuFM8xm9yh9zK` | `exact` | 3 | `missing_bonding_curve` |
| `6a8f8f6c2c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 242 | `87gtLvK91gT4YXqK33uc2f8SPxRTkUMn27phuLvFADqX` | `exact` | 28 | `missing_bonding_curve` |
| `628bc31474` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2794 | `7Z535knn8diYG1zRU2BsVqRQN782UNah7ykabTn5ZKtr` | `exact` | 4 | `missing_bonding_curve` |
| `c4d54f44bc` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 359 | `YhqWtwyXHe1TBATmiJa3ywaYvLREJobN5agPVY1FYgj` | `exact` | 19 | `missing_bonding_curve` |
| `b7b75b4978` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3cdf386829` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 601 | `4z4Dd5kqyNKvWEnV7W6RY6dxbHwFwnPuC4DRgyfeugo1` | `exact` | 149 | `missing_bonding_curve` |
| `28d2639820` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4360 | `J4EiPpXBSSVWcGgNrKapyZUEKH2ivApSBk2Vinc5Pcq` | `exact` | 5 | `missing_bonding_curve` |
| `faab15f4d1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 6705 | `Haj14znFwPwnJKC1rRbYhNhneq4ivqVjdPkHVuiVmKYq` | `exact` | 6 | `missing_bonding_curve` |
| `7da02c7f44` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2040 | `2URBaNNTsidJg7Y95t9Z9ToMqL2eFhUPNujWXEabZJ3n` | `exact` | 11 | `missing_bonding_curve` |
| `8a05389c92` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 8235 | `6JrZBL1duiW7iUZXn8TLRYdr9S118YxG8qYiyBtiLBxM` | `exact` | 11 | `missing_bonding_curve` |
| `0d314fe9a1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10113 | `G2WtZXsZSgkw54mS3h2T7csTBLZMdzsvvL7mhbvGRGg6` | `exact` | 5 | `missing_bonding_curve` |
| `3afe5d3926` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `919c03ea40` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 280304 | `H4X3vHMpBjXWjtzU2RgcVsgQo6iM1gfp9crGH28YArMM` | `exact` | 4 | `missing_bonding_curve` |
| `edec895e76` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 23644 | `BUi2RsZfYZ9TJYfh6WmT1e7oPFAdvbFUpyNGkTbLPHEj` | `exact` | 2 | `missing_bonding_curve` |
| `44ca725fa6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10593 | `9Wc2tSxTHFGCzaCyP8FXnxFbYnCc1uahqhrHDDUXRJUz` | `exact` | 3 | `missing_bonding_curve` |
| `f49c1458bd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 271 | `FdcKVgfT2jdMmGmnZ8SfMzqabSg9aUTBwoxpPHJZs3jr` | `exact` | 135 | `missing_bonding_curve` |
| `3606c5806e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ccdc12a54a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 60944 | `2KLBGMJWSsofp8VHiF6FZVkTxiUPrGLcLYsRZ6w1gmo1` | `exact` | 7 | `missing_bonding_curve` |
| `f309692dea` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 18017 | `GbMKdaP2rYPrd47hPvhMk4qm1V4GW8QRtxrEzn1D6UDE` | `exact` | 10 | `missing_bonding_curve` |
| `68f2ea1e53` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 276 | `8vhyQmCkYiZLrBjsjc5L4h7dv6p8xu4eAFXiKDuyfFVp` | `exact` | 44 | `missing_bonding_curve` |
| `757b964e8a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `Av2HnxeEZdPkncf6XrrTVw5KSnKZe6PA5fqWz7aG9Pff` | `exact` | 4 | `missing_bonding_curve` |
| `fedeeac687` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 197 | `DRpAUaC4V4n47KtGk9P71WWWxcaoD5MNrFSFGPmspD6Z` | `exact` | 31 | `missing_bonding_curve` |
| `b448e75746` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 9037 | `Dm1dcSxsRq3nmsA1cNVtsCCAFqzdsZJ9qaoyWkuDh1C1` | `exact` | 3 | `missing_bonding_curve` |
| `6b668882da` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 17285 | `3ZEVWKxSmxTb1yG36DPCp3iDNNNzZehpGexs1E6LRUuN` | `exact` | 10 | `missing_bonding_curve` |
| `975abb944d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 16334 | `DLMo75hf6A7AUv9JWWhyThHY1mGqrCQDQtq8jfPSRbrV` | `exact` | 6 | `missing_bonding_curve` |
| `141f70fd6f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 23841 | `Edffc8TXTuzVe42S5b3M7JK2BMr37AVdXSdB9f1BcyPp` | `exact` | 11 | `missing_bonding_curve` |
| `4026381e44` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e14d6bcf61` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 816 | `6ARhvAAkt6LQFKQdKpJeTmAiNuTRNAjR7n7BUGrWEMN8` | `exact` | 29 | `missing_bonding_curve` |
| `3eade2b4ae` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `69e13aa3dd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 7972 | `7pYmhoNKfCWGJ2Y4nWCge4HeqgcHWG7h48arguyo3PUa` | `exact` | 13 | `missing_bonding_curve` |
| `2be9e1b005` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 364 | `Ggjw71U3LD7NYVe9BEpdg6bTbhibVkSzopd3R5h93jub` | `exact` | 268 | `missing_bonding_curve` |
| `3d2dc008d0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 352 | `Az142UAQYmENaZ2pRenhrUcJnYtAu5VFLabVeBBpqPpN` | `exact` | 85 | `missing_bonding_curve` |
| `94adaa956f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `H9BpRqWrYedrmU4B4XKJT9ReXUxnQkJMrpU1sRURosrD` | `exact` | 2 | `missing_bonding_curve` |
| `bfe82f8e50` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1490 | `DwikuzBMDmrGJ63LsHkXq6H2fXcVHhBsHxxbxwhQxZyq` | `exact` | 14 | `missing_bonding_curve` |
| `5c4aa8403b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 364459 | `8cdbPa61EWsWrvhb6TAm4YJxEd5dkb1gTBMnD6RY7sdV` | `exact` | 2 | `missing_bonding_curve` |
| `ecc151feb0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4939 | `E9XvJGPDKHfdqKBBNPUkzhqAHSKM3Tq6EKUMyAGy2QYm` | `exact` | 4 | `missing_bonding_curve` |
| `e933bdc27a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `1db3839f11` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a09b0a6f06` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 405 | `QU5sXEBmptRye9Ac14YkS3U7BZRY4jvHyiDYyBaU6as` | `exact` | 420 | `missing_bonding_curve` |
| `14eb2d73f8` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6013c76533` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 84 | `A1CCNahpdg8WkMipNMyWQvLZoiP19kj56d8pMpFv3APt` | `exact` | 150 | `missing_bonding_curve` |
| `9f8202047c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 196 | `7KRmeUDHTypabu9aiVYaApbRjp6w42VaWnfCfaboMa2G` | `exact` | 24 | `missing_bonding_curve` |
| `f0dd6378e1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 33 | `77hGmeRdCkDYmRQNPmhbNoYzJJdAqLc39C7NMZaTuYbx` | `exact` | 22 | `missing_bonding_curve` |
| `9e20aad6df` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2241 | `8JPQ4uQesGsX9YaKKdfdUt4bfiM9XfpgjHLZvn1DbwuD` | `exact` | 24 | `missing_bonding_curve` |
| `b092ce93c0` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `b7b94c86d2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4278 | `7P95PiheXXLb9rxQuKfnUoGg1CYAvKCjMYKwU7jsaKGf` | `exact` | 9 | `missing_bonding_curve` |
| `bc29dcd4e5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 452 | `5F98BALyA7pNgdYtWwgZxwTWWX9Vt4myAeTnKUywMEaN` | `exact` | 67 | `missing_bonding_curve` |
| `3ad76ccf11` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `HUpAXkPyQDxJqc2LTzxv22HveH5dTK3MD78jMMFUERb5` | `exact` | 10 | `missing_bonding_curve` |
| `29c3e2b143` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3106 | `Ddu6evmbJ9GMMwgbfLk7ew5V4ht3DzvG3BNpb2dnJrpu` | `exact` | 31 | `missing_bonding_curve` |
| `4be4c86407` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1621 | `8W7YaMqWbzLvJCnZMiN2whe1smDwBWWWPGtg7SAPNQ9K` | `exact` | 24 | `missing_bonding_curve` |
| `7689081154` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `DSwdxK4iQkF5JSGwc66dVBYnCoYEE1svNqCBt7YTKFoJ` | `exact` | 7 | `missing_bonding_curve` |
| `23ae1b725e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 476 | `BCQXwaaWQm86HSpSuxumVQsBfuVYrLiw5QWxvebisJY1` | `exact` | 682 | `missing_bonding_curve` |
| `e9bbbfa3f8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 321 | `DDtS9Q7n6nR5fu4nPefoQhvYsMWT2ncAjav7Ec7y1M9S` | `exact` | 9 | `missing_bonding_curve` |
| `040d7ea8c0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 36791 | `DNTWXMBnhDgFn7DaEermLL1A3NsKU4pGkmrCXxQ3ToQt` | `exact` | 14 | `missing_bonding_curve` |
| `81bb416c8a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `aec53be865` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 272 | `AK9EgPvrc77yy99LkSxpsM5xefbaDuPaS3pdiABhM1NV` | `exact` | 30 | `missing_bonding_curve` |
| `a565e96744` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `59602adedc` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `fd5d5d5581` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c525ba404a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 15693 | `6Eb7nSoqYQ6vccs84E7Am7fJQZ84wCbmBVwmD5y55T81` | `exact` | 10 | `missing_bonding_curve` |
| `8b9cea5007` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 36844 | `Ck2sHC9muZBbW5ZEWRdy8bVxTsiGbMSHZwHNye9tmniC` | `exact` | 23 | `missing_bonding_curve` |
| `28ede21301` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 895 | `3tCoBPoZ4CTkTGnrKuH1DZGxzFzc4HmGbWGtAXJnCY5d` | `exact` | 625 | `missing_bonding_curve` |
| `83891cc2c6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 9656 | `MYPG3eYV9DP9PVVkwP4eusfozeXWDQbHpj3zt71vN14` | `exact` | 19 | `missing_bonding_curve` |
| `288323cc85` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 421 | `5xn4rEvNC1oMYJ8z5WkSjo8oVpRgbAULcJP25RHuA5JD` | `exact` | 28 | `missing_bonding_curve` |
| `d077248fe4` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 139 | `HgrMjvGM1EGNicmTJKywG8wmvBrAXivn5mLnafcUcGoG` | `exact` | 20 | `missing_bonding_curve` |
| `3d28a40bed` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 653 | `BpfquEhHCEBHdMC3HU66hQ92XnUJx2P3cdri67qy1Y1V` | `exact` | 69 | `missing_bonding_curve` |
| `216c97164a` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 20526 | `8iZwKJWeKqw1pjdgH86DndiGJ7YHSo6wcGnACzE2LFV5` | `exact` | 22 | `missing_bonding_curve` |
| `1c6f18bd66` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d91542dbb6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 20320 | `G492h6KyTkWfopiEHZNxZLQSg6GvZsPzMWarMNkvr9SM` | `exact` | 202 | `missing_bonding_curve` |
| `68a24b102a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5e9abdee31` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e2bd27c5f3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 258970 | `E62XzGohhPHHnb8GLWktKK1S9J9JBY52SPYpHRJ9ADP3` | `exact` | 6 | `missing_bonding_curve` |
| `836154f4e2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3551 | `5pZH8gb422Z11JCGNe9Ytuvf9qKdaAngXQ4yqz6ShByn` | `exact` | 28 | `missing_bonding_curve` |
| `357b96e838` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 77 | `6SR9BgbZb2cv7peF9DxmXvRB9MDZQVa1beHyHRWzF7KL` | `exact` | 42 | `missing_bonding_curve` |
| `57b309386c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1175 | `6B8S6Lo3pkeuBJRP7EAqgfmrQDW6gZM2jFejQ6bF5iHQ` | `exact` | 16 | `missing_bonding_curve` |
| `4eb98a0ba3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10587 | `362UeAG4ppcsAnJMDi7jwzocLjwCqT3DsFaexdFErFex` | `exact` | 21 | `missing_bonding_curve` |
| `924deb24a4` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a5a09c8e10` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d4b369ea7f` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `796763858b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 545 | `viCJhy4iFT2SnMX8VnGW6nXKsWBv5mCbpBXY6b2DWVE` | `exact` | 29 | `missing_bonding_curve` |
| `6e3e2591d2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 195 | `DbpEzxeYJFgV4uFdtHfpxWCRQfWwr8hEhWfsBkNKpNJ` | `exact` | 119 | `missing_bonding_curve` |
| `92d3e22100` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1910 | `BKAK6RUApAkqTBKH9L3w1hAC9Tz4tx6Uw6VV5NK7dgQF` | `exact` | 22 | `missing_bonding_curve` |
| `85587fc6b9` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 432 | `4pDPnChVVYtvYYxNpzwhWm3JYuso6PW8hHhLBUpcSD2u` | `exact` | 38 | `missing_bonding_curve` |
| `110ec54b28` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 125 | `AWa6DCGpdyMBNBuo2m5AaLcHG9jDZ2Hp2p1AePbHo6uu` | `exact` | 28 | `missing_bonding_curve` |
| `16db0f85ad` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 37036 | `4bvBipJbuN9jqxbQG23vFQVozhGX1u5adkvBUPw6UvmK` | `exact` | 8 | `missing_bonding_curve` |
| `57cf420b28` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 585 | `4Q3VypQHMxTvoBvQtfhhGDJFPkcK91vdMBLBCSUbrz5b` | `exact` | 26 | `missing_bonding_curve` |
| `4918a4dbf8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `CpePK6c5YSLhujYYoquBty5kDZam6JSzLfvuGNpVXLFN` | `exact` | 9 | `missing_bonding_curve` |
| `6cbd1772d1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `GuiRN8ksGvi5sxHdCyHbCvSJXBFuHUfsKyrJGAWtM2TY` | `exact` | 12 | `missing_bonding_curve` |
| `35de8beef8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 73 | `KzXy91PEV6PTP6GvN4zZnSuRz6tR6mxRWQdzSb3vXo8` | `exact` | 19 | `missing_bonding_curve` |
| `df7270832b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 53 | `7kSqF7FQsNXRBgdhZwWBghUSXgnXuFuDa7YXz5G9zrrv` | `exact` | 34 | `missing_bonding_curve` |
| `8833febec7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 29053 | `9MnvLrJJywEta9HfgSkG858rBEoPxZtJzdjKcxXV3i4A` | `exact` | 9 | `missing_bonding_curve` |
| `17fd71bb26` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 157 | `3apSH9rUfr93FabeunFkY1TJZhfNmrXj2E7dBxrNY4AY` | `exact` | 113 | `missing_bonding_curve` |
| `2973ec1a3d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `792ce1890f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 852 | `CztMRTC8bCF1S222Y6xJ1xR1Y3LpBB6nNDHQhKgmDSvM` | `exact` | 24 | `missing_bonding_curve` |
| `f165391c79` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `efaac9ddf3` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3217 | `6u3XhMp2kSMjhB11qMWhzbNJYSSuVuKcou1uHZgoMih9` | `exact` | 120 | `missing_bonding_curve` |
| `699849380e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 204 | `9mdF6D75UevZsQoKB57d9HL1M92KQjk2oTpsD4YhTCvD` | `exact` | 42 | `missing_bonding_curve` |
| `3c2afe8105` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3622bac738` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 18230 | `Gwma6azjtHyfoYWLfywye95zvEMrwtHq3WxbaRsixbta` | `exact` | 17 | `missing_bonding_curve` |
| `fa86bfd809` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a54fd5709e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0c39670d88` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `AR843BARdRMmhJmZN3ekvFQcoVnT72JpRaqeZxYnPRvF` | `exact` | 2 | `missing_bonding_curve` |
| `480872414c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9d9e04759b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 8516 | `D8AY3KM29mf9GJnPui35v5xig1bLJTV2VqLT2ig5vWnP` | `exact` | 38 | `missing_bonding_curve` |
| `501416016d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 235 | `6GFmWoiyTXB2AYu2Ssg1PHiJ52MiS3NgNgCLtfQDgyN3` | `exact` | 43 | `missing_bonding_curve` |
| `965f2db33f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `Dmy9QBhrNkbwAzzRxhRxJ6LUZ8W8YejmXnWe27qeCCe4` | `exact` | 13 | `missing_bonding_curve` |
| `0b78eb6a06` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 17782 | `AqUJM94o53cM3zp5wW4cpimMMwLPNDKGSczUzC3VApdh` | `exact` | 4 | `missing_bonding_curve` |
| `97c535ed57` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 694 | `5j6NA27SLB1ioMqocLkK7YN8i7hLmR9Dao9V7HbeQgpT` | `exact` | 31 | `missing_bonding_curve` |
| `c2b941eb53` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 75535 | `5JsvWvvkM2Y2t3Qp4qodDWCcf4K7E8KUt9T8Z8BFy3uT` | `exact` | 2 | `missing_bonding_curve` |
| `9573bc8b34` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 86 | `EChtpxdAH6jSPfw4oEZ7oUhVckb22oJ7GXJkwKFCqLnq` | `exact` | 31 | `missing_bonding_curve` |
| `048b198563` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4892 | `2jRY83wEq7h8HeKVkhGWfpf7HnVDqhkcrXeXbWxv1YGW` | `exact` | 8 | `missing_bonding_curve` |
| `30b4f5a621` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2424 | `9hCguDrx48sbV1MUiE9Hod9babJXkq9zir6sVFbwXgHo` | `exact` | 16 | `missing_bonding_curve` |
| `8f8c8af55b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 105 | `BHzYt45RTi9ZnVWAUGKdZUveFSfwX32mPrzH8i6jV8ky` | `exact` | 77 | `missing_bonding_curve` |
| `f395479591` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 21766 | `3Ui287Hmca6T9NumbiCHrmRRgSDRVzdM6uP92x9D7fGC` | `exact` | 7 | `missing_bonding_curve` |
| `49f8634fe8` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `49377b03d5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 443 | `6q8dJatrRutcFR86jXdqomP5qsUQvVPVTLQdykunzSQn` | `exact` | 41 | `missing_bonding_curve` |
| `8d5f77365b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1069 | `9mcGHy6W3dTn6n8RzUjjUe8Uu6s3GsFuZs4Pnzx45RXn` | `exact` | 10 | `missing_bonding_curve` |
| `27b2555525` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 17763 | `DbcAZXJNgSEM8wgno3T82Tzhwgnh7XQ1wh8hAZmkULHt` | `exact` | 10 | `missing_bonding_curve` |
| `4801d8b46f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51109 | `6J8J59jtw3mPd5BGaV1HLTL5iDWmQE3Ly3jpbt9beGHN` | `exact` | 3 | `missing_bonding_curve` |
| `953c12629f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 366 | `3ZW6hQNQjaymhikF93e3qf2nNYea62DonWteE2udJnAU` | `exact` | 19 | `missing_bonding_curve` |
| `3f38f4b684` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1054 | `DFC4shRfCQzK8D1V6q5YtR2frNd7GJYiGhM6ox9aMKzW` | `exact` | 72 | `missing_bonding_curve` |
| `fecda8187e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 359 | `6HMCksyy5u2D357QRqC9obputw79Vj5zcSGNuGZVP9x` | `exact` | 75 | `missing_bonding_curve` |
| `705ee7776b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1631 | `4prLkSfhoHXxLHKcNVtqhbajPPhWbdE6zm7sFQnRUsmp` | `exact` | 42 | `missing_bonding_curve` |
| `6f19273a75` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1638 | `FUBHB8o7cm8sVEYZDcG25px2FaroZHApw3yLoETrdpMu` | `exact` | 99 | `missing_bonding_curve` |
| `45f7e195ed` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 618 | `5tbhKCpC73ao49kdLcChJtmChbhnsK4Cj4aYgE9JEJ7t` | `exact` | 382 | `missing_bonding_curve` |
| `a36a082682` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 491 | `5DSTRKyKCiphj9kPJpsHoSUbuEwrjU1tGVvvqC6EgYPN` | `exact` | 47 | `missing_bonding_curve` |
| `4c8fe165c2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 264 | `FVRr4PpCE6CqRphNgq9ptEr8H5pZ4fLcMhJgNWpn8d57` | `exact` | 48 | `missing_bonding_curve` |
| `f8a6901484` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 51253 | `2tsd3mV3Ae7WLpWhudC4CMxpythdcuyYNt9dLSybqvbJ` | `exact` | 4 | `missing_bonding_curve` |
| `1923f31271` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9a9d65a609` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 197 | `Er8Mnp9ivjj9B6iJw8rJ1MFSdtn5Hssk75niYWzsyyCx` | `exact` | 69 | `missing_bonding_curve` |
| `d867da9908` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10983 | `EU5C7GLmHZRLefLmM8upsS8fzMUEw8CTKLpsHCY7vPhZ` | `exact` | 191 | `missing_bonding_curve` |
| `e7573f00f5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 6264 | `9WDeexRCMW7SZX2s2yJJnhuFmzvjyidn1nPXbZWi7Mh5` | `exact` | 19 | `missing_bonding_curve` |
| `91afa3b751` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 2323 | `GYgCujEdczoghKXwAGLJiaKkZq5AZZ7cqs7kPR14Lf3v` | `exact` | 55 | `missing_bonding_curve` |
| `a6d7dfd7f0` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 24579 | `98EnXpxuLtKewAYJQYjXTz3QnkhuknX1QdtTb7av1Vch` | `exact` | 10 | `missing_bonding_curve` |
| `68fd14e663` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 533 | `Amgh82hLVp6npGj3eFi7aVoPqgLDB37kFN8oTJ1btXWq` | `exact` | 33 | `missing_bonding_curve` |
| `633b56a94a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9d9385b9e9` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c4e99e76b7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 298 | `5vapaGQ6jW1kTfuDiy1YPTQgD4S48aqdhtVo2DNkEKxs` | `exact` | 31 | `missing_bonding_curve` |
| `6f1ec70fcd` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 119 | `93iphZLPxgGcFuUHpfFSNEJJtfq2sjci6eBhq6xcBm1j` | `exact` | 65 | `missing_bonding_curve` |
| `696929da6d` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3736 | `9fjQS2ZGnYGJtLcAMh9tYhBURzU7EQNeW2cR6hDH5TWZ` | `exact` | 40 | `missing_bonding_curve` |
| `6004f8ccc6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6042ac017c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c2e1a90833` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `1196335f8b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 529 | `6KVx8fcMv8hCL4WD5ByTwR6Dhbd6mWSTpkTANbwfQJN7` | `exact` | 109 | `missing_bonding_curve` |
| `2e78b7c9e9` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 5710 | `75RLZu4sEGVztcsncrgeshFmDmvqBMW6Z9a8zPxXGp6B` | `exact` | 11 | `missing_bonding_curve` |
| `ed56729798` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 7942 | `HRZrwKXSkG6qYVnyh6Ly8m3X2BZgUZGavpnBELA4yafX` | `exact` | 15 | `missing_bonding_curve` |
| `99a0f3d17c` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 19811 | `9qE2WC8nRe7LYfSUWTCHVLRuDGXwHWur37Hg5M96BqFQ` | `exact` | 36 | `missing_bonding_curve` |
| `36972f544b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 15 | `8VY9S7uPnfGqdzsAp5juQBmpxpibe5A7A4gZ8znRAu2u` | `exact` | 676 | `missing_bonding_curve` |
| `7ecbab8879` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 628 | `FB6DMJ1ikMJRYZHTCAys9JmHC8tGh7oYwQQcMDFj5gXL` | `exact` | 65 | `missing_bonding_curve` |
| `e165670611` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 90 | `J7rW5tVEjCUPxS9DGBVhYRpUK7mm6s48PJTR594eMyaR` | `exact` | 104 | `missing_bonding_curve` |
| `0aae672361` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 418 | `6MXd7qn3MDTn8ih2gokoSyVAiijPA1Rxurewq258VpmJ` | `exact` | 33 | `missing_bonding_curve` |
| `929f0a84a5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 484 | `AP9DZ9AtXKGfRZeBM6Eay8KcEwempYo78Ru9k7TvC8bK` | `exact` | 41 | `missing_bonding_curve` |
| `2cf65c600c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5afc5110f2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 4439 | `Gbon54hpSbRoJs6Yc5MPULJBiSk8F6aR5GMiJDydfyUD` | `exact` | 34 | `missing_bonding_curve` |
| `05821356d2` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3461123d2e` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3409 | `CjAeY11MtpdhJKK7GruCB8aoQqNphXADiEEb9R9Q68oU` | `exact` | 164 | `missing_bonding_curve` |
| `75dc15c5da` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 574 | `8BRkp4WWVRHggYk1eQCCmWrkCAHFzKb2bZATdbXWi5gv` | `exact` | 24 | `missing_bonding_curve` |
| `a6d45d553f` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 16398 | `6hUDVFUCbp8PVi8RJaUvsZWSpyho3BPpBT1JpLKPuG9j` | `exact` | 5 | `missing_bonding_curve` |
| `2c2d7794c5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 8896 | `8Y6eAkap2Lx88NvEXXeVEAPMVq7bzwW3YGGesYkCaeUT` | `exact` | 3 | `missing_bonding_curve` |
| `a9f453ccc4` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1178 | `8WW6wSPUqC2yZnEiftZU1B55gKBWN1tp1ojcXyyMdH1r` | `exact` | 41 | `missing_bonding_curve` |
| `36f89dcf3b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 89338 | `5Pti8D7TuHt1Vknnw56X34KXbi14aDm8cE8ck46WtqoX` | `exact` | 6 | `missing_bonding_curve` |
| `debee97c02` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 154595 | `9vRTuCj8n8zTTQocSzzBfw2CSqHWSR7fN2V4C7qpAJvn` | `exact` | 2 | `missing_bonding_curve` |
| `0d8ac81822` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 56388 | `3t2H8mqt6Aa3UrLUrpzPCTXTqdcNjaDoXgPbAGsHA69d` | `exact` | 6 | `missing_bonding_curve` |
| `59e4aed6f6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `36abae8ce1` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 181 | `DSLfZqZFo7PLUvwWGmejUv7BLXSxCymg9eNKNwzgPaHK` | `exact` | 107 | `missing_bonding_curve` |
| `b331311cf6` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 31442 | `6h2CTefqb56JcMCvYk8S7miv6Nw4kdvBZvQYGyFEfYhW` | `exact` | 5 | `missing_bonding_curve` |
| `c992b32656` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d950fec034` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `bf463f975b` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 10063 | `7V6fh3zW192yhQgaZTw8UMugonskSZfW65sYNRU2okSx` | `exact` | 62 | `missing_bonding_curve` |
| `480e654a33` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 36096 | `ES6FS9bsoiNx9EjUtjkfrJHn2NaMZAnEUGWzBuCurMvW` | `exact` | 7 | `missing_bonding_curve` |
| `14f30e0300` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `D5ohNP718RKXnemr6ykFsKvRSKeZDK4LkfGuXADWgoSM` | `exact` | 15 | `missing_bonding_curve` |
| `1d6eb26897` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `3LDxcHjBa7ZBX8PXg3vmNWeRy7F6FPtkZJnB2hp5ypJ4` | `exact` | 8 | `missing_bonding_curve` |
| `4ac72b4014` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 115 | `CVPeumbwHitMogkP4fMJPUh8VFZ6v4vEGqXYrjqHuFqL` | `exact` | 206 | `missing_bonding_curve` |
| `e4b085babd` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a83268a620` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `fce0b54355` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `8af04c1298` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1114 | `GMrhgUNM3mkqd3B7VYG6oBaPRHtsTfrWF1jY7RnLvPid` | `exact` | 35 | `missing_bonding_curve` |
| `b7ac2f4688` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1034 | `H2HPCCXcZQ4nr5MjvgG3PUcoRpQ5Wn3ofTYx2AzjCmjR` | `exact` | 5 | `missing_bonding_curve` |
| `06b35b1afa` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1182 | `D5cYjCRb27qup8wQB6aZs36UHCy8mstAc8QCER8BkfBg` | `exact` | 102 | `missing_bonding_curve` |
| `5da85f4fd8` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3463 | `4kjEoFLa25bX5qKZgoRDiVeg3F7To21ux9wFaU8Syaxi` | `exact` | 9 | `missing_bonding_curve` |
| `268e96c766` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 764 | `7K6ebCnTwZ9T3TmAyRAqFNpWMjEt7zWnvgj95AtVpDbX` | `exact` | 141 | `missing_bonding_curve` |
| `8c3744ec32` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 1084 | `7oG9EGuGdfQktiTfnVVAqRZs5BFnY3c7gJhJsGj3HBt1` | `exact` | 65 | `missing_bonding_curve` |
| `37721f8321` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` |  | `9qdWR61o2oecY6969UobFH7zZd6XbcJCqWEc8MYTM9n8` | `exact` | 15 | `missing_bonding_curve` |
| `87f829a9f7` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 3582 | `5jYBCw5mG1przhHrrcikd67LoB9pPZWWcQteTvEhu4zn` | `exact` | 5 | `missing_bonding_curve` |
| `83036445f2` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 648 | `98JsWkLXvgVVgGX2qe6zPC5p6fc5UzxzTb8QvZP54SEg` | `exact` | 5 | `missing_bonding_curve` |
| `65a435d2cf` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 715 | `FYhWMVwxAaSPPZA18fCaqqPtF8w3GckpJRVd8Vddmkep` | `exact` | 44 | `missing_bonding_curve` |
| `8d48c75be5` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 133 | `9JP8QYMMLDnLfzzLycmBe9LZ52CWTyzJx1e8mJgwviXy` | `exact` | 73 | `missing_bonding_curve` |
| `056e746fab` | `bonding_curve` | `missing_legacy_bonding_curve` | `observed_before_decision` | 67 | `6sRweFbnGngTBZx5eVjon7hw1bUbvuqBdXa8FyGEd6c2` | `exact` | 27 | `missing_bonding_curve` |
| `9facea8b29` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `e336c2fe91` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `41fa2dbc82` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a45d5f69a6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9e5ca27604` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `44f74252ac` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `663fa08774` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `46f2dfc99c` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `0fee4911df` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a02c38880d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b3d4794f3a` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `739ef936c0` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `d0e6797d7d` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `b47243dcc9` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `2ed3277b36` | `none` | `missing_execution_route_identity` | `never_observed_in_run` |  | `none` | `exact` | 0 | `missing_execution_route_identity` |

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
