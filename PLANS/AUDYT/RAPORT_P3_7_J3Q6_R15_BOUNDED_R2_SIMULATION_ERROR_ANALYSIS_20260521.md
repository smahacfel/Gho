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
transport_rows = 4
simulation_error_rows = 0
category_counts = {}
custom_code_counts = {}
program_counts = {}
```

## Error Rows

No simulation error rows were found.
## Decision

Rows without `simulation_error_program_id`, instruction account roles or log tail
are treated as pre-Q4 diagnostic-limited rows. Future probe transport rows now
carry the fields needed to classify whether the error is isolated, route-specific,
amount/slippage-related, or an account-layout mismatch.
