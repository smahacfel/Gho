import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l2b_manifest_axis_ablation as l2b


ALLOWED_J4C = l2b.J4C_NAMESPACE
ALLOWED_R16 = l2b.R16_R1_NAMESPACE
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
        "sha256": l2b.l2a.sha256_file(path),
    }


def replay_decision(ab_record_id: str, **extra: object) -> dict:
    row = {
        "ab_record_id": ab_record_id,
        "v3_materialized_feature_snapshot": {"session_metadata": {}},
        "v3_policy_config_payload": {"profiles": {}},
        "v3_shadow_verdict": "REJECT",
        "v3_shadow_reason_code": "REJECT_V3_MANIPULATION_CONTRADICTION",
        "v3_replay_payload_schema_version": 1,
        "v3_materialization_version": 1,
    }
    row.update(extra)
    return row


def build_fixture(root: Path, *, missing_payload: bool = False, blocked_label: bool = False) -> Path:
    j4c_label = root / "j4c_labels.jsonl"
    r16_label = root / "r16_labels.jsonl"
    j4c_decisions = root / "j4c_decisions.jsonl"
    r16_decisions = root / "r16_decisions.jsonl"
    manifest_path = root / "manifest.json"

    write_jsonl(
        j4c_label,
        [
            {
                "source_ab_record_id": "j1",
                "buy_quality_class": "buy_quality_bad",
                "rollout_namespace": ALLOWED_J4C,
                "run_id": ALLOWED_J4C,
            }
        ],
    )
    write_jsonl(
        r16_label,
        [
            {
                "source_ab_record_id": "r1",
                "buy_quality_class": "buy_quality_dirty_good",
                "rollout_namespace": BLOCKED_R13 if blocked_label else ALLOWED_R16,
                "run_id": BLOCKED_R13 if blocked_label else ALLOWED_R16,
            },
            {
                "source_ab_record_id": "r2",
                "buy_quality_class": "buy_quality_not_executable",
                "execution_feasibility_status": "not_executable_route",
                "rollout_namespace": ALLOWED_R16,
                "run_id": ALLOWED_R16,
            },
        ],
    )
    write_jsonl(j4c_decisions, [replay_decision("j1", pdd_hard_fail="WHALE", hhi=0.4)])
    r16_decision = replay_decision(
        "r1",
        pdd_hard_fail="ENTRY_DRIFT",
        gatekeeper_first_kill_gate="pdd",
        pdd_entry_drift_pct=7.0,
        pdd_entry_drift_effective_max_pct=15.0,
        prosperity_filter_enabled=False,
        aps_shadow_prosperity_would_pass=False,
        hhi=0.15,
    )
    if missing_payload:
        r16_decision.pop("v3_materialized_feature_snapshot")
    write_jsonl(
        r16_decisions,
        [
            r16_decision,
            replay_decision("r2"),
        ],
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
                ],
            },
            {
                "namespace": ALLOWED_R16,
                "artifacts": [
                    artifact(r16_decisions, "decision"),
                    artifact(r16_label, "lifecycle_label_file"),
                ],
            },
        ],
    }
    write_json(manifest_path, manifest)
    return manifest_path


class P37L2BManifestAxisAblationTests(unittest.TestCase):
    def test_full_payload_but_axis_backend_unsupported_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            with patch.object(l2b, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2b, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2b.build_report(manifest)

        self.assertEqual(report["status"], "blocked")
        self.assertEqual(report["final_decision"], "BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED")
        self.assertIn("BLOCK_L2B_AXIS_REPLAY_UNSUPPORTED", report["blockers"])
        self.assertEqual(report["replay_payload"]["ablation_evaluable_rows"], 2)
        self.assertEqual(report["variant_results"]["A0_j4c_baseline"]["accepted_bad"], 1)
        self.assertEqual(report["variant_results"]["Afull_r16_r1_bundle"]["accepted_dirty_good"], 1)
        self.assertEqual(
            report["axis_diagnostic_matrix"]["soft_pdd_instead_of_hard_pdd"][
                "dirty_good_flagged_rows"
            ],
            1,
        )

    def test_replay_payload_gap_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp), missing_payload=True)
            with patch.object(l2b, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2b, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2b.build_report(manifest)

        self.assertEqual(report["status"], "blocked")
        self.assertIn("BLOCK_L2B_REPLAY_PAYLOAD_GAP", report["blockers"])
        self.assertEqual(report["replay_payload"]["ablation_not_evaluable_rows"], 1)

    def test_denominator_mismatch_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            with patch.object(l2b, "EXPECTED_DENOMINATOR", 3), patch.object(
                l2b, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2b.build_report(manifest)

        self.assertEqual(report["status"], "blocked")
        self.assertIn("BLOCK_L2_DENOMINATOR_MISMATCH:rows", report["blockers"])

    def test_blocked_namespace_in_label_rows_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp), blocked_label=True)
            with patch.object(l2b, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2b, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2b.build_report(manifest)

        self.assertEqual(report["status"], "blocked")
        self.assertIn(f"BLOCK_L2_BLOCKED_NAMESPACE_PRESENT:{BLOCKED_R13}", report["blockers"])


if __name__ == "__main__":
    unittest.main()
