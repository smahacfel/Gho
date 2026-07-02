#!/usr/bin/env python3
from __future__ import annotations

from collections import Counter, defaultdict

from shadow_v2_offline_audit_common import density_rows, distribution, emit, parser


VERDICTS = [
    "EVALUABLE_EXACT",
    "EVALUABLE_APPROX",
    "SPARSE_APPROX_ONLY",
    "NOT_EVALUABLE_NO_COVERAGE",
    "NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
]


def main() -> int:
    args = parser("Offline Shadow V2 path density horizon evaluability audit").parse_args()
    rows, malformed = density_rows(args.scope_root)
    by_horizon: dict[int, list[dict]] = defaultdict(list)
    unknown_horizon = 0
    for row in rows:
        horizon = row.get("horizon_ms")
        if isinstance(horizon, int):
            by_horizon[horizon].append(row)
        else:
            unknown_horizon += 1
    per_horizon = []
    any_evaluable = False
    for horizon in sorted(by_horizon):
        group = by_horizon[horizon]
        counts = Counter(str(r.get("verdict") or "UNKNOWN") for r in group)
        if counts["EVALUABLE_EXACT"] or counts["EVALUABLE_APPROX"] or counts["SPARSE_APPROX_ONLY"]:
            any_evaluable = True
        item = {
            "horizon_ms": horizon,
            "total_density_rows": len(group),
            **{f"{v}_count": counts[v] for v in VERDICTS},
            "coverage_points_distribution": distribution(r.get("coverage_points") for r in group),
            "path_points_distribution": distribution(r.get("path_points") for r in group),
            "median_interval_ms_distribution": distribution(r.get("median_interval_ms") for r in group),
            "max_interval_ms_distribution": distribution(r.get("max_interval_ms") for r in group),
        }
        per_horizon.append(item)
    if malformed or unknown_horizon:
        verdict = "FAIL_DENSITY_SCHEMA_OR_HORIZON_BROKEN"
    elif any_evaluable:
        verdict = "PASS_DENSITY_EVALUABLE_FOR_REQUIRED_HORIZONS"
    else:
        verdict = "BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS"
    result = {
        "audit": "path_density_horizon_evaluability",
        "scope_root": args.scope_root,
        "density_rows": len(rows),
        "malformed_density_rows": malformed,
        "unknown_horizon_rows": unknown_horizon,
        "horizon_count": len(by_horizon),
        "per_horizon": per_horizon,
        "verdict": verdict,
    }
    emit(result, args.pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
