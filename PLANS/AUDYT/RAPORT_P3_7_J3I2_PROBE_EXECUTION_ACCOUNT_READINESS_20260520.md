# RAPORT P3.7-J3I2 R15-r8 Probe Execution-Account Readiness

Date: 2026-05-20

Status:

```text
P3.7-J3I2 account readiness audit: PASS
R15-r8 runtime smoke: NOT_READY_DIAGNOSED / stopped early after useful blocker signal
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8/decisions`

## Summary

```text
selected_probe_rows = 57
diagnosed_selected_probe_rows = 57
exact_decision_v3_join_rows = 57
missing_account_roles = {'bonding_curve_v2': 54, 'creator_vault': 2, 'creator_pubkey': 1}
classifications = {'execution_account_not_ready': 56, 'unknown': 1}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `c24f59e5a8` | `bonding_curve_v2` | `execution_account_not_ready` | `9opMcihAxWGN4JjL2AGXprZpa6pjFPeZue1qW3HGFYCh` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9opMcihAxWGN4JjL2AGXprZpa6pjFPeZue1qW3HGFYCh` |
| `c1e2ab92ee` | `creator_vault` | `execution_account_not_ready` | `FM3xC8EK3JQQeBCHrF1h36NGSWB9VLTmSmysR7ZuT3kb` | `exact` | 0 | `execution_account_not_ready:creator_vault:FM3xC8EK3JQQeBCHrF1h36NGSWB9VLTmSmysR7ZuT3kb` |
| `8dd2679222` | `bonding_curve_v2` | `execution_account_not_ready` | `6HWmm3Egyy7Rw6dUJ7xsHiyVddwzwruBdhKycduha7Y2` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:6HWmm3Egyy7Rw6dUJ7xsHiyVddwzwruBdhKycduha7Y2` |
| `4caa1b8034` | `bonding_curve_v2` | `execution_account_not_ready` | `7BX56vEjg7Rt3e83J5HH6txj7KxudEM2pNaQGLptRo78` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7BX56vEjg7Rt3e83J5HH6txj7KxudEM2pNaQGLptRo78` |
| `7d34a1d4df` | `bonding_curve_v2` | `execution_account_not_ready` | `GBadfFeE1wq5f2BSoZbPyHy4vqD1XQK6FoGVkvyUBBVB` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GBadfFeE1wq5f2BSoZbPyHy4vqD1XQK6FoGVkvyUBBVB` |
| `5fcda40caf` | `bonding_curve_v2` | `execution_account_not_ready` | `CdqxsTpgtGa9RmKz3tzRn8EG3pAEj1nrXHFKZHkfgKKA` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CdqxsTpgtGa9RmKz3tzRn8EG3pAEj1nrXHFKZHkfgKKA` |
| `daaf09f6be` | `bonding_curve_v2` | `execution_account_not_ready` | `EjVyMbMTNZDW6YgffXR83XMEvfVEarWuuiQAPpXpGafy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EjVyMbMTNZDW6YgffXR83XMEvfVEarWuuiQAPpXpGafy` |
| `c2804086eb` | `bonding_curve_v2` | `execution_account_not_ready` | `GU6a9AcZnzFA8ecPeMppZqUSkEyjVdYuwMHuvwdEfwai` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GU6a9AcZnzFA8ecPeMppZqUSkEyjVdYuwMHuvwdEfwai` |
| `5538e2aaae` | `bonding_curve_v2` | `execution_account_not_ready` | `J4inACAu1kT3rxsJZLCWEuDC8ft9VySCcQRSZW1xwJyJ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:J4inACAu1kT3rxsJZLCWEuDC8ft9VySCcQRSZW1xwJyJ` |
| `d050c1cbb1` | `bonding_curve_v2` | `execution_account_not_ready` | `4NbPKgvr1rXvyPsU4sxBft6SFHan1X1HHJcuCt8M9D4Q` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4NbPKgvr1rXvyPsU4sxBft6SFHan1X1HHJcuCt8M9D4Q` |
| `05615f6d3b` | `creator_vault` | `execution_account_not_ready` | `F6GsEyRVcYGtcLXbbptaZsHLMwoaRJ9uJHNZLbFe4NT1` | `exact` | 0 | `execution_account_not_ready:creator_vault:F6GsEyRVcYGtcLXbbptaZsHLMwoaRJ9uJHNZLbFe4NT1` |
| `305523c955` | `bonding_curve_v2` | `execution_account_not_ready` | `Eev5dv6o4DPS9jPmzuk6nfCLXNXw6nb8VyCWVjn8nVqV` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Eev5dv6o4DPS9jPmzuk6nfCLXNXw6nb8VyCWVjn8nVqV` |
| `2c01f13d38` | `bonding_curve_v2` | `execution_account_not_ready` | `497C9V59FXf3MjtnUmYF5mY8pDK39zhAhxowhRcksngB` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:497C9V59FXf3MjtnUmYF5mY8pDK39zhAhxowhRcksngB` |
| `03a4342989` | `bonding_curve_v2` | `execution_account_not_ready` | `2zqPukrWzL2S5YBaAigHhjqdgGSynZBxT7e7P9dpo4jk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2zqPukrWzL2S5YBaAigHhjqdgGSynZBxT7e7P9dpo4jk` |
| `d01ff08bdc` | `bonding_curve_v2` | `execution_account_not_ready` | `ChgfKgg6btUMjTjLMZz2Ya5NAHRk9zjSsFazoCE53iT7` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:ChgfKgg6btUMjTjLMZz2Ya5NAHRk9zjSsFazoCE53iT7` |
| `e0b619684a` | `bonding_curve_v2` | `execution_account_not_ready` | `AuMaUVjHY6UU6VsuhMDNG2Ukn5XcrEr4Am4riQHaDGua` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:AuMaUVjHY6UU6VsuhMDNG2Ukn5XcrEr4Am4riQHaDGua` |
| `57950351a0` | `bonding_curve_v2` | `execution_account_not_ready` | `5bVdd9U8nohwoEkaFxi2JARF8A1bX7nnGRhrEhTZQp6a` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5bVdd9U8nohwoEkaFxi2JARF8A1bX7nnGRhrEhTZQp6a` |
| `8f31c5c3ed` | `bonding_curve_v2` | `execution_account_not_ready` | `Cco1RSWeonA8wTVqXAHZJ1NeVyfLm7neLMUP8fEzY9p2` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Cco1RSWeonA8wTVqXAHZJ1NeVyfLm7neLMUP8fEzY9p2` |
| `f6e00e5df7` | `bonding_curve_v2` | `execution_account_not_ready` | `CSDTwpRmDC9UKz226ZtYQw42ZHqEB23rt4gYr9tUhMaz` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CSDTwpRmDC9UKz226ZtYQw42ZHqEB23rt4gYr9tUhMaz` |
| `8b20d632cc` | `bonding_curve_v2` | `execution_account_not_ready` | `2wxFvjWesC1NoSdeAgC1RvR6dnfY9wvQ4iFiRBmUJCEx` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2wxFvjWesC1NoSdeAgC1RvR6dnfY9wvQ4iFiRBmUJCEx` |
| `5e292dc7c5` | `bonding_curve_v2` | `execution_account_not_ready` | `7Wp2Z4wPwTfPv9fZahFhRrbL9vakPs1LYBn81u7f7dMv` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7Wp2Z4wPwTfPv9fZahFhRrbL9vakPs1LYBn81u7f7dMv` |
| `c076a7de67` | `bonding_curve_v2` | `execution_account_not_ready` | `FD36nXZjFg89MXzg1rqQEEAFdUm8m5Axc61WUxjWFJiH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FD36nXZjFg89MXzg1rqQEEAFdUm8m5Axc61WUxjWFJiH` |
| `63fce42472` | `bonding_curve_v2` | `execution_account_not_ready` | `ECuvF3paCTbZidu3dfUPT8GpTbqQhLnimJa26TZDFkyL` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:ECuvF3paCTbZidu3dfUPT8GpTbqQhLnimJa26TZDFkyL` |
| `87d3ed0ca1` | `bonding_curve_v2` | `execution_account_not_ready` | `GAEuFEtQvypTrNhEu3wcF1upyqDfou7A3PjNdTuG8ySN` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GAEuFEtQvypTrNhEu3wcF1upyqDfou7A3PjNdTuG8ySN` |
| `517d491856` | `bonding_curve_v2` | `execution_account_not_ready` | `FYqY2h5itiew1Goti6yb3A1nRM8WTtpxPod34GLG2F7L` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FYqY2h5itiew1Goti6yb3A1nRM8WTtpxPod34GLG2F7L` |
| `8188d25133` | `bonding_curve_v2` | `execution_account_not_ready` | `5GAUGZ58eVG4GeEkFoZdo33sdSQaHsji92Y6d1AGGjq5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5GAUGZ58eVG4GeEkFoZdo33sdSQaHsji92Y6d1AGGjq5` |
| `7e1c7782ac` | `bonding_curve_v2` | `execution_account_not_ready` | `9p7M9KswvtboBPARakJJGmpjHPB3YJkRSahBT1J7XptK` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9p7M9KswvtboBPARakJJGmpjHPB3YJkRSahBT1J7XptK` |
| `bb66969f2a` | `creator_pubkey` | `unknown` | `89oMkeLu3hQtW9DJnv13FrZS71GJdyoq2ZQDENHfso9y` | `exact` | 0 | `missing_required_account:creator_pubkey:89oMkeLu3hQtW9DJnv13FrZS71GJdyoq2ZQDENHfso9y` |
| `453a5c5567` | `bonding_curve_v2` | `execution_account_not_ready` | `7uXtJyvrKEvt2GhJAtiC4NTiKC8N1YjCe4ZPPeKuprvM` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7uXtJyvrKEvt2GhJAtiC4NTiKC8N1YjCe4ZPPeKuprvM` |
| `42819ff54f` | `bonding_curve_v2` | `execution_account_not_ready` | `A9VPGhK1jc9CcxyfsPABx82vYCNNWaGTAmFNvtv8G9UD` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:A9VPGhK1jc9CcxyfsPABx82vYCNNWaGTAmFNvtv8G9UD` |
| `3f3943a7b4` | `bonding_curve_v2` | `execution_account_not_ready` | `AesXvVxRtK2BoD9wXpe4jza3TRwtXQtFgw9p4To2rCnF` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:AesXvVxRtK2BoD9wXpe4jza3TRwtXQtFgw9p4To2rCnF` |
| `438cd31ea2` | `bonding_curve_v2` | `execution_account_not_ready` | `5VKeMQiUqhD9XbVVLjWZNLj3CSn4ScXsdWsLTQVYsQio` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5VKeMQiUqhD9XbVVLjWZNLj3CSn4ScXsdWsLTQVYsQio` |
| `536fc483c6` | `bonding_curve_v2` | `execution_account_not_ready` | `5Ff8LMSXdcJZWHXVfgKQd158px935wFWqQAW9ZRnyK75` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5Ff8LMSXdcJZWHXVfgKQd158px935wFWqQAW9ZRnyK75` |
| `9e263b06f0` | `bonding_curve_v2` | `execution_account_not_ready` | `FiYFGPCEXGhYqad5v8mvD7gF2wRr2FE1MzyWmrqdK4TX` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FiYFGPCEXGhYqad5v8mvD7gF2wRr2FE1MzyWmrqdK4TX` |
| `cb59635cb8` | `bonding_curve_v2` | `execution_account_not_ready` | `F5KZ74jCRaujBtKBK7LJnvWsmU6gBHrnAk4TbhUzcLKk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:F5KZ74jCRaujBtKBK7LJnvWsmU6gBHrnAk4TbhUzcLKk` |
| `17b06c415f` | `bonding_curve_v2` | `execution_account_not_ready` | `2YBXsfURepD5dNXq2HfKHtsrQPqsHyNLACNbJHJHghM5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2YBXsfURepD5dNXq2HfKHtsrQPqsHyNLACNbJHJHghM5` |
| `aa836a118f` | `bonding_curve_v2` | `execution_account_not_ready` | `Hmu27n3k1Eqt8pQGFjouuxd9JSkVy7kYHiE7fJ2NDfBu` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Hmu27n3k1Eqt8pQGFjouuxd9JSkVy7kYHiE7fJ2NDfBu` |
| `63fb94ccb8` | `bonding_curve_v2` | `execution_account_not_ready` | `EBdCGKFpvSiGCFr4prrPoboZz9NxTfroyC2kSekHRUhm` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EBdCGKFpvSiGCFr4prrPoboZz9NxTfroyC2kSekHRUhm` |
| `805fe6a53e` | `bonding_curve_v2` | `execution_account_not_ready` | `2FBRNmocgaJqFdrao9XipoGe2yAUt7pT3btyjXeqjWYu` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2FBRNmocgaJqFdrao9XipoGe2yAUt7pT3btyjXeqjWYu` |
| `3f30354c42` | `bonding_curve_v2` | `execution_account_not_ready` | `5QfxYPXXrJeDtn6NEpC75fs5M7UfS3VEAv9uwCXaekov` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5QfxYPXXrJeDtn6NEpC75fs5M7UfS3VEAv9uwCXaekov` |
| `f4c0e1eb6f` | `bonding_curve_v2` | `execution_account_not_ready` | `9isMbZqwmVdrUZt6bgqAz8C3K2JEDBrvNnH8shAnZPyn` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9isMbZqwmVdrUZt6bgqAz8C3K2JEDBrvNnH8shAnZPyn` |
| `8b219ea53c` | `bonding_curve_v2` | `execution_account_not_ready` | `uyvnfwW4tU8wRm95Mj819D5WVnCbArV6Qkmf1b6kLbG` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:uyvnfwW4tU8wRm95Mj819D5WVnCbArV6Qkmf1b6kLbG` |
| `10c8d325a4` | `bonding_curve_v2` | `execution_account_not_ready` | `HAqdVpZKkgVpEfAwpicQwWSfmgEQuPRor3QaqNfSTpSb` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HAqdVpZKkgVpEfAwpicQwWSfmgEQuPRor3QaqNfSTpSb` |
| `7382b1081d` | `bonding_curve_v2` | `execution_account_not_ready` | `GGopJXekPwyCw8Aapvki6AdhWKMZ9Y3jytyyX3LoDMCH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GGopJXekPwyCw8Aapvki6AdhWKMZ9Y3jytyyX3LoDMCH` |
| `a46219fc12` | `bonding_curve_v2` | `execution_account_not_ready` | `xHCtk77ugo33gpJ9YPwREuFNhaewBKBTbFfCZz45uas` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:xHCtk77ugo33gpJ9YPwREuFNhaewBKBTbFfCZz45uas` |
| `9f148756e3` | `bonding_curve_v2` | `execution_account_not_ready` | `FikNxWViupkFYmo91NcZKmuFHEywS9tbK5GEqy9gGSr8` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FikNxWViupkFYmo91NcZKmuFHEywS9tbK5GEqy9gGSr8` |
| `b3cb4b66c5` | `bonding_curve_v2` | `execution_account_not_ready` | `A8yjqABrUkDiHTg9Ez5zYCU9KV5Tt8WfogdGQeus9QkV` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:A8yjqABrUkDiHTg9Ez5zYCU9KV5Tt8WfogdGQeus9QkV` |
| `95fd26f230` | `bonding_curve_v2` | `execution_account_not_ready` | `Ckc6M2rjx9QQ4KvP1xUJzYfdu6AsUZFvrvmf81yXHDRb` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Ckc6M2rjx9QQ4KvP1xUJzYfdu6AsUZFvrvmf81yXHDRb` |
| `4ff307ee79` | `bonding_curve_v2` | `execution_account_not_ready` | `FhN1Udo6LE4RyaNLBVEpNjyMFhFMNz298azTZF1G1epU` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FhN1Udo6LE4RyaNLBVEpNjyMFhFMNz298azTZF1G1epU` |
| `5e76eb7daf` | `bonding_curve_v2` | `execution_account_not_ready` | `55UNhnA7nck5ct4oaub7T868FKDv88Q931WbPg1u6tvC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:55UNhnA7nck5ct4oaub7T868FKDv88Q931WbPg1u6tvC` |
| `2fb3bfa009` | `bonding_curve_v2` | `execution_account_not_ready` | `ECYjH7Nr7JwFTxEwqSJmLzvE52BUxj2STmpzKFC4SkXA` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:ECYjH7Nr7JwFTxEwqSJmLzvE52BUxj2STmpzKFC4SkXA` |
| `cc3676b85c` | `bonding_curve_v2` | `execution_account_not_ready` | `HkySUUq6g5rjLQrsD7UcuDWvPEhXh4GTSs18ShfhwAqy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HkySUUq6g5rjLQrsD7UcuDWvPEhXh4GTSs18ShfhwAqy` |
| `dadcfa17d1` | `bonding_curve_v2` | `execution_account_not_ready` | `Exx4pqDkoj46jLeER4UysbGvJjHtTjnFjj6jdkn2DKyp` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Exx4pqDkoj46jLeER4UysbGvJjHtTjnFjj6jdkn2DKyp` |
| `e4a4d75097` | `bonding_curve_v2` | `execution_account_not_ready` | `3hD2xWnpN5aLW3bBcTGiK59RUin85Q6Bk7z7Yu4CQvfj` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3hD2xWnpN5aLW3bBcTGiK59RUin85Q6Bk7z7Yu4CQvfj` |
| `ffd3b015f3` | `bonding_curve_v2` | `execution_account_not_ready` | `5aALvxHVGsRZtZotHtDenkJXtuvEe9vcrA76LD6kySub` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5aALvxHVGsRZtZotHtDenkJXtuvEe9vcrA76LD6kySub` |
| `61fa5ccbc6` | `bonding_curve_v2` | `execution_account_not_ready` | `81naJBsLuSfFxdq2BMnBFfjJbBiBRVFw3VoK4yRch7ZQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:81naJBsLuSfFxdq2BMnBFfjJbBiBRVFw3VoK4yRch7ZQ` |
| `3b43d915e4` | `bonding_curve_v2` | `execution_account_not_ready` | `9q4wT78GSrsRs7FHxgNE2V8iWm8Ko489BqrAoB2Bwzku` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9q4wT78GSrsRs7FHxgNE2V8iWm8Ko489BqrAoB2Bwzku` |

