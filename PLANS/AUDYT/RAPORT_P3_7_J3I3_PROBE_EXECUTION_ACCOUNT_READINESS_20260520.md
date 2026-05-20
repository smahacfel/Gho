# RAPORT P3.7-J3I3 R15-r8c Probe Execution-Account Readiness

Date: 2026-05-20

Status:

```text
P3.7-J3I3 account readiness audit: PASS
R15-r8c runtime smoke: NOT_READY_DIAGNOSED / stopped early after useful blocker signal
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8c.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8c/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8c/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8c/decisions`

## Summary

```text
selected_probe_rows = 33
diagnosed_selected_probe_rows = 31
exact_decision_v3_join_rows = 33
missing_account_roles = {'bonding_curve_v2': 29, 'creator_vault': 2, 'none': 2}
classifications = {'execution_account_not_ready': 31, 'unknown': 2}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `bcd7a5df1b` | `bonding_curve_v2` | `execution_account_not_ready` | `tdLeCghAdbSWcMfwqLrzvHznQhEjPCUQ9aononmvFEV` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:tdLeCghAdbSWcMfwqLrzvHznQhEjPCUQ9aononmvFEV` |
| `4fbe6affd7` | `bonding_curve_v2` | `execution_account_not_ready` | `25XPxojoCtJwnPhwxX4qUaVovtajA2XhX7L7QsZx9r9Q` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:25XPxojoCtJwnPhwxX4qUaVovtajA2XhX7L7QsZx9r9Q` |
| `a7456fcf79` | `bonding_curve_v2` | `execution_account_not_ready` | `8RgXZiXHkF4Myxe4FqM47TGQ1U9sMvek6yK8sy6BHig6` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:8RgXZiXHkF4Myxe4FqM47TGQ1U9sMvek6yK8sy6BHig6` |
| `abf915cdab` | `bonding_curve_v2` | `execution_account_not_ready` | `71byWVQqXALRpZCbsyiCQfQWbNwznCDr1DRJi3aViL3J` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:71byWVQqXALRpZCbsyiCQfQWbNwznCDr1DRJi3aViL3J` |
| `cf7c46600b` | `bonding_curve_v2` | `execution_account_not_ready` | `76mSzinpsifaWGjtihmdubmC9XgGDxsr3np7NeQMhnB6` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:76mSzinpsifaWGjtihmdubmC9XgGDxsr3np7NeQMhnB6` |
| `2ff54f94ab` | `bonding_curve_v2` | `execution_account_not_ready` | `3vjMTGBRxct3YhJRRQvziMChwBuUm6Atj7A6mSuFpH2K` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3vjMTGBRxct3YhJRRQvziMChwBuUm6Atj7A6mSuFpH2K` |
| `4a678d5b14` | `bonding_curve_v2` | `execution_account_not_ready` | `5pN1MW3KVciotuajiMiiUUuzG18PDEueqUEXrYTwpHmy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5pN1MW3KVciotuajiMiiUUuzG18PDEueqUEXrYTwpHmy` |
| `e9c41c7b5d` | `bonding_curve_v2` | `execution_account_not_ready` | `4cUmq8TPTQj82kbocQknvcsJMWgiK3wdwB6yUmoqhEum` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4cUmq8TPTQj82kbocQknvcsJMWgiK3wdwB6yUmoqhEum` |
| `0e331bb9f7` | `bonding_curve_v2` | `execution_account_not_ready` | `8vUSEAESp8ENuajck9j5HuphRPffFKdhP41F2erEELuR` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:8vUSEAESp8ENuajck9j5HuphRPffFKdhP41F2erEELuR` |
| `66f5b9a45f` | `bonding_curve_v2` | `execution_account_not_ready` | `7mq1F8nfVN6Q2kWnRWTrhQHUTWiUaQ3AgkRA4QkSmAjb` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7mq1F8nfVN6Q2kWnRWTrhQHUTWiUaQ3AgkRA4QkSmAjb` |
| `37297300be` | `bonding_curve_v2` | `execution_account_not_ready` | `DmSENv6cu9TM9LusHBxbtr5XgHMangd2XkSgJrnNsyXr` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DmSENv6cu9TM9LusHBxbtr5XgHMangd2XkSgJrnNsyXr` |
| `babda917fd` | `bonding_curve_v2` | `execution_account_not_ready` | `BZAL1eEpCA68kWq2Ry1G5HgoQzRqgLaEjwZJw4ua5zi6` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BZAL1eEpCA68kWq2Ry1G5HgoQzRqgLaEjwZJw4ua5zi6` |
| `ad4e26312f` | `bonding_curve_v2` | `execution_account_not_ready` | `2DSNwzXpTL6sMhL9FebodYqeXRbmdQWDQMSMSgZTXT8q` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2DSNwzXpTL6sMhL9FebodYqeXRbmdQWDQMSMSgZTXT8q` |
| `6dd4b8a399` | `bonding_curve_v2` | `execution_account_not_ready` | `DN9K5BEgr7XuXs8zJt9vSs6PFAnRidLLjxaBU7n8dhEf` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DN9K5BEgr7XuXs8zJt9vSs6PFAnRidLLjxaBU7n8dhEf` |
| `738c585d2b` | `bonding_curve_v2` | `execution_account_not_ready` | `9wWSYGnC8d1TFWy6p1xBAsfwZ9PMKQCsquFKUYoXZ9h7` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9wWSYGnC8d1TFWy6p1xBAsfwZ9PMKQCsquFKUYoXZ9h7` |
| `f06a1ab8fe` | `bonding_curve_v2` | `execution_account_not_ready` | `CTisufD7i98HQn5JEnML9ETemGS3hecyWQwPmUuDDDAo` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CTisufD7i98HQn5JEnML9ETemGS3hecyWQwPmUuDDDAo` |
| `724e148191` | `bonding_curve_v2` | `execution_account_not_ready` | `CU4x5phoYWc14KQJVnf2UqmJromb3ixNdqKDN7hc3WaU` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CU4x5phoYWc14KQJVnf2UqmJromb3ixNdqKDN7hc3WaU` |
| `e8f5c097d8` | `bonding_curve_v2` | `execution_account_not_ready` | `4kYVjiegbo7VMP5f66ptqM9fNfQsYwQNmDi3r1R6kDW4` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4kYVjiegbo7VMP5f66ptqM9fNfQsYwQNmDi3r1R6kDW4` |
| `8e88e36f9b` | `bonding_curve_v2` | `execution_account_not_ready` | `Eeyz5BK6nMM7dCTtZiymGvCri8tFeWSJmqmKEHXvpjGo` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Eeyz5BK6nMM7dCTtZiymGvCri8tFeWSJmqmKEHXvpjGo` |
| `e88a3af0d5` | `bonding_curve_v2` | `execution_account_not_ready` | `4rFDcNMiE7ooRK4w9wLnEqcpDYdhHUGzExL135UoCsNk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4rFDcNMiE7ooRK4w9wLnEqcpDYdhHUGzExL135UoCsNk` |
| `11ecb04fa2` | `bonding_curve_v2` | `execution_account_not_ready` | `HFqRmSCyuXfDJ87rqJEj8PsCAAHFgi2GKA8CE3v9LdpQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HFqRmSCyuXfDJ87rqJEj8PsCAAHFgi2GKA8CE3v9LdpQ` |
| `7fcc4702c8` | `bonding_curve_v2` | `execution_account_not_ready` | `D9bwt6ipvW2egusBJqCiNfceAFGFU8pudk86ZMEFeF8f` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:D9bwt6ipvW2egusBJqCiNfceAFGFU8pudk86ZMEFeF8f` |
| `d79c53d5e0` | `bonding_curve_v2` | `execution_account_not_ready` | `54sDJRjLnW4FL9otJ3PETYAfbpNAcmt5aDc2ANiwYS3a` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:54sDJRjLnW4FL9otJ3PETYAfbpNAcmt5aDc2ANiwYS3a` |
| `93e229aca7` | `bonding_curve_v2` | `execution_account_not_ready` | `EHaqf7M2iVErRSRSrGxLUv2GryunKdPmfUsytjCkGDTF` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EHaqf7M2iVErRSRSrGxLUv2GryunKdPmfUsytjCkGDTF` |
| `d63e5eeb1f` | `bonding_curve_v2` | `execution_account_not_ready` | `C3rQpxdaABFo4rvBdVAU551Lqa9F4XXe571mcYd1f6sK` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:C3rQpxdaABFo4rvBdVAU551Lqa9F4XXe571mcYd1f6sK` |
| `4055fb0fc7` | `bonding_curve_v2` | `execution_account_not_ready` | `EzeB8ChJPb3eTy55jrGobk59PvfFWr8pNCNTa4G5eYuP` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EzeB8ChJPb3eTy55jrGobk59PvfFWr8pNCNTa4G5eYuP` |
| `4aa1595afa` | `bonding_curve_v2` | `execution_account_not_ready` | `57dNWs4HKggmk93h2TjXpEEPVTZ56vKUhN4SFvaCBh1G` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:57dNWs4HKggmk93h2TjXpEEPVTZ56vKUhN4SFvaCBh1G` |
| `8e04fce32f` | `creator_vault` | `execution_account_not_ready` | `E1oP31PMi3pHqkEcL4XdRKiC4YAhpaYP7w7ZGwWNq1tm` | `exact` | 0 | `execution_account_not_ready:creator_vault:E1oP31PMi3pHqkEcL4XdRKiC4YAhpaYP7w7ZGwWNq1tm` |
| `5606df55a9` | `bonding_curve_v2` | `execution_account_not_ready` | `2ZavQLMqPCHx8ZvWPUGKgq4TT72zh6w68q3z8FScHVRn` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2ZavQLMqPCHx8ZvWPUGKgq4TT72zh6w68q3z8FScHVRn` |
| `38b77956d8` | `bonding_curve_v2` | `execution_account_not_ready` | `G7G4EB4cVeKoh64JgboFuwKfJs1jXpMdU78ygyV6TSd5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:G7G4EB4cVeKoh64JgboFuwKfJs1jXpMdU78ygyV6TSd5` |
| `71806bfd1d` | `creator_vault` | `execution_account_not_ready` | `7SgfJrryT6JxrSTEh6KPVeHVvtziQRGoC523MpMhBpun` | `exact` | 0 | `execution_account_not_ready:creator_vault:7SgfJrryT6JxrSTEh6KPVeHVvtziQRGoC523MpMhBpun` |
| `ac78ce4a50` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f073f4e329` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

## Interpretation

R15-r8c was stopped early by design. It was only meant to verify the J3I3
scan-backlog repair, not to wait for a full timeout.

The useful result is:

```text
probe_selection_rows = 33
probe_skips_rows = 46
probe_transport_rows = 0
probe_entry_rows = 0
execution_account_not_ready = 31
verdict_type_not_in_sample_scope = 15
probe_scan_concurrency_limit_exceeded = 0
```

J3I3 therefore removed scan concurrency as the immediate blocker. The next
blocker is again strict execution-account readiness. The dominant missing role
is still `bonding_curve_v2`; smaller route-aware `creator_vault` gaps remain.

## Decision

Do not bypass required-account precheck. Do not start collection.

Recommended next repair path:

```text
P3.7-J3J Execution Account Readiness Source / Wait Strategy
```

J3J should decide whether probe eligibility can be made decision-time-safe by
waiting briefly for account readiness within the observation window, or whether
explicit execution-account readiness/materialization must be added before
counterfactual probe dispatch can produce transport/entry rows.
