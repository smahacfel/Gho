import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_r17a_replay_readiness_audit as r17a


def snapshot(target: int, actual: int, *, source: str = "nearest_eval", omit: str | None = None) -> dict:
    payload = {
        "snapshot_target_ms": target,
        "snapshot_actual_elapsed_ms": actual,
        "snapshot_drift_ms": abs(target - actual),
        "snapshot_source": source,
        "gatekeeper_gate_trace": [{"gate": "phase1", "status": "pass"}],
        "phase_pass_vector": {"phase1": True},
        "pdd_diagnostics": {"pdd_hard_fail": False},
        "prosperity_diagnostics": {"prosperity_pass": True},
        "hhi_diversity_diagnostics": {"hhi": 0.1},
    }
    if omit:
        payload.pop(omit, None)
    return payload


def row(
    *,
    temporal_ready: bool,
    reason: str = "REJECT_CORE_FAIL",
    verdict: str = "REJECT_CORE_FAIL",
    duration: int = 10_000,
    snapshots: list[dict] | None = None,
    missing: list[str] | None = None,
) -> dict:
    return {
        "ab_record_id": f"mint:0:{duration}:{verdict}",
        "verdict_type": verdict,
        "reason_code": reason,
        "observation_duration_ms": duration,
        "gatekeeper_v2_replay_ready_non_temporal": True,
        "gatekeeper_v2_replay_ready_temporal": temporal_ready,
        "gatekeeper_v2_replay_missing_fields": missing or [],
        "decision_eval_snapshots": snapshots if snapshots is not None else [
            snapshot(2000, 2000),
            snapshot(5000, 5000),
            snapshot(7000, 7000),
            snapshot(10000, 10000, source="terminal"),
        ],
    }


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for item in rows:
            handle.write(json.dumps(item, sort_keys=True) + "\n")


class R17AReplayReadinessAuditTests(unittest.TestCase):
    def build_report(self, rows: list[dict]) -> dict:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "decisions.jsonl"
            write_jsonl(path, rows)
            return r17a.build_report(path, None, [2000, 5000, 7000], 10000, 2000)

    def test_ready_row_is_counted_ready(self) -> None:
        report = self.build_report([row(temporal_ready=True)])

        self.assertEqual(report["summary"]["temporal_ready_rows"], 1)
        self.assertEqual(report["summary"]["temporal_not_ready_rows"], 0)
        self.assertEqual(report["summary"]["reason_counts"], {"ready": 1})

    def test_early_close_missing_late_targets_is_natural_not_applicable(self) -> None:
        report = self.build_report([
            row(
                temporal_ready=False,
                reason="BUY_EARLY",
                verdict="BUY",
                duration=3300,
                snapshots=[snapshot(2000, 3300), snapshot(10000, 3300, source="terminal")],
            )
        ])

        self.assertEqual(report["summary"]["reason_counts"], {"early_close_before_targets": 1})
        self.assertEqual(report["summary"]["root_cause_class_counts"], {"natural_not_applicable": 1})

    def test_no_data_missing_targets_is_natural_not_applicable(self) -> None:
        report = self.build_report([
            row(
                temporal_ready=False,
                reason="TIMEOUT_PHASE1_NO_DATA",
                verdict="TIMEOUT_PHASE1_NO_DATA",
                snapshots=[snapshot(7000, 10000), snapshot(10000, 10000, source="terminal")],
            )
        ])

        self.assertEqual(report["summary"]["reason_counts"], {"timeout_no_data_before_targets": 1})
        self.assertEqual(report["summary"]["root_cause_class_counts"], {"natural_not_applicable": 1})

    def test_insufficient_data_missing_mid_target_is_natural_not_applicable(self) -> None:
        report = self.build_report([
            row(
                temporal_ready=False,
                reason="TIMEOUT_PHASE1_INSUFFICIENT",
                verdict="TIMEOUT_PHASE1_INSUFFICIENT",
                snapshots=[snapshot(2000, 2100), snapshot(7000, 10000), snapshot(10000, 10000, source="terminal")],
            )
        ])

        self.assertEqual(report["summary"]["reason_counts"], {"insufficient_sample_before_target": 1})
        self.assertEqual(report["summary"]["root_cause_class_counts"], {"natural_not_applicable": 1})
        self.assertEqual(report["summary"]["insufficient_sample_before_target_rows"], 1)

    def test_missing_terminal_is_runtime_emission_bug(self) -> None:
        report = self.build_report([
            row(
                temporal_ready=False,
                snapshots=[snapshot(2000, 2000), snapshot(5000, 5000), snapshot(7000, 7000)],
            )
        ])

        self.assertEqual(report["summary"]["reason_counts"], {"missing_terminal_snapshot": 1})
        self.assertEqual(report["summary"]["root_cause_class_counts"], {"runtime_emission_bug": 1})
        self.assertEqual(report["final_decision"], "R17B_SNAPSHOT_EMISSION_FIX_REQUIRED")

    def test_missing_snapshot_payload_field_is_payload_missing(self) -> None:
        report = self.build_report([
            row(
                temporal_ready=False,
                snapshots=[
                    snapshot(2000, 2000, omit="gatekeeper_gate_trace"),
                    snapshot(5000, 5000),
                    snapshot(7000, 7000),
                    snapshot(10000, 10000, source="terminal"),
                ],
            )
        ])

        self.assertEqual(
            report["summary"]["reason_counts"],
            {"missing_gatekeeper_gate_trace_in_snapshot": 1},
        )
        self.assertEqual(report["summary"]["root_cause_class_counts"], {"payload_missing": 1})
        self.assertEqual(report["summary"]["payload_gap_rows"], 1)
        self.assertEqual(report["final_decision"], "R17B_SNAPSHOT_EMISSION_FIX_REQUIRED")

    def test_unknown_false_ready_is_audit_gap(self) -> None:
        report = self.build_report([
            row(temporal_ready=False)
        ])

        self.assertEqual(report["summary"]["reason_counts"], {"unknown_temporal_readiness_gap": 1})
        self.assertEqual(report["summary"]["unknown_rows"], 1)
        self.assertEqual(report["final_decision"], "R17A_AUDIT_GAP")

    def test_natural_not_ready_rows_close_snapshot_side(self) -> None:
        report = self.build_report([
            row(
                temporal_ready=False,
                reason="TIMEOUT_PHASE1_INSUFFICIENT",
                verdict="TIMEOUT_PHASE1_INSUFFICIENT",
                snapshots=[snapshot(2000, 2100), snapshot(7000, 10000), snapshot(10000, 10000, source="terminal")],
            ),
            row(temporal_ready=True),
        ])

        self.assertEqual(report["summary"]["runtime_emission_bug_rows"], 0)
        self.assertEqual(report["summary"]["payload_gap_rows"], 0)
        self.assertEqual(report["summary"]["unknown_rows"], 0)
        self.assertEqual(report["final_decision"], "SNAPSHOT_SIDE_PASS_GO_E1")


if __name__ == "__main__":
    unittest.main()
