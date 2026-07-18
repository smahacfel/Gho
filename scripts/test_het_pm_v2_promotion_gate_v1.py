#!/usr/bin/env python3

from __future__ import annotations

import copy
import contextlib
import importlib.util
import io
import json
import math
import subprocess
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
        # Arithmetic fixtures predate the prospective sample-size contract.
        # Keep their narrow assertions independent of the production release
        # thresholds; dedicated tests below assert that the checked-in values
        # are non-vacuous.
        self.criteria["contract_state"] = "locked"
        economic = self.criteria["gates"]["economic_result"]["thresholds"]
        economic.update({
            "min_matched_v2_candidate_positions": 50,
            "min_executable_trailing_candidate_positions": 5,
            "min_executable_trailing_matched_positions": 3,
            "min_vitality_candidate_positions": 5,
            "min_vitality_matched_positions": 3,
            "min_mfe_capture_positions": 30,
            "min_missed_protection_eligible_positions": 30,
            "mean_peak_to_terminal_giveback_delta_bps_min": -1000,
            "mean_mfe_capture_ratio_delta_min": -1.0,
            "mean_terminal_loss_delta_bps_min": -1000,
            "tail_loss_p10_delta_bps_min": -1500,
            "cvar_20_delta_bps_min": -1500,
            "worst_cost_scenario_mean_delta_bps_min": -1500,
            "top_k_positive_improvement_share_max": 1.0,
            "trimmed_mean_delta_bps_min": -1000,
            "false_early_exit_proxy_rate_max": 1.0,
            "missed_protection_proxy_rate_max": 1.0,
        })
        self.criteria["gates"]["data_coverage"]["thresholds"].update({
            "candidate_executable_continuation_coverage_min": 0.0,
            "route_availability_after_candidate_min": 0.0,
        })
        self.criteria["gates"]["stability"]["thresholds"].update({
            "per_run_min_primary_positions_min": 0,
            "per_run_min_matched_v2_candidate_positions_min": 0,
            "per_run_min_executable_trailing_matched_positions_min": 0,
            "per_run_min_vitality_matched_positions_min": 0,
            "per_run_min_candidate_executable_continuation_coverage_min": 0.0,
            "per_run_worst_mean_delta_bps_min": -2_000,
            "per_run_worst_tail_loss_p10_delta_bps_min": -2_000,
            "per_run_worst_cost_scenario_mean_delta_bps_min": -2_000,
        })
        for gate_key in ("executable_trailing", "vitality_decay"):
            # The historical arithmetic golden fixture predates the
            # per-run-by-gate projection.  Supply its deterministic legacy
            # equivalent only in this fixture; production criteria require the
            # runtime-generated values and fail closed when they are absent.
            self.observed["economic_result"]["gate_specific_economics"][gate_key].update({
                "per_run_min_matched_positions": 3,
                "per_run_worst_mean_peak_to_terminal_giveback_delta_bps": -1_000,
                "per_run_worst_tail_loss_p10_delta_bps": -1_500,
                "per_run_worst_cvar_20_delta_bps": -1_500,
                "per_run_worst_cost_scenario_mean_delta_bps": -1_500,
                "per_run_max_false_early_exit_proxy_rate": 1.0,
                "per_run_min_candidate_executable_continuation_coverage": 0.0,
            })
        for gate_key, candidate_name, matched_name in (
            ("executable_trailing", "executable_trailing_candidate_positions_min", "executable_trailing_matched_positions_min"),
            ("vitality_decay", "vitality_candidate_positions_min", "vitality_matched_positions_min"),
        ):
            thresholds = self.criteria["gate_specific_thresholds"][gate_key]["thresholds"]
            thresholds.update({
                candidate_name: 5,
                matched_name: 3,
                "mean_peak_to_terminal_giveback_delta_bps_min": -1000,
                "mean_mfe_capture_ratio_delta_min": -1.0,
                "mean_terminal_loss_delta_bps_min": -1000,
                "tail_loss_p10_delta_bps_min": -1500,
                "cvar_20_delta_bps_min": -1500,
                "worst_cost_scenario_mean_delta_bps_min": -1500,
                "top_k_positive_improvement_share_max": 1.0,
                "trimmed_mean_delta_bps_min": -1000,
                "false_early_exit_proxy_rate_max": 1.0,
                "candidate_executable_continuation_coverage_min": 0.0,
                "route_availability_after_candidate_min": 0.8,
                "per_run_min_matched_positions_min": 0,
                "per_run_worst_mean_peak_to_terminal_giveback_delta_bps_min": -2000,
                "per_run_worst_tail_loss_p10_delta_bps_min": -2000,
                "per_run_worst_cvar_20_delta_bps_min": -2000,
                "per_run_worst_cost_scenario_mean_delta_bps_min": -2000,
                "per_run_max_false_early_exit_proxy_rate_max": 1.0,
                "per_run_min_candidate_executable_continuation_coverage_min": 0.0,
            })

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
        gate_specific = observed["gate_specific_economics"]
        result = {}
        for gate_key, contract in self.criteria["gate_promotion_contract"].items():
            gate_result = None
            gate_passed = None
            if contract["promotion_requested"]:
                gate_result = gate.evaluate_gate_specific_promotion(
                    gate_key, gate_specific[gate_key], self.criteria
                )
                gate_passed = gate_result["passed"]
            metric_prefix = "vitality" if gate_key == "vitality_decay" else gate_key
            result[gate_key] = {
                "promotion_requested": contract["promotion_requested"],
                "authority_eligible": contract["authority_eligible"],
                "promotion_gate_passed": gate_passed,
                "candidate_positions": observed.get(f"{metric_prefix}_candidate_positions", 0),
                "matched_positions": observed.get(f"{metric_prefix}_matched_positions", 0),
                "economic_checks": gate_result["checks"] if gate_result is not None else None,
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
        route_status: str = "pump_curve_supported",
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

    def schema_v3_lattice_row(
        self,
        *,
        key: tuple[str, str, int],
        timestamp_ms: int,
        exits: dict[str, int],
        current_executable_bps: int | None = None,
    ) -> dict:
        """Build one producer-shaped Schema-V3 comparison tick.

        A real HET record carries every gate evaluation in one row.  Tests of
        selective promotion must use that shape; two same-timestamp legacy
        rows would not prove that the analyzer can see a suppressed lower gate
        from the runtime's actual evidence surface.
        """
        row = self.comparison_row(
            key=key,
            timestamp_ms=timestamp_ms,
            current_executable_bps=current_executable_bps,
        )
        row["schema_version"] = 3
        row["v2_gate_evaluations"] = []
        for gate_key, reason in gate.V2_REASON_KEYS.items():
            if reason in exits:
                return_bps = exits[reason]
                row["v2_gate_evaluations"].append(
                    {
                        "gate": reason,
                        "prequote": {"kind": "quote_required", "detail": reason},
                        "quote_status": "resolved",
                        "final_decision": {
                            "kind": "exit_all",
                            "detail": {
                                "reason": reason,
                                "quantity_raw": 1,
                                "executable_gross_return_bps": return_bps,
                            },
                        },
                        "executable_gross_return_bps": return_bps,
                    }
                )
            else:
                row["v2_gate_evaluations"].append(
                    {
                        "gate": reason,
                        "prequote": {"kind": "hold"},
                        "quote_status": "not_required",
                        "final_decision": {"kind": "hold"},
                        "executable_gross_return_bps": None,
                    }
                )
        return row

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

    def criteria_template(self) -> dict:
        template = json.loads(CRITERIA_PATH.read_text(encoding="utf-8"))
        template["contract_state"] = "calibration_pending"
        for field in (
            "expected_runtime_commit_sha",
            "expected_release_binary_sha256",
            "expected_brain_config_content_hash",
            "expected_normalized_behavioral_config_hash",
            "expected_promotion_tool_hash",
            "expected_pr_a_analyzer_hash",
        ):
            template[field] = "unlocked"
        template["allowed_exact_run_config_hashes"] = {}
        gate.validate_criteria(template)
        return template

    def write_lock_inputs(self, root: Path) -> tuple[Path, Path, Path, Path]:
        brain = root / "brain.toml"
        first = root / "first.toml"
        second = root / "second.toml"
        binary = root / "ghost-launcher"
        brain.write_text("[post_buy_guardian.het_pm_v2]\nenabled = true\n", encoding="utf-8")
        first.write_text(
            "[p37_shadow_probe]\nrun_id = 'validation-v1a'\n"
            "session_id = 'a'\nselection_log_path = '/tmp/a.jsonl'\n"
            "sampling_version = 'frozen-v1'\nmax_probes_per_run = 100\n",
            encoding="utf-8",
        )
        second.write_text(
            "[p37_shadow_probe]\nrun_id = 'validation-v1b'\n"
            "session_id = 'b'\nselection_log_path = '/tmp/b.jsonl'\n"
            "sampling_version = 'frozen-v1'\nmax_probes_per_run = 100\n",
            encoding="utf-8",
        )
        binary.write_bytes(b"release-binary")
        return brain, first, second, binary

    def git(self, repo: Path, *args: str) -> str:
        result = subprocess.run(
            ["git", "-C", str(repo), *args],
            capture_output=True,
            check=True,
            text=True,
        )
        return result.stdout.strip()

    def test_golden_pass_fixture_sets_root_true(self) -> None:
        gates = self.evaluate()
        self.assertTrue(all(result["passed"] for result in gates.values()))
        artifact = self.artifact(gates, True)
        gate.validate_promotion_artifact(artifact, self.criteria)

    def test_checked_in_template_keeps_runtime_policy_identity_and_non_vacuous_floors(self) -> None:
        production = json.loads(CRITERIA_PATH.read_text(encoding="utf-8"))
        self.assertEqual(production["policy_version"], 2)
        self.assertEqual(production["comparison_schema_version"], 3)
        self.assertEqual(production["contract_state"], "locked")
        self.assertEqual(
            gate.canonicalize_runtime_commit_sha(
                production["expected_runtime_commit_sha"],
                ROOT,
            ),
            production["expected_runtime_commit_sha"],
        )
        for gate_key, prefix in (("executable_trailing", "executable_trailing"), ("vitality_decay", "vitality")):
            thresholds = production["gate_specific_thresholds"][gate_key]["thresholds"]
            self.assertGreaterEqual(thresholds[f"{prefix}_candidate_positions_min"], 100)
            self.assertGreaterEqual(thresholds[f"{prefix}_matched_positions_min"], 80)
            self.assertGreater(thresholds["candidate_executable_continuation_coverage_min"], 0.0)
            self.assertLess(thresholds["false_early_exit_proxy_rate_max"], 1.0)
            self.assertLess(thresholds["top_k_positive_improvement_share_max"], 1.0)

    def test_criteria_lock_normalizes_operational_paths_but_binds_exact_run_configs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            brain, first, second, binary = self.write_lock_inputs(root)
            runtime_commit = self.criteria["expected_runtime_commit_sha"]
            locked = gate.lock_criteria_template(
                criteria_template=self.criteria_template(),
                runtime_commit_sha=runtime_commit,
                release_binary=binary,
                brain_config=brain,
                run_configs={"validation-v1a": first, "validation-v1b": second},
                repo_root=ROOT,
            )
            self.assertEqual(locked["contract_state"], "locked")
            self.assertEqual(locked["expected_runtime_commit_sha"], runtime_commit)
            self.assertEqual(
                set(locked["allowed_exact_run_config_hashes"]),
                {"validation-v1a", "validation-v1b"},
            )
            self.assertNotEqual(
                locked["allowed_exact_run_config_hashes"]["validation-v1a"],
                locked["allowed_exact_run_config_hashes"]["validation-v1b"],
            )
            self.assertNotEqual(locked["expected_normalized_behavioral_config_hash"], "unlocked")

            second.write_text(
                second.read_text(encoding="utf-8").replace("max_probes_per_run = 100", "max_probes_per_run = 101"),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(gate.ContractError, "normalized behavioural contract"):
                gate.lock_criteria_template(
                    criteria_template=self.criteria_template(),
                    runtime_commit_sha=runtime_commit,
                    release_binary=binary,
                    brain_config=brain,
                    run_configs={"validation-v1a": first, "validation-v1b": second},
                    repo_root=ROOT,
                )

    def test_criteria_lock_rejects_nonexistent_runtime_commit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            brain, first, second, binary = self.write_lock_inputs(root)
            with self.assertRaisesRegex(gate.ContractError, "does not resolve to a commit"):
                gate.lock_criteria_template(
                    criteria_template=self.criteria_template(),
                    runtime_commit_sha="f" * 40,
                    release_binary=binary,
                    brain_config=brain,
                    run_configs={"validation-v1a": first, "validation-v1b": second},
                    repo_root=ROOT,
                )

    def test_criteria_lock_canonicalizes_short_commit_sha(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            brain, first, second, binary = self.write_lock_inputs(root)
            runtime_commit = self.criteria["expected_runtime_commit_sha"]
            locked = gate.lock_criteria_template(
                criteria_template=self.criteria_template(),
                runtime_commit_sha=runtime_commit[:12],
                release_binary=binary,
                brain_config=brain,
                run_configs={"validation-v1a": first, "validation-v1b": second},
                repo_root=ROOT,
            )
            self.assertEqual(locked["expected_runtime_commit_sha"], runtime_commit)

    def test_locked_runtime_commit_must_be_ancestor_of_pr_head(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            repo = root / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", str(repo)], check=True, capture_output=True, text=True)
            self.git(repo, "-c", "user.name=Ghost", "-c", "user.email=ghost@example.invalid", "commit", "--allow-empty", "-m", "main")
            non_head_commit = self.git(repo, "rev-parse", "HEAD")
            self.git(repo, "checkout", "--orphan", "other")
            self.git(repo, "-c", "user.name=Ghost", "-c", "user.email=ghost@example.invalid", "commit", "--allow-empty", "-m", "other")
            brain, first, second, binary = self.write_lock_inputs(root)

            with self.assertRaisesRegex(gate.ContractError, "ancestor of the current PR head"):
                gate.lock_criteria_template(
                    criteria_template=self.criteria_template(),
                    runtime_commit_sha=non_head_commit,
                    release_binary=binary,
                    brain_config=brain,
                    run_configs={"validation-v1a": first, "validation-v1b": second},
                    repo_root=repo,
                )

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
        observed["economic_result"]["gate_specific_economics"]["executable_trailing"][
            "candidate_positions"
        ] = 0
        observed["economic_result"]["gate_specific_economics"]["executable_trailing"][
            "matched_positions"
        ] = 0
        gates = self.evaluate(observed)
        self.assertFalse(gates["economic_result"]["checks"]["executable_trailing_candidate_positions"])
        self.assertFalse(gates["economic_result"]["checks"]["executable_trailing_matched_positions"])
        self.assertFalse(self.gate_eligibility(gates)["executable_trailing"]["promotion_gate_passed"])

    def test_crash_candidates_cannot_satisfy_trailing_sample_minimum(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["matched_v2_candidate_positions"] = 50
        observed["economic_result"]["executable_trailing_candidate_positions"] = 0
        observed["economic_result"]["executable_trailing_matched_positions"] = 0
        observed["economic_result"]["gate_specific_economics"]["executable_trailing"][
            "candidate_positions"
        ] = 0
        observed["economic_result"]["gate_specific_economics"]["executable_trailing"][
            "matched_positions"
        ] = 0
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

    def test_same_tick_unpromoted_crash_does_not_block_promoted_trailing(self) -> None:
        key = self.position_key()
        observed, matched = gate.economic_observations(
            {key: self.opened_position(key)},
            {
                key: [
                    self.schema_v3_lattice_row(
                        key=key,
                        timestamp_ms=1_000,
                        exits={"crash": -500, "executable_trailing": 250},
                    ),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=2_000,
                        current_executable_bps=350,
                    ),
                ]
            },
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual(observed["matched_v2_candidate_positions"], 1)
        self.assertEqual([row["reason"] for row in matched], ["ExecutableTrailing"])
        self.assertEqual(observed["gate_specific_economics"]["crash"]["candidate_positions"], 1)
        self.assertEqual(
            observed["gate_specific_economics"]["executable_trailing"]["matched_positions"],
            1,
        )

    def test_one_position_with_trailing_and_vitality_counts_once_globally(self) -> None:
        key = self.position_key()
        observed, matched = gate.economic_observations(
            {key: self.opened_position(key)},
            {
                key: [
                    self.comparison_row(
                        key=key,
                        timestamp_ms=1_000,
                        reason="VitalityDecay",
                        return_bps=50,
                    ),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=2_000,
                        reason="ExecutableTrailing",
                        return_bps=250,
                    ),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=3_000,
                        current_executable_bps=300,
                    ),
                ]
            },
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual(observed["matched_v2_candidate_positions"], 1)
        self.assertEqual(len(matched), 1)
        self.assertEqual(matched[0]["reason"], "VitalityDecay")
        self.assertEqual(
            observed["gate_specific_economics"]["vitality_decay"]["matched_positions"],
            1,
        )
        self.assertEqual(
            observed["gate_specific_economics"]["executable_trailing"]["matched_positions"],
            1,
        )

    def test_same_tick_trailing_and_vitality_follow_hierarchy(self) -> None:
        key = self.position_key()
        _, matched = gate.economic_observations(
            {key: self.opened_position(key)},
            {
                key: [
                    self.comparison_row(
                        key=key,
                        timestamp_ms=1_000,
                        reason="VitalityDecay",
                        return_bps=50,
                    ),
                    self.comparison_row(
                        key=key,
                        timestamp_ms=1_000,
                        reason="ExecutableTrailing",
                        return_bps=250,
                    ),
                ]
            },
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual([row["reason"] for row in matched], ["ExecutableTrailing"])

    def test_good_trailing_cannot_rescue_bad_vitality(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["gate_specific_economics"]["vitality_decay"][
            "cvar_20_delta_bps"
        ] = -2000
        gates = self.evaluate(observed)
        self.assertTrue(gates["economic_result"]["passed"])
        eligibility = self.gate_eligibility(gates)
        self.assertTrue(eligibility["executable_trailing"]["promotion_gate_passed"])
        self.assertFalse(eligibility["vitality_decay"]["promotion_gate_passed"])
        gate.validate_promotion_artifact(self.artifact(gates, False), self.criteria)

    def test_good_vitality_cannot_rescue_bad_trailing(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["gate_specific_economics"]["executable_trailing"][
            "worst_cost_scenario_mean_delta_bps"
        ] = -2000
        gates = self.evaluate(observed)
        self.assertTrue(gates["economic_result"]["passed"])
        eligibility = self.gate_eligibility(gates)
        self.assertFalse(eligibility["executable_trailing"]["promotion_gate_passed"])
        self.assertTrue(eligibility["vitality_decay"]["promotion_gate_passed"])
        gate.validate_promotion_artifact(self.artifact(gates, False), self.criteria)

    def test_root_cannot_pass_when_one_requested_gate_fails_economics(self) -> None:
        observed = copy.deepcopy(self.observed)
        observed["economic_result"]["gate_specific_economics"]["vitality_decay"][
            "tail_loss_p10_delta_bps"
        ] = -2000
        gates = self.evaluate(observed)
        with self.assertRaisesRegex(gate.ContractError, "manual root boolean"):
            gate.validate_promotion_artifact(self.artifact(gates, True), self.criteria)

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

    def test_unsupported_route_is_not_available_after_candidate(self) -> None:
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
                        current_executable_bps=120,
                        route_status="curve_complete_pump_swap_unsupported",
                    ),
                ]
            },
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual(observed["candidate_executable_continuation_coverage"], 1.0)
        self.assertEqual(observed["route_availability_after_candidate"], 0.0)

    def test_promoted_candidate_missing_economic_field_is_join_failure(self) -> None:
        key = self.position_key()
        row = self.comparison_row(
            key=key,
            timestamp_ms=1_000,
            reason="ExecutableTrailing",
            return_bps=100,
        )
        row.pop("entry_value_quote_raw")
        observed, matched = gate.economic_observations(
            {key: self.opened_position(key)},
            {key: [row]},
            {key: self.terminal()},
            {key: self.replay()},
            {},
            self.criteria,
        )
        self.assertEqual(matched, [])
        self.assertEqual(observed["promoted_candidate_economic_join_failure_count"], 1)
        self.assertEqual(
            observed["gate_specific_economics"]["executable_trailing"][
                "economic_join_failure_count"
            ],
            1,
        )

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
        reconciled = gate.reconcile_admission_with_opened_positions(
            [], opened, empty_summary, {}, 1_000
        )
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
                "timestamp_ms": 1_000,
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
            {key: [self.comparison_row(key=key, timestamp_ms=2_000)]},
            1_000,
        )
        self.assertEqual(complete["admission_missing_final_count"], 0)
        self.assertEqual(complete["admission_missing_monitoring_registered_count"], 0)
        self.assertEqual(complete["admission_missing_release_count"], 0)

    def test_admission_reconciliation_is_bidirectional_and_has_first_het_sla(self) -> None:
        key = self.position_key()
        opened = {key: self.opened_position(key)}
        registered_only = [
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
                "stage": "monitoring_registered",
                "position_id": key[1],
                "position_epoch": key[2],
                "timestamp_ms": 1_000,
            },
            {
                "run_id": key[0],
                "candidate_id": "candidate-orphan",
                "pool_id": "pool-orphan",
                "base_mint": "mint-orphan",
                "lane": "shadow",
                "stage": "post_buy_submitted",
            },
            {
                "run_id": key[0],
                "candidate_id": "candidate-orphan",
                "pool_id": "pool-orphan",
                "base_mint": "mint-orphan",
                "lane": "shadow",
                "stage": "monitoring_registered",
                "position_id": "pool-orphan:mint-orphan:shadow",
                "position_epoch": 99,
                "timestamp_ms": 1_000,
            }
        ]
        reconciled = gate.reconcile_admission_with_opened_positions(
            registered_only,
            opened,
            gate.summarize_admission(registered_only),
            {key: [self.comparison_row(key=key, timestamp_ms=10_000)]},
            1_000,
        )
        self.assertEqual(
            reconciled["monitoring_registered_without_position_open_count"],
            1,
        )
        self.assertEqual(reconciled["registered_without_het_within_2_ticks_count"], 1)


if __name__ == "__main__":
    unittest.main()
