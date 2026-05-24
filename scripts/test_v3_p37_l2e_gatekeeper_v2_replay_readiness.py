import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l2e_gatekeeper_v2_replay_readiness as l2e


J4C = l2e.l2c.J4C_NAMESPACE
R16 = l2e.l2c.R16_R1_NAMESPACE
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
        "sha256": l2e.l2a.sha256_file(path),
    }


def label(ab_record_id: str, quality: str, namespace: str) -> dict:
    return {
        "source_ab_record_id": ab_record_id,
        "buy_quality_class": quality,
        "rollout_namespace": namespace,
        "run_id": namespace,
    }


def decision(ab_record_id: str, *, contract: dict | None = None) -> dict:
    payload = {
        "ab_record_id": ab_record_id,
        "decision_verdict_buy": False,
        "legacy_live_verdict_type": "REJECT_CORE_FAIL",
        "reason_code": "REJECT_CORE_FAIL",
        "gatekeeper_terminal_gate": "core",
        "gatekeeper_gate_trace": [
            {"gate": "phase1_quantity", "status": "pass", "hard_or_soft": "hard"}
        ],
        "v3_materialized_feature_snapshot": {"session_metadata": {}},
        "v3_policy_config_payload": {"profiles": {}},
        "v3_shadow_verdict": "REJECT",
        "v3_shadow_reason_code": "REJECT_V3_MANIPULATION_CONTRADICTION",
        "v3_replay_payload_schema_version": 1,
        "v3_materialization_version": 1,
    }
    if contract:
        payload.update(contract)
    return payload


def replay_contract(*, non_temporal_ready: bool, temporal_ready: bool = False) -> dict:
    missing = [] if non_temporal_ready else ["non_temporal:pdd_assessment"]
    if not temporal_ready:
        missing.append("temporal:decision_eval_snapshots")
    return {
        "gatekeeper_v2_replay_input_schema_version": 1,
        "gatekeeper_v2_replay_ready_non_temporal": non_temporal_ready,
        "gatekeeper_v2_replay_ready_temporal": temporal_ready,
        "gatekeeper_v2_replay_missing_fields": missing,
        "gatekeeper_v2_phase_pass_vector": {"phase1": True},
        "observed_mode": "standard",
        "observed_window_ms": 5000,
        "observed_stage": "terminal",
        "decision_eval_snapshots": [{"elapsed_ms": 5000}] if temporal_ready else None,
    }


def build_manifest(root: Path, rows: list[tuple[str, dict, dict]]) -> Path:
    j4c_label_path = root / "j4c_labels.jsonl"
    j4c_decision_path = root / "j4c_decisions.jsonl"
    r16_label_path = root / "r16_labels.jsonl"
    r16_decision_path = root / "r16_decisions.jsonl"
    manifest_path = root / "manifest.json"

    j4c_labels = [label for namespace, label, _decision in rows if namespace == J4C]
    j4c_decisions = [decision for namespace, _label, decision in rows if namespace == J4C]
    r16_labels = [label for namespace, label, _decision in rows if namespace == R16]
    r16_decisions = [decision for namespace, _label, decision in rows if namespace == R16]

    write_jsonl(j4c_label_path, j4c_labels)
    write_jsonl(j4c_decision_path, j4c_decisions)
    write_jsonl(r16_label_path, r16_labels)
    write_jsonl(r16_decision_path, r16_decisions)

    all_labels = j4c_labels + r16_labels
    dirty = sum(1 for row in all_labels if row["buy_quality_class"] == "buy_quality_dirty_good")
    manifest = {
        "manifest_status": "pass",
        "final_decision": "GO_L2_INPUT_MANIFEST_LOCKED",
        "actual_contract": {
            "allowed_runs": [J4C, R16],
            "blocked_runs": [BLOCKED],
            "buy_quality_denominator_rows": len(all_labels),
            "buy_quality_dirty_good": dirty,
            "dirty_good_rate": dirty / len(all_labels) if all_labels else 0.0,
        },
        "allowed_run_manifests": [
            {
                "namespace": J4C,
                "artifacts": [
                    artifact(j4c_decision_path, "decision"),
                    artifact(j4c_label_path, "lifecycle_label_file"),
                ],
            },
            {
                "namespace": R16,
                "artifacts": [
                    artifact(r16_decision_path, "decision"),
                    artifact(r16_label_path, "lifecycle_label_file"),
                ],
            },
        ],
    }
    write_json(manifest_path, manifest)
    return manifest_path


