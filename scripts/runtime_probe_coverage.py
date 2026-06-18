#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any, Iterable


def iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    if not path.exists():
        return
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict):
                yield row


def candidate_id(row: dict[str, Any]) -> str | None:
    value = row.get("candidate_id")
    return value if isinstance(value, str) and value else None


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Summarize BUY shadow and counterfactual probe simulation coverage for a runtime scope."
    )
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--scope", required=True, help="Runtime scope under logs/shadow_run/<scope>.")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")
    args = parser.parse_args()

    shadow_dir = args.root / "logs" / "shadow_run" / args.scope
    paths = {
        "probe_selection": shadow_dir / "probe_selection.jsonl",
        "probe_transport": shadow_dir / "probe_transport.jsonl",
        "probe_entries": shadow_dir / "probe_shadow_entries.jsonl",
        "probe_lifecycle": shadow_dir / "probe_shadow_lifecycle.jsonl",
        "probe_skips": shadow_dir / "probe_skips.jsonl",
        "buy_entries": shadow_dir / "shadow_entries.jsonl",
        "buy_lifecycle": shadow_dir / "shadow_lifecycle.jsonl",
    }

    result: dict[str, Any] = {
        "scope": args.scope,
        "shadow_dir": str(shadow_dir),
        "files": {},
        "probe": {},
        "buy": {},
    }
    sets: dict[str, set[str]] = {key: set() for key in paths}
    counters: dict[str, Counter[str]] = {key: Counter() for key in paths}
    line_counts: dict[str, int] = {}

    for key, path in paths.items():
        lines = 0
        for row in iter_jsonl(path):
            lines += 1
            cid = candidate_id(row)
            if cid:
                sets[key].add(cid)
            for field in ("event_type", "record_type", "execution_outcome", "probe_bucket", "skip_reason", "close_reason", "truth_status"):
                value = row.get(field)
                if value is not None:
                    counters[key][f"{field}={value}"] += 1
        line_counts[key] = lines
        result["files"][key] = {
            "path": str(path),
            "exists": path.exists(),
            "lines": lines,
            "unique_candidates": len(sets[key]),
            "top_counts": counters[key].most_common(20),
        }

    selected = sets["probe_selection"]
    transported = sets["probe_transport"]
    simulated_transport: set[str] = set()
    transport_outcomes = Counter()
    simulated_by_bucket = Counter()
    for row in iter_jsonl(paths["probe_transport"]):
        cid = candidate_id(row)
        outcome = row.get("execution_outcome")
        transport_outcomes[str(outcome)] += 1
        if cid and outcome == "counterfactual_shadow_probe_simulated":
            simulated_transport.add(cid)
            simulated_by_bucket[str(row.get("probe_bucket"))] += 1

    lifecycle = sets["probe_lifecycle"]
    terminal = set()
    close_reasons = Counter()
    for row in iter_jsonl(paths["probe_lifecycle"]):
        cid = candidate_id(row)
        if row.get("record_type") == "position_closed" and cid:
            terminal.add(cid)
            close_reasons[str(row.get("close_reason"))] += 1

    buy_entries = sets["buy_entries"]
    buy_lifecycle = sets["buy_lifecycle"]
    buy_terminal = set()
    buy_close_reasons = Counter()
    for row in iter_jsonl(paths["buy_lifecycle"]):
        cid = candidate_id(row)
        if row.get("record_type") == "position_closed" and cid:
            buy_terminal.add(cid)
            buy_close_reasons[str(row.get("close_reason"))] += 1

    def ratio(numerator: int, denominator: int) -> float | None:
        return numerator / denominator if denominator else None

    result["probe"] = {
        "selected_candidates": len(selected),
        "transported_candidates": len(transported),
        "simulated_transport_candidates": len(simulated_transport),
        "lifecycle_candidates": len(lifecycle),
        "terminal_closed_candidates": len(terminal),
        "transport_vs_selected": ratio(len(transported), len(selected)),
        "simulated_vs_selected": ratio(len(simulated_transport), len(selected)),
        "lifecycle_vs_selected": ratio(len(lifecycle), len(selected)),
        "lifecycle_vs_simulated_transport": ratio(len(lifecycle), len(simulated_transport)),
        "terminal_vs_lifecycle": ratio(len(terminal), len(lifecycle)),
        "transport_outcomes": transport_outcomes.most_common(),
        "simulated_by_bucket": simulated_by_bucket.most_common(),
        "close_reasons": close_reasons.most_common(),
    }
    result["buy"] = {
        "entry_candidates": len(buy_entries),
        "lifecycle_candidates": len(buy_lifecycle),
        "terminal_closed_candidates": len(buy_terminal),
        "lifecycle_vs_entries": ratio(len(buy_lifecycle), len(buy_entries)),
        "terminal_vs_entries": ratio(len(buy_terminal), len(buy_entries)),
        "terminal_vs_lifecycle": ratio(len(buy_terminal), len(buy_lifecycle)),
        "close_reasons": buy_close_reasons.most_common(),
    }

    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"scope={args.scope}")
        print(f"shadow_dir={shadow_dir}")
        print("files:")
        for key, info in result["files"].items():
            print(f"  {key}: lines={info['lines']} unique_candidates={info['unique_candidates']} path={info['path']}")
        print("probe_coverage:")
        for key, value in result["probe"].items():
            print(f"  {key}={value}")
        print("buy_coverage:")
        for key, value in result["buy"].items():
            print(f"  {key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
