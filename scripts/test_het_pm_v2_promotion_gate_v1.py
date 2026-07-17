#!/usr/bin/env python3

from __future__ import annotations

import copy
import contextlib
import importlib.util
import io
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

    def gate_eligibility(self, gates: dict) -> dict:
        economic = gates["economic_result"]
        observed = economic["observed"]
        checks = economic["checks"]
        result = {}
        for gate_key, contract in self.criteria["gate_promotion_contract"].items():
            if gate_key == "executable_trailing":
                gate_passed = bool(
                    checks.get("executable_trailing_candidate_positions")
                    and checks.get("executable_trailing_matched_positions")
                )
            elif gate_key == "vitality_decay":
                gate_passed = bool(
                    checks.get("vitality_candidate_positions")
                    and checks.get("vitality_matched_positions")
                )
            elif contract["promotion_requested"]:
                gate_passed = economic["passed"]
            else:
                gate_passed = None
            metric_prefix = "vitality" if gate_key == "vitality_decay" else gate_key
            result[gate_key] = {
                "promotion_requested": contract["promotion_requested"],
                "authority_eligible": contract["authority_eligible"],
                "promotion_gate_passed": gate_passed,
                "candidate_positions": observed.get(f"{metric_prefix}_candidate_positions", 0),
                "matched_positions": observed.get(f"{metric_prefix}_matched_positions", 0),
            }
        return result

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
            "analysis_dependency_hashes": {
                "promotion_tool": gate.sha256(TOOL_PATH),
                "pr_a_analyzer": gate.sha256(ROOT / "scripts" / "het_pm_v2_analysis.py"),
            },
            "analysis_tool_hash": gate.sha256(TOOL_PATH),
            "run_ids": ["validation-a", "validation-b"],
            "validation_run_ids": ["validation-a", "validation-b"],
            "launch_cohort_ids": ["cohort-a", "cohort-b"],
            "input_manifests": [],
            "criteria": {
                "criteria_version": self.criteria["criteria_version"],
                "criteria_hash": gate.hash_bytes(gate.canonical_json(self.criteria)),
            },
            "denominator_contract": self.criteria["position_denominator"],
            "gate_eligibility": self.gate_eligibility(gates),
            "gates": gates,
            "promotion_gate_passed": root,
        }

    def position_key(self, suffix: str = "1") -> tuple[str, str, int]:
        return ("validation-run", f"pool{suffix}:mint{suffix}:shadow", int(suffix))

    def opened_position(self, key: tuple[str, str, int]) -> dict:
        return {
            "candidate_id": f"candidate-{key[2]}",
            "event_time_ms": 1_000,
            "pool_id": f"pool{key[2]}",
            "base_mint": f"mint{key[2]}",
        }

    def comparison_row(
        self,
        *,
        key: tuple[str, str, int],
        timestamp_ms: int,
        reason: str | None = None,
        return_bps: int = 100,
        current_executable_bps: int | None = None,
        route_status: str = "PumpCurveSupported",
    ) -> dict:
        return {
            "run_id": key[0],
            "position_id": key[1],
            "position_epoch": key[2],
            "observation_timestamp_ms": timestamp_ms,
            "v2_final": (
                None
                if reason is None
                else f"ExitAll {{ reason: {reason}, quantity_raw: 1, executable_gross_return_bps: {return_bps} }}"
            ),
            "current_executable_gross_return_bps": current_executable_bps,
            "entry_value_quote_raw": 1_000_000_000,
            "trajectory": {"quality": "usable"},
            "route_status": route_status,
            "anchor_before": None,
            "anchor_applied": False,
        }

    def terminal(self) -> dict:
        return {
            "executable_gross_return_pct": 0.5,
            "absolute_age_ms": 10_000,
            "close_reason": "target",
        }

    def replay(self) -> dict:
        return {
            "mfe_bps": 1_000,
            "entry_ts_ms": 0,
            "path_bps": [[0, 0], [1_000, 100], [3_000, 200]],
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

    def test_validate_command_requires_source_manifests(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                gate.parse_args(
                    [
                        "validate",
                        "--criteria",
                        str(CRITERIA_PATH),
                        "--artifact",
                        "promotion.json",
                    ]
                )

    def test_structure_valid_artifact_cannot_source_validate_without_manifests(self) -> None:
        artifact = self.artifact(self.evaluate(), True)
        gate.validate_promotion_artifact(artifact, self.criteria)
        with tempfile.TemporaryDirectory() as tmp:
            artifact_path = Path(tmp) / "promotion.json"
            artifact_path.write_bytes(gate.canonical_json(artifact))
            with self.assertRaisesRegex(gate.ContractError, "source run manifest"):
                gate.validate_promotion_artifact_against_sources(
                    criteria=self.criteria,
                    manifest_paths=[],
                    repo_root=Path(tmp),
                    artifact_path=artifact_path,
                )

    def test_non_promoted_crash_gate_cannot_report_promotion_pass(self) -> None:
        artifact = self.artifact(self.evaluate(), True)
        artifact["gate_eligibility"]["crash"]["promotion_gate_passed"] = True
        with self.assertRaisesRegex(gate.ContractError, "non-promoted gate"):
            gate.validate_promotion_artifact(artifact, self.criteria)

    def test_zero_trailing_candidates_cannot_authorize_trailing(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["executable_trailing_candidate_positions"] = 0
        observed["economic_result"]["executable_trailing_matched_positions"] = 0
        gates = self.evaluate(observed)
        self.assertFalse(gates["economic_result"]["checks"]["executable_trailing_candidate_positions"])
        self.assertFalse(gates["economic_result"]["checks"]["executable_trailing_matched_positions"])
        self.assertFalse(self.gate_eligibility(gates)["executable_trailing"]["promotion_gate_passed"])

    def test_crash_candidates_cannot_satisfy_trailing_sample_minimum(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["matched_v2_candidate_positions"] = 50
        observed["economic_result"]["executable_trailing_candidate_positions"] = 0
        observed["economic_result"]["executable_trailing_matched_positions"] = 0
        observed["economic_result"]["vitality_candidate_positions"] = 50
        observed["economic_result"]["vitality_matched_positions"] = 50
        gates = self.evaluate(observed)
        self.assertFalse(gates["economic_result"]["passed"])
        self.assertFalse(self.gate_eligibility(gates)["executable_trailing"]["promotion_gate_passed"])

    def test_earlier_unpromoted_crash_does_not_remove_later_trailing(self) -> None:
        key = self.position_key()
        observed, matched = gate.economic_observations(
            {key: self.opened_position(key)},
            {
                key: [
                    self.comparison_row(key=key, timestamp_ms=1_000, reason="Crash", return_bps=-500),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=2_000,
                        reason="ExecutableTrailing",
                        return_bps=250,
                    ),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=3_000,
                        current_executable_bps=350,
                    ),
                ]
            },
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual(observed["gate_specific_economics"]["crash"]["candidate_positions"], 1)
        self.assertEqual(
            observed["gate_specific_economics"]["executable_trailing"]["candidate_positions"],
            1,
        )
        self.assertEqual(
            observed["gate_specific_economics"]["executable_trailing"]["matched_positions"],
            1,
        )
        self.assertEqual([row["reason"] for row in matched], ["ExecutableTrailing"])

    def test_later_candidate_recurrence_is_not_executable_continuation(self) -> None:
        key = self.position_key()
        observed, _ = gate.economic_observations(
            {key: self.opened_position(key)},
            {
                key: [
                    self.comparison_row(
                        key=key,
                        timestamp_ms=1_000,
                        reason="ExecutableTrailing",
                        return_bps=100,
                    ),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=2_000,
                        reason="ExecutableTrailing",
                        return_bps=200,
                    ),
                ]
            },
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual(observed["later_candidate_recurrence_rate"], 1.0)
        self.assertEqual(observed["candidate_executable_continuation_coverage"], 0.0)

    def test_candidate_bearing_censored_count_fails_economic_gate(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["candidate_bearing_censored_count"] = 1
        gates = self.evaluate(observed)
        self.assertFalse(gates["economic_result"]["checks"]["candidate_bearing_censored_count"])
        self.assertFalse(gates["economic_result"]["passed"])

    def test_calibration_manifest_is_rejected_by_promotion_evaluate(self) -> None:
        with self.assertRaisesRegex(gate.ContractError, "validation manifests only"):
            gate.evaluate(
                self.criteria,
                [
                    (
                        Path("r1b_input_manifest.json"),
                        {"run_id": "r1b", "run_role": "calibration"},
                        {},
                    )
                ],
            )

    def test_terminal_correlation_requires_exact_identity_action_snapshot_and_writer(self) -> None:
        key = ("run-a", "pool:mint:shadow", 7)
        comparison = {
            "run_id": key[0],
            "position_id": key[1],
            "position_epoch": key[2],
            "writer_instance_id": "writer-a",
            "snapshot_id": "snapshot-a",
            "v1_authority_receipt": {"action_id": "action-a"},
        }
        terminal = {
            "het_pm_v2_comparison_write_status": "written",
            "het_pm_v2_writer_instance_id": "writer-a",
            "het_pm_v2_source_snapshot_id": "snapshot-a",
            "action_id": "action-a",
        }
        self.assertTrue(gate.terminal_comparison_exactly_correlated(key, terminal, comparison))
        for field, bad_value in (
            ("run_id", "run-b"),
            ("position_id", "other-position"),
            ("position_epoch", 8),
            ("writer_instance_id", "writer-b"),
            ("snapshot_id", "snapshot-b"),
        ):
            changed = copy.deepcopy(comparison)
            changed[field] = bad_value
            self.assertFalse(
                gate.terminal_comparison_exactly_correlated(key, terminal, changed),
                field,
            )
        changed_terminal = copy.deepcopy(terminal)
        changed_terminal["action_id"] = "action-b"
        self.assertFalse(
            gate.terminal_comparison_exactly_correlated(key, changed_terminal, comparison)
        )

    def test_admission_reconciliation_counts_missing_opened_position_evidence(self) -> None:
        key = self.position_key()
        opened = {key: self.opened_position(key)}
        empty_summary = gate.summarize_admission([])
        reconciled = gate.reconcile_admission_with_opened_positions([], opened, empty_summary)
        self.assertEqual(reconciled["admission_missing_final_count"], 1)
        self.assertEqual(reconciled["admission_missing_monitoring_registered_count"], 1)
        self.assertEqual(reconciled["admission_missing_release_count"], 1)

        complete_rows = [
            {
                "run_id": key[0],
                "candidate_id": opened[key]["candidate_id"],
                "pool_id": opened[key]["pool_id"],
                "base_mint": opened[key]["base_mint"],
                "lane": "shadow",
                "stage": "post_buy_submitted",
            },
            {
                "run_id": key[0],
                "candidate_id": opened[key]["candidate_id"],
                "pool_id": opened[key]["pool_id"],
                "base_mint": opened[key]["base_mint"],
                "lane": "shadow",
                "stage": "handoff_accepted",
                "handoff_accepted": True,
            },
            {
                "run_id": key[0],
                "candidate_id": opened[key]["candidate_id"],
                "pool_id": opened[key]["pool_id"],
                "base_mint": opened[key]["base_mint"],
                "lane": "shadow",
                "stage": "monitoring_registered",
                "position_id": key[1],
                "position_epoch": key[2],
                "handoff_accepted": True,
            },
            {
                "run_id": key[0],
                "candidate_id": opened[key]["candidate_id"],
                "pool_id": opened[key]["pool_id"],
                "base_mint": opened[key]["base_mint"],
                "lane": "shadow",
                "stage": "terminal_release",
                "position_id": key[1],
                "position_epoch": key[2],
                "release_status": "released",
            },
        ]
        complete = gate.reconcile_admission_with_opened_positions(
            complete_rows,
            opened,
            gate.summarize_admission(complete_rows),
        )
        self.assertEqual(complete["admission_missing_final_count"], 0)
        self.assertEqual(complete["admission_missing_monitoring_registered_count"], 0)
        self.assertEqual(complete["admission_missing_release_count"], 0)


if __name__ == "__main__":
    unittest.main()
