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


def find_decision_logs(root: Path, scope: str) -> list[Path]:
    decisions_dir = root / "logs" / "rollout" / scope / "decisions" / scope
    if not decisions_dir.exists():
        return []
    return sorted(decisions_dir.glob("**/gatekeeper_v2_decisions.jsonl"))


def nested_get(row: dict[str, Any], path: list[str]) -> Any:
    current: Any = row
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def first_dict(row: dict[str, Any], paths: list[list[str]]) -> dict[str, Any] | None:
    for path in paths:
        value = nested_get(row, path)
        if isinstance(value, dict):
            return value
    return None


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Summarize FSC/funding-source coverage from Gatekeeper decision logs."
    )
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--scope", required=True, help="Runtime scope under logs/rollout/<scope>.")
    parser.add_argument(
        "--decision-log",
        type=Path,
        action="append",
        help="Explicit gatekeeper_v2_decisions.jsonl path. Can be passed multiple times.",
    )
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON.")
    args = parser.parse_args()

    decision_logs = args.decision_log or find_decision_logs(args.root, args.scope)
    result: dict[str, Any] = {
        "scope": args.scope,
        "decision_logs": [],
        "totals": {
            "rows": 0,
            "funding_status": Counter(),
            "shadow_fsc_reason": Counter(),
            "shadow_fsc_policy_signal": Counter(),
            "known_source_count": Counter(),
            "buyer_sample_count": Counter(),
            "unknown_buyer_count": Counter(),
            "miss_reasons": Counter(),
        },
    }

    source_paths = [
        ["funding_source_v2"],
        ["v3_materialized_feature_snapshot", "sybil_resistance", "funding_source_v2"],
        ["materialized_feature_snapshot", "sybil_resistance", "funding_source_v2"],
    ]
    diagnostics_paths = [
        ["funding_source_diagnostics"],
        ["v3_materialized_feature_snapshot", "sybil_resistance", "funding_source_diagnostics"],
        ["materialized_feature_snapshot", "sybil_resistance", "funding_source_diagnostics"],
    ]

    for path in decision_logs:
        rows = 0
        funding_status = Counter()
        shadow_fsc_reason = Counter()
        shadow_fsc_policy_signal = Counter()
        known_source_count = Counter()
        buyer_sample_count = Counter()
        unknown_buyer_count = Counter()
        miss_reasons = Counter()
        for row in iter_jsonl(path):
            rows += 1
            fsv2 = first_dict(row, source_paths)
            if fsv2 is None:
                funding_status["missing"] += 1
            else:
                funding_status[str(fsv2.get("status"))] += 1
            reason = row.get("shadow_fsc_v2_reason_if_enabled")
            if reason is not None:
                shadow_fsc_reason[str(reason)] += 1
            signal = row.get("shadow_fsc_v2_policy_signal")
            if signal is not None:
                shadow_fsc_policy_signal[str(signal)] += 1
            diag = first_dict(row, diagnostics_paths)
            if diag is not None:
                for source_key, counter in (
                    ("known_source_count", known_source_count),
                    ("buyer_sample_count", buyer_sample_count),
                    ("unknown_buyer_count", unknown_buyer_count),
                ):
                    if source_key in diag:
                        counter[str(diag.get(source_key))] += 1
                reasons = diag.get("miss_reason_counts")
                if isinstance(reasons, list):
                    for item in reasons:
                        if isinstance(item, dict):
                            miss_reasons[str(item.get("reason"))] += int(item.get("count") or 0)

        log_summary = {
            "path": str(path),
            "rows": rows,
            "funding_status": funding_status.most_common(),
            "shadow_fsc_reason": shadow_fsc_reason.most_common(),
            "shadow_fsc_policy_signal": shadow_fsc_policy_signal.most_common(),
            "known_source_count": known_source_count.most_common(20),
            "buyer_sample_count": buyer_sample_count.most_common(20),
            "unknown_buyer_count": unknown_buyer_count.most_common(20),
            "miss_reasons": miss_reasons.most_common(20),
        }
        result["decision_logs"].append(log_summary)
        totals = result["totals"]
        totals["rows"] += rows
        totals["funding_status"].update(funding_status)
        totals["shadow_fsc_reason"].update(shadow_fsc_reason)
        totals["shadow_fsc_policy_signal"].update(shadow_fsc_policy_signal)
        totals["known_source_count"].update(known_source_count)
        totals["buyer_sample_count"].update(buyer_sample_count)
        totals["unknown_buyer_count"].update(unknown_buyer_count)
        totals["miss_reasons"].update(miss_reasons)

    totals = result["totals"]
    result["totals"] = {
        "rows": totals["rows"],
        "funding_status": totals["funding_status"].most_common(),
        "shadow_fsc_reason": totals["shadow_fsc_reason"].most_common(),
        "shadow_fsc_policy_signal": totals["shadow_fsc_policy_signal"].most_common(),
        "known_source_count": totals["known_source_count"].most_common(20),
        "buyer_sample_count": totals["buyer_sample_count"].most_common(20),
        "unknown_buyer_count": totals["unknown_buyer_count"].most_common(20),
        "miss_reasons": totals["miss_reasons"].most_common(20),
    }

    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"scope={args.scope}")
        print(f"decision_logs={len(decision_logs)}")
        for log in result["decision_logs"]:
            print(f"path={log['path']}")
            print(f"  rows={log['rows']}")
            print(f"  funding_status={log['funding_status']}")
            print(f"  shadow_fsc_reason={log['shadow_fsc_reason'][:10]}")
            print(f"  shadow_fsc_policy_signal={log['shadow_fsc_policy_signal']}")
            print(f"  known_source_count={log['known_source_count'][:10]}")
            print(f"  unknown_buyer_count={log['unknown_buyer_count'][:10]}")
            print(f"  miss_reasons={log['miss_reasons'][:10]}")
        print(f"totals={result['totals']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
