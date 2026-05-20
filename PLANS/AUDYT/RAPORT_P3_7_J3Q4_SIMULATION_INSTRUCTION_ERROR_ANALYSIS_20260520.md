# RAPORT P3.7-J3Q4 Simulation Instruction Error Analysis

## Verdict

```text
J3Q4 diagnostic propagation: IMPLEMENTED
R15-r8m legacy error classification: PARTIAL because program/log fields are absent
R15-r8n fresh Q4 classification: PASS via dedicated r8n report
small bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

Rows without `simulation_error_program_id`, instruction account roles or log tail
are parsed but treated as diagnostic-limited, not fully understood.

Fresh R15-r8n probe transport rows include the Q4 diagnostic fields and are
classified in:

```text
PLANS/AUDYT/RAPORT_P3_7_J3Q4_R15_R8N_SIMULATION_ERROR_ANALYSIS_20260520.md
```

The r8n classification identified `InstructionError(3, Custom(6002))` as a
Pump.fun `TooMuchSolRequired` amount/slippage mismatch, not as an unknown
account-layout error.

## Summary

```text
transport_rows = 4
simulation_error_rows = 1
category_counts = {'simulation_account_layout_mismatch_unclassified_missing_q4_fields': 1}
custom_code_counts = {'2006': 1}
program_counts = {'unknown': 1}
```

## Error Rows

### `5540ebad88d4e4e42e32354bf5457a7a9c76a85e84e908195aaf8d36d8ab47eb`

```text
ab_record_id = 6APXsq7qPh1cVd92twgakv2CZnJCCMbqJgvKKAnyS4V:1779311805841:1779311807841:REJECT
pool_id = 6APXsq7qPh1cVd92twgakv2CZnJCCMbqJgvKKAnyS4V
base_mint = CBxXV2wmpNmPXYy3dpUn5xMyWPAPJUXeXzEZtvYdnSoM
probe_bucket = v3_reject_manipulation_contradiction
err = InstructionError(3, Custom(2006))
instruction_index = 3
custom_code = 2006
program_id = None
program_name = None
program_error_name = anchor_constraint_seeds_best_effort
category = simulation_account_layout_mismatch_unclassified_missing_q4_fields
route_kind = None
diagnostic_limit = transport row predates Q4 log/program/account-role propagation
```

## Decision

Rows without `simulation_error_program_id`, instruction account roles or log tail
are treated as pre-Q4 diagnostic-limited rows. Future probe transport rows now
carry the fields needed to classify whether the error is isolated, route-specific,
amount/slippage-related, or an account-layout mismatch.
