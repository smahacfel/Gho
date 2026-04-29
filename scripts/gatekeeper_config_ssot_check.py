#!/usr/bin/env python3
"""
Check that rollout documentation comments match the active Gatekeeper config.

This is intentionally narrow: it validates fields that operators read before a
shadow burn-in and that have caused drift in the Gatekeeper +40% workflow.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

try:
    import tomllib  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BRAIN_CONFIG = REPO_ROOT / "ghost-brain" / "ghost_brain_config.toml"
DEFAULT_ROLLOUT_CONFIG = REPO_ROOT / "configs" / "shadow-burnin.toml"


CHECKS = (
    {
        "name": "max_wait_time_ms",
        "toml_key": "max_wait_time_ms",
        "pattern": r"window:\s*max_wait_time_ms\s*=\s*(?P<value>\d+)",
        "type": int,
    },
    {
        "name": "max_interval_cv",
        "toml_key": "max_interval_cv",
        "pattern": r"interval_cv\s*<=\s*(?P<value>\d+(?:\.\d+)?)",
        "type": float,
    },
    {
        "name": "min_market_cap_sol",
        "toml_key": "min_market_cap_sol",
        "pattern": r"market_cap_sol\s*>=\s*(?P<value>\d+(?:\.\d+)?)",
        "type": float,
    },
    {
        "name": "max_dev_volume_ratio",
        "toml_key": "max_dev_volume_ratio",
        "pattern": r"dev_volume_ratio\s*<=\s*(?P<value>\d+(?:\.\d+)?)",
        "type": float,
    },
    {
        "name": "enable_prosperity_overlay",
        "toml_key": "enable_prosperity_overlay",
        "pattern": r"prosperity overlay (?P<value>ON|OFF)",
        "type": bool,
    },
)


def load_toml(path: Path) -> dict[str, Any]:
    if tomllib is None:
        raise RuntimeError("tomllib unavailable; use Python 3.11+")
    with path.open("rb") as fh:
        return tomllib.load(fh)


def parse_comment_value(text: str, pattern: str, value_type: type) -> Any:
    match = re.search(pattern, text, flags=re.IGNORECASE)
    if match is None:
        return None
    raw = match.group("value")
    if value_type is bool:
        return raw.upper() == "ON"
    return value_type(raw)


def values_equal(left: Any, right: Any) -> bool:
    if isinstance(left, float) or isinstance(right, float):
        try:
            return abs(float(left) - float(right)) < 1e-9
        except (TypeError, ValueError):
            return False
    return left == right


def run_check(brain_config: Path, rollout_config: Path) -> tuple[list[dict[str, Any]], bool]:
    config = load_toml(brain_config)
    gatekeeper = config.get("gatekeeper_v2", {})
    rollout_text = rollout_config.read_text(encoding="utf-8", errors="ignore")

    rows: list[dict[str, Any]] = []
    ok = True
    for check in CHECKS:
        active_value = gatekeeper.get(check["toml_key"])
        documented_value = parse_comment_value(rollout_text, check["pattern"], check["type"])
        passed = documented_value is not None and values_equal(active_value, documented_value)
        ok = ok and passed
        rows.append(
            {
                "name": check["name"],
                "active_value": active_value,
                "documented_value": documented_value,
                "pass": passed,
            }
        )
    return rows, ok


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--brain-config", type=Path, default=DEFAULT_BRAIN_CONFIG)
    parser.add_argument("--rollout-config", type=Path, default=DEFAULT_ROLLOUT_CONFIG)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    rows, ok = run_check(args.brain_config, args.rollout_config)
    if args.json:
        print(json.dumps({"pass": ok, "checks": rows}, ensure_ascii=False, sort_keys=True))
    else:
        for row in rows:
            status = "ok" if row["pass"] else "drift"
            print(
                f"{status:5s} {row['name']}: "
                f"active={row['active_value']} documented={row['documented_value']}"
            )
    raise SystemExit(0 if ok else 1)


if __name__ == "__main__":
    main()
