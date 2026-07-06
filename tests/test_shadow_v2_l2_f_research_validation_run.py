#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "shadow_v2_l2_f_research_validation_run.py"
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import shadow_v2_l2_f_research_validation_run as l2_f  # noqa: E402


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True))
            fh.write("\n")


def canonical_row(schema: str, position_id: str, record: dict) -> dict:
    return {
        "schema": "shadow_position_event_v2",
        "envelope": {
            "schema": schema,
            "position_id": position_id,
            "run_id": "fixture-l2-f-dedicated",
        },
        "payload": {
            "record": record,
        },
    }


def density_row(
    position_id: str,
    horizon_ms: int,
    *,
    verdict: str = "EVALUABLE_APPROX",
    replay_horizon_ms: int = 121_000,
    max_interval_ms: int = 1_000,
) -> dict:
    return {
        "schema": "shadow_path_density_v2",
        "position_id": position_id,
        "horizon_ms": horizon_ms,
        "verdict": verdict,
        "path_points": 122,
        "coverage_points": 10,
        "replay_horizon_ms": replay_horizon_ms,
        "max_interval_ms": max_interval_ms,
        "created_at_wall_ms": 1_785_000_000_000,
        "source_canonical_high_watermark": f"watermark:{position_id}:{horizon_ms}",
        "limitations": [],
    }


