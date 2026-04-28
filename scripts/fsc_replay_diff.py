#!/usr/bin/env python3
"""Compare FSC-relevant surfaces between two gatekeeper_v2_buys.jsonl artifacts.

This helper is intentionally narrow: it validates replay/bake expectations for
PR-4 FSC unlock work without pretending to diff every buy-log field.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

EXIT_OK = 0
EXIT_DIFF = 2
FSC_REASON_PREFIX = "FSC_"
SAMPLE_LIMIT = 8


@dataclass(frozen=True)
class BuySurface:
    key: str
    pool_id: str
    verdict_type: str | None
    decision_verdict_buy: bool | None
    funding_source_concentration: float | None
    sybil_metric_degraded_reasons: tuple[str, ...]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare two gatekeeper_v2_buys.jsonl artifacts and verify FSC bake "
            "expectations for neutral-disabled or authoritative-enabled replay runs."
        )
    )
    parser.add_argument(
        "--mode",
        choices=("neutral-disabled", "authoritative-enabled"),
        required=True,
        help=(
            "neutral-disabled: expect zero verdict drift and zero FSC drift. "
            "authoritative-enabled: expect zero verdict drift and only FSC-surface drift."
        ),
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        required=True,
        help="Baseline gatekeeper_v2_buys.jsonl artifact.",
    )
    parser.add_argument(
        "--candidate",
        type=Path,
        required=True,
        help="Candidate gatekeeper_v2_buys.jsonl artifact to compare against baseline.",
    )
    return parser.parse_args()


def stable_join_key(payload: dict[str, Any], *, path: Path, line_no: int) -> str:
    join_key = payload.get("join_key")
    if isinstance(join_key, str) and join_key.strip():
        return join_key.strip()

    pool_id = payload.get("pool_id")
    base_mint = payload.get("base_mint") or "unknown_base_mint"
    first_seen = payload.get("first_seen_ts_ms")
    if first_seen is None:
        first_seen = payload.get("observation_start_ts_ms")
    if first_seen is None:
        first_seen = payload.get("timestamp")

    if not isinstance(pool_id, str) or not pool_id.strip() or first_seen is None:
        raise ValueError(
            f"{path}:{line_no}: missing join_key and fallback identity "
            "(need pool_id plus first_seen_ts_ms/observation_start_ts_ms/timestamp)"
        )
    return f"{pool_id.strip()}:{base_mint}:{first_seen}"


def normalize_optional_float(value: Any) -> float | None:
    if value is None:
        return None
    if isinstance(value, (int, float)):
        return float(value)
    raise ValueError(f"expected number or null, got {value!r}")


def load_surfaces(path: Path) -> dict[str, BuySurface]:
    if not path.is_file():
        raise FileNotFoundError(f"buy-log artifact not found: {path}")

    rows: dict[str, BuySurface] = {}
    with path.open("r", encoding="utf-8") as handle:
        for line_no, raw_line in enumerate(handle, start=1):
            line = raw_line.strip()
            if not line:
                continue
            try:
                payload = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"{path}:{line_no}: invalid JSONL row: {exc}") from exc

            key = stable_join_key(payload, path=path, line_no=line_no)
            if key in rows:
                raise ValueError(f"{path}:{line_no}: duplicate join key {key!r}")

            reasons = payload.get("sybil_metric_degraded_reasons") or []
            if not isinstance(reasons, list) or not all(isinstance(item, str) for item in reasons):
                raise ValueError(f"{path}:{line_no}: invalid sybil_metric_degraded_reasons payload")

            rows[key] = BuySurface(
                key=key,
                pool_id=str(payload.get("pool_id", "")),
                verdict_type=payload.get("verdict_type"),
                decision_verdict_buy=payload.get("decision_verdict_buy"),
                funding_source_concentration=normalize_optional_float(
                    payload.get("funding_source_concentration")
                ),
                sybil_metric_degraded_reasons=tuple(sorted(set(reasons))),
            )
    return rows


def floats_equal(left: float | None, right: float | None) -> bool:
    if left is None or right is None:
        return left is right
    return math.isclose(left, right, rel_tol=1e-12, abs_tol=1e-12)


def trimmed_samples(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return items[:SAMPLE_LIMIT]


def main() -> int:
    args = parse_args()
    baseline = load_surfaces(args.baseline.resolve())
    candidate = load_surfaces(args.candidate.resolve())

    baseline_keys = set(baseline)
    candidate_keys = set(candidate)
    shared_keys = sorted(baseline_keys & candidate_keys)
    missing_keys = sorted(baseline_keys - candidate_keys)
    extra_keys = sorted(candidate_keys - baseline_keys)

    verdict_drifts: list[dict[str, Any]] = []
    fsc_value_drifts: list[dict[str, Any]] = []
    fsc_reason_drifts: list[dict[str, Any]] = []
    unexpected_reason_drifts: list[dict[str, Any]] = []

    for key in shared_keys:
        base = baseline[key]
        cand = candidate[key]

        if (
            base.verdict_type != cand.verdict_type
            or base.decision_verdict_buy != cand.decision_verdict_buy
        ):
            verdict_drifts.append(
                {
                    "join_key": key,
                    "baseline_verdict_type": base.verdict_type,
                    "candidate_verdict_type": cand.verdict_type,
                    "baseline_decision_verdict_buy": base.decision_verdict_buy,
                    "candidate_decision_verdict_buy": cand.decision_verdict_buy,
                }
            )

        if not floats_equal(
            base.funding_source_concentration, cand.funding_source_concentration
        ):
            fsc_value_drifts.append(
                {
                    "join_key": key,
                    "baseline_funding_source_concentration": base.funding_source_concentration,
                    "candidate_funding_source_concentration": cand.funding_source_concentration,
                }
            )

        reason_diff = sorted(
            set(base.sybil_metric_degraded_reasons)
            ^ set(cand.sybil_metric_degraded_reasons)
        )
        if reason_diff:
            sample = {
                "join_key": key,
                "baseline_reasons": list(base.sybil_metric_degraded_reasons),
                "candidate_reasons": list(cand.sybil_metric_degraded_reasons),
                "diff_reasons": reason_diff,
            }
            fsc_reason_drifts.append(sample)
            if any(not reason.startswith(FSC_REASON_PREFIX) for reason in reason_diff):
                unexpected_reason_drifts.append(sample)

    summary = {
        "mode": args.mode,
        "baseline_path": str(args.baseline.resolve()),
        "candidate_path": str(args.candidate.resolve()),
        "baseline_records": len(baseline),
        "candidate_records": len(candidate),
        "shared_records": len(shared_keys),
        "missing_records": len(missing_keys),
        "extra_records": len(extra_keys),
        "verdict_drift_count": len(verdict_drifts),
        "fsc_value_drift_count": len(fsc_value_drifts),
        "fsc_reason_drift_count": len(fsc_reason_drifts),
        "unexpected_reason_drift_count": len(unexpected_reason_drifts),
        "samples": {
            "missing_records": trimmed_samples([{"join_key": key} for key in missing_keys]),
            "extra_records": trimmed_samples([{"join_key": key} for key in extra_keys]),
            "verdict_drifts": trimmed_samples(verdict_drifts),
            "fsc_value_drifts": trimmed_samples(fsc_value_drifts),
            "fsc_reason_drifts": trimmed_samples(fsc_reason_drifts),
            "unexpected_reason_drifts": trimmed_samples(unexpected_reason_drifts),
        },
    }

    failures: list[str] = []
    if missing_keys:
        failures.append("candidate artifact is missing baseline join keys")
    if extra_keys:
        failures.append("candidate artifact contains extra join keys not present in baseline")
    if verdict_drifts:
        failures.append("verdict drift detected")

    if args.mode == "neutral-disabled":
        if fsc_value_drifts:
            failures.append("neutral-disabled replay must not change funding_source_concentration")
        if fsc_reason_drifts:
            failures.append(
                "neutral-disabled replay must not change sybil_metric_degraded_reasons"
            )
    elif unexpected_reason_drifts:
        failures.append(
            "authoritative-enabled replay changed non-FSC degraded reasons; drift is not FSC-only"
        )

    summary["result"] = "PASS" if not failures else "FAIL"
    summary["failure_reasons"] = failures
    print(json.dumps(summary, indent=2, sort_keys=True))
    return EXIT_OK if not failures else EXIT_DIFF


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # explicit top-level failure for operator UX
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
