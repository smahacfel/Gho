#!/usr/bin/env python3
"""Fail-closed acceptance checks for the PR10-PR12 metric repair.

The script reads Gatekeeper buy-log/report JSONL rows and verifies the narrow
semantic regressions called out in PLAN_NAPRAWY_METRYK.md PR12. It is a replay
or reporting guard only; it must not be used as a runtime policy source.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


DES_NO_COMPARABLE_PAIRS = "DES_NO_COMPARABLE_PAIRS"
CPV_COVERAGE_WINDOW_UNAVAILABLE = "CPV_COVERAGE_WINDOW_UNAVAILABLE"


@dataclass(frozen=True)
class Violation:
    check: str
    row_id: str
    detail: str

    def as_dict(self) -> dict[str, str]:
        return {"check": self.check, "row_id": self.row_id, "detail": self.detail}


def iter_jsonl(path: Path) -> Iterable[tuple[int, dict[str, Any]]]:
    with path.open("r", encoding="utf-8") as handle:
        for line_no, raw in enumerate(handle, start=1):
            raw = raw.strip()
            if not raw:
                continue
            value = json.loads(raw)
            if isinstance(value, dict):
                yield line_no, value


def canonical_row(row: dict[str, Any]) -> dict[str, Any]:
    for key in ("buy_log", "gatekeeper_buy_log", "gatekeeper_buy"):
        nested = row.get(key)
        if isinstance(nested, dict):
            return nested
    return row


def row_id(path: Path | None, line_no: int, row: dict[str, Any]) -> str:
    identity = (
        row.get("pool_amm_id")
        or row.get("pool_id")
        or row.get("mint")
        or row.get("signature")
        or row.get("record_id")
        or "unknown"
    )
    if path is None:
        return f"row={line_no} id={identity}"
    return f"{path}:{line_no} id={identity}"


def string_set(value: Any) -> set[str]:
    if value is None:
        return set()
    if isinstance(value, str):
        return {part.strip() for part in value.replace("|", ",").split(",") if part.strip()}
    if isinstance(value, list):
        return {str(part).strip() for part in value if str(part).strip()}
    if isinstance(value, dict):
        return {str(key) for key, enabled in value.items() if enabled}
    return {str(value)}


def reason_set(row: dict[str, Any]) -> set[str]:
    reasons = string_set(row.get("sybil_metric_degraded_reasons"))
    reasons.update(string_set(row.get("degraded_reasons")))
    return reasons


def normalized_status(value: Any) -> str | None:
    if value is None:
        return None
    return str(value).strip().lower()


def is_zero_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and float(value) == 0.0


def check_fsc_degraded_v2(row: dict[str, Any], rid: str) -> list[Violation]:
    fsc_v2 = row.get("funding_source_v2")
    if not isinstance(fsc_v2, dict):
        return []

    status = normalized_status(fsc_v2.get("status"))
    if status in (None, "clean"):
        return []

    violations: list[Violation] = []
    if row.get("funding_source_concentration") is not None:
        violations.append(
            Violation(
                "fsc_degraded_v2_actionable",
                rid,
                f"funding_source_concentration present while funding_source_v2.status={status}",
            )
        )
    if row.get("shadow_fsc_v2_policy_signal") is True:
        violations.append(
            Violation(
                "fsc_degraded_v2_shadow_signal",
                rid,
                f"shadow_fsc_v2_policy_signal=true while funding_source_v2.status={status}",
            )
        )
    return violations


def check_des_no_comparable_pairs(row: dict[str, Any], rid: str) -> list[Violation]:
    if DES_NO_COMPARABLE_PAIRS not in reason_set(row):
        return []
    if not is_zero_number(row.get("demand_elasticity_score")):
        return []
    return [
        Violation(
            "des_no_comparable_pairs_zero_score",
            rid,
            "DES_NO_COMPARABLE_PAIRS must not materialize demand_elasticity_score=0.0",
        )
    ]


def check_dbia_solo_high_ftdi(row: dict[str, Any], rid: str) -> list[Violation]:
    flags = string_set(row.get("sybil_soft_flags"))
    patterns = string_set(row.get("sybil_interference_patterns"))
    violations: list[Violation] = []

    if "high_dbia" in flags and "low_ftdi" not in flags:
        illegal_patterns = sorted(
            pattern for pattern in patterns if pattern.startswith("HIGH_DBIA_LOW_FTDI")
        )
        if illegal_patterns:
            violations.append(
                Violation(
                    "dbia_solo_high_ftdi_structural_penalty",
                    rid,
                    f"high_dbia without low_ftdi emitted patterns={illegal_patterns}",
                )
            )

    ftdi = row.get("fee_topology_diversity_index")
    min_ftdi = row.get("min_fee_topology_diversity_index")
    if (
        isinstance(ftdi, (int, float))
        and isinstance(min_ftdi, (int, float))
        and float(ftdi) >= float(min_ftdi)
        and "low_ftdi" in flags
    ):
        violations.append(
            Violation(
                "dbia_solo_high_ftdi_low_ftdi_flag",
                rid,
                f"low_ftdi flag present even though ftdi={ftdi} >= min_ftdi={min_ftdi}",
            )
        )

    return violations


def check_cpv_without_coverage_window(row: dict[str, Any], rid: str) -> list[Violation]:
    if CPV_COVERAGE_WINDOW_UNAVAILABLE not in reason_set(row):
        return []
    if row.get("signer_cross_pool_velocity") is None:
        return []
    return [
        Violation(
            "cpv_without_coverage_window",
            rid,
            "signer_cross_pool_velocity present while CPV_COVERAGE_WINDOW_UNAVAILABLE is set",
        )
    ]


def analyze_rows(rows: Iterable[tuple[Path | None, int, dict[str, Any]]]) -> dict[str, Any]:
    violations: list[Violation] = []
    rows_checked = 0

    for path, line_no, raw_row in rows:
        row = canonical_row(raw_row)
        rows_checked += 1
        rid = row_id(path, line_no, row)
        violations.extend(check_fsc_degraded_v2(row, rid))
        violations.extend(check_des_no_comparable_pairs(row, rid))
        violations.extend(check_dbia_solo_high_ftdi(row, rid))
        violations.extend(check_cpv_without_coverage_window(row, rid))

    return {
        "artifact": "gatekeeper_metric_repair_acceptance_v1",
        "rows_checked": rows_checked,
        "pass": not violations,
        "violation_count": len(violations),
        "violations": [violation.as_dict() for violation in violations],
    }


def analyze_paths(paths: Iterable[Path]) -> dict[str, Any]:
    def rows() -> Iterable[tuple[Path | None, int, dict[str, Any]]]:
        for path in paths:
            for line_no, row in iter_jsonl(path):
                yield path, line_no, row

    return analyze_rows(rows())


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("jsonl", nargs="+", type=Path, help="Gatekeeper buy-log/report JSONL path")
    parser.add_argument("--output", type=Path, help="Optional JSON report output path")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    report = analyze_paths(args.jsonl)
    payload = json.dumps(report, indent=2, sort_keys=True)
    print(payload)
    if args.output:
        args.output.write_text(payload + "\n", encoding="utf-8")
    return 0 if report["pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
