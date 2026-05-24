import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l2c_gatekeeper_v2_axis_replay as l2c


ALLOWED_J4C = l2c.J4C_NAMESPACE
ALLOWED_R16 = l2c.R16_R1_NAMESPACE
BLOCKED = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver"


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
        "sha256": l2c.l2a.sha256_file(path),
    }


def full_decision(ab_record_id: str, *, buy: bool = False, trace: list[dict] | None = None) -> dict:
    return {
        "ab_record_id": ab_record_id,
        "decision_verdict_buy": buy,
        "legacy_live_verdict_type": "BUY" if buy else "REJECT_CORE_FAIL",
        "reason_code": "BUY_NORMAL" if buy else "REJECT_CORE_FAIL",
        "gatekeeper_terminal_gate": "buy" if buy else "core",
        "gatekeeper_gate_trace": trace,
        "v3_materialized_feature_snapshot": {"session_metadata": {}},
        "v3_policy_config_payload": {"profiles": {}},
        "v3_shadow_verdict": "REJECT",
        "v3_shadow_reason_code": "REJECT_V3_MANIPULATION_CONTRADICTION",
        "v3_replay_payload_schema_version": 1,
        "v3_materialization_version": 1,
        "pdd_hard_fail": "ENTRY_DRIFT",
        "soft_points": 0,
        "max_soft_points": 8,
        "prosperity_filter_enabled": False,
        "aps_shadow_prosperity_would_pass": True,
        "hhi": 0.08,
        "max_hhi": 0.155,
        "top3_volume_pct": 0.5,
        "same_ms_tx_ratio": 0.1,
        "pdd_entry_drift_pct": 10.0,
        "pdd_entry_drift_effective_max_pct": 15.0,
        "pdd_entry_drift_threshold_source": "elapsed_scaled",
    }


PASS_TRACE = [
    {"gate": "phase1_quantity", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "pdd", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "core1", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core2", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core3", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "soft_budget", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "alpha", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "prosperity", "status": "skipped", "hard_or_soft": "hard"},
]

FAIL_TRACE = [
    {"gate": "phase1_quantity", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "pdd", "status": "fail", "hard_or_soft": "hard"},
    {"gate": "core3", "status": "fail", "hard_or_soft": "hard"},
]


def build_fixture(
    root: Path,
    *,
    j4c_trace: bool = False,
    r16_buy_trace_parity_gap: bool = False,
    blocked_label: bool = False,
) -> Path:
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
                "rollout_namespace": BLOCKED if blocked_label else ALLOWED_R16,
                "run_id": BLOCKED if blocked_label else ALLOWED_R16,
            }
        ],
    )
    write_jsonl(
        j4c_decisions,
        [full_decision("j1", buy=False, trace=PASS_TRACE if j4c_trace else None)],
    )
    write_jsonl(
        r16_decisions,
        [
            full_decision(
                "r1",
                buy=True,
                trace=FAIL_TRACE if r16_buy_trace_parity_gap else PASS_TRACE,
            )
        ],
    )

    manifest = {
        "manifest_status": "pass",
        "final_decision": "GO_L2_INPUT_MANIFEST_LOCKED",
        "actual_contract": {
            "allowed_runs": [ALLOWED_J4C, ALLOWED_R16],
            "blocked_runs": [BLOCKED],
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


class P37L2CGatekeeperV2AxisReplayTests(unittest.TestCase):
    def test_reports_missing_gate_trace_and_blocks_l2d(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp))
            with patch.object(l2c, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c, "EXPECTED_DIRTY_GOOD", 1
            ), patch.object(l2c.l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c.l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2c.build_report(manifest, ["soft_pdd_instead_of_hard_pdd"])

        self.assertEqual(report["analysis_status"], "pass")
        self.assertEqual(report["final_decision"], "BLOCK_L2D_GATEKEEPER_V2_AXIS_REPLAY_INPUT_GAP")
        axis = report["axis_results"]["soft_pdd_instead_of_hard_pdd"]
        self.assertEqual(axis["axis_status"], "unsupported_missing_fields")
        self.assertIn("unsupported_missing_fields:gatekeeper_gate_trace", axis["row_status_counts"])

    def test_temporal_standard_axis_is_explicitly_unsupported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp), j4c_trace=True)
            with patch.object(l2c, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c, "EXPECTED_DIRTY_GOOD", 1
            ), patch.object(l2c.l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c.l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2c.build_report(manifest, ["standard_mode_shorter_window"])

        self.assertEqual(
            report["axis_results"]["standard_mode_shorter_window"]["axis_status"],
            "unsupported_temporal_replay_required",
        )

    def test_baseline_parity_gap_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(
                Path(tmp),
                j4c_trace=True,
                r16_buy_trace_parity_gap=True,
            )
            with patch.object(l2c, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c, "EXPECTED_DIRTY_GOOD", 1
            ), patch.object(l2c.l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c.l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2c.build_report(manifest, ["soft_pdd_instead_of_hard_pdd"])

        axis = report["axis_results"]["soft_pdd_instead_of_hard_pdd"]
        self.assertEqual(axis["axis_status"], "unsupported_baseline_parity_gap")
        self.assertIn("unsupported_baseline_parity_gap", axis["row_status_counts"])
        self.assertEqual(report["payload_and_support"]["baseline_parity_counts"]["baseline_parity_gap"], 2)

    def test_blocked_namespace_in_labels_fails_contract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_fixture(Path(tmp), blocked_label=True)
            with patch.object(l2c, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c, "EXPECTED_DIRTY_GOOD", 1
            ), patch.object(l2c.l2a, "EXPECTED_DENOMINATOR", 2), patch.object(
                l2c.l2a, "EXPECTED_DIRTY_GOOD", 1
            ):
                report = l2c.build_report(manifest, ["soft_pdd_instead_of_hard_pdd"])

        self.assertEqual(report["analysis_status"], "fail")
        self.assertIn(f"BLOCK_L2C_BLOCKED_NAMESPACE_PRESENT:{BLOCKED}", report["blockers"])


if __name__ == "__main__":
    unittest.main()