## Interpretation

R15-r8 was intentionally stopped early once the next useful blocker was visible.
The run did not produce counterfactual probe transport or entry rows. It did
prove that selected probe rows still exact-join to persisted V3/MFS decision
rows, and that the dominant execution blocker remains strict account readiness:
mostly `bonding_curve_v2`, with smaller `creator_vault`/`creator_pubkey`
coverage.

The generated shadow transport/entry/lifecycle row in this namespace was a
natural shadow BUY artifact, not a counterfactual probe artifact: it had no
`probe_id` and no `dispatch_source=counterfactual_shadow_probe`.

## Decision

Do not bypass required-account precheck. Do not start collection.

R15-r8 also showed that a follow-up smoke should not wait for a full timeout
when a structural blocker appears early. A later R15-r8b attempt made that
blocker explicit: `probe_scan_concurrency_limit_exceeded` could still discard
candidate scans before the finite scan budget was exhausted. That is handled by
J3I3 scan backlog admission repair.

J3I should decide whether to add explicit decision-time-safe materialization
for `bonding_curve_v2` and route-specific `creator_vault`, or narrow probe
eligibility to rows where these strict execution accounts are already known
and ready. If the accounts are known but absent on RPC at processed
commitment, the row should remain classified as
`execution_account_not_ready` rather than dispatched.

R15-r6 should only run after a concrete eligibility/materialization fix. It
should not increase probe limits and should not weaken strict precheck.
