# RAPORT P3.7-J3Q4 Simulation Instruction Error Analysis

## Verdict

```text
J3Q4 diagnostic propagation: IMPLEMENTED
error classification: PASS when program/log/account-role fields are present
rows predating Q4 fields: diagnostic-limited
small bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

Rows without `simulation_error_program_id`, instruction account roles or log tail
are parsed but treated as diagnostic-limited, not fully understood.

## Summary

```text
transport_rows = 25
simulation_error_rows = 25
category_counts = {'simulation_instruction_error': 25}
custom_code_counts = {'3005': 25}
program_counts = {'unknown': 25}
```

## Error Rows

### `77272dc2c492effe360510756b86bdf80366486e52c8507ef0e4089a92cd9174`

```text
ab_record_id = 8ikbDqrBDrTy7uF9ERRiMVAcS3XdQYJCjBXghTQWxdri:1779319467226:1779319469226:TIMEOUT
pool_id = 8ikbDqrBDrTy7uF9ERRiMVAcS3XdQYJCjBXghTQWxdri
base_mint = 7iRKkMB62QvLbTkzp6Nr3Q3dDnZc969TvoMDt4fppump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `c83cdd9397a194eb94a33282cf0b2a2b87b306d07755eb0ee25b75cfc7eaf540`

```text
ab_record_id = 2HsFjLVvNJF7e6zU6WPPfFiexRG6UF6bL3wrUuSVBYkt:1779319468767:1779319470767:REJECT
pool_id = 2HsFjLVvNJF7e6zU6WPPfFiexRG6UF6bL3wrUuSVBYkt
base_mint = H7egVCE1mTPmwmujvsR8r5DUNLYYCarSoY8ANx51pump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `905771cd8e88365eefbf4b213da4cf160738f99afdfb4071328ee5c218630f62`

```text
ab_record_id = CpFgZMYWtASisgSx81EfSrcNiHGSwi7dwcPPWWchjK3K:1779319499674:1779319501674:TIMEOUT
pool_id = CpFgZMYWtASisgSx81EfSrcNiHGSwi7dwcPPWWchjK3K
base_mint = D5hbZ9JN6arTpLwXd2QB5RzZbqfFADnD9RsvJWfCpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `412a751dfa6e360ef7fac07add9a147832df863666651caabe19af6cc083eb6d`

```text
ab_record_id = 9mepMoc7mF5ikUxEgtfmwbibyjmkVutdS76kAfD88HsA:1779319503121:1779319505121:TIMEOUT
pool_id = 9mepMoc7mF5ikUxEgtfmwbibyjmkVutdS76kAfD88HsA
base_mint = 64HDcL7TwUhcYdjEbvZ3asDDx9nQD3dGPqRLdQWfpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `cdfe6b3a3c9c9b7d3c12b3de7cb776e3b064b76614f5234731d3d3eb05b4694a`

```text
ab_record_id = 3KrnZuLGXabnfmhsQuo75mPuAqn5DM2iqWtEmmkU6Q46:1779319508404:1779319510404:TIMEOUT
pool_id = 3KrnZuLGXabnfmhsQuo75mPuAqn5DM2iqWtEmmkU6Q46
base_mint = 46QJmgjCgsfMXB9Wc4mJT5mLyPtMBWDxRrZfvhzPpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `84ecd2dc6f9e21dd5030933ee43772893192295d2f5e083a1f25f1b010a814e8`

```text
ab_record_id = D9nq8Ycuf5rGywMyNEmrphGDKeeUGDBAED5YH2u9wepd:1779319515310:1779319517310:TIMEOUT
pool_id = D9nq8Ycuf5rGywMyNEmrphGDKeeUGDBAED5YH2u9wepd
base_mint = 5gTrZjaQKuBNGM5cxAPvAoMSb17hDzkz3vjNkKTZpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `e913d794a578170546b63145861734a0590896b4e93efa46899e8cefc8b5fba6`

```text
ab_record_id = 4Y5rcLoVLiWAb63Sha5DMecotdcAAXChiVyJ6977ucaV:1779319525409:1779319527409:TIMEOUT
pool_id = 4Y5rcLoVLiWAb63Sha5DMecotdcAAXChiVyJ6977ucaV
base_mint = 6SESVDJX9y45H1Ce8wTjE6DBpP3TTx3hexfx4f4Vpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `9cd07935d5a0360bc482488d777d3b7329c8a11a33038fa9f1709bf0c648bc12`

