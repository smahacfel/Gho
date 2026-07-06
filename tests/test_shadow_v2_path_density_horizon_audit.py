#!/usr/bin/env python3
from __future__ import annotations

import json
import csv
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
AUDIT_SCRIPT = REPO_ROOT / "scripts" / "shadow_v2_path_density_horizon_audit.py"
DECLARED_HORIZONS = [2_000, 3_000, 10_000, 30_000, 120_000]
UNDECLARED_LONG_HORIZONS = [300_000, 500_000]


def density_row(
    horizon_ms: int,
    *,
    position_id: str = "pos-a",
    verdict: str = "EVALUABLE_EXACT",
    replay_horizon_ms: int = 121_000,
    path_points: int = 122,
    coverage_points: int = 10,
    max_interval_ms: int | None = 1_000,
    duplicate_age_count: int = 0,
    non_monotonic_input: bool = False,
    truncated: bool = False,
    limitations: list[str] | None = None,
) -> dict:
    return {
        "schema": "shadow_path_density_v2",
        "schema_version": 1,
        "run_id": "fixture-run",
        "session_id": "fixture-session",
        "position_id": position_id,
        "pool_id": "pool-a",
        "base_mint": "mint-a",
        "canonical_event_stream_ref": "shadow_position_event_v2.jsonl",
        "source_path_sample_event_ids": ["path-a"],
        "source_canonical_high_watermark": "event-high-watermark-a",
        "horizon_ms": horizon_ms,
        "verdict": verdict,
        "path_points": path_points,
        "coverage_points": coverage_points,
        "replay_horizon_ms": replay_horizon_ms,
        "first_path_point_age_ms": 0,
        "median_interval_ms": max_interval_ms,
        "p90_interval_ms": max_interval_ms,
        "max_interval_ms": max_interval_ms,
        "duplicate_age_count": duplicate_age_count,
        "non_monotonic_input": non_monotonic_input,
        "truncated": truncated,
        "limitations": limitations or [],
        "created_at_wall_ms": 1_785_000_000_000,
    }


