#!/usr/bin/env python3
from __future__ import annotations

import csv
import json
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import shadow_v2_l2_e2_candidate_universe_denominator_repair as repair


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True))
            fh.write("\n")


def read_jsonl(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def read_metric_csv(path: Path) -> dict[str, str]:
    with path.open(encoding="utf-8", newline="") as fh:
        return {row["metric"]: row["value"] for row in csv.DictReader(fh)}


def event(candidate_id: str, mint: str, pool: str, birth_ts_ms: int) -> dict:
    return {
        "type": "NewPoolDetected",
        "candidate_id": candidate_id,
        "base_mint": mint,
        "pool_id": pool,
        "bonding_curve": pool,
        "quote_mint": "SOL",
        "birth_ts_ms": birth_ts_ms,
    }


def buy_decision(candidate_id: str, mint: str, pool: str) -> dict:
    return {
        "candidate_id": candidate_id,
        "base_mint": mint,
        "pool_id": pool,
        "verdict_type": "BUY",
        "decision_verdict_buy": True,
    }


def typed_reject(candidate_id: str, mint: str, pool: str, reason: str) -> dict:
    return {
        "candidate_id": candidate_id,
        "base_mint": mint,
        "pool_id": pool,
        "verdict_type": "REJECT_HARD_FAIL",
        "reason_code": reason,
    }


class ShadowV2L2E2CandidateUniverseDenominatorRepairTest(unittest.TestCase):
    def run_repair(
        self,
        root: Path,
        *,
        events: list[dict] | None = None,
        decisions: list[dict] | None = None,
    ) -> dict:
        event_path = root / "events" / "events.jsonl"
        decision_path = root / "decisions" / "gatekeeper_v2_decisions.jsonl"
        args = [
            "--root",
            str(root),
            "--scope",
            "unit-l2-e2",
            "--output-csv",
            str(root / "summary.csv"),
        ]
        if events is not None:
            write_jsonl(event_path, events)
            args.extend(["--events", str(event_path)])
        if decisions is not None:
            write_jsonl(decision_path, decisions)
            args.extend(["--decision-jsonl", str(decision_path)])
        return repair.run(repair.build_parser().parse_args(args))

    def test_event_level_denominator_ready_with_typed_reasons(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.run_repair(
                root,
                events=[
                    event("c-buy", "mint-buy", "pool-buy", 1_000),
                    event("c-shadow", "mint-shadow", "pool-shadow", 2_000),
                ],
                decisions=[
                    buy_decision("c-buy", "mint-buy", "pool-buy"),
                    {
                        "candidate_id": "c-shadow",
                        "base_mint": "mint-shadow",
                        "pool_id": "pool-shadow",
                        "verdict_type": "SHADOW_INSUFFICIENT_DATA",
                        "reason_code": "SHADOW_INSUFFICIENT_DATA",
                        "decision_reason": "TIMER_FIRED_INSUFFICIENT_DATA: tx=2/3 elapsed_ms=9750",
                    },
                ],
            )
            rows = read_jsonl(root / "datasets" / "selector" / "unit-l2-e2" / "candidate_universe_v1.jsonl")
            metrics = read_metric_csv(root / "summary.csv")

        self.assertEqual(report["final_verdict"], "CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E")
        self.assertEqual(len(rows), 2)
        self.assertEqual(report["candidate_universe_manifest"]["denominator_invariant_status"], "PASS")
        self.assertEqual(report["candidate_universe_manifest"]["decision_logs_created_denominator_rows"], 0)
        self.assertEqual(report["candidate_universe_manifest"]["candidate_ids_from_decision_only"], 0)
        self.assertEqual(report["l2_e_audit"]["unknown_reason_count"], 0)
        self.assertEqual(metrics["final_verdict"], "CANDIDATE_UNIVERSE_DENOMINATOR_READY_FOR_L2_E")

    def test_missing_events_blocks_without_decision_denominator_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.run_repair(
                root,
                decisions=[buy_decision("decision-only", "mint-only", "pool-only")],
            )
            rows = read_jsonl(root / "datasets" / "selector" / "unit-l2-e2" / "candidate_universe_v1.jsonl")

        self.assertEqual(report["final_verdict"], "BLOCKED_CANDIDATE_UNIVERSE_SOURCE_MISSING")
        self.assertEqual(rows, [])
        self.assertEqual(report["candidate_universe_manifest"]["decision_logs_created_denominator_rows"], 0)
        self.assertEqual(report["candidate_universe_manifest"]["candidate_ids_from_decision_only"], 0)

    def test_decision_join_missing_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.run_repair(
                root,
                events=[event("c-a", "mint-a", "pool-a", 1_000)],
                decisions=[buy_decision("c-other", "mint-other", "pool-other")],
            )

        self.assertEqual(report["final_verdict"], "BLOCKED_GATEKEEPER_DECISION_JOIN_MISSING")
        self.assertEqual(report["l2_e_audit"]["gatekeeper_decision_count"], 1)
        self.assertEqual(report["l2_e_audit"]["gatekeeper_decision_joined_to_candidate_count"], 0)

    def test_unknown_reject_reason_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.run_repair(
                root,
                events=[event("c-reject", "mint-reject", "pool-reject", 1_000)],
                decisions=[
                    {
                        "candidate_id": "c-reject",
                        "base_mint": "mint-reject",
                        "pool_id": "pool-reject",
                        "verdict_type": "REJECT",
                    }
                ],
            )

        self.assertEqual(report["final_verdict"], "BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS")
        self.assertEqual(report["l2_e_audit"]["unknown_reason_count"], 1)

    def test_typed_reject_only_blocks_threshold_starvation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.run_repair(
                root,
                events=[
                    event("c-a", "mint-a", "pool-a", 1_000),
                    event("c-b", "mint-b", "pool-b", 2_000),
                ],
                decisions=[
                    typed_reject("c-a", "mint-a", "pool-a", "HARD_FAIL_EXTREME_TOP3"),
                    typed_reject("c-b", "mint-b", "pool-b", "HARD_FAIL_PDD_ENTRY_DRIFT"),
                ],
            )

        self.assertEqual(report["final_verdict"], "BLOCKED_GATEKEEPER_THRESHOLD_STARVATION")
        self.assertEqual(report["l2_e_audit"]["gatekeeper_buy_count"], 0)
        self.assertEqual(report["l2_e_audit"]["unknown_reason_count"], 0)


if __name__ == "__main__":
    unittest.main()
