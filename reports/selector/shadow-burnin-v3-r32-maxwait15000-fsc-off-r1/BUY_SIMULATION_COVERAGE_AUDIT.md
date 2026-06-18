# BUY Simulation Coverage Audit

- scope: `shadow-burnin-v3-r32-maxwait15000-fsc-off-r1`
- decision_plane: `legacy_live`
- status: `AUDIT_COMPLETE_WITH_FINDINGS`
- buy_rows: `1531`
- simulation_success_coverage: `0.636839`
- simulation_attempt_coverage: `0.932071`
- not_executable_rate: `0.073155`
- simulation_failure_rate: `0.290007`
- position_limit_rate: `0.000000`

## Failure Classes

| class | count | rate |
|---|---:|---:|
| `SIM_FAIL_CUSTOM_2006` | 52 | 0.033965 |
| `SIM_FAIL_CUSTOM_6002` | 28 | 0.018289 |
| `ROUTE_INCOMPLETE_BCV2_MISSING` | 18 | 0.011757 |
| `ROUTE_INCOMPLETE_STATE_NOT_READY` | 7 | 0.004572 |
| `ROUTE_INCOMPLETE_CREATOR_OR_ACCOUNT_ROLE` | 60 | 0.039190 |
| `SIM_FAIL_TIMEOUT` | 336 | 0.219464 |
| `UNKNOWN_UNCLASSIFIED` | 55 | 0.035924 |

## Route Manifest Cache

- lookup_rows: `726`

| cache_status | count | rate |
|---|---:|---:|
| `ROUTE_CACHE_HIT_REUSED` | 28 | 0.018289 |
| `ROUTE_CACHE_MISS_NO_PRIOR_MANIFEST` | 99 | 0.064664 |
| `ROUTE_CACHE_MISS_CONFLICT` | 52 | 0.033965 |

## State Latch Contract

- contract_status: `FAIL`
- state_not_ready_rows: `26`
- state_latch_eligibility_checked_rows: `172`
- state_latch_attempted_rows: `68`
- state_latch_skipped_rows: `104`
- state_not_ready_latch_marker_missing_rows: `5`

| outcome | count |
|---|---:|
| `STATE_LATCH_RECOVERED_BY_FRESH_READ` | 68 |
| `STATE_LATCH_SKIPPED_REASON_NOT_STATE_NOT_READY` | 32 |
| `STATE_LATCH_SKIPPED_ROUTE_KIND_NOT_LEGACY_BUY` | 72 |

## Critical Markers

| marker | row_count | occurrence_count |
|---|---:|---:|
| `AccountNotFound` | 4 | 8 |
| `unsupported_legacy_buy_layout_requires_bcv2` | 0 | 0 |
| `Custom(6062)` | 0 | 0 |
| `custom program error: 0x17ae` | 0 | 0 |
| `0x17ae` | 0 | 0 |
| `ResourceExhausted` | 0 | 0 |
| `relative URL without a base` | 0 | 0 |

## Claim Boundaries

- offline audit only: `true`
- runtime changed: `false`
- r8 remains R2/GK-feature scope only: `true`
- every-BUY lifecycle claim: `false`

## Fail Reasons

- `INSUFFICIENT_SIMULATION_DIAGNOSTICS`
- `STATE_LATCH_MARKER_MISSING_FOR_STATE_NOT_READY`
- `critical_marker_present:AccountNotFound`
