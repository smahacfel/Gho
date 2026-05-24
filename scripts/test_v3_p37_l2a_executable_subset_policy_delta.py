import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l2a_executable_subset_policy_delta as l2a


ALLOWED_J4C = "shadow-burnin-v3-p37-counterfactual-probe-r15-lifecycle-label-j4c-r1"
ALLOWED_R16 = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1"
BLOCKED_R13 = (
    "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver"
)


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


def artifact(path: Path, role: str) -> dict:
    return {
        "exists": True,
        "path": str(path),
        "role": role,
        "sha256": l2a.sha256_file(path),
    }


def build_fixture(root: Path, *, blocked_label_namespace: bool = False) -> Path:
    j4c_label = root / "j4c_labels.jsonl"
    r16_label = root / "r16_labels.jsonl"
    j4c_decisions = root / "j4c_decisions.jsonl"
    r16_decisions = root / "r16_decisions.jsonl"
    j4c_feature = root / "j4c_feature.json"
    r16_feature = root / "r16_feature.json"
    manifest_path = root / "manifest.json"

    write_jsonl(
        j4c_label,
        [
            {
                "source_ab_record_id": "a1",
                "buy_quality_class": "buy_quality_bad",
                "market_outcome_class": "market_bad_clean",
                "rollout_namespace": ALLOWED_J4C,
                "run_id": ALLOWED_J4C,
            }
        ],
    )
    write_jsonl(
        r16_label,
        [
            {
                "source_ab_record_id": "b1",
                "buy_quality_class": "buy_quality_dirty_good",
                "market_outcome_class": "market_dirty_good",
                "rollout_namespace": BLOCKED_R13 if blocked_label_namespace else ALLOWED_R16,
                "run_id": BLOCKED_R13 if blocked_label_namespace else ALLOWED_R16,
            },
            {
                "source_ab_record_id": "b2",
                "buy_quality_class": "buy_quality_not_executable",
                "execution_feasibility_status": "not_executable_route",
                "rollout_namespace": ALLOWED_R16,
                "run_id": ALLOWED_R16,
            },
        ],
    )
    write_jsonl(
        j4c_decisions,
        [
            {
                "ab_record_id": "a1",
                "decision_verdict_buy": False,
                "reason_code": "REJECT_PDD",
                "hhi": 0.12,
                "top3_volume_pct": 0.91,
            }
        ],
    )
    write_jsonl(
        r16_decisions,
        [
            {
                "ab_record_id": "b1",
                "decision_verdict_buy": True,
                "reason_code": "BUY",
                "gatekeeper_terminal_gate": "three_layer_score",
                "pdd_entry_drift_pct": 4.0,
                "hhi": 0.18,
                "top3_volume_pct": 0.80,
            },
            {
                "ab_record_id": "b2",
                "decision_verdict_buy": True,
                "reason_code": "BUY",
            },
        ],
    )
    write_json(j4c_feature, {"feature_availability_status": "insufficient_for_selector"})
    write_json(
        r16_feature,
        {
            "feature_availability_status": "insufficient_for_selector",
            "diagnostic_minimums": {"dirty_good_with_features": 1, "bad_with_features": 0},
        },
    )

    manifest = {
        "manifest_status": "pass",
        "final_decision": "GO_L2_INPUT_MANIFEST_LOCKED",
        "actual_contract": {
            "allowed_runs": [ALLOWED_J4C, ALLOWED_R16],
            "blocked_runs": [BLOCKED_R13],
            "buy_quality_denominator_rows": 2,
            "buy_quality_dirty_good": 1,
            "dirty_good_rate": 0.5,
        },
        "allowed_run_manifests": [
            {
                "namespace": ALLOWED_J4C,
                "artifacts": [
                    artifact(j4c_decisions, "decision"),
                    artifact(j4c_label, "lifecycle_label_file"),
                    artifact(j4c_feature, "feature_availability_file"),
                ],
            },
            {
                "namespace": ALLOWED_R16,
                "artifacts": [
                    artifact(r16_decisions, "decision"),
                    artifact(r16_label, "lifecycle_label_file"),
                    artifact(r16_feature, "feature_availability_file"),
                ],
            },
        ],
    }
    write_json(manifest_path, manifest)
    return manifest_path


class P37L2AExecutableSubsetPolicyDeltaTests(unittest.TestCase):
    def test_passes_locked_manifest_and_excludes_non_executable_label(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            with patch.object(l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2a.build_report(manifest)

        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["final_decision"], "GO_L2B_AXIS_ABLATION_PREP")
        self.assertEqual(report["combined_allowed_subset"]["buy_quality_denominator_rows"], 2)
        self.assertEqual(report["combined_allowed_subset"]["buy_quality_dirty_good"], 1)
        r16 = [row for row in report["run_results"] if row["namespace"] == ALLOWED_R16][0]
        self.assertEqual(r16["buy_quality_not_executable"], 1)
        self.assertEqual(r16["non_executable_rows_in_denominator"], 0)
        self.assertIn("soft_pdd_instead_of_hard_pdd", report["policy_delta"]["l2b_candidate_axes"])

    def test_artifact_hash_mismatch_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            payload = json.loads(manifest.read_text(encoding="utf-8"))
            payload["allowed_run_manifests"][0]["artifacts"][0]["sha256"] = "bad"
            write_json(manifest, payload)
            with patch.object(l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2a.build_report(manifest)

        self.assertEqual(report["status"], "fail")
        self.assertEqual(report["final_decision"], "BLOCK_L2A_INPUT_MANIFEST_CONTRACT")
        self.assertTrue(any(item.startswith("artifact_hash_mismatch") for item in report["blockers"]))

    def test_denominator_mismatch_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            with patch.object(l2a, "EXPECTED_DENOMINATOR", 3), patch.object(
                l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2a.build_report(manifest)

        self.assertEqual(report["status"], "fail")
        self.assertIn("BLOCK_L2_DENOMINATOR_MISMATCH", report["blockers"])

    def test_dirty_good_mismatch_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            with patch.object(l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2a, "EXPECTED_DIRTY_GOOD", 2
            ):
                report = l2a.build_report(manifest)

        self.assertEqual(report["status"], "fail")
        self.assertIn("BLOCK_L2_DIRTY_GOOD_MISMATCH", report["blockers"])

    def test_blocked_namespace_inside_label_rows_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp), blocked_label_namespace=True)
            with patch.object(l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2a.build_report(manifest)

        self.assertEqual(report["status"], "fail")
        self.assertIn(f"BLOCK_L2_BLOCKED_NAMESPACE_PRESENT:{BLOCKED_R13}", report["blockers"])


if __name__ == "__main__":
    unittest.main()