class ShadowV2PathDensityHorizonAuditTest(unittest.TestCase):
    def run_audit(self, rows: list[dict], *extra_args: str) -> dict:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp)
            with (scope / "shadow_path_density_v2.jsonl").open("w", encoding="utf-8") as fh:
                for row in rows:
                    fh.write(json.dumps(row, sort_keys=True))
                    fh.write("\n")
            result = subprocess.run(
                [sys.executable, str(AUDIT_SCRIPT), "--scope-root", str(scope), *extra_args],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            return json.loads(result.stdout)

    def test_declared_horizons_pass_and_long_horizons_are_non_blocking(self) -> None:
        rows = [density_row(horizon) for horizon in DECLARED_HORIZONS]
        rows.extend(
            density_row(
                horizon,
                verdict="NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
                coverage_points=0,
                max_interval_ms=None,
                limitations=["HORIZON_EXCEEDS_REPLAY_COVERAGE"],
            )
            for horizon in UNDECLARED_LONG_HORIZONS
        )

        result = self.run_audit(rows)

        self.assertEqual(
            result["verdict"],
            "L2_D2_DENSITY_RETENTION_READY_FOR_L2_F",
        )
        self.assertTrue(result["l2_f_allowed_next"])
        self.assertFalse(result["undeclared_horizons_block_l2_baseline"])
        by_horizon = {row["horizon_ms"]: row for row in result["per_horizon"]}
        self.assertEqual(by_horizon[300_000]["verdict"], "NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE")
        self.assertFalse(by_horizon[300_000]["l2_baseline_blocker"])
        self.assertFalse(by_horizon[300_000]["positive_research_claim_allowed"])
        self.assertEqual(by_horizon[120_000]["verdict"], "PASS")
        self.assertEqual(result["declared_horizon_missing_count"], 0)

    def test_missing_declared_horizon_blocks_l2_density(self) -> None:
        rows = [density_row(horizon) for horizon in DECLARED_HORIZONS if horizon != 120_000]

        result = self.run_audit(rows)

        self.assertEqual(result["verdict"], "BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE")
        self.assertEqual(result["missing_declared_horizons_ms"], [120_000])
        self.assertEqual(result["declared_horizon_missing_count"], 1)

    def test_sparse_declared_horizon_is_not_promoted_to_pass(self) -> None:
        rows = [
            density_row(
                horizon,
                verdict="SPARSE_APPROX_ONLY" if horizon == 30_000 else "EVALUABLE_EXACT",
                max_interval_ms=12_000 if horizon == 30_000 else 1_000,
                limitations=(
                    ["PATH_DENSITY_INTERVAL_TOO_SPARSE_FOR_APPROX"]
                    if horizon == 30_000
                    else []
                ),
            )
            for horizon in DECLARED_HORIZONS
        ]

        result = self.run_audit(rows)

        self.assertEqual(result["verdict"], "BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE")
        by_horizon = {row["horizon_ms"]: row for row in result["per_horizon"]}
        self.assertEqual(by_horizon[30_000]["verdict"], "FAILED_DECLARED_HORIZON_INCOMPLETE")
        self.assertTrue(by_horizon[30_000]["l2_baseline_blocker"])

    def test_retention_margin_shortfall_blocks_even_when_declared_rows_are_evaluable(self) -> None:
        rows = [density_row(horizon, replay_horizon_ms=120_000) for horizon in DECLARED_HORIZONS]

        result = self.run_audit(rows)

        self.assertEqual(result["verdict"], "BLOCKED_RETENTION_CONTRACT_INSUFFICIENT")
        self.assertEqual(result["required_replay_horizon_ms"], 121_000)
        self.assertEqual(result["declared_horizon_retention_blocker_count"], len(DECLARED_HORIZONS))
        by_horizon = {row["horizon_ms"]: row for row in result["per_horizon"]}
        self.assertEqual(by_horizon[2_000]["verdict"], "FAILED_RETENTION_GAP")

    def test_no_path_coverage_blocks_even_when_declared_horizon_rows_exist(self) -> None:
        rows = [
            density_row(
                horizon,
                verdict="NOT_EVALUABLE_NO_COVERAGE",
                replay_horizon_ms=121_000,
                path_points=0,
                coverage_points=0,
                max_interval_ms=None,
                limitations=["PATH_DENSITY_NO_PATH_POINTS"],
            )
            for horizon in DECLARED_HORIZONS
        ]

        result = self.run_audit(rows)

        self.assertEqual(result["verdict"], "BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT")
        self.assertFalse(result["l2_f_allowed_next"])
        self.assertEqual(
            result["declared_horizon_path_coverage_blocker_count"],
            len(DECLARED_HORIZONS),
        )
        by_horizon = {row["horizon_ms"]: row for row in result["per_horizon"]}
        self.assertEqual(
            by_horizon[120_000]["verdict"],
            "FAILED_PATH_SAMPLE_COVERAGE_INSUFFICIENT",
        )

    def test_output_csv_contains_l2_d2_required_metrics(self) -> None:
        rows = [density_row(horizon) for horizon in DECLARED_HORIZONS]
        rows.extend(
            density_row(
                horizon,
                verdict="NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
                coverage_points=0,
                max_interval_ms=None,
                limitations=["HORIZON_EXCEEDS_REPLAY_COVERAGE"],
            )
            for horizon in UNDECLARED_LONG_HORIZONS
        )
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp) / "scope"
            scope.mkdir()
            csv_path = Path(tmp) / "summary.csv"
            with (scope / "shadow_path_density_v2.jsonl").open("w", encoding="utf-8") as fh:
                for row in rows:
                    fh.write(json.dumps(row, sort_keys=True))
                    fh.write("\n")

            subprocess.run(
                [
                    sys.executable,
                    str(AUDIT_SCRIPT),
                    "--scope-root",
                    str(scope),
                    "--output-csv",
                    str(csv_path),
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )

            with csv_path.open("r", encoding="utf-8", newline="") as fh:
                csv_rows = list(csv.DictReader(fh))

        metrics = {row["metric"]: row["value"] for row in csv_rows}
        self.assertEqual(metrics["density_audit_verdict"], "L2_D2_DENSITY_RETENTION_READY_FOR_L2_F")
        self.assertEqual(metrics["retention_contract_ms"], "121000")
        self.assertEqual(metrics["required_replay_coverage_ms"], "121000")
        self.assertEqual(metrics["horizon_120000_eligible_positions"], "1")
        self.assertEqual(metrics["horizon_120000_verdict"], "PASS")
        self.assertIn("300000", metrics["unsupported_horizons_ms"])

    def test_d3_contract_fixture_pass_does_not_allow_l2_f(self) -> None:
        rows = [density_row(horizon) for horizon in DECLARED_HORIZONS]

        result = self.run_audit(rows, "--pass-verdict", "L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED")

        self.assertEqual(result["verdict"], "L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED")
        self.assertEqual(result["pass_verdict"], "L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED")
        self.assertTrue(result["density_contract_fixture_pass"])
        self.assertEqual(
            result["pr55_contract_fixture_verdict"],
            "PR55_AS_L2_D3_CONTRACT_FIXTURE_ACCEPTED",
        )
        self.assertEqual(
            result["pr55_runtime_density_verdict"],
            "PR55_AS_RUNTIME_DENSITY_EMISSION_PROOF_NOT_ACCEPTED",
        )
        self.assertFalse(result["density_fixture_l2_f_allowed_next"])
        self.assertFalse(result["runtime_density_emission_proof"])
        self.assertEqual(result["next_stage"], "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_PROOF")
        self.assertFalse(result["l2_f_allowed_next"])

    def test_d3b_runtime_harness_pass_allows_l2_f_next_without_approval_flags(self) -> None:
        rows = [density_row(horizon) for horizon in DECLARED_HORIZONS]

        result = self.run_audit(
            rows,
            "--pass-verdict",
            "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F",
        )

        self.assertEqual(
            result["verdict"],
            "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F",
        )
        self.assertFalse(result["density_contract_fixture_pass"])
        self.assertTrue(result["runtime_harness_density_emission_proof"])
        self.assertTrue(result["density_derivation_from_canonical_rows"])
        self.assertTrue(result["runtime_density_emission_proof"])
        self.assertFalse(result["live_runtime_density_emission_proof"])
        self.assertEqual(result["next_stage"], "L2_F_RESEARCH_VALIDATION_RUN")
        self.assertTrue(result["l2_f_allowed_next"])
        self.assertFalse(result["runtime_approval"])
        self.assertFalse(result["research_grade"])
        self.assertFalse(result["live_equivalence"])
        self.assertFalse(result["strategy_research_unblocked"])
        self.assertFalse(result["shadow_close_only"])
        self.assertFalse(result["active_close"])

    def test_latest_density_snapshot_mode_ignores_immature_prior_snapshots(self) -> None:
        rows = []
        for horizon in DECLARED_HORIZONS:
            rows.append(
                density_row(
                    horizon,
                    verdict="NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY",
                    replay_horizon_ms=0,
                    path_points=1,
                    coverage_points=1,
                    max_interval_ms=None,
                    limitations=["HORIZON_EXCEEDS_REPLAY_COVERAGE"],
                )
            )
            rows.append(density_row(horizon, replay_horizon_ms=121_000, path_points=122))

        result = self.run_audit(
            rows,
            "--latest-density-per-position-horizon",
            "--pass-verdict",
            "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F",
        )

        self.assertEqual(result["density_rows_input"], len(rows))
        self.assertEqual(result["density_rows_evaluated"], len(DECLARED_HORIZONS))
        self.assertTrue(result["latest_density_per_position_horizon"])
        self.assertEqual(
            result["verdict"],
            "L2_D3B_RUNTIME_HARNESS_DENSITY_EMISSION_READY_FOR_L2_F",
        )
        self.assertEqual(result["declared_horizon_path_coverage_blocker_count"], 0)
        self.assertEqual(result["declared_horizon_retention_blocker_count"], 0)
        self.assertTrue(result["l2_f_allowed_next"])

    def test_validation_smoke_marker_rows_are_excluded_from_l2_density_scope(self) -> None:
        rows = [density_row(horizon) for horizon in DECLARED_HORIZONS]
        for horizon in DECLARED_HORIZONS:
            marker = density_row(
                horizon,
                position_id=f"validation-smoke-marker:fixture-run:{horizon}",
                verdict="NOT_EVALUABLE_NO_COVERAGE",
                replay_horizon_ms=121_000,
                path_points=0,
                coverage_points=0,
                max_interval_ms=None,
                limitations=["PATH_DENSITY_NO_PATH_POINTS"],
            )
            marker["session_id"] = f"validation-smoke-session:{horizon}"
            marker["source_canonical_high_watermark"] = (
                f"validation_smoke_marker_v2:fixture-run:{horizon}"
            )
            rows.append(marker)

        result = self.run_audit(
            rows,
            "--latest-density-per-position-horizon",
            "--pass-verdict",
            "L2_F_DENSITY_RETENTION_PASS",
        )

        self.assertEqual(result["density_rows_input"], len(rows))
        self.assertEqual(result["density_rows_candidate_scope"], len(DECLARED_HORIZONS))
        self.assertEqual(
            result["excluded_validation_smoke_density_rows"],
            len(DECLARED_HORIZONS),
        )
        self.assertEqual(result["density_rows_evaluated"], len(DECLARED_HORIZONS))
        self.assertEqual(result["verdict"], "L2_F_DENSITY_RETENTION_PASS")
        self.assertEqual(result["declared_horizon_path_coverage_blocker_count"], 0)
        by_horizon = {row["horizon_ms"]: row for row in result["per_horizon"]}
        self.assertEqual(by_horizon[120_000]["verdict"], "PASS")

    def test_position_scope_jsonl_filters_density_rows_deterministically(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = root / "scope"
            scope.mkdir()
            position_scope = root / "position_scope.jsonl"
            position_scope.write_text(
                json.dumps(
                    {
                        "schema": "shadow_v2_l2_f_evidence_complete_position_scope_v1",
                        "position_id": "pos-good",
                    },
                    sort_keys=True,
                )
                + "\n",
                encoding="utf-8",
            )
            rows = []
            for horizon in DECLARED_HORIZONS:
                rows.append(density_row(horizon, position_id="pos-good"))
                rows.append(
                    density_row(
                        horizon,
                        position_id="pos-bad",
                        verdict="SPARSE_APPROX_ONLY",
                        max_interval_ms=9_000,
                        limitations=["PATH_DENSITY_INTERVAL_TOO_SPARSE_FOR_APPROX"],
                    )
                )
            with (scope / "shadow_path_density_v2.jsonl").open("w", encoding="utf-8") as fh:
                for row in rows:
                    fh.write(json.dumps(row, sort_keys=True))
                    fh.write("\n")

            result = subprocess.run(
                [
                    sys.executable,
                    str(AUDIT_SCRIPT),
                    "--scope-root",
                    str(scope),
                    "--latest-density-per-position-horizon",
                    "--position-scope-jsonl",
                    str(position_scope),
                    "--pass-verdict",
                    "L2_F_DENSITY_RETENTION_PASS",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            payload = json.loads(result.stdout)

        self.assertEqual(payload["verdict"], "L2_F_DENSITY_RETENTION_PASS")
        self.assertTrue(payload["position_scope_filter_present"])
        self.assertEqual(payload["position_scope_position_count"], 1)
        self.assertEqual(
            payload["density_rows_excluded_outside_position_scope"],
            len(DECLARED_HORIZONS),
        )
        self.assertEqual(payload["declared_horizon_path_coverage_blocker_count"], 0)


if __name__ == "__main__":
    unittest.main()