```text
ab_record_id = D66CP8V7gLZJ1wSNGWfLYFTvTVETXuFD3P8qLhPkMxNy:1779319529743:1779319531743:TIMEOUT
pool_id = D66CP8V7gLZJ1wSNGWfLYFTvTVETXuFD3P8qLhPkMxNy
base_mint = Asbmyjg7KhohJSse2zgzzhyRiJyG7s8dCjBbC7CHpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `3203111893207a52bc0dd15443659f316f7ab25989318a650a3ed4c73a14865b`

```text
ab_record_id = 7FynxSq4gyJkfH6RQFtMvGhmeHDFTXbGg2QhAEGGtw6c:1779319531187:1779319533187:REJECT
pool_id = 7FynxSq4gyJkfH6RQFtMvGhmeHDFTXbGg2QhAEGGtw6c
base_mint = 6st55pFEeT7YXrvZUowGKrzAS6ZFGeyKsxp4s1Utpump
probe_bucket = active_reject_v3_pending
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `623aaf4de20ef121552facb23891d746829496095e43ce5a39bd3581c7660c00`

```text
ab_record_id = 6itJGJ8RP9XbkgU4Qpf9irJdvREiVaCKxxm7dqcgEv5h:1779319539010:1779319541010:TIMEOUT
pool_id = 6itJGJ8RP9XbkgU4Qpf9irJdvREiVaCKxxm7dqcgEv5h
base_mint = 6Q9E1iJ6UmWYkSWE6kwajPfA9syBk9qx13iv6m7Bpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `c6a678394ff691321fda2bd13993abdef2a1c6e38d5bcb6ac07b7afadf76c4ab`

```text
ab_record_id = 6JHgmzo31fVCuJdk9dCkAZmVPQ6CHVGFhS1QJza5izwm:1779319577819:1779319579819:TIMEOUT
pool_id = 6JHgmzo31fVCuJdk9dCkAZmVPQ6CHVGFhS1QJza5izwm
base_mint = 2ChTwHRoYRifmxL2D1XpiSHYZJvoMrpBrMGWjf6tViRL
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `d2e3d2d52c7fdc3b8a67a0a0c5c4207a8b901573457ca86aef4c073f484cca02`

```text
ab_record_id = CwtxX8VZGAjdUYqHJAZ6bz1MXxbamRbTyQKYF7B5LgMg:1779319580187:1779319582187:TIMEOUT
pool_id = CwtxX8VZGAjdUYqHJAZ6bz1MXxbamRbTyQKYF7B5LgMg
base_mint = F1bBqcXCdSar7A95tXfHtPBbwzZstNEv4UyGFFkGpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `546067cfbecd0d81117457a74c7c52f5891f36de8fdda75a22050fbbc9d1a18f`

```text
ab_record_id = 4dYEWEMJ7p338EFch5ayFSN4bEsPQQsRJcxqCNjddM4E:1779319582452:1779319584452:REJECT
pool_id = 4dYEWEMJ7p338EFch5ayFSN4bEsPQQsRJcxqCNjddM4E
base_mint = ApeiqeDsubsuy62ubGLK9gd1sSie37mrSmkaCJHopump
probe_bucket = active_reject_v3_pending
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `d9f861eda0cd58d66b6b67e93fa58f6d02fbe2780691f2b7237e1f7e977f6a96`

```text
ab_record_id = G5QCdmNkVemznVtDZqoTvkMUGy5Y2vbkhZip2vFoJKcg:1779319593156:1779319595156:TIMEOUT
pool_id = G5QCdmNkVemznVtDZqoTvkMUGy5Y2vbkhZip2vFoJKcg
base_mint = 9KZP2FqB8DfztFX56U9GUBN9wgPqtxmzayWLe4bqpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `25cabc962ce58832210ac63ff4bd5c36974e87ac224fc4635a6ffa4fdd2fbb8f`

```text
ab_record_id = 9YCezbw9fCyQMkiFnHEG6Gcwx6vxMJwhN4xYNcs2Mo1R:1779319600915:1779319602915:REJECT
pool_id = 9YCezbw9fCyQMkiFnHEG6Gcwx6vxMJwhN4xYNcs2Mo1R
base_mint = 5bV4Pka5yQF1h6KMoJ9HYQoHDbQWo9PTuNfDZ4Pppump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `3009d76dea7fa91757ae47d0b9c1a2d52ef6f60529fef07ebcc6a473e4f1c2ba`

```text
ab_record_id = HUovUaiAQVTfQJod6j9hiuc8Yq2smMJPo6D3jktidYaF:1779319621995:1779319623995:REJECT
pool_id = HUovUaiAQVTfQJod6j9hiuc8Yq2smMJPo6D3jktidYaF
base_mint = DrjYR9AMF4uKMv8AAs9mQNT39KTWw8mjo4eANtRjpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `a4992936b59903c8764dc539a5e2c4c90657605f2e40499013f04f8a81da2890`

```text
ab_record_id = 9cj9T7SZGJ3hnqCGeziNfxgQT1XVwZnQdyyMWDK4UPqy:1779319624353:1779319626353:TIMEOUT
pool_id = 9cj9T7SZGJ3hnqCGeziNfxgQT1XVwZnQdyyMWDK4UPqy
base_mint = DAQP2j5PMggccrhLtKKHDobN29z5o8xePeELSsCfpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `35f71e9bc437b9c3937c75c40e71385f8c2ccf5bd150861d5ca51bec3f8617e0`

