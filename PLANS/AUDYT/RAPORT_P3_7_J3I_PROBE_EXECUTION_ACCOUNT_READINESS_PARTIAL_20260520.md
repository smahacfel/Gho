# RAPORT P3.7-J3I Probe Execution-Account Eligibility Partial

Date: 2026-05-20

Status:

```text
P3.7-J3I account readiness audit: PASS on in-flight snapshot
R15-r7 runtime smoke: IN_FLIGHT_PARTIAL / NOT_READY on current snapshot
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7/decisions`

## Summary

```text
selected_probe_rows = 106
diagnosed_selected_probe_rows = 105
exact_decision_v3_join_rows = 106
missing_account_roles = {'bonding_curve_v2': 99, 'creator_vault': 5, 'associated_bonding_curve': 1, 'none': 1}
classifications = {'execution_account_not_ready': 105, 'unknown': 1}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `1088941838` | `bonding_curve_v2` | `execution_account_not_ready` | `FRbaKK34zkVLEhcLRJF15H6A67VUE1NoEsSZzfRDiEpU` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FRbaKK34zkVLEhcLRJF15H6A67VUE1NoEsSZzfRDiEpU` |
| `c7ab824b1e` | `bonding_curve_v2` | `execution_account_not_ready` | `BySXofBnzff3X7mRq6PRCmxSh96tZZcah7Z6AmiqDyLB` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BySXofBnzff3X7mRq6PRCmxSh96tZZcah7Z6AmiqDyLB` |
| `9797c0af06` | `bonding_curve_v2` | `execution_account_not_ready` | `BoLd2TEWk3U5uQwF9kNP8PHVtVjKWdFCG6W2GcZAkn66` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BoLd2TEWk3U5uQwF9kNP8PHVtVjKWdFCG6W2GcZAkn66` |
| `fcf2c16643` | `bonding_curve_v2` | `execution_account_not_ready` | `FQwdZcF5KLz5GGSoTaV7NK1Sf5uP5meBwwyWZ85e9NYc` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FQwdZcF5KLz5GGSoTaV7NK1Sf5uP5meBwwyWZ85e9NYc` |
| `44f729ca4e` | `bonding_curve_v2` | `execution_account_not_ready` | `BzMK46Upfq1qYgvhjJZ2HP4oRAzgD2fbS9RBN8GrnfT1` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BzMK46Upfq1qYgvhjJZ2HP4oRAzgD2fbS9RBN8GrnfT1` |
| `b6b5c05297` | `bonding_curve_v2` | `execution_account_not_ready` | `5XTiSdutbTcADc1KdXMLwjbSFgAmkgtNAzNpFqbzvdvF` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5XTiSdutbTcADc1KdXMLwjbSFgAmkgtNAzNpFqbzvdvF` |
| `54a0941337` | `bonding_curve_v2` | `execution_account_not_ready` | `5y5pamttJibj1kKCGaA3nWJCNQMxiDdPzGGLQ6s6CGPH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5y5pamttJibj1kKCGaA3nWJCNQMxiDdPzGGLQ6s6CGPH` |
| `8aa876ed16` | `bonding_curve_v2` | `execution_account_not_ready` | `JBwxVwijvqKb5qiPEm7QNv3dCgxmWqNL8Vh4U1PNwwda` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:JBwxVwijvqKb5qiPEm7QNv3dCgxmWqNL8Vh4U1PNwwda` |
| `085b285255` | `bonding_curve_v2` | `execution_account_not_ready` | `7S5xUVqycQkzipYQ9YEGRSrkYPwDsdru2kjcQj5yjYu6` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7S5xUVqycQkzipYQ9YEGRSrkYPwDsdru2kjcQj5yjYu6` |
| `d0241d1a79` | `bonding_curve_v2` | `execution_account_not_ready` | `5bfpke65cTRu728jNNFmj6XvxEfDR9BrPCeKDn8AqVwz` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5bfpke65cTRu728jNNFmj6XvxEfDR9BrPCeKDn8AqVwz` |
| `c5ad67bf0a` | `bonding_curve_v2` | `execution_account_not_ready` | `nujhJakUDjLrunaxpbUGnvsAUtW5TpBWtFKUs5TXSwM` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:nujhJakUDjLrunaxpbUGnvsAUtW5TpBWtFKUs5TXSwM` |
| `ddbb8b1f46` | `bonding_curve_v2` | `execution_account_not_ready` | `APDNhBFpFN5oo2BdtVkwd7kWd3TxdGiAHJToihvto6HF` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:APDNhBFpFN5oo2BdtVkwd7kWd3TxdGiAHJToihvto6HF` |
| `ba298d9824` | `bonding_curve_v2` | `execution_account_not_ready` | `BR9TE97rhC9FRFiLpSeGh6jXftKVqHFbXzdaJExdvCbt` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BR9TE97rhC9FRFiLpSeGh6jXftKVqHFbXzdaJExdvCbt` |
| `fe134045ba` | `bonding_curve_v2` | `execution_account_not_ready` | `DuRXr4FUBwAkXyit3XqkdmbAR7MANJ3noMBDmWCGLncE` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DuRXr4FUBwAkXyit3XqkdmbAR7MANJ3noMBDmWCGLncE` |
| `72e2b4b1c2` | `bonding_curve_v2` | `execution_account_not_ready` | `9H1HvcXKDmBRxNknDnLWe21aftkkMyezLpgwHRZHU4Wy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9H1HvcXKDmBRxNknDnLWe21aftkkMyezLpgwHRZHU4Wy` |
| `fe3cc0c379` | `bonding_curve_v2` | `execution_account_not_ready` | `9RayJd14SE6dvnKEVhfzEyft6QSVKMPf4v4CYnrHiUDo` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9RayJd14SE6dvnKEVhfzEyft6QSVKMPf4v4CYnrHiUDo` |
| `3b079abe81` | `bonding_curve_v2` | `execution_account_not_ready` | `Crp9XBR6PtZ6HtUd9zELfjaWG9oEm3Z6Yr3x5MDQ6nPp` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Crp9XBR6PtZ6HtUd9zELfjaWG9oEm3Z6Yr3x5MDQ6nPp` |
| `bcddf4078f` | `bonding_curve_v2` | `execution_account_not_ready` | `CSNVmLb4mXCGyydHF5xvfFgMx7Um2ihD2Ru848mqBnC7` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CSNVmLb4mXCGyydHF5xvfFgMx7Um2ihD2Ru848mqBnC7` |
| `c632867cff` | `bonding_curve_v2` | `execution_account_not_ready` | `7mo3NPgGjbxzttYy2mcDF3KEGU3gv4gpSvU9Bx2Gxydk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7mo3NPgGjbxzttYy2mcDF3KEGU3gv4gpSvU9Bx2Gxydk` |
| `23a2d99bc7` | `bonding_curve_v2` | `execution_account_not_ready` | `6njbQHT7CMVisWMYRXXqYaETyCV9kamfrTy2T9YpfN9Y` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:6njbQHT7CMVisWMYRXXqYaETyCV9kamfrTy2T9YpfN9Y` |
| `c9b64df453` | `bonding_curve_v2` | `execution_account_not_ready` | `GYQtp3WN11b75M7NjY1f3fnG4Wjhv7SfBhd2JMbo8E6y` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GYQtp3WN11b75M7NjY1f3fnG4Wjhv7SfBhd2JMbo8E6y` |
| `8f6a8ff6d0` | `bonding_curve_v2` | `execution_account_not_ready` | `DdvGkRYau4JTxScqPi791RbgAhRaNvwrxDgdukFA126T` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DdvGkRYau4JTxScqPi791RbgAhRaNvwrxDgdukFA126T` |
| `6416aa66c2` | `bonding_curve_v2` | `execution_account_not_ready` | `9WMqRaEHQL2eaCFST5VqRcWC25iVX7Sud3vfxZNPR8Aa` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9WMqRaEHQL2eaCFST5VqRcWC25iVX7Sud3vfxZNPR8Aa` |
| `4194fea0c0` | `bonding_curve_v2` | `execution_account_not_ready` | `7XgrKY3152ErYpFGUzmkrgJZttioRghbp38cKDBbAb3i` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7XgrKY3152ErYpFGUzmkrgJZttioRghbp38cKDBbAb3i` |
| `3e170c360c` | `bonding_curve_v2` | `execution_account_not_ready` | `F8deyxuzezyv7pkcMKKYVzui65LZJQ68QWg1QfPGLK6L` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:F8deyxuzezyv7pkcMKKYVzui65LZJQ68QWg1QfPGLK6L` |
| `b7e7ee7fc3` | `bonding_curve_v2` | `execution_account_not_ready` | `HLt5k6UWD3mxe4nHkLzasyoEbQFvfRwtuwTLCuz18Bnj` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HLt5k6UWD3mxe4nHkLzasyoEbQFvfRwtuwTLCuz18Bnj` |
| `af76c1a6ab` | `bonding_curve_v2` | `execution_account_not_ready` | `133ZSX3RTXhK9HPHExqTYzuKvPXoCVsbQuQe8m2eVstD` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:133ZSX3RTXhK9HPHExqTYzuKvPXoCVsbQuQe8m2eVstD` |
| `fc28d15124` | `bonding_curve_v2` | `execution_account_not_ready` | `Hm8s8wTj9oBsEVrvTDjNVuzfzJucgx3NkqtyS3yth7F7` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Hm8s8wTj9oBsEVrvTDjNVuzfzJucgx3NkqtyS3yth7F7` |
| `189711e598` | `bonding_curve_v2` | `execution_account_not_ready` | `BTj5bYzkyJys1hNUZ4cf1wPDNmUo7pVDXRnYzk4qwP1f` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BTj5bYzkyJys1hNUZ4cf1wPDNmUo7pVDXRnYzk4qwP1f` |
| `cf4c454750` | `bonding_curve_v2` | `execution_account_not_ready` | `69r65eMxi7ppwMbeLZk1kMfdDoyoxwAG9axgKjqu9bnQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:69r65eMxi7ppwMbeLZk1kMfdDoyoxwAG9axgKjqu9bnQ` |
| `6918cc2538` | `bonding_curve_v2` | `execution_account_not_ready` | `tLgMBk7L3ojs7hP5cjsWRiKXKukny73zGj8Ur7UkRZ5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:tLgMBk7L3ojs7hP5cjsWRiKXKukny73zGj8Ur7UkRZ5` |
| `6d154c4db2` | `bonding_curve_v2` | `execution_account_not_ready` | `FgdacZSPfVvEMYCBdrB9x2tSinxYNjtC8kQWjha7f3oF` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FgdacZSPfVvEMYCBdrB9x2tSinxYNjtC8kQWjha7f3oF` |
| `b94977a903` | `bonding_curve_v2` | `execution_account_not_ready` | `5yUSD8UyjGhFfLCDYVcTMs25Zb8yRpnuDzmJLtdiKrY7` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5yUSD8UyjGhFfLCDYVcTMs25Zb8yRpnuDzmJLtdiKrY7` |
| `7c774d201c` | `bonding_curve_v2` | `execution_account_not_ready` | `CTtcdd2N2er1d7qFKncRFUBNvtFDnRdnzDoZAPYcKrVN` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CTtcdd2N2er1d7qFKncRFUBNvtFDnRdnzDoZAPYcKrVN` |
| `7547e79172` | `bonding_curve_v2` | `execution_account_not_ready` | `BRfvCffFbbDgPu85Amko8bHozVHD8xosVojFpKoxw3Rk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BRfvCffFbbDgPu85Amko8bHozVHD8xosVojFpKoxw3Rk` |
| `434a5f2ade` | `creator_vault` | `execution_account_not_ready` | `VFC27LwA1shctjGcvc9iio6ZFP68HFbLVr9bozroUwX` | `exact` | 0 | `execution_account_not_ready:creator_vault:VFC27LwA1shctjGcvc9iio6ZFP68HFbLVr9bozroUwX` |
| `46f058bfba` | `bonding_curve_v2` | `execution_account_not_ready` | `BZ2wRKdWgfLnCsbYJxGnivyNYcD4ujBBD3jMsq6S98cm` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BZ2wRKdWgfLnCsbYJxGnivyNYcD4ujBBD3jMsq6S98cm` |
| `15f1bd34f3` | `bonding_curve_v2` | `execution_account_not_ready` | `4R7BBasZXZGjqNyhETtkQdC6Eng96uLwqHBE7wac2zac` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4R7BBasZXZGjqNyhETtkQdC6Eng96uLwqHBE7wac2zac` |
| `435c79c532` | `bonding_curve_v2` | `execution_account_not_ready` | `ATgEcLMHddF271UudsjL4cqKp1aQi8q3ghprL5jc28X` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:ATgEcLMHddF271UudsjL4cqKp1aQi8q3ghprL5jc28X` |
| `e0713cddbb` | `bonding_curve_v2` | `execution_account_not_ready` | `7yDejyiSmk3NUn19G6aFCfpZJv1mDwHgaZuhn72VyFwC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7yDejyiSmk3NUn19G6aFCfpZJv1mDwHgaZuhn72VyFwC` |
| `7d2a6a3b18` | `bonding_curve_v2` | `execution_account_not_ready` | `EjfDctPYBBdgwzFPQ4oQ7kpZazAuhvin8oJKaDtXbdNv` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EjfDctPYBBdgwzFPQ4oQ7kpZazAuhvin8oJKaDtXbdNv` |
| `3ac903ee65` | `bonding_curve_v2` | `execution_account_not_ready` | `HrQQzbiJP3WZ7FMPWRTPsibMJishjKczdLLNYyYj8v4t` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HrQQzbiJP3WZ7FMPWRTPsibMJishjKczdLLNYyYj8v4t` |
| `2b015f34c1` | `bonding_curve_v2` | `execution_account_not_ready` | `12gLgq8rDrW6VW3hGkcunRJJ94xtVpsyzxAEHbgaUjFi` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:12gLgq8rDrW6VW3hGkcunRJJ94xtVpsyzxAEHbgaUjFi` |
| `d8caa8dbe8` | `bonding_curve_v2` | `execution_account_not_ready` | `53kGeWwoxqRCQkTeja6FTAceuaHdEfUF2yqufY87UvXy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:53kGeWwoxqRCQkTeja6FTAceuaHdEfUF2yqufY87UvXy` |
| `d8a6990df4` | `bonding_curve_v2` | `execution_account_not_ready` | `ES26SeaX799FUYHYjzvCAo2QzwssLze1nG1ggkEL5Anw` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:ES26SeaX799FUYHYjzvCAo2QzwssLze1nG1ggkEL5Anw` |
| `b0ea9b7cd5` | `bonding_curve_v2` | `execution_account_not_ready` | `opwzBNFpYDcSbkYE4Yb1Kh9AJimkgCsW6Y7nMsPR9Tk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:opwzBNFpYDcSbkYE4Yb1Kh9AJimkgCsW6Y7nMsPR9Tk` |
| `3db7f8164e` | `bonding_curve_v2` | `execution_account_not_ready` | `AfR3e7HACznvyF4h6WxnXMJNaD8Vpdr6E6NTe524b8iJ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:AfR3e7HACznvyF4h6WxnXMJNaD8Vpdr6E6NTe524b8iJ` |
| `8968d6d72b` | `bonding_curve_v2` | `execution_account_not_ready` | `7LPgnmg2CqfCf1S8C78PhDXD9tDgcFcJZieKk9ix3ghX` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7LPgnmg2CqfCf1S8C78PhDXD9tDgcFcJZieKk9ix3ghX` |
| `b1e731906c` | `bonding_curve_v2` | `execution_account_not_ready` | `3yxUcMFHR4sjisabx1YJigP9EG2VZkjtGgBhMZ454xfy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3yxUcMFHR4sjisabx1YJigP9EG2VZkjtGgBhMZ454xfy` |
| `4b46fd79d3` | `bonding_curve_v2` | `execution_account_not_ready` | `5jWodomDhGBuuwo9pUSwJksfuwtSWa1sP4aecQYfE1dQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5jWodomDhGBuuwo9pUSwJksfuwtSWa1sP4aecQYfE1dQ` |
| `d9b73037c4` | `bonding_curve_v2` | `execution_account_not_ready` | `Hg3xnvbAvvuNQhaUjfFzszTojFs7r2crxYmt9QL83N3M` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Hg3xnvbAvvuNQhaUjfFzszTojFs7r2crxYmt9QL83N3M` |
| `8c6c28e3d2` | `bonding_curve_v2` | `execution_account_not_ready` | `9jcZmTyfGT6KZWnF3TzXZuxFp4t6HrFsqF5cNYYhcb5h` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9jcZmTyfGT6KZWnF3TzXZuxFp4t6HrFsqF5cNYYhcb5h` |
| `7dadab3b72` | `bonding_curve_v2` | `execution_account_not_ready` | `E76e3qovTcrwZMPUXscxThbomRFQekbyyMVKZKgM1vy6` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:E76e3qovTcrwZMPUXscxThbomRFQekbyyMVKZKgM1vy6` |
| `ad60ad0b5f` | `creator_vault` | `execution_account_not_ready` | `CoPiji9Jk7A1z9oxaPFT5jRoKmWJPVJhv1q8tWs4ENzz` | `exact` | 0 | `execution_account_not_ready:creator_vault:CoPiji9Jk7A1z9oxaPFT5jRoKmWJPVJhv1q8tWs4ENzz` |
| `e4814b85ae` | `associated_bonding_curve` | `execution_account_not_ready` | `395GRhXDFKDo2SEKNEKiEH5eQhntPyYadAqUhz9FFfys` | `exact` | 0 | `execution_account_not_ready:associated_bonding_curve:395GRhXDFKDo2SEKNEKiEH5eQhntPyYadAqUhz9FFfys` |
| `0e59ff2943` | `bonding_curve_v2` | `execution_account_not_ready` | `DMpoYd7XB5pY9h4N9Cs72SXJq9TXWPdtsCdYDEMsinNV` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DMpoYd7XB5pY9h4N9Cs72SXJq9TXWPdtsCdYDEMsinNV` |
| `4b4c929387` | `bonding_curve_v2` | `execution_account_not_ready` | `C8vTe8zNnBJm7nDnvC3cBEyfUgbpxRJztNWdxtPeidQt` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:C8vTe8zNnBJm7nDnvC3cBEyfUgbpxRJztNWdxtPeidQt` |
| `68dd441a93` | `bonding_curve_v2` | `execution_account_not_ready` | `2yj8KzFHpG4P7D4qSaqFFEZhmyW1kW7cnLjZ8ncUu7Fz` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2yj8KzFHpG4P7D4qSaqFFEZhmyW1kW7cnLjZ8ncUu7Fz` |
| `3e67709998` | `bonding_curve_v2` | `execution_account_not_ready` | `DN2YPFBamPQ4PPXchEzsRh9xXDeJjJkgDmVZXiDXWgDZ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DN2YPFBamPQ4PPXchEzsRh9xXDeJjJkgDmVZXiDXWgDZ` |
| `82cdf3a9cc` | `bonding_curve_v2` | `execution_account_not_ready` | `ARvE2PgmsDKTBR6PtBGhTByPKZXdMMnDKAhfgAArEHrH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:ARvE2PgmsDKTBR6PtBGhTByPKZXdMMnDKAhfgAArEHrH` |
| `a7a6184c68` | `bonding_curve_v2` | `execution_account_not_ready` | `FLr7vHLkwHvV3TRr7xiwWFBj9DTrg4hrk2JnNyAXYZZo` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FLr7vHLkwHvV3TRr7xiwWFBj9DTrg4hrk2JnNyAXYZZo` |
| `0dee5de256` | `bonding_curve_v2` | `execution_account_not_ready` | `7wbb6FULaD7crXdxMh8JpdtkfjfkxZSnG89xDAHmsYrY` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7wbb6FULaD7crXdxMh8JpdtkfjfkxZSnG89xDAHmsYrY` |
| `2f96120693` | `bonding_curve_v2` | `execution_account_not_ready` | `2PtjcA8tbJ5hWPPnDYPz7SSQDebP9DW6d2kNHoPUw53J` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2PtjcA8tbJ5hWPPnDYPz7SSQDebP9DW6d2kNHoPUw53J` |
| `fdb90f3ddc` | `bonding_curve_v2` | `execution_account_not_ready` | `3AACU5NUt9Re2SGKPoGfjSBVTjQe4xyQJhyfSgYZoUcd` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3AACU5NUt9Re2SGKPoGfjSBVTjQe4xyQJhyfSgYZoUcd` |
| `d642926aaf` | `bonding_curve_v2` | `execution_account_not_ready` | `7DB9ev8NJamU3WHJAXvgN6MZC5Bppy6CWY1UWKXK46dM` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7DB9ev8NJamU3WHJAXvgN6MZC5Bppy6CWY1UWKXK46dM` |
| `bec9d6ce2f` | `bonding_curve_v2` | `execution_account_not_ready` | `FrdrUkpqrGeLAJW6N4zcoBSusDAdM82p7RpsdADmxWSv` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FrdrUkpqrGeLAJW6N4zcoBSusDAdM82p7RpsdADmxWSv` |
| `6104ad0ae9` | `bonding_curve_v2` | `execution_account_not_ready` | `AWJte1hfj5SHtHUVjX6jrpHC7GJUMYb6w22zjQRLyDyq` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:AWJte1hfj5SHtHUVjX6jrpHC7GJUMYb6w22zjQRLyDyq` |
| `5464000343` | `bonding_curve_v2` | `execution_account_not_ready` | `3tbWV5d7mXCo9kJRgRJak5rz6aJinEnk71dQAMoCfcSV` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3tbWV5d7mXCo9kJRgRJak5rz6aJinEnk71dQAMoCfcSV` |
| `3c6570b022` | `bonding_curve_v2` | `execution_account_not_ready` | `4mmnSfYZq9aaL6zHJx9Q1AKrcRnQJLX81pvJLeb3Zpy5` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4mmnSfYZq9aaL6zHJx9Q1AKrcRnQJLX81pvJLeb3Zpy5` |
| `945c2d640f` | `bonding_curve_v2` | `execution_account_not_ready` | `HteSLY5LMiVYJijfgnCLEtZ4wJbDBJ83KRP9xXAWT8iu` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HteSLY5LMiVYJijfgnCLEtZ4wJbDBJ83KRP9xXAWT8iu` |
| `59f39ee1b0` | `bonding_curve_v2` | `execution_account_not_ready` | `EeDD11u1B5wEooqiLsmsrZmGuEx1DKghNSfRZ1Efsq62` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EeDD11u1B5wEooqiLsmsrZmGuEx1DKghNSfRZ1Efsq62` |
| `e1fd749db7` | `bonding_curve_v2` | `execution_account_not_ready` | `GZiD1QZR7Zh9PP6zDAELqSXGJFZbd4BEz5mhto81WRuw` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:GZiD1QZR7Zh9PP6zDAELqSXGJFZbd4BEz5mhto81WRuw` |
| `1d74525fbe` | `bonding_curve_v2` | `execution_account_not_ready` | `CpaHTM7qyRXFBFqVdhb9wksZHMSZoxbsi6BEM8tLKbNs` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CpaHTM7qyRXFBFqVdhb9wksZHMSZoxbsi6BEM8tLKbNs` |
| `6a1d7e657b` | `bonding_curve_v2` | `execution_account_not_ready` | `6Z3GvC3wsovqfmB3jafzGNd82DvLV9of711Ja57YBDXr` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:6Z3GvC3wsovqfmB3jafzGNd82DvLV9of711Ja57YBDXr` |
| `e062446fde` | `bonding_curve_v2` | `execution_account_not_ready` | `Az55dK7KdieuAVDou1oAR4nNCk3M7vrBZj5yGEdURGeg` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Az55dK7KdieuAVDou1oAR4nNCk3M7vrBZj5yGEdURGeg` |
| `a8f6cc8486` | `bonding_curve_v2` | `execution_account_not_ready` | `97vFh8MAypckFQeNEhmkvqX1TUcnCJxDZcgBXaKpj1AC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:97vFh8MAypckFQeNEhmkvqX1TUcnCJxDZcgBXaKpj1AC` |
| `42e3aec416` | `bonding_curve_v2` | `execution_account_not_ready` | `2MnE7EKdxyuxUKeaUn7DWMRyJrWtjG8EvXhXCXC1kSqC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2MnE7EKdxyuxUKeaUn7DWMRyJrWtjG8EvXhXCXC1kSqC` |
| `781ec19247` | `bonding_curve_v2` | `execution_account_not_ready` | `8ogDbN5uDWX3QZts7uPJu8qtevqjHaNzimTdw98ogXKY` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:8ogDbN5uDWX3QZts7uPJu8qtevqjHaNzimTdw98ogXKY` |
| `358d31a875` | `creator_vault` | `execution_account_not_ready` | `43haDevMgJg64UGUNehCjKoNEGdXeMdzjZb7DRpp2WaE` | `exact` | 0 | `execution_account_not_ready:creator_vault:43haDevMgJg64UGUNehCjKoNEGdXeMdzjZb7DRpp2WaE` |
| `26ac2bcf09` | `bonding_curve_v2` | `execution_account_not_ready` | `3uCUFy1rogLQ8udTmaGgAz1mKU6rM1kzJwEYGqe5F845` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3uCUFy1rogLQ8udTmaGgAz1mKU6rM1kzJwEYGqe5F845` |
| `b85881d80d` | `bonding_curve_v2` | `execution_account_not_ready` | `BEnZcJ58RnTgM15gRSPEwaL2Am2E7EgXR5qZGNfkVXjG` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BEnZcJ58RnTgM15gRSPEwaL2Am2E7EgXR5qZGNfkVXjG` |
| `db3864ac9d` | `bonding_curve_v2` | `execution_account_not_ready` | `J8UPEscFdkWjf6NFLQmqvm9cyorm8v3MikSsjhiCZTx8` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:J8UPEscFdkWjf6NFLQmqvm9cyorm8v3MikSsjhiCZTx8` |
| `f0010433c2` | `creator_vault` | `execution_account_not_ready` | `FPdYvjEUia12h534RMZTD4PRt3tnDt2Ue2DbjFuEQtA` | `exact` | 0 | `execution_account_not_ready:creator_vault:FPdYvjEUia12h534RMZTD4PRt3tnDt2Ue2DbjFuEQtA` |
| `977c278411` | `bonding_curve_v2` | `execution_account_not_ready` | `56mgv3E3H4bdKVofm4oWo8Vd9UwaXCmAJ58sePJrVJMr` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:56mgv3E3H4bdKVofm4oWo8Vd9UwaXCmAJ58sePJrVJMr` |
| `a1f230d571` | `bonding_curve_v2` | `execution_account_not_ready` | `6CGXrK5mPwd6geRofqkPYroUUJJ1kdeSDyw4AdPG8SYZ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:6CGXrK5mPwd6geRofqkPYroUUJJ1kdeSDyw4AdPG8SYZ` |
| `ff02e12648` | `bonding_curve_v2` | `execution_account_not_ready` | `3ECcW3ouzUmq4BXZ7uB6k4953pqcaQcVXtmcneCcoqR9` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3ECcW3ouzUmq4BXZ7uB6k4953pqcaQcVXtmcneCcoqR9` |
| `bf6ce0c465` | `bonding_curve_v2` | `execution_account_not_ready` | `CpdyZrhLGQoKPpGJ9tHZiqESinmMi1tvf64rBqL19V8k` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CpdyZrhLGQoKPpGJ9tHZiqESinmMi1tvf64rBqL19V8k` |
| `bbcf7f0748` | `bonding_curve_v2` | `execution_account_not_ready` | `Gs5QCFbdgrfNansQS1xHLRXXq2GzpuB1VgGqdAMoZHKv` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Gs5QCFbdgrfNansQS1xHLRXXq2GzpuB1VgGqdAMoZHKv` |
| `d010fb2133` | `bonding_curve_v2` | `execution_account_not_ready` | `9LfruMHuqpXGY443bFHkZ1DxipyErHQraaYF1zoYAekt` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9LfruMHuqpXGY443bFHkZ1DxipyErHQraaYF1zoYAekt` |
| `e02210405e` | `bonding_curve_v2` | `execution_account_not_ready` | `HNarnUimBU953XSD4U1ZJjHyZdw4mrmprewyU6qZ3sAk` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HNarnUimBU953XSD4U1ZJjHyZdw4mrmprewyU6qZ3sAk` |
| `4ad576d312` | `bonding_curve_v2` | `execution_account_not_ready` | `92wpqje2YjD7qn119Q7wfyYWKx49Gegsy266cniVk6Yj` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:92wpqje2YjD7qn119Q7wfyYWKx49Gegsy266cniVk6Yj` |
| `ba8a726080` | `bonding_curve_v2` | `execution_account_not_ready` | `4yw1D874K3n4YVsUvdu7CfFpaUjZAr8wyYZjjYSEnjzQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4yw1D874K3n4YVsUvdu7CfFpaUjZAr8wyYZjjYSEnjzQ` |
| `cd2237e68c` | `bonding_curve_v2` | `execution_account_not_ready` | `HUFYuDyootVfJSCk2kA4sUkMWicXyy7JMpbSBV2VxStT` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HUFYuDyootVfJSCk2kA4sUkMWicXyy7JMpbSBV2VxStT` |
| `0e5f7b6fdf` | `bonding_curve_v2` | `execution_account_not_ready` | `5hPjxH3kYhRyfg9GqQbY6wMRBhwdkfSK7HBSmc4dN24V` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5hPjxH3kYhRyfg9GqQbY6wMRBhwdkfSK7HBSmc4dN24V` |
| `2d3eec29cd` | `bonding_curve_v2` | `execution_account_not_ready` | `2HB757A2XZiVBsAUy4CiLwXBFwHPa6Y5FRvDYypihmwE` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2HB757A2XZiVBsAUy4CiLwXBFwHPa6Y5FRvDYypihmwE` |
| `d525740bae` | `bonding_curve_v2` | `execution_account_not_ready` | `FQXdM8MXqn5Qu6wSUMneVVgBAtpMdrSD3EMiNicRxPHo` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FQXdM8MXqn5Qu6wSUMneVVgBAtpMdrSD3EMiNicRxPHo` |
| `e60b76abb0` | `creator_vault` | `execution_account_not_ready` | `7SgfJrryT6JxrSTEh6KPVeHVvtziQRGoC523MpMhBpun` | `exact` | 0 | `execution_account_not_ready:creator_vault:7SgfJrryT6JxrSTEh6KPVeHVvtziQRGoC523MpMhBpun` |
| `98f7d09812` | `bonding_curve_v2` | `execution_account_not_ready` | `FhLvqHfc3gcT6ZEXTK9qfPDjn1BXmj4SHgoNr1Au1zVS` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FhLvqHfc3gcT6ZEXTK9qfPDjn1BXmj4SHgoNr1Au1zVS` |
| `567b38c8a7` | `bonding_curve_v2` | `execution_account_not_ready` | `9K3Fjx16sSEcZmi9ZXqoR9pCMUXkSczPyUF5SX92AurQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9K3Fjx16sSEcZmi9ZXqoR9pCMUXkSczPyUF5SX92AurQ` |
| `d9e406004e` | `bonding_curve_v2` | `execution_account_not_ready` | `8JXVy6JWzH8nnHZzraYUcghsv9nSE6DTuryoAbPYGYXE` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:8JXVy6JWzH8nnHZzraYUcghsv9nSE6DTuryoAbPYGYXE` |
| `cd8455d90b` | `bonding_curve_v2` | `execution_account_not_ready` | `88BE6L6MJe6ggwG3rBq1TebyozhgjQaUBAqzEETP3B7v` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:88BE6L6MJe6ggwG3rBq1TebyozhgjQaUBAqzEETP3B7v` |
| `71c5ee593f` | `bonding_curve_v2` | `execution_account_not_ready` | `5sG7rc7syoyFkPwWsGh82xRn74wNw9CVLxwjzCrxNcGH` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5sG7rc7syoyFkPwWsGh82xRn74wNw9CVLxwjzCrxNcGH` |
| `9598695369` | `bonding_curve_v2` | `execution_account_not_ready` | `FLNMNtxGfcYkGg73T1GiRHu394tt9RHdir4VtYEG3ycT` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FLNMNtxGfcYkGg73T1GiRHu394tt9RHdir4VtYEG3ycT` |
| `38715ff6ee` | `bonding_curve_v2` | `execution_account_not_ready` | `Bw37JE5FWUFUh3ahKi768cwBMNP2EpdiNoTcYYcE7uvM` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Bw37JE5FWUFUh3ahKi768cwBMNP2EpdiNoTcYYcE7uvM` |
| `d21809b71c` | `bonding_curve_v2` | `execution_account_not_ready` | `5znvTKZaZSdENjXP8iSzM3PEKKRRF49acvpoaHgysaBG` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5znvTKZaZSdENjXP8iSzM3PEKKRRF49acvpoaHgysaBG` |
| `e4cc10dafd` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

## Interpretation

The R15-r7 selected probes do not fail on payer, user-volume, or generic
`transaction_account` handling in this snapshot. They fail on strict routed
execution accounts.

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

Current J3I finding:

```text
candidate scan and dispatch quota are separated, but the scan plane itself can
be throttled by probe_scan_concurrency_limit_exceeded while strict readiness
prechecks are in flight
```

The current snapshot does not justify collection. It also does not fully prove
absence of execution-ready rows in the entire candidate universe, because
`probe_scan_concurrency_limit_exceeded` appears in `probe_skips.jsonl`.

If the still-running R15-r7 completes without probe transport/entry rows, the
next decision should focus on scan-plane pressure versus explicit
decision-time-safe execution-account readiness filtering. It should not
increase dispatch limits and should not weaken strict precheck.
