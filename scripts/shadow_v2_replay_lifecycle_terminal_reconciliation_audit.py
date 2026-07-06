#!/usr/bin/env python3
from __future__ import annotations

from shadow_v2_offline_audit_common import (
    emit,
    envelope,
    iter_lifecycle_rows,
    iter_replay_rows,
    parser,
    position_id,
)


def terminal_key(row: dict) -> str | None:
    return row.get("canonical_terminal_event_id") or row.get("terminal_truth_event_id")


def main() -> int:
    args = parser("Offline Shadow V2 replay/lifecycle terminal reconciliation audit").parse_args()
    replay_rows_seen = 0
    lifecycle_rows_seen = 0
    replay_malformed = 0
    lifecycle_malformed = 0
    replay_terminal_count = 0
    lifecycle_terminal_count = 0
    replay_open_count = 0
    lifecycle_open_count = 0
    lifecycle_by_position: dict[str, dict] = {}

    for row, row_malformed in iter_lifecycle_rows(args.scope_root) or ():
        if row_malformed or row is None:
            lifecycle_malformed += 1
            continue
        lifecycle_rows_seen += 1
        quality = envelope(row).get("quality")
        if quality == "LIFECYCLE_DERIVED_FROM_CANONICAL_TERMINAL":
            lifecycle_terminal_count += 1
            pos = position_id(row)
            if pos:
                lifecycle_by_position[pos] = row
        elif quality == "LIFECYCLE_DERIVED_OPEN_OR_BLOCKED":
            lifecycle_open_count += 1

    exact_join = 0
    terminal_event_match = 0
    terminal_reason_match = 0
    final_pnl_mark_match = 0
    final_pnl_executable_match = 0
    close_age_match = 0
    mismatch = 0
    missing_terminal_link = 0
    for r, row_malformed in iter_replay_rows(args.scope_root) or ():
        if row_malformed or r is None:
            replay_malformed += 1
            continue
        replay_rows_seen += 1
        quality = envelope(r).get("quality")
        if quality == "REPLAY_DERIVED_OPEN_OR_BLOCKED":
            replay_open_count += 1
            continue
        if quality != "REPLAY_DERIVED_FROM_CANONICAL_TERMINAL":
            continue
        replay_terminal_count += 1
        pos = position_id(r)
        l = lifecycle_by_position.get(pos)
        if not l:
            mismatch += 1
            continue
        exact_join += 1
        if terminal_key(r) and terminal_key(r) == terminal_key(l):
            terminal_event_match += 1
        else:
            missing_terminal_link += 1
        pairs = [
            ("terminal_reason", "terminal_reason"),
            ("terminal_pnl_mark_bps", "final_pnl_mark_bps"),
            ("terminal_pnl_executable_bps", "final_pnl_executable_bps"),
            ("close_age_ms", "close_age_ms"),
        ]
        for left, right in pairs:
            if r.get(left) == l.get(right):
                if left == "terminal_reason":
                    terminal_reason_match += 1
                elif left == "terminal_pnl_mark_bps":
                    final_pnl_mark_match += 1
                elif left == "terminal_pnl_executable_bps":
                    final_pnl_executable_match += 1
                elif left == "close_age_ms":
                    close_age_match += 1
            else:
                mismatch += 1
    if replay_malformed or lifecycle_malformed:
        verdict = "FAIL_REPLAY_LIFECYCLE_MISMATCH"
    elif mismatch or missing_terminal_link:
        verdict = "FAIL_REPLAY_LIFECYCLE_MISMATCH"
    elif replay_terminal_count and exact_join == replay_terminal_count:
        verdict = "PASS_REPLAY_LIFECYCLE_RECONCILED"
    else:
        verdict = "BLOCKED_REPLAY_LIFECYCLE_TERMINAL_INCOMPLETE"
    result = {
        "audit": "replay_lifecycle_terminal_reconciliation",
        "scope_root": args.scope_root,
        "shadow_replay_v2_rows": replay_rows_seen,
        "shadow_lifecycle_v2_rows": lifecycle_rows_seen,
        "replay_malformed_rows": replay_malformed,
        "lifecycle_malformed_rows": lifecycle_malformed,
        "replay_rows_derived_from_canonical_terminal": replay_terminal_count,
        "lifecycle_rows_derived_from_canonical_terminal": lifecycle_terminal_count,
        "replay_rows_open_or_blocked": replay_open_count,
        "lifecycle_rows_open_or_blocked": lifecycle_open_count,
        "exact_join_match_count": exact_join,
        "terminal_event_id_match_count": terminal_event_match,
        "terminal_reason_match_count": terminal_reason_match,
        "final_pnl_mark_match_count": final_pnl_mark_match,
        "final_pnl_executable_match_count": final_pnl_executable_match,
        "close_age_match_count": close_age_match,
        "mismatch_count": mismatch,
        "missing_terminal_link_count": missing_terminal_link,
        "verdict": verdict,
    }
    emit(result, args.pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