class P37L2EGatekeeperV2ReplayReadinessTests(unittest.TestCase):
    def run_report(self, manifest: Path, rows: int, dirty: int) -> dict:
        with patch.object(l2e, "EXPECTED_DENOMINATOR", rows), patch.object(
            l2e, "EXPECTED_DIRTY_GOOD", dirty
        ), patch.object(l2e.l2c, "EXPECTED_DENOMINATOR", rows), patch.object(
            l2e.l2c, "EXPECTED_DIRTY_GOOD", dirty
        ), patch.object(l2e.l2a, "EXPECTED_DENOMINATOR", rows), patch.object(
            l2e.l2a, "EXPECTED_DIRTY_GOOD", dirty
        ):
            return l2e.build_report(manifest)

    def test_legacy_rows_block_l2e_replay_contract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_manifest(
                Path(tmp),
                [
                    (
                        J4C,
                        label("j1", "buy_quality_bad", J4C),
                        decision("j1"),
                    )
                ],
            )
            report = self.run_report(manifest, 1, 0)

        self.assertEqual(report["analysis_status"], "pass")
        self.assertEqual(
            report["final_decision"],
            "BLOCK_L2E_HISTORICAL_ROWS_MISSING_V22_REPLAY_CONTRACT",
        )
        self.assertEqual(
            report["input_support"]["schema_status_counts"],
            {"missing_v2_replay_contract": 1},
        )
        self.assertEqual(
            report["axis_readiness"]["soft_pdd_instead_of_hard_pdd"]["axis_status"],
            "unsupported_v2_replay_input_gap",
        )

    def test_non_temporal_ready_keeps_temporal_axis_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_manifest(
                Path(tmp),
                [
                    (
                        J4C,
                        label("j1", "buy_quality_bad", J4C),
                        decision("j1", contract=replay_contract(non_temporal_ready=True)),
                    )
                ],
            )
            report = self.run_report(manifest, 1, 0)

        self.assertEqual(
            report["final_decision"],
            "GO_L2D_NON_TEMPORAL_REPLAY_READY_TEMPORAL_BLOCKED",
        )
        self.assertEqual(
            report["axis_readiness"]["soft_pdd_instead_of_hard_pdd"]["axis_status"],
            "replay_ready",
        )
        self.assertEqual(
            report["axis_readiness"]["standard_mode_shorter_window"]["axis_status"],
            "unsupported_temporal_snapshots_missing",
        )

    def test_full_replay_contract_allows_full_axis_replay(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_manifest(
                Path(tmp),
                [
                    (
                        J4C,
                        label("j1", "buy_quality_dirty_good", J4C),
                        decision(
                            "j1",
                            contract=replay_contract(
                                non_temporal_ready=True,
                                temporal_ready=True,
                            ),
                        ),
                    )
                ],
            )
            report = self.run_report(manifest, 1, 1)

        self.assertEqual(report["final_decision"], "GO_L2D_FULL_GATEKEEPER_V2_AXIS_REPLAY_READY")
        self.assertEqual(
            report["axis_readiness"]["standard_mode_shorter_window"]["axis_status"],
            "replay_ready",
        )

    def test_manifest_contract_failure_blocks_report(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_manifest(
                Path(tmp),
                [
                    (
                        J4C,
                        label("j1", "buy_quality_bad", J4C),
                        decision("j1", contract=replay_contract(non_temporal_ready=True)),
                    )
                ],
            )
            report = self.run_report(manifest, 2, 0)

        self.assertEqual(report["analysis_status"], "fail")
        self.assertEqual(report["final_decision"], "BLOCK_L2E_INPUT_MANIFEST_CONTRACT")


if __name__ == "__main__":
    unittest.main()