class ShadowV2L2FResearchValidationRunTest(unittest.TestCase):
    def test_position_level_density_gate_uses_only_evidence_complete_roundtrips(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            positions = {"pos-a", "pos-b", "pos-c"}
            rows = []
            for position in positions:
                for horizon in l2_f.DECLARED_HORIZONS_MS:
                    if position == "pos-c" and horizon == 120_000:
                        rows.append(
                            density_row(
                                position,
                                horizon,
                                verdict="SPARSE_APPROX_ONLY",
                                max_interval_ms=7_000,
                            )
                        )
                    else:
                        rows.append(density_row(position, horizon))
            write_jsonl(scope / "shadow_path_density_v2.jsonl", rows)

            result = l2_f.position_level_density_retention_gate(
                scope,
                positions,
                required_roundtrips=2,
            )

        self.assertEqual(result["verdict"], l2_f.POSITION_LEVEL_DENSITY_PASS_VERDICT)
        self.assertEqual(result["l2_research_evidence_complete_roundtrip_positions"], 2)
        self.assertEqual(result["density_excluded_roundtrip_positions"], 1)
        self.assertEqual(result["sparse_approx_only_position_count"], 1)
        self.assertEqual(result["retention_gap_position_count"], 0)

    def test_position_level_density_gate_blocks_when_complete_density_scope_is_too_small(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            positions = {"pos-a", "pos-b", "pos-c"}
            rows = []
            for position in positions:
                for horizon in l2_f.DECLARED_HORIZONS_MS:
                    replay_horizon_ms = 120_500 if position != "pos-a" else 121_000
                    rows.append(
                        density_row(
                            position,
                            horizon,
                            replay_horizon_ms=replay_horizon_ms,
                        )
                    )
            write_jsonl(scope / "shadow_path_density_v2.jsonl", rows)

            result = l2_f.position_level_density_retention_gate(
                scope,
                positions,
                required_roundtrips=2,
            )

        self.assertEqual(
            result["verdict"],
            "BLOCKED_L2_F_POSITION_LEVEL_DENSITY_RETENTION",
        )
        self.assertEqual(result["l2_research_evidence_complete_roundtrip_positions"], 1)
        self.assertEqual(result["density_excluded_roundtrip_positions"], 2)
        self.assertEqual(result["retention_gap_position_count"], 2)

    def test_missing_dedicated_scope_blocks_manifest_replay_without_grants(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidate_universe = root / "candidate_universe_v1.jsonl"
            candidate_manifest = root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            output_root = root / "l2-f-output"
            output_csv = root / "summary.csv"
            missing_history = root / "missing_history.csv"

            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                    }
                ],
            )
            write_json(
                candidate_manifest,
                {
                    "status": "ok",
                    "denominator_invariant_status": "PASS",
                    "decision_logs_created_denominator_rows": 0,
                    "candidate_ids_from_decision_only": 0,
                },
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "verdict_type": "BUY",
                        "decision_verdict_buy": True,
                    }
                ],
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--run-id",
                    "fixture-l2-f",
                    "--output-root",
                    str(output_root),
                    "--candidate-universe",
                    str(candidate_universe),
                    "--candidate-manifest",
                    str(candidate_manifest),
                    "--decision-root",
                    str(decisions),
                    "--historical-summary-csv",
                    str(missing_history),
                    "--precondition-density-summary",
                    str(missing_history),
                    "--output-csv",
                    str(output_csv),
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            payload = json.loads(result.stdout)
            strict_summary = json.loads((output_root / "strict_audit_summary.json").read_text())
            runtime_manifest = json.loads((output_root / "runtime_post_run_manifest.json").read_text())
            copied_candidate_universe = (output_root / "candidate_universe_v1.jsonl").exists()
            copied_candidate_manifest = (output_root / "candidate_universe_manifest_v1.json").exists()
            wrote_decision_root_evidence = (output_root / "gatekeeper_decision_root_evidence.json").exists()

        self.assertEqual(payload["final_verdict"], "BLOCKED_L2_F_MANIFEST_OR_REPLAY_LIFECYCLE")
        self.assertEqual(strict_summary["final_verdict"], payload["final_verdict"])
        self.assertFalse(runtime_manifest["validation_run_executed"])
        self.assertFalse(runtime_manifest["approval_flags"]["runtime_approval"])
        self.assertFalse(runtime_manifest["approval_flags"]["research_grade"])
        self.assertFalse(runtime_manifest["approval_flags"]["live_equivalence"])
        self.assertEqual(
            payload["gatekeeper_denominator"]["verdict"],
            "GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN",
        )
        self.assertEqual(
            payload["sample_gates"]["status"],
            "BLOCKED_NO_DEDICATED_L2_F_SCOPE",
        )
        self.assertTrue(copied_candidate_universe)
        self.assertTrue(copied_candidate_manifest)
        self.assertTrue(wrote_decision_root_evidence)

    def test_dedicated_scope_uses_canonical_roundtrips_not_stale_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope_root = root / "dedicated-scope"
            candidate_universe = root / "candidate_universe_v1.jsonl"
            candidate_manifest = root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            stale_summary = root / "stale_summary.csv"
            missing_history = root / "missing_history.csv"
            output_root = root / "l2-f-output"
            output_csv = root / "summary.csv"

            for name in [
                "shadow_position_event_v2.jsonl",
                "shadow_replay_v2.jsonl",
                "shadow_lifecycle_v2.jsonl",
                "shadow_path_density_v2.jsonl",
            ]:
                (scope_root / name).parent.mkdir(parents=True, exist_ok=True)
                (scope_root / name).write_text("", encoding="utf-8")

            stale_summary.write_text(
                "\n".join(
                    [
                        "metric,value,notes",
                        "complete_executable_roundtrip_positions,999,stale",
                        "research_candidate_roundtrip_count,999,stale",
                        "entry_execution_label_grade_RESEARCH_CANDIDATE_count,999,stale",
                        "exit_execution_label_grade_RESEARCH_CANDIDATE_count,999,stale",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                    }
                ],
            )
            write_json(
                candidate_manifest,
                {
                    "status": "ok",
                    "denominator_invariant_status": "PASS",
                    "decision_logs_created_denominator_rows": 0,
                    "candidate_ids_from_decision_only": 0,
                },
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "verdict_type": "BUY",
                        "decision_verdict_buy": True,
                    }
                ],
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--run-id",
                    "fixture-l2-f-dedicated",
                    "--scope-root",
                    str(scope_root),
                    "--output-root",
                    str(output_root),
                    "--candidate-universe",
                    str(candidate_universe),
                    "--candidate-manifest",
                    str(candidate_manifest),
                    "--decision-root",
                    str(decisions),
                    "--summary-csv",
                    str(stale_summary),
                    "--historical-summary-csv",
                    str(missing_history),
                    "--precondition-density-summary",
                    str(missing_history),
                    "--output-csv",
                    str(output_csv),
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            payload = json.loads(result.stdout)
            summary = output_csv.read_text(encoding="utf-8")

        self.assertEqual(payload["sample_gates"]["source"], "dedicated_scope_canonical_stream")
        self.assertEqual(payload["sample_gates"]["complete_executable_roundtrip_positions"], 0)
        self.assertEqual(payload["sample_gates"]["research_candidate_roundtrip_count"], 0)
        self.assertEqual(
            payload["sample_gates"]["entry_execution_label_grade_RESEARCH_CANDIDATE_count"],
            0,
        )
        self.assertEqual(
            payload["sample_gates"]["exit_execution_label_grade_RESEARCH_CANDIDATE_count"],
            0,
        )
        self.assertIn("complete_executable_roundtrip_positions,0,", summary)
        self.assertIn("research_candidate_roundtrip_count,0,", summary)

    def test_dedicated_scope_counts_executable_diagnostic_without_research_grant(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope_root = root / "dedicated-scope"
            candidate_universe = root / "candidate_universe_v1.jsonl"
            candidate_manifest = root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            missing_history = root / "missing_history.csv"
            output_root = root / "l2-f-output"
            output_csv = root / "summary.csv"
            position = "position-a"

            write_jsonl(
                scope_root / "shadow_position_event_v2.jsonl",
                [
                    canonical_row(
                        "shadow_entry_fill_v2",
                        position,
                        {
                            "fill_status": "FILLED",
                            "execution_label_grade": "DIAGNOSTIC_SIM",
                            "execution_simulation_ready": True,
                            "research_provenance_ready": False,
                        },
                    ),
                    canonical_row(
                        "shadow_exit_fill_v2",
                        position,
                        {
                            "fill_status": "FILLED",
                            "execution_label_grade": "DIAGNOSTIC_SIM",
                            "execution_simulation_ready": True,
                            "research_provenance_ready": False,
                        },
                    ),
                    canonical_row(
                        "shadow_terminal_truth_v2",
                        position,
                        {"final_pnl_executable_bps": 12.5},
                    ),
                ],
            )
            for name in [
                "shadow_replay_v2.jsonl",
                "shadow_lifecycle_v2.jsonl",
                "shadow_path_density_v2.jsonl",
            ]:
                (scope_root / name).write_text("", encoding="utf-8")
            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                    }
                ],
            )
            write_json(
                candidate_manifest,
                {
                    "status": "ok",
                    "denominator_invariant_status": "PASS",
                    "decision_logs_created_denominator_rows": 0,
                    "candidate_ids_from_decision_only": 0,
                },
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "verdict_type": "BUY",
                        "decision_verdict_buy": True,
                    }
                ],
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--run-id",
                    "fixture-l2-f-dedicated",
                    "--scope-root",
                    str(scope_root),
                    "--output-root",
                    str(output_root),
                    "--candidate-universe",
                    str(candidate_universe),
                    "--candidate-manifest",
                    str(candidate_manifest),
                    "--decision-root",
                    str(decisions),
                    "--historical-summary-csv",
                    str(missing_history),
                    "--precondition-density-summary",
                    str(missing_history),
                    "--output-csv",
                    str(output_csv),
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            payload = json.loads(result.stdout)
            summary = output_csv.read_text(encoding="utf-8")

        self.assertEqual(payload["sample_gates"]["source"], "dedicated_scope_canonical_stream")
        self.assertEqual(payload["sample_gates"]["complete_executable_roundtrip_positions"], 1)
        self.assertEqual(payload["sample_gates"]["research_candidate_roundtrip_count"], 0)
        self.assertEqual(
            payload["sample_gates"]["status"],
            "BLOCKED_INSUFFICIENT_COMPLETE_EXECUTABLE_ROUNDTRIPS",
        )
        self.assertFalse(payload["approval_flags"]["research_grade"])
        self.assertIn("complete_executable_roundtrip_positions,1,", summary)
        self.assertIn("research_candidate_roundtrip_count,0,", summary)

    def test_candidate_artifacts_already_in_output_root_are_not_copied_over_themselves(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output_root = root / "l2-f-output"
            scope_root = root / "dedicated-scope"
            candidate_universe = output_root / "candidate_universe_v1.jsonl"
            candidate_manifest = output_root / "candidate_universe_manifest_v1.json"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            missing_history = root / "missing_history.csv"
            output_csv = root / "summary.csv"

            for name in [
                "shadow_position_event_v2.jsonl",
                "shadow_replay_v2.jsonl",
                "shadow_lifecycle_v2.jsonl",
                "shadow_path_density_v2.jsonl",
            ]:
                (scope_root / name).parent.mkdir(parents=True, exist_ok=True)
                (scope_root / name).write_text("", encoding="utf-8")
            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                    }
                ],
            )
            write_json(
                candidate_manifest,
                {
                    "status": "ok",
                    "denominator_invariant_status": "PASS",
                    "decision_logs_created_denominator_rows": 0,
                    "candidate_ids_from_decision_only": 0,
                },
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "candidate-a",
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "verdict_type": "BUY",
                        "decision_verdict_buy": True,
                    }
                ],
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--run-id",
                    "fixture-l2-f-dedicated",
                    "--scope-root",
                    str(scope_root),
                    "--output-root",
                    str(output_root),
                    "--candidate-universe",
                    str(candidate_universe),
                    "--candidate-manifest",
                    str(candidate_manifest),
                    "--decision-root",
                    str(decisions),
                    "--historical-summary-csv",
                    str(missing_history),
                    "--precondition-density-summary",
                    str(missing_history),
                    "--output-csv",
                    str(output_csv),
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            payload = json.loads(result.stdout)
            copied_candidate_universe = (output_root / "candidate_universe_v1.jsonl").exists()
            copied_candidate_manifest = (output_root / "candidate_universe_manifest_v1.json").exists()

        self.assertEqual(payload["sample_gates"]["source"], "dedicated_scope_canonical_stream")
        self.assertTrue(copied_candidate_universe)
        self.assertTrue(copied_candidate_manifest)

    def test_derives_run_local_candidate_universe_from_launcher_new_pool_events(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope_root = root / "dedicated-scope"
            output_root = root / "l2-f-output"
            decisions = root / "gatekeeper_v2_decisions.jsonl"
            missing_history = root / "missing_history.csv"
            output_csv = root / "summary.csv"

            for name in [
                "shadow_position_event_v2.jsonl",
                "shadow_replay_v2.jsonl",
                "shadow_lifecycle_v2.jsonl",
                "shadow_path_density_v2.jsonl",
            ]:
                (scope_root / name).parent.mkdir(parents=True, exist_ok=True)
                (scope_root / name).write_text("", encoding="utf-8")
            (scope_root / "launcher.stdout.log").write_text(
                "2026-07-05T22:06:11.611278Z  INFO ghost_launcher::components::seer: "
                "Seer: 🚀 Emitting NewPoolDetected: pool_amm_id=pool-a, "
                "base_mint=mint-a, slot=Some(431034924), amm_program=program-a\n",
                encoding="utf-8",
            )
            write_jsonl(
                decisions,
                [
                    {
                        "base_mint": "mint-a",
                        "pool_id": "pool-a",
                        "verdict_type": "BUY",
                        "decision_verdict_buy": True,
                    }
                ],
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--run-id",
                    "fixture-l2-f-derived-denominator",
                    "--scope-root",
                    str(scope_root),
                    "--output-root",
                    str(output_root),
                    "--decision-root",
                    str(decisions),
                    "--historical-summary-csv",
                    str(missing_history),
                    "--precondition-density-summary",
                    str(missing_history),
                    "--output-csv",
                    str(output_csv),
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            payload = json.loads(result.stdout)
            candidate_rows = (output_root / "candidate_universe_v1.jsonl").read_text(encoding="utf-8").splitlines()
            candidate_manifest = json.loads((output_root / "candidate_universe_manifest_v1.json").read_text())

        self.assertEqual(len(candidate_rows), 1)
        self.assertEqual(candidate_manifest["denominator_source"], "launcher_stdout_new_pool_detected")
        self.assertEqual(candidate_manifest["decision_logs_created_denominator_rows"], 0)
        self.assertEqual(candidate_manifest["candidate_ids_from_decision_only"], 0)
        self.assertEqual(
            payload["gatekeeper_denominator"]["verdict"],
            "GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN",
        )
        self.assertEqual(payload["gatekeeper_denominator"]["gatekeeper_decision_joined_to_candidate_count"], 1)


if __name__ == "__main__":
    unittest.main()