```text
ab_record_id = 9VoUnbyKDi9k3w9mweqQnq1VtuRVYs7pRJkHCYkb4zTj:1779319626935:1779319628935:TIMEOUT
pool_id = 9VoUnbyKDi9k3w9mweqQnq1VtuRVYs7pRJkHCYkb4zTj
base_mint = ATt5JAwT4Cga6qerpsMvzT6MvqS91bFFM6CYCCrEpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `0b1687878749bbdce840be3c2f59ffca5fac45477bfa87cc5224c2c67bfe5042`

```text
ab_record_id = 2UcAQK3XZsAYUKdJnK9PdCYmo2dwXdJRZEzR5coEYsbT:1779319629114:1779319631114:TIMEOUT
pool_id = 2UcAQK3XZsAYUKdJnK9PdCYmo2dwXdJRZEzR5coEYsbT
base_mint = GTxoxj8xnTtd7JyDQDZiVnRUd16kLpa36p7TUXcdW9JQ
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `fb58a9fe1f43ae6e3b30102cf97827ff5694787edaaf84d7acaabc678cfde663`

```text
ab_record_id = 8diy7QCTf91fBi8DuvV4pDj8FpyeiMvdGVLezZGASAwL:1779319642307:1779319644307:TIMEOUT
pool_id = 8diy7QCTf91fBi8DuvV4pDj8FpyeiMvdGVLezZGASAwL
base_mint = 223fgPr1vzsPxD8uxE3gQVFYUqUX4BYTFpLcjWfJpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `c713ec21dbf204f9e8da26b753ef312e456c4ed04309b9869d9822fa454ef088`

```text
ab_record_id = 3QseY5wFG3hf4HHCVtxSM2oyJk7iQoRq2Lp5LZW1Q4Ug:1779319673810:1779319675810:TIMEOUT
pool_id = 3QseY5wFG3hf4HHCVtxSM2oyJk7iQoRq2Lp5LZW1Q4Ug
base_mint = UHTEvsnJVaCtm1RGZCA1ip5QEWRsrtgCXu4BsHypump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `a9ade395cac8923d8e9f885440e692d8896a61eea49e86d9572975b2a7d1c18e`

```text
ab_record_id = BbAkdVGBjXwHnwZLRqaEMQPAiJomBBzgaWB8QuhK8AL3:1779319684909:1779319686909:TIMEOUT
pool_id = BbAkdVGBjXwHnwZLRqaEMQPAiJomBBzgaWB8QuhK8AL3
base_mint = GeQBJiQq8WvE9fooAxMr6JPEYfY4T4MJzhnTD1Wmpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `938ad8e9846aafa3aadd19f2711c7ddf5faa71ffd29b18605bba706ab584b645`

```text
ab_record_id = 2QLFGLnH4nuDdYx38gHxczXis8rbEHUmZkDgMGXTB7D4:1779319691929:1779319693929:TIMEOUT
pool_id = 2QLFGLnH4nuDdYx38gHxczXis8rbEHUmZkDgMGXTB7D4
base_mint = DvzVivvkTvw5vUCruKgTpfjPbxNsxWvMHihXwKCbpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `811dffc41a24f1a6cf7fd97f1e1b9394d5710be437cce02c37b7431322f8f9a6`

```text
ab_record_id = AuUpzpEdU3LSzXy7vMo7Lrau6Es974nPcgkxqrv9VTKL:1779319695129:1779319697129:REJECT
pool_id = AuUpzpEdU3LSzXy7vMo7Lrau6Es974nPcgkxqrv9VTKL
base_mint = 9tkayCFZJ1KBnxrY2NcRHWzfpjTrnuSea1vRKsokpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

### `f57af24b5335425f6cdc264ad17fa29dcfc4fe230e0392bfa5d44b4ad88d6b6e`

```text
ab_record_id = EFLTpAURwSWnFbd7gpChw15F8g2t9Sw5ufUi5FpTfcnL:1779319710538:1779319712538:TIMEOUT
pool_id = EFLTpAURwSWnFbd7gpChw15F8g2t9Sw5ufUi5FpTfcnL
base_mint = APC3NvAu93qmfhnBXkNKADHgVHvCzj8NG3i951ugpump
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(3005))
instruction_index = 3
custom_code = 3005
program_id = None
program_name = None
program_error_name = unknown_custom_program_error
category = simulation_instruction_error
route_kind = None
buy_variant = None
token_param_role = None
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
entry_token_amount_raw = None
min_tokens_out = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

## Decision

Rows without `simulation_error_program_id`, instruction account roles or log tail
are treated as pre-Q4 diagnostic-limited rows. Future probe transport rows now
carry the fields needed to classify whether the error is isolated, route-specific,
amount/slippage-related, or an account-layout mismatch.
