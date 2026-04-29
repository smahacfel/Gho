#!/usr/bin/env python3
"""
Produce a shadow-bake go/no-go decision from replay and validation reports.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def best_policy(replay: dict[str, Any], min_selected: int) -> dict[str, Any] | None:
    for row in replay.get("top", []):
        if int(row.get("selected") or 0) >= min_selected:
            return row
    return None


def evaluate(
    replay: dict[str, Any],
    validation: dict[str, Any] | None,
    min_precision: float,
    max_rug_rate: float,
    min_selected: int,
    max_permutation_p: float,
) -> dict[str, Any]:
    policy = best_policy(replay, min_selected)
    checks: list[dict[str, Any]] = []
    if policy is None:
        checks.append({"name": "candidate_policy_exists", "pass": False, "detail": "no policy above min_selected"})
        return {"go": False, "policy": None, "checks": checks}

    precision = policy.get("precision")
    rug_rate = policy.get("rug_rate")
    checks.append(
        {
            "name": "precision_floor",
            "pass": isinstance(precision, (int, float)) and precision >= min_precision,
            "value": precision,
            "threshold": min_precision,
        }
    )
    checks.append(
        {
            "name": "rug_rate_ceiling",
            "pass": isinstance(rug_rate, (int, float)) and rug_rate <= max_rug_rate,
            "value": rug_rate,
            "threshold": max_rug_rate,
        }
    )
    checks.append(
        {
            "name": "sample_floor",
            "pass": int(policy.get("selected") or 0) >= min_selected,
            "value": int(policy.get("selected") or 0),
            "threshold": min_selected,
        }
    )

    if validation is not None:
        permutation = validation.get("permutation", {})
        p_value = permutation.get("p_value")
        checks.append(
            {
                "name": "permutation_sanity",
                "pass": isinstance(p_value, (int, float)) and p_value <= max_permutation_p,
                "value": p_value,
                "threshold": max_permutation_p,
            }
        )

    return {
        "go": all(check["pass"] for check in checks),
        "policy": policy,
        "checks": checks,
        "next_lane": "shadow_only" if all(check["pass"] for check in checks) else "blocked",
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--replay-report", required=True, type=Path)
    parser.add_argument("--validation-report", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--min-precision", type=float, default=0.65)
    parser.add_argument("--max-rug-rate", type=float, default=0.10)
    parser.add_argument("--min-selected", type=int, default=20)
    parser.add_argument("--max-permutation-p", type=float, default=0.05)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    replay = load_json(args.replay_report)
    validation = load_json(args.validation_report) if args.validation_report else None
    report = evaluate(
        replay,
        validation,
        min_precision=args.min_precision,
        max_rug_rate=args.max_rug_rate,
        min_selected=args.min_selected,
        max_permutation_p=args.max_permutation_p,
    )
    encoded = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)
    raise SystemExit(0 if report["go"] else 1)


if __name__ == "__main__":
    main()
