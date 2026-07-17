#!/usr/bin/env python3

from __future__ import annotations

import copy
import importlib.util
import json
import math
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TOOL_PATH = ROOT / "scripts" / "het_pm_v2_promotion_gate_v1.py"
CRITERIA_PATH = ROOT / "PLANS" / "DO_REALIZACJI" / "HET_PM_V2_PROMOTION_CRITERIA_V1.json"
GOLDEN_PATH = (
    ROOT
    / "scripts"
    / "fixtures"
    / "het_pm_v2_promotion"
    / "golden_pass_observed_v1.json"
)

SPEC = importlib.util.spec_from_file_location("het_pm_v2_promotion_gate_v1", TOOL_PATH)
assert SPEC is not None and SPEC.loader is not None
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class HetPmV2PromotionGateV1Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.criteria = json.loads(CRITERIA_PATH.read_text(encoding="utf-8"))
        self.observed = json.loads(GOLDEN_PATH.read_text(encoding="utf-8"))
        gate.validate_criteria(self.criteria)

    def evaluate(self, observed: dict | None = None) -> dict:
        source = observed or self.observed
        return {
            name: gate.evaluate_gate(
                name,
                source[name],
                self.criteria["gates"][name]["thresholds"],
            )
            for name in gate.GATE_NAMES
        }

    def artifact(self, gates: dict, root: bool) -> dict:
        return {
            "schema_version": gate.PROMOTION_SCHEMA_VERSION,
            "tool_id": gate.TOOL_ID,
            "tool_version": gate.TOOL_VERSION,
            "policy_id": self.criteria["policy_id"],
            "policy_version": self.criteria["policy_version"],
            "het_config_hash": self.criteria["expected_het_config_hash"],
            "v1_config_hash": self.criteria["expected_v1_config_hash"],
            "time_stop_v2_config_hash": self.criteria[
                "expected_time_stop_v2_config_hash"
            ],
            "input_manifest_hash": "0" * 64,
            "analysis_tool_hash": gate.sha256(TOOL_PATH),
            "criteria": {
                "criteria_version": self.criteria["criteria_version"],
                "criteria_hash": gate.hash_bytes(gate.canonical_json(self.criteria)),
            },
            "gates": gates,
            "promotion_gate_passed": root,
        }

    def test_golden_pass_fixture_sets_root_true(self) -> None:
        gates = self.evaluate()
        self.assertTrue(all(result["passed"] for result in gates.values()))
        artifact = self.artifact(gates, True)
        gate.validate_promotion_artifact(artifact, self.criteria)

    def test_each_gate_can_fail_and_forces_root_false(self) -> None:
        failing_fields = {
            "lifecycle_integrity": ("runtime_panic_count", 1),
            "data_coverage": ("primary_positions", 99),
            "quote_budget": ("hold_quote_count", 1),
            "economic_result": ("matched_v2_candidate_positions", 49),
            "stability": ("validation_runs", 1),
        }
        for gate_name, (field, value) in failing_fields.items():
            with self.subTest(gate=gate_name):
                observed = copy.deepcopy(self.observed)
                observed[gate_name][field] = value
                gates = self.evaluate(observed)
                self.assertFalse(gates[gate_name]["passed"])
                self.assertFalse(all(result["passed"] for result in gates.values()))

    def test_missing_and_non_finite_metrics_fail_closed(self) -> None:
        missing = copy.deepcopy(self.observed)
        missing["economic_result"].pop("cvar_20_delta_bps")
        self.assertFalse(self.evaluate(missing)["economic_result"]["passed"])

        non_finite = copy.deepcopy(self.observed)
        non_finite["economic_result"]["cvar_20_delta_bps"] = math.nan
        self.assertFalse(self.evaluate(non_finite)["economic_result"]["passed"])

    def test_insufficient_run_and_cohort_counts_fail(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["stability"]["validation_runs"] = 1
        observed["stability"]["launch_cohorts"] = 1
        observed["stability"]["creator_or_funder_cohorts"] = 19
        self.assertFalse(self.evaluate(observed)["stability"]["passed"])

    def test_manual_root_boolean_mismatch_is_rejected(self) -> None:
        with self.assertRaisesRegex(gate.ContractError, "manual root boolean"):
            gate.validate_promotion_artifact(
                self.artifact(self.evaluate(), False), self.criteria
            )

    def test_gate_passed_boolean_cannot_disagree_with_observed(self) -> None:
        gates = self.evaluate()
        gates["lifecycle_integrity"]["observed"]["runtime_panic_count"] = 1
        with self.assertRaisesRegex(gate.ContractError, "gate result mismatch"):
            gate.validate_promotion_artifact(self.artifact(gates, True), self.criteria)

    def test_manifest_hash_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_path = root / "row.jsonl"
            artifact_path.write_text("{}\n", encoding="utf-8")
            manifest = {
                "artifacts": {
                    "comparison": [
                        {
                            "path": "row.jsonl",
                            "sha256": "f" * 64,
                            "size_bytes": artifact_path.stat().st_size,
                        }
                    ]
                }
            }
            with self.assertRaisesRegex(gate.ContractError, "hash/size mismatch"):
                gate.verify_manifest_artifacts(manifest, root)

    def test_identical_inputs_produce_identical_bytes_and_hash(self) -> None:
        artifact = self.artifact(self.evaluate(), True)
        first = gate.canonical_json(artifact)
        second = gate.canonical_json(copy.deepcopy(artifact))
        self.assertEqual(first, second)
        self.assertEqual(gate.hash_bytes(first), gate.hash_bytes(second))


if __name__ == "__main__":
    unittest.main()
