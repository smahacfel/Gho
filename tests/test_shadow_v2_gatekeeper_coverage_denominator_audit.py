#!/usr/bin/env python3
from __future__ import annotations

import csv
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
AUDIT_SCRIPT = REPO_ROOT / "scripts" / "shadow_v2_gatekeeper_coverage_denominator_audit.py"


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True))
            fh.write("\n")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


def write_summary(path: Path, metrics: dict[str, str | int]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=["metric", "value", "notes"])
        writer.writeheader()
        for metric, value in metrics.items():
            writer.writerow({"metric": metric, "value": value, "notes": "fixture"})


def candidate(candidate_id: str, mint: str, pool: str) -> dict:
    return {
        "candidate_id": candidate_id,
        "base_mint": mint,
        "pool_id": pool,
        "candidate_universe_status": "ok",
        "cohort_in_scope": True,
        "stream_completeness_ok": True,
    }


def manifest() -> dict:
    return {
        "status": "ok",
        "denominator_invariant_status": "PASS",
        "decision_logs_created_denominator_rows": 0,
        "candidate_ids_from_decision_only": 0,
    }


class ShadowV2GatekeeperCoverageDenominatorAuditTest(unittest.TestCase):
    def run_audit(
        self,
        root: Path,
        *,
        candidate_universe: Path | None = None,
        candidate_manifest: Path | None = None,
        decision_jsonl: Path | None = None,
        summary_csv: Path | None = None,
    ) -> dict:
        args = [
            sys.executable,
            str(AUDIT_SCRIPT),
            "--candidate-universe",
            str(candidate_universe or root / "missing_candidate_universe_v1.jsonl"),
        ]
        if candidate_manifest is not None:
            args.extend(["--candidate-manifest", str(candidate_manifest)])
        if decision_jsonl is not None:
            args.extend(["--decision-jsonl", str(decision_jsonl)])
        if summary_csv is not None:
            args.extend(["--summary-csv", str(summary_csv)])
        result = subprocess.run(
            args,
            cwd=REPO_ROOT,
            check=True,
            text=True,
            capture_output=True,
        )
        return json.loads(result.stdout)

    def test_missing_candidate_universe_blocks_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            result = self.run_audit(root)

        self.assertEqual(
            result["final_verdict"],
            "BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN",
        )
        self.assertIn("candidate_universe_file_missing", result["denominator_contract_failures"])
        self.assertIn("candidate_universe_empty", result["denominator_contract_failures"])
        self.assertIn("eligible_denominator_zero", result["denominator_contract_failures"])

    def test_known_denominator_with_typed_reasons_passes_l2e_metrology(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            manifest_path = root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            summary = root / "shadow_v2_summary.csv"
            write_jsonl(
                candidates,
                [
                    candidate("c-buy", "mint-buy", "pool-buy"),
                    candidate("c-reject", "mint-reject", "pool-reject"),
                ],
            )
            write_json(manifest_path, manifest())
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "c-buy",
                        "base_mint": "mint-buy",
                        "pool_id": "pool-buy",
                        "verdict_type": "BUY",
                        "decision_verdict_buy": True,
                    },
                    {
                        "candidate_id": "c-reject",
                        "base_mint": "mint-reject",
                        "pool_id": "pool-reject",
                        "verdict_type": "REJECT_HARD_FAIL",
                        "reason_code": "HARD_FAIL_EXTREME_TOP3",
                    },
                ],
            )
            write_summary(
                summary,
                {
                    "entry_execution_label_grade_RESEARCH_CANDIDATE_count": 1,
                    "exit_execution_label_grade_RESEARCH_CANDIDATE_count": 1,
                    "research_candidate_roundtrip_count": 1,
                    "complete_executable_roundtrip_positions": 2,
                },
            )

            result = self.run_audit(
                root,
                candidate_universe=candidates,
                candidate_manifest=manifest_path,
                decision_jsonl=decisions,
                summary_csv=summary,
            )

        self.assertEqual(result["final_verdict"], "GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN")
        self.assertEqual(result["eligible_denominator_count"], 2)
        self.assertEqual(result["checkpoint_reach_count"], 2)
        self.assertEqual(result["gatekeeper_buy_count"], 1)
        self.assertEqual(result["gatekeeper_reject_count"], 1)
        self.assertEqual(result["unknown_reason_count"], 0)
        self.assertEqual(
            result["threshold_starvation_verdict"],
            "NO_GATEKEEPER_THRESHOLD_STARVATION_OBSERVED",
        )
        self.assertEqual(result["research_candidate_roundtrip_count"], 1)

    def test_generic_reject_reason_blocks_l2e(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            manifest_path = root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            write_jsonl(candidates, [candidate("c-reject", "mint-reject", "pool-reject")])
            write_json(manifest_path, manifest())
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "c-reject",
                        "base_mint": "mint-reject",
                        "pool_id": "pool-reject",
                        "verdict_type": "REJECT",
                    }
                ],
            )

            result = self.run_audit(
                root,
                candidate_universe=candidates,
                candidate_manifest=manifest_path,
                decision_jsonl=decisions,
            )

        self.assertEqual(result["final_verdict"], "BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS")
        self.assertEqual(result["unknown_reason_count"], 1)
        self.assertEqual(result["unknown_reason_samples"][0]["bucket"], "REJECT_OTHER")

    def test_typed_rejects_without_buy_are_classified_as_threshold_starvation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            manifest_path = root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            write_jsonl(
                candidates,
                [
                    candidate("c-a", "mint-a", "pool-a"),
                    candidate("c-b", "mint-b", "pool-b"),
                ],
            )
            write_json(manifest_path, manifest())
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "c-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "verdict_type": "REJECT_HARD_FAIL",
                        "reason_code": "HARD_FAIL_EXTREME_TOP3",
                    },
                    {
                        "candidate_id": "c-b",
                        "base_mint": "mint-b",
                        "pool_id": "pool-b",
                        "verdict_type": "REJECT_HARD_FAIL",
                        "reason_code": "HARD_FAIL_PDD_ENTRY_DRIFT",
                    },
                ],
            )

            result = self.run_audit(
                root,
                candidate_universe=candidates,
                candidate_manifest=manifest_path,
                decision_jsonl=decisions,
            )

        self.assertEqual(result["final_verdict"], "BLOCKED_GATEKEEPER_THRESHOLD_STARVATION")
        self.assertEqual(result["gatekeeper_buy_count"], 0)
        self.assertEqual(result["checkpoint_reach_count"], 2)
        self.assertEqual(result["unknown_reason_count"], 0)
        self.assertEqual(
            result["threshold_starvation_verdict"],
            "BLOCKED_GATEKEEPER_THRESHOLD_STARVATION",
        )

    def test_manifest_decision_created_denominator_rows_blocks_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            manifest_path = root / "candidate_universe_manifest_v1.json"
            write_jsonl(candidates, [candidate("c-a", "mint-a", "pool-a")])
            bad_manifest = manifest()
            bad_manifest["denominator_invariant_status"] = "NO-GO"
            bad_manifest["decision_logs_created_denominator_rows"] = 1
            bad_manifest["candidate_ids_from_decision_only"] = 1
            write_json(manifest_path, bad_manifest)

            result = self.run_audit(
                root,
                candidate_universe=candidates,
                candidate_manifest=manifest_path,
            )

        self.assertEqual(
            result["final_verdict"],
            "BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN",
        )
        self.assertIn("denominator_invariant_status_NO-GO", result["denominator_contract_failures"])
        self.assertIn(
            "decision_logs_created_denominator_rows_nonzero",
            result["denominator_contract_failures"],
        )


if __name__ == "__main__":
    unittest.main()
