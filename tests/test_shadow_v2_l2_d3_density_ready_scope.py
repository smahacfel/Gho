#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
GENERATOR = REPO_ROOT / "scripts" / "shadow_v2_l2_d3_density_ready_scope.py"
AUDIT = REPO_ROOT / "scripts" / "shadow_v2_path_density_horizon_audit.py"


class ShadowV2L2D3DensityReadyScopeTest(unittest.TestCase):
    def test_generated_scope_passes_declared_density_horizons(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scope = Path(tmp) / "scope"
            generated = subprocess.run(
                [
                    sys.executable,
                    str(GENERATOR),
                    "--scope-root",
                    str(scope),
                    "--run-id",
                    "fixture-l2-d3-density",
                    "--positions",
                    "3",
                    "--duration-ms",
                    "121000",
                    "--sample-interval-ms",
                    "1000",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            generation = json.loads(generated.stdout)
            self.assertEqual(generation["configured_run_seconds"], 121)
            self.assertEqual(generation["path_sample_count"], 366)
            self.assertEqual(generation["density_row_count"], 21)

            audit = subprocess.run(
                [
                    sys.executable,
                    str(AUDIT),
                    "--scope-root",
                    str(scope),
                    "--pass-verdict",
                    "L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            result = json.loads(audit.stdout)

        self.assertEqual(result["verdict"], "L2_D3_DENSITY_CONTRACT_FIXTURE_ACCEPTED")
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
        self.assertEqual(result["declared_horizon_present_count"], 5)
        self.assertEqual(result["declared_horizon_path_coverage_blocker_count"], 0)
        self.assertEqual(result["declared_horizon_retention_blocker_count"], 0)
        by_horizon = {row["horizon_ms"]: row for row in result["per_horizon"]}
        self.assertEqual(by_horizon[120_000]["evaluable_positions"], 3)
        self.assertEqual(by_horizon[120_000]["coverage_ratio"], 1.0)
        self.assertEqual(by_horizon[120_000]["max_gap_ms_max"], 1_000)
        self.assertFalse(by_horizon[120_000]["positive_research_claim_allowed"])
        self.assertEqual(
            by_horizon[300_000]["verdict"],
            "NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE",
        )


if __name__ == "__main__":
    unittest.main()
