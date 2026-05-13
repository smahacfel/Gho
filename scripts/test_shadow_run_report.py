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
                shadow_log=tmp / "shadow_run" / "shadow-burnin-v25-repair-r2-buys.jsonl",
                shadow_entry_log=tmp / "shadow_run" / "shadow-burnin-v25-repair-r2" / "shadow_entries.jsonl",
                shadow_lifecycle_log=tmp / "shadow_run" / "shadow-burnin-v25-repair-r2" / "shadow_lifecycle.jsonl",
                events_dir=tmp / "events",
                wal_dir=tmp / "data" / "shadow-burnin-v25-repair-r2" / "wal",
                snapshot_dir=tmp / "data" / "shadow-burnin-v25-repair-r2" / "snapshots",
                system_log_path=tmp / "logs" / "shadow-burnin-v25-repair-r2" / "system.log",
                oracle_log_path=tmp / "logs" / "shadow-burnin-v25-repair-r2" / "oracle.log",
                session_start_ms=None,
                expected_rollout_profile="shadow-burnin-v25-repair-r2",
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

    def test_p6_rollout_scope_contract_requires_expected_profile(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            inputs = repair_validation.Inputs(
                config_path=tmp / "shadow-burnin.toml",
                ghost_brain_config_path=tmp / "ghost_brain_config.toml",
                decisions_dir=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair" / "decisions",
                buys_log=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair" / "decisions" / shadow_run_report.BUY_LOG_NAME,
                decisions_log=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair" / "decisions" / shadow_run_report.DECISIONS_LOG_NAME,
                coverage_audit_log=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair" / "decisions" / "seer_runtime_coverage_audit.jsonl",
                shadow_log=tmp / "logs" / "shadow_run" / "shadow-burnin-v25-repair-buys.jsonl",
                shadow_entry_log=tmp / "logs" / "shadow_run" / "shadow-burnin-v25-repair" / "shadow_entries.jsonl",
                shadow_lifecycle_log=tmp / "logs" / "shadow_run" / "shadow-burnin-v25-repair" / "shadow_lifecycle.jsonl",
                events_dir=tmp / "datasets" / "events" / "shadow-burnin-v25-repair",
                wal_dir=tmp / "data" / "rollout" / "shadow-burnin-v25-repair" / "wal",
                snapshot_dir=tmp / "data" / "rollout" / "shadow-burnin-v25-repair" / "snapshots",
                system_log_path=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair" / "system.log",
                oracle_log_path=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair" / "oracle.log",
                session_start_ms=None,
                expected_rollout_profile="shadow-burnin-v25-repair-r2",
                expected_plane="v25_shadow",
            )

            gate = repair_validation.gate_rollout_scope_contract(inputs)

            self.assertFalse(gate.passed)
            self.assertIn("decisions_dir", gate.observed["mismatched_paths"])
            self.assertIn("shadow_log", gate.observed["mismatched_paths"])

    def test_p6_rollout_scope_contract_rejects_prefix_only_match(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_raw:
            tmp = Path(tmp_raw)
            inputs = repair_validation.Inputs(
                config_path=tmp / "shadow-burnin.toml",
                ghost_brain_config_path=tmp / "ghost_brain_config.toml",
                decisions_dir=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair-r2" / "decisions",
                buys_log=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair-r2" / "decisions" / shadow_run_report.BUY_LOG_NAME,
                decisions_log=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair-r2" / "decisions" / shadow_run_report.DECISIONS_LOG_NAME,
                coverage_audit_log=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair-r2" / "decisions" / "seer_runtime_coverage_audit.jsonl",
                shadow_log=tmp / "logs" / "shadow_run" / "shadow-burnin-v25-repair-r2-buys.jsonl",
                shadow_entry_log=tmp / "logs" / "shadow_run" / "shadow-burnin-v25-repair-r2" / "shadow_entries.jsonl",
                shadow_lifecycle_log=tmp / "logs" / "shadow_run" / "shadow-burnin-v25-repair-r2" / "shadow_lifecycle.jsonl",
                events_dir=tmp / "datasets" / "events" / "shadow-burnin-v25-repair-r2",
                wal_dir=tmp / "data" / "rollout" / "shadow-burnin-v25-repair-r2" / "wal",
                snapshot_dir=tmp / "data" / "rollout" / "shadow-burnin-v25-repair-r2" / "snapshots",
                system_log_path=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair-r2" / "system.log",
                oracle_log_path=tmp / "logs" / "rollout" / "shadow-burnin-v25-repair-r2" / "oracle.log",
                session_start_ms=None,
                expected_rollout_profile="shadow-burnin-v25-repair",
                expected_plane="v25_shadow",
            )

            gate = repair_validation.gate_rollout_scope_contract(inputs)

            self.assertFalse(gate.passed)
            self.assertIn("decisions_dir", gate.observed["mismatched_paths"])
            self.assertIn("shadow_log", gate.observed["mismatched_paths"])

    def test_p6_builds_decision_breakdowns_per_plane_regime_stage(self) -> None:
        rows = [
            {
                "decision_plane": "v25_shadow",
                "aps_regime": "Normal",
                "observation_stage": "Early",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
            },
            {
                "decision_plane": "v25_shadow",
                "aps_regime": "HighVolatility",
                "observation_stage": "Extended",
                "decision_verdict_buy": False,
                "verdict_type": "REJECT_CORE_FAIL",
            },
            {
                "decision_plane": "legacy_live",
                "aps_regime": "Normal",
                "observation_stage": "Extended",
                "decision_verdict_buy": None,
                "verdict_type": "TIMEOUT_PHASE1_NO_DATA",
            },
        ]

        breakdowns = repair_validation.build_decision_breakdowns(rows)

        self.assertEqual(breakdowns["by_decision_plane"]["v25_shadow"]["BUY"], 1)
        self.assertEqual(breakdowns["by_decision_plane"]["v25_shadow"]["REJECT"], 1)
        self.assertEqual(breakdowns["by_decision_plane"]["legacy_live"]["TIMEOUT"], 1)
        self.assertEqual(breakdowns["by_aps_regime"]["Normal"]["BUY"], 1)
        self.assertEqual(breakdowns["by_aps_regime"]["HighVolatility"]["REJECT"], 1)
        self.assertEqual(breakdowns["by_observation_stage"]["Extended"]["REJECT"], 1)
        self.assertEqual(breakdowns["by_observation_stage"]["Extended"]["TIMEOUT"], 1)

    def test_p6_canonical_breakdown_rows_supplement_buy_log_without_double_counting(self) -> None:
        decision_rows = [
            {
                "candidate_id": "candidate_a_1000",
                "decision_plane": "v25_shadow",
                "aps_regime": "Normal",
                "observation_stage": "Early",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
            }
        ]
        buy_rows = [
            {
                "candidate_id": "candidate_a_1000",
                "decision_plane": "v25_shadow",
                "aps_regime": "Normal",
                "observation_stage": "Early",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
            },
            {
                "candidate_id": "candidate_b_2000",
                "decision_plane": "v25_shadow",
                "aps_regime": "HighVolatility",
                "observation_stage": "Extended",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
            },
        ]

        canonical_rows = repair_validation.build_canonical_breakdown_rows(decision_rows, buy_rows)
        breakdowns = repair_validation.build_decision_breakdowns(canonical_rows)

        self.assertEqual(len(canonical_rows), 2)
        self.assertEqual(breakdowns["by_decision_plane"]["v25_shadow"]["BUY"], 2)
        self.assertEqual(breakdowns["by_aps_regime"]["Normal"]["BUY"], 1)
        self.assertEqual(breakdowns["by_aps_regime"]["HighVolatility"]["BUY"], 1)


if __name__ == "__main__":
    unittest.main()
