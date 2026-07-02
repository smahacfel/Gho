#!/usr/bin/env python3
from __future__ import annotations

from shadow_v2_offline_audit_common import (
    blocked_reasons,
    canonical_rows,
    count_present,
    emit,
    filter_schema,
    measurement_grade,
    nested_record,
    parser,
    quality,
)


def main() -> int:
    args = parser("Offline Shadow V2 exit reconstruction readiness audit").parse_args()
    rows, malformed = canonical_rows(args.scope_root)
    attempts = filter_schema(rows, "shadow_exit_attempt_v2")
    fills = filter_schema(rows, "shadow_exit_fill_v2")
    terminals = filter_schema(rows, "shadow_terminal_truth_v2")
    blocked = [
        row
        for row in fills
        if quality(row) == "BLOCKED_BY_DATA"
        or measurement_grade(row) == "BLOCKED_BY_DATA"
        or nested_record(row).get("fill_status") == "BLOCKED_BY_DATA"
    ]
    ready = [
        row
        for row in fills
        if nested_record(row).get("pool_state_before") is not None
        and nested_record(row).get("pool_state_after") is not None
        and nested_record(row).get("fill_price") is not None
        and nested_record(row).get("slippage_bps") is not None
        and nested_record(row).get("own_impact_bps") is not None
        and nested_record(row).get("fee_bps") is not None
        and row not in blocked
    ]
    if malformed or (attempts and not fills):
        verdict = "FAIL_EXIT_SCHEMA_OR_JOIN_BROKEN"
    elif blocked:
        verdict = "BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA"
    elif ready and len(ready) == len(fills):
        verdict = "PASS_EXIT_RECONSTRUCTION_READY"
    else:
        verdict = "FAIL_EXIT_SCHEMA_OR_JOIN_BROKEN"
    result = {
        "audit": "exit_reconstruction_readiness",
        "scope_root": args.scope_root,
        "malformed_canonical_rows": malformed,
        "shadow_exit_attempt_v2_rows": len(attempts),
        "shadow_exit_fill_v2_rows": len(fills),
        "exit_fill_blocked_by_data_rows": len(blocked),
        "exit_fills_with_pool_state_before_present": count_present(fills, "pool_state_before"),
        "exit_fills_with_pool_state_after_present": count_present(fills, "pool_state_after"),
        "exit_fills_with_fill_price_present": count_present(fills, "fill_price"),
        "exit_fills_with_slippage_bps_present": count_present(fills, "slippage_bps"),
        "exit_fills_with_own_impact_bps_present": count_present(fills, "own_impact_bps"),
        "exit_fills_with_fee_bps_present": count_present(fills, "fee_bps"),
        "exit_reconstruction_ready_count": len(ready),
        "exit_reconstruction_blocked_count": len(fills) - len(ready),
        "typed_blocked_reasons_frequency": dict(blocked_reasons(blocked).most_common()),
        "terminal_truth_rows": len(terminals),
        "terminal_truth_with_final_pnl_mark_bps": count_present(terminals, "final_pnl_mark_bps"),
        "terminal_truth_with_final_pnl_executable_bps": count_present(
            terminals, "final_pnl_executable_bps"
        ),
        "verdict": verdict,
    }
    emit(result, args.pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
