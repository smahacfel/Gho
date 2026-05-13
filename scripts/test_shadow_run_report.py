#!/usr/bin/env python3
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import gatekeeper_v25_repair_validation as repair_validation
import shadow_run_report


class ShadowRunReportP5Tests(unittest.TestCase):
    def test_p5_no_dispatch_does_not_require_shadow_lifecycle_or_economics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            decisions_log = tmp / "decisions" / shadow_run_report.DECISIONS_LOG_NAME
            decisions_log.parent.mkdir(parents=True)
            decisions_log.write_text(
                (
                    '{"candidate_id":"reject_candidate_1_1000",'
                    '"decision_verdict_buy":false,'
                    '"decision_reason":"REJECT_CORE_FAIL"}\n'
                ),
                encoding="utf-8",
            )
            events_dir = tmp / "events"
            events_dir.mkdir()
            system_log = tmp / "system.log"
            system_log.write_text(
                "\n".join(shadow_run_report.REQUIRED_RECOVERY_MARKERS) + "\n",
                encoding="utf-8",
            )
            metrics_text = tmp / "metrics.prom"
            metrics_text.write_text(
                "eventbus_lag_total 0\nprovider_stall_total 0\n"
                "trigger_buy_safety_rejections_total 0\n",
                encoding="utf-8",
            )
            inputs = shadow_run_report.Inputs(
                config_path=tmp / "shadow-burnin.toml",
                execution_mode="shadow",
                entry_mode="shadow_only",
                runtime_lane="shadow",
                decisions_dir=decisions_log.parent,
                buys_log=decisions_log.parent / shadow_run_report.BUY_LOG_NAME,
                decisions_log=decisions_log,
                shadow_log=tmp / "missing-shadow.jsonl",
                shadow_lifecycle_log=tmp / "missing-shadow-lifecycle.jsonl",
                events_dir=events_dir,
                system_log=system_log,
                metrics_text=metrics_text,
                min_net_pnl_sol=None,
                max_position_size_sol=0.007,
                emergency_floor_sol=0.0001,
                position_size_buffer_sol=0.0001,
                session_run_id=None,
                session_start_ms=None,
                session_end_ms=None,
            )

            report = shadow_run_report.build_report(inputs)

            self.assertEqual(report["verdict"], "GO")
            self.assertTrue(report["gates"]["mandatory_artifacts"]["passed"])
            self.assertTrue(report["gates"]["runtime_lifecycle_complete"]["passed"])
            self.assertTrue(report["gates"]["economics_not_fatal"]["passed"])
            self.assertIn(
                "no_dispatch_no_economics_required",
                report["gates"]["economics_not_fatal"]["details"],
            )

    def test_p5_repair_validation_artifacts_allow_no_dispatch_buy_log_absent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            inputs = repair_validation.Inputs(
                config_path=tmp / "shadow-burnin.toml",
                ghost_brain_config_path=tmp / "ghost_brain_config.toml",
                decisions_dir=tmp / "decisions",
                buys_log=tmp / "decisions" / shadow_run_report.BUY_LOG_NAME,
                decisions_log=tmp / "decisions" / shadow_run_report.DECISIONS_LOG_NAME,
                coverage_audit_log=tmp / "decisions" / "seer_runtime_coverage_audit.jsonl",
                events_dir=tmp / "events",
                session_start_ms=None,
                expected_rollout_profile="shadow-burnin-v25-repair",
                expected_plane="v25_shadow",
            )
            inputs.decisions_log.parent.mkdir(parents=True)
            inputs.decisions_log.write_text("{}\n", encoding="utf-8")
            inputs.coverage_audit_log.write_text("{}\n", encoding="utf-8")

            gate = repair_validation.gate_artifacts_present(
                inputs,
                [{"candidate_id": "reject_candidate_1_1000", "decision_verdict_buy": False}],
            )

            self.assertTrue(gate.passed)
            self.assertFalse(gate.observed["buy_log_required"])


if __name__ == "__main__":
    unittest.main()
