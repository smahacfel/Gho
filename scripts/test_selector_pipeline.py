#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import build_selector_accepted_lifecycle as accepted
import build_selector_candidate_universe as universe
import build_selector_canonical_r2_source as canonical_r2
import build_selector_dataset as dataset
import build_selector_feature_snapshots as snapshots
import build_selector_gatekeeper_feature_context as gk_context
import build_selector_phase1_report as phase1_report
import build_selector_phase2 as phase2
import build_selector_phase3_r2only as phase3_r2only
import build_selector_r2_market_paths as r2_paths
import build_selector_r2only_baseline_report as r2only_baseline
import build_selector_r2only_ablation_report as r2only_ablation
import build_selector_r2only_feature_contribution as r2only_feature_contribution
import build_selector_r2only_feature_audit as r2only_feature_audit
import build_selector_r2only_model_candidate as r2only_model_candidate
import build_selector_r2only_candidate_selection as r2only_candidate_selection
import build_selector_shadow_score_contract as shadow_score_contract
import build_selector_route_manifest_reuse_projection as route_manifest_reuse
import build_selector_route_evidence_join_report as route_evidence_join
import build_selector_training_view as training
import compare_selector_gatekeepers as compare
import audit_selector_buy_simulation_coverage as simcov_audit
import audit_selector_shadow_score_sidecar as shadow_score_sidecar_audit
import audit_selector_shadow_score_parity as shadow_score_parity_audit
try:
    import build_selector_coverage_breakthrough_projection as coverage_breakthrough
except ModuleNotFoundError:
    coverage_breakthrough = None
import ci_assert_selector_regression_gates as selector_regression_gates
import guard_gatekeeper_decision_feature_surface as gk_surface_guard
import start_selector_lifecycle_run as lifecycle_launcher
import selector_pipeline_common as common
import train_selector_baseline as baseline


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")


def read_jsonl(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


class SelectorPipelineTests(unittest.TestCase):
    def write_gatekeeper_surface_guard_rows(
        self,
        root: Path,
        *,
        source_scope: str = "source-gk-surface-guard-test",
        rows: list[dict] | None = None,
    ) -> Path:
        decision_dir = (
            root
            / "logs"
            / "rollout"
            / source_scope
            / "decisions"
            / source_scope
            / "v2.5"
            / "v25_shadow"
            / "fixture"
        )
        decision_dir.mkdir(parents=True)
        if rows is None:
            rows = []
            for idx in range(5):
                rows.append(
                    {
                        "log_schema_version": 25,
                        "decision_plane": "v25_shadow",
                        "bonding_progress_pct": 40.0 + idx,
                        "curve_data_known": True,
                        "current_market_cap_sol": 50.0 + idx,
                        "price_change_ratio": 1.0 + idx,
                        "observation_duration_ms": 8_000,
                        "curve_wait_ms": 800,
                        "curve_wait_elapsed_ms": 8_000,
                        "total_tx_evaluated": 10 + idx,
                        "unique_signers_evaluated": 4 + idx,
                        "buy_count": 3 + idx,
                        "buy_ratio": 0.5,
                        "sell_buy_ratio": 0.2,
                        "hhi": 0.1 + idx,
                        "top3_volume_pct": 0.2 + idx,
                    }
                )
        path = decision_dir / "gatekeeper_v2_decisions.jsonl"
        write_jsonl(path, rows)
        return path

    def test_gatekeeper_decision_feature_surface_guard_passes_required_metrics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_scope = "source-gk-surface-pass"
            self.write_gatekeeper_surface_guard_rows(root, source_scope=source_scope)

            report = gk_surface_guard.build_guard(
                gk_surface_guard.build_parser().parse_args(
                    [
                        "--source-scope",
                        source_scope,
                        "--root",
                        str(root),
                        "--decision-plane",
                        "v25_shadow",
                        "--min-rows",
                        "5",
                    ]
                )
            )
            output_exists = Path(report["output"]).exists()

        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["decision_rows"], 5)
        self.assertEqual(report["field_presence"]["bonding_progress_pct"]["present_rate"], 1.0)
        self.assertEqual(report["field_presence"]["current_market_cap_sol"]["present_rate"], 1.0)
        self.assertEqual(report["field_presence"]["hhi"]["present_rate"], 1.0)
        self.assertFalse(report["claim_boundaries"]["runtime_changed"])
        self.assertTrue(output_exists)

    def test_gatekeeper_decision_feature_surface_guard_fails_without_curve_metrics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_scope = "source-gk-surface-missing"
            self.write_gatekeeper_surface_guard_rows(
                root,
                source_scope=source_scope,
                rows=[
                    {
                        "log_schema_version": 25,
                        "decision_plane": "v25_shadow",
                        "observation_duration_ms": 8_000,
                        "hhi": 0.1,
                        "top3_volume_pct": 0.2,
                    }
                ],
            )

            report = gk_surface_guard.build_guard(
                gk_surface_guard.build_parser().parse_args(
                    [
                        "--source-scope",
                        source_scope,
                        "--root",
                        str(root),
                        "--decision-plane",
                        "v25_shadow",
                        "--min-rows",
                        "1",
                    ]
                )
            )

        self.assertEqual(report["status"], "FAIL_NO_REQUIRED_CURVE_METRICS")
        self.assertIn("missing_required_curve_metrics", report["fail_reasons"][0])

    def test_gatekeeper_decision_feature_surface_guard_uses_lower_concentration_threshold(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_scope = "source-gk-surface-concentration"
            rows = []
            for idx in range(10):
                row = {
                    "log_schema_version": 25,
                    "decision_plane": "v25_shadow",
                    "bonding_progress_pct": 40.0 + idx,
                    "curve_data_known": True,
                    "current_market_cap_sol": 50.0 + idx,
                    "price_change_ratio": 1.0 + idx,
                    "observation_duration_ms": 8_000,
                    "curve_wait_ms": 800,
                    "curve_wait_elapsed_ms": 8_000,
                    "total_tx_evaluated": 10 + idx,
                    "unique_signers_evaluated": 4 + idx,
                    "buy_count": 3 + idx,
                }
                if idx < 6:
                    row["hhi"] = 0.1 + idx
                    row["top3_volume_pct"] = 0.2 + idx
                rows.append(row)
            self.write_gatekeeper_surface_guard_rows(root, source_scope=source_scope, rows=rows)

            pass_report = gk_surface_guard.build_guard(
                gk_surface_guard.build_parser().parse_args(
                    [
                        "--source-scope",
                        source_scope,
                        "--root",
                        str(root),
                        "--decision-plane",
                        "v25_shadow",
                        "--min-rows",
                        "10",
                    ]
                )
            )
            fail_report = gk_surface_guard.build_guard(
                gk_surface_guard.build_parser().parse_args(
                    [
                        "--source-scope",
                        source_scope,
                        "--root",
                        str(root),
                        "--decision-plane",
                        "v25_shadow",
                        "--min-rows",
                        "10",
                        "--concentration-metric-min-present-rate",
                        "0.80",
                    ]
                )
            )

        self.assertEqual(pass_report["status"], "PASS")
        self.assertEqual(pass_report["field_presence"]["hhi"]["present_rate"], 0.6)
        self.assertEqual(fail_report["status"], "FAIL_LOW_CONCENTRATION_COVERAGE")

    def write_gatekeeper_context_fixture(
        self,
        root: Path,
        *,
        scope: str = "selector-gk-context-test",
        source_scope: str = "source-gk-context-test",
        observation_duration_ms: int = 8_000,
        candidate_decision_ts_ms: int | None = 9_000,
    ) -> tuple[Path, Path]:
        dataset_dir = root / "datasets" / "selector" / scope
        decision_dir = (
            root
            / "logs"
            / "rollout"
            / source_scope
            / "decisions"
            / source_scope
            / "v2.5"
            / "v25_shadow"
            / "fixture"
        )
        dataset_dir.mkdir(parents=True)
        decision_dir.mkdir(parents=True)
        candidate = {
            "candidate_id": "candidate",
            "candidate_universe_status": "ok",
            "cohort_in_scope": True,
            "stream_completeness_ok": True,
            "base_mint": "mint",
            "mint_id": "mint",
            "pool_id": "pool",
            "bonding_curve": "pool",
            "quote_mint": "SOL",
            "birth_ts_ms": 1_000,
        }
        if candidate_decision_ts_ms is not None:
            candidate["decision_ts_ms"] = candidate_decision_ts_ms
        write_jsonl(dataset_dir / "candidate_universe_v1.jsonl", [candidate])
        write_jsonl(
            decision_dir / "gatekeeper_v2_decisions.jsonl",
            [
                {
                    "log_schema_version": 25,
                    "pool_id": "pool",
                    "join_key": "pool:mint:1000",
                    "base_mint": "mint",
                    "first_seen_ts_ms": 1_000,
                    "observation_start_ts_ms": 1_000,
                    "observation_end_ts_ms": 1_000 + observation_duration_ms,
                    "observation_window_ms": observation_duration_ms,
                    "observation_duration_ms": observation_duration_ms,
                    "decision_plane": "v25_shadow",
                    "bonding_progress_pct": 46.0,
                    "curve_data_known": True,
                    "current_market_cap_sol": 48.7,
                    "price_change_ratio": 1.6,
                    "hhi": 0.04,
                    "top3_volume_pct": 0.27,
                    "funding_source_diagnostics": {
                        "buyer_sample_count": 10,
                        "known_source_count": 3,
                        "unknown_buyer_count": 2,
                    },
                    "vectors_prices": [1.0, 1.5, 1.25],
                    "vectors_sol_amounts": [0.1, 0.2, 0.3],
                    "vectors_ts_offsets_ms": [0, 500, 1_000],
                    "decision_verdict_buy": True,
                    "verdict_type": "BUY",
                    "decision_reason": "BUY fixture",
                }
            ],
        )
        return dataset_dir / "candidate_universe_v1.jsonl", decision_dir / "gatekeeper_v2_decisions.jsonl"

    def build_gatekeeper_context_fixture(
        self,
        root: Path,
        *,
        scope: str = "selector-gk-context-test",
        source_scope: str = "source-gk-context-test",
        observation_profile: str = "observation_8s_10s",
        observation_duration_ms: int = 8_000,
        candidate_decision_ts_ms: int | None = 9_000,
    ) -> dict:
        self.write_gatekeeper_context_fixture(
            root,
            scope=scope,
            source_scope=source_scope,
            observation_duration_ms=observation_duration_ms,
            candidate_decision_ts_ms=candidate_decision_ts_ms,
        )
        return gk_context.run(
            gk_context.build_parser().parse_args(
                [
                    "--root",
                    str(root),
                    "--scope",
                    scope,
                    "--source-scope",
                    source_scope,
                    "--decision-plane",
                    "v25_shadow",
                    "--observation-profile",
                    observation_profile,
                ]
            )
        )

    def test_gatekeeper_feature_context_extracts_allowed_metrics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary = self.build_gatekeeper_context_fixture(root)
            rows = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))

        self.assertEqual(summary["manifest"]["status"], "PASS")
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["gk_bonding_progress_pct"], 46.0)
        self.assertTrue(rows[0]["gk_curve_data_known"])
        self.assertEqual(rows[0]["gk_current_market_cap_sol"], 48.7)
        self.assertEqual(rows[0]["gk_price_change_ratio"], 1.6)
        self.assertEqual(rows[0]["gk_hhi"], 0.04)
        self.assertEqual(rows[0]["gk_top3_volume_pct"], 0.27)
        self.assertEqual(rows[0]["gk_fsc_known_source_rate"], 0.3)
        self.assertEqual(rows[0]["gk_vector_event_count"], 3)

    def test_gatekeeper_feature_context_rejects_forbidden_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary = self.build_gatekeeper_context_fixture(root)
            row = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))[0]

        self.assertEqual(summary["manifest"]["forbidden_fields_detected"], [])
        self.assertNotIn("decision_verdict_buy", row)
        self.assertNotIn("verdict_type", row)
        self.assertNotIn("decision_reason", row)
        self.assertNotIn("gk_decision_verdict_buy", row)

    def test_gatekeeper_feature_context_warns_on_degraded_concentration_surface(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-gk-context-concentration-warning"
            source_scope = "source-gk-context-concentration-warning"
            dataset_dir = root / "datasets" / "selector" / scope
            decision_dir = (
                root
                / "logs"
                / "rollout"
                / source_scope
                / "decisions"
                / source_scope
                / "v2.5"
                / "v25_shadow"
                / "fixture"
            )
            dataset_dir.mkdir(parents=True)
            decision_dir.mkdir(parents=True)
            candidates = []
            decisions = []
            for idx in range(10):
                candidates.append(
                    {
                        "candidate_id": f"candidate-{idx}",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": f"mint-{idx}",
                        "mint_id": f"mint-{idx}",
                        "pool_id": f"pool-{idx}",
                        "bonding_curve": f"pool-{idx}",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000 + idx,
                        "decision_ts_ms": 9_000 + idx,
                    }
                )
                decision = {
                    "log_schema_version": 25,
                    "pool_id": f"pool-{idx}",
                    "join_key": f"pool-{idx}:mint-{idx}:1000",
                    "base_mint": f"mint-{idx}",
                    "first_seen_ts_ms": 1_000 + idx,
                    "observation_start_ts_ms": 1_000 + idx,
                    "observation_end_ts_ms": 9_000 + idx,
                    "observation_window_ms": 8_000,
                    "observation_duration_ms": 8_000,
                    "decision_plane": "v25_shadow",
                    "bonding_progress_pct": 46.0,
                    "curve_data_known": True,
                    "current_market_cap_sol": 48.7,
                    "price_change_ratio": 1.6,
                }
                if idx < 7:
                    decision["hhi"] = 0.04 + idx
                    decision["top3_volume_pct"] = 0.27 + idx
                decisions.append(decision)
            write_jsonl(dataset_dir / "candidate_universe_v1.jsonl", candidates)
            write_jsonl(decision_dir / "gatekeeper_v2_decisions.jsonl", decisions)

            summary = gk_context.run(
                gk_context.build_parser().parse_args(
                    [
                        "--root",
                        str(root),
                        "--scope",
                        scope,
                        "--source-scope",
                        source_scope,
                    ]
                )
            )

        manifest = summary["manifest"]
        self.assertEqual(manifest["status"], "PASS_CORE_WITH_CONCENTRATION_COVERAGE_WARNING")
        self.assertEqual(
            manifest["gatekeeper_feature_context_status"],
            "PASS_CORE_WITH_CONCENTRATION_COVERAGE_WARNING",
        )
        self.assertEqual(manifest["core_feature_surface_status"], "PASS")
        self.assertEqual(manifest["concentration_feature_surface_status"], "DEGRADED")
        self.assertEqual(manifest["model_policy"], "missing_not_zero")
        self.assertFalse(manifest["fail_reasons"])
        self.assertIn("gk_hhi_present_rate_below_80pct", manifest["warning_reasons"])
        self.assertIn("gk_top3_volume_pct_present_rate_below_80pct", manifest["warning_reasons"])

    def test_gatekeeper_feature_context_does_not_create_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-gk-denominator-test"
            source_scope = "source-gk-denominator-test"
            self.write_gatekeeper_context_fixture(root, scope=scope, source_scope=source_scope)
            decision_path = next(
                (
                    root
                    / "logs"
                    / "rollout"
                    / source_scope
                    / "decisions"
                ).glob("**/gatekeeper_v2_decisions.jsonl")
            )
            with decision_path.open("a", encoding="utf-8") as fh:
                fh.write(
                    json.dumps(
                        {
                            "log_schema_version": 25,
                            "pool_id": "unmatched_pool",
                            "base_mint": "unmatched_mint",
                            "first_seen_ts_ms": 1_000,
                            "observation_end_ts_ms": 9_000,
                            "observation_duration_ms": 8_000,
                            "decision_plane": "v25_shadow",
                            "bonding_progress_pct": 99.0,
                            "current_market_cap_sol": 99.0,
                        }
                    )
                    + "\n"
                )

            summary = gk_context.run(
                gk_context.build_parser().parse_args(
                    [
                        "--root",
                        str(root),
                        "--scope",
                        scope,
                        "--source-scope",
                        source_scope,
                    ]
                )
            )
            rows = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))

        self.assertEqual(summary["manifest"]["denominator_created_rows"], 0)
        self.assertEqual(summary["manifest"]["join_method_counts"]["unmatched"], 1)
        self.assertEqual([row["candidate_id"] for row in rows], ["candidate"])

    def test_gatekeeper_feature_context_joins_by_pool_id_base_mint(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary = self.build_gatekeeper_context_fixture(root)
            row = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))[0]

        self.assertEqual(row["join_method"], "pool_id_base_mint")
        self.assertEqual(summary["manifest"]["join_method_counts"]["pool_id_base_mint"], 1)

    def test_gatekeeper_feature_context_classifies_observation_8s_10s(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary = self.build_gatekeeper_context_fixture(root, observation_duration_ms=8_000)
            row = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))[0]

        self.assertEqual(row["gk_observation_profile"], "observation_8s_10s")
        self.assertEqual(summary["manifest"]["observation_profile_counts"]["observation_8s_10s"], 1)

    def test_gatekeeper_feature_context_classifies_observation_60s(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary = self.build_gatekeeper_context_fixture(
                root,
                observation_profile="observation_60s",
                observation_duration_ms=60_000,
                candidate_decision_ts_ms=61_000,
            )
            row = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))[0]

        self.assertEqual(row["gk_observation_profile"], "observation_60s")
        self.assertEqual(summary["manifest"]["observation_profile_counts"]["observation_60s"], 1)

    def test_gatekeeper_feature_context_marks_unverified_cutoff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary = self.build_gatekeeper_context_fixture(root, candidate_decision_ts_ms=None)
            row = read_jsonl(Path(summary["outputs"]["gatekeeper_feature_context_v1"]))[0]

        self.assertEqual(row["gk_cutoff_status"], "unverified")

    def test_training_view_joins_gatekeeper_feature_context(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidate_universe = root / "candidate_universe_v1.jsonl"
            accepted_lifecycle = root / "accepted_lifecycle_v1.jsonl"
            feature_snapshots = root / "feature_snapshots_v1.jsonl"
            r2_paths_file = root / "r2_market_paths_v1.jsonl"
            gatekeeper_context = root / "gatekeeper_feature_context_v1.jsonl"
            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "candidate",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint",
                        "pool_id": "pool",
                        "bonding_curve": "curve",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 9_000,
                    }
                ],
            )
            write_jsonl(accepted_lifecycle, [])
            write_jsonl(
                feature_snapshots,
                [
                    {
                        "candidate_id": "candidate",
                        "snapshot_kind": "decision",
                        "feature_snapshot_status": "ok",
                        "feature_cutoff_ts_ms": 9_000,
                        "feature_cutoff_slot": 1,
                        "feature_source": "unit_test_feature_snapshot",
                        "feature_source_max_ts_ms": 9_000,
                        "feature_observed_lag_ms": 0,
                    }
                ],
            )
            write_jsonl(r2_paths_file, [])
            write_jsonl(
                gatekeeper_context,
                [
                    {
                        "schema_version": "gatekeeper_feature_context_v1",
                        "candidate_id": "candidate",
                        "gk_context_status": "ok",
                        "gk_cutoff_status": "same_decision_time",
                        "gk_observation_profile": "observation_8s_10s",
                        "gk_bonding_progress_pct": 46.0,
                    }
                ],
            )

            rows, coverage, _audit = training.build_training_view(
                candidate_universe=candidate_universe,
                accepted_lifecycle=accepted_lifecycle,
                feature_snapshots=feature_snapshots,
                price_paths=r2_paths_file,
                target_net_pct=40.0,
                stop_net_pct=40.0,
                horizon_ms=60_000,
                snapshot_kind="decision",
                fallback_snapshot_kind="birth+30s",
                gatekeeper_feature_context=gatekeeper_context,
            )

        self.assertEqual(rows[0]["gk_bonding_progress_pct"], 46.0)
        self.assertEqual(rows[0]["gk_cutoff_status"], "same_decision_time")
        self.assertTrue(rows[0]["gatekeeper_feature_context_joined"])
        self.assertTrue(coverage["gatekeeper_feature_context"]["enabled"])
        self.assertEqual(coverage["gatekeeper_feature_context"]["training_rows_joined"], 1)

    def test_phase3_r2only_passes_gatekeeper_feature_context_to_training_view(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-phase3-gk-context-test"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            write_jsonl(
                dataset_dir / "candidate_universe_v1.jsonl",
                [
                    {
                        "candidate_id": "candidate",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint",
                        "pool_id": "pool",
                        "bonding_curve": "curve",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 9_000,
                    }
                ],
            )
            write_jsonl(dataset_dir / "accepted_lifecycle_v1.jsonl", [])
            write_jsonl(
                dataset_dir / "feature_snapshots_v1.jsonl",
                [
                    {
                        "candidate_id": "candidate",
                        "snapshot_kind": "decision",
                        "feature_snapshot_status": "ok",
                        "feature_cutoff_ts_ms": 9_000,
                        "feature_cutoff_slot": 1,
                        "feature_source": "unit_test_feature_snapshot",
                        "feature_source_max_ts_ms": 9_000,
                        "feature_observed_lag_ms": 0,
                    }
                ],
            )
            write_jsonl(
                dataset_dir / "r2_market_paths_v1.jsonl",
                [
                    {
                        "candidate_id": "candidate",
                        "base_mint": "mint",
                        "pool_id": "pool",
                        "bonding_curve": "curve",
                        "path_source": "yellowstone_accountupdate",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [
                            {"offset_ms": 0, "return_pct": 0.0},
                            {"offset_ms": 60_000, "return_pct": -5.0},
                        ],
                    }
                ],
            )
            gatekeeper_context = dataset_dir / "gatekeeper_feature_context_v1.jsonl"
            write_jsonl(
                gatekeeper_context,
                [
                    {
                        "schema_version": "gatekeeper_feature_context_v1",
                        "candidate_id": "candidate",
                        "gk_context_status": "ok",
                        "gk_cutoff_status": "same_decision_time",
                        "gk_observation_profile": "observation_8s_10s",
                        "gk_bonding_progress_pct": 46.0,
                    }
                ],
            )
            (report_dir / "dataset_manifest_v1.json").write_text(
                json.dumps(
                    {
                        "denominator_source": "event_artifact_only",
                        "phase2_status": "P2C_PASS_LABEL_COVERAGE_R2_ONLY",
                        "r2_resolved_denominator_built": True,
                        "selector_training_view_built": False,
                        "baseline_built": False,
                        "gatekeeper_compare_built": False,
                        "outputs": {
                            "candidate_universe_v1": {
                                "path": str(dataset_dir / "candidate_universe_v1.jsonl")
                            },
                            "accepted_lifecycle_v1": {
                                "path": str(dataset_dir / "accepted_lifecycle_v1.jsonl")
                            },
                            "feature_snapshots_v1": {
                                "path": str(dataset_dir / "feature_snapshots_v1.jsonl")
                            },
                            "r2_market_paths_v1": {
                                "path": str(dataset_dir / "r2_market_paths_v1.jsonl")
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )

            manifest = phase3_r2only.run(
                phase3_r2only.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--gatekeeper-feature-context",
                        str(gatekeeper_context),
                        "--min-resolved-rows",
                        "1",
                    ]
                )
            )
            training_rows = read_jsonl(dataset_dir / "selector_training_view_v1.jsonl")
            training_manifest = json.loads(
                (report_dir / "selector_training_view_manifest_v1.json").read_text(
                    encoding="utf-8"
                )
            )

        self.assertEqual(manifest["status"], "PASS_R2_ONLY_DRAFT")
        self.assertTrue(manifest["gatekeeper_feature_context_enabled"])
        self.assertTrue(training_manifest["gatekeeper_feature_context_enabled"])
        self.assertIn("gatekeeper_feature_context_v1", manifest["input_provenance"])
        self.assertIn("gatekeeper_feature_context_v1", training_manifest["input_provenance"])
        self.assertEqual(training_rows[0]["gk_bonding_progress_pct"], 46.0)
        self.assertFalse(manifest["claim_boundaries"]["gatekeeper_tuning_started"])
        self.assertFalse(manifest["claim_boundaries"]["production_promotion_claim"])

    def test_candidate_universe_dedupes_and_fails_closed_on_missing_quote(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            decisions = root / "decisions.jsonl"
            output = root / "candidate_universe_v1.jsonl"
            manifest = root / "manifest.json"
            write_jsonl(
                events,
                [
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    },
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    },
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "c2",
                        "base_mint": "mint2",
                        "pool_id": "pool2",
                        "bonding_curve": "curve2",
                        "birth_ts_ms": 2_000,
                    },
                ],
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "c1",
                        "decision_verdict_buy": False,
                        "verdict_type": "REJECT",
                    }
                ],
            )

            summary = universe.run(
                universe.build_parser().parse_args(
                    [
                        "--events",
                        str(events),
                        "--decisions",
                        str(decisions),
                        "--output",
                        str(output),
                        "--manifest-output",
                        str(manifest),
                    ]
                )
            )
            rows = read_jsonl(output)

        self.assertEqual(summary["duplicates"], 2)
        self.assertEqual(summary["status"], "NO-GO")
        self.assertEqual(summary["denominator_source"], "event_artifact_only")
        self.assertEqual(summary["event_denominator_rows_after_dedupe"], 2)
        self.assertEqual(summary["decision_logs_created_denominator_rows"], 0)
        self.assertEqual(summary["candidate_ids_from_decision_only"], 0)
        self.assertEqual(summary["denominator_invariant_status"], "PASS")
        self.assertEqual(summary["identity_collisions"], [])
        self.assertEqual(len(rows), 2)
        by_id = {row["candidate_id"]: row for row in rows}
        self.assertEqual(by_id["c1"]["candidate_universe_status"], "ok")
        self.assertFalse(by_id["c1"]["decision_verdict_buy"])
        self.assertEqual(by_id["c2"]["candidate_universe_status"], "universe_incomplete")
        self.assertIn("quote_mint", by_id["c2"]["candidate_identity_missing_fields"])

    def test_candidate_universe_window_filters_birth_events_not_decision_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            decisions = root / "decisions.jsonl"
            output = root / "candidate_universe_v1.jsonl"
            write_jsonl(
                events,
                [
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "before",
                        "base_mint": "mint_before",
                        "pool_id": "pool_before",
                        "bonding_curve": "curve_before",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 900,
                    },
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "inside",
                        "base_mint": "mint_inside",
                        "pool_id": "pool_inside",
                        "bonding_curve": "curve_inside",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_500,
                    },
                ],
            )
            write_jsonl(
                decisions,
                [
                    {
                        "base_mint": "mint_inside",
                        "pool_id": "pool_inside",
                        "bonding_curve": "curve_inside",
                        "decision_ts_ms": 1_600,
                        "decision_verdict_buy": True,
                    }
                ],
            )
            summary = universe.run(
                universe.build_parser().parse_args(
                    [
                        "--events",
                        str(events),
                        "--decisions",
                        str(decisions),
                        "--output",
                        str(output),
                        "--window-start-ms",
                        "1000",
                        "--window-end-ms",
                        "2000",
                    ]
                )
            )
            rows = read_jsonl(output)

        self.assertEqual(summary["status"], "ok")
        self.assertEqual(summary["scope_kind"], "windowed")
        self.assertEqual(summary["event_load"]["skipped_counts"]["before_window"], 1)
        self.assertEqual(summary["decision_logs_created_denominator_rows"], 0)
        self.assertEqual(summary["event_denominator_rows_after_dedupe"], 1)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["candidate_id"], "inside")
        self.assertTrue(rows[0]["decision_verdict_buy"])

    def test_phase1_report_join_coverage_keeps_r2_disabled(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidate_universe = root / "candidate_universe_v1.jsonl"
            accepted_lifecycle = root / "accepted_lifecycle_v1.jsonl"
            candidate_manifest = root / "candidate_universe_manifest_v1.json"
            accepted_manifest = root / "accepted_lifecycle_manifest_v1.json"
            phase1_join = root / "phase1_join_coverage_v1.json"
            label_coverage = root / "label_coverage_v1.json"
            dataset_manifest = root / "dataset_manifest_v1.json"
            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(
                accepted_lifecycle,
                [
                    {
                        "candidate_id": "c1",
                        "lifecycle_status": "resolved",
                        "label_resolved": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                    }
                ],
            )
            candidate_manifest.write_text(
                json.dumps(
                    {
                        "status": "ok",
                        "identity_collisions": [],
                        "decision_logs_created_denominator_rows": 0,
                    }
                ),
                encoding="utf-8",
            )
            accepted_manifest.write_text(json.dumps({"status": "ok"}), encoding="utf-8")

            manifest = phase1_report.run(
                phase1_report.build_parser().parse_args(
                    [
                        "--scope",
                        "selector-phase1-test",
                        "--source-scope",
                        "source-test",
                        "--root",
                        str(root),
                        "--candidate-universe",
                        str(candidate_universe),
                        "--accepted-lifecycle",
                        str(accepted_lifecycle),
                        "--candidate-manifest",
                        str(candidate_manifest),
                        "--accepted-manifest",
                        str(accepted_manifest),
                        "--lifecycle-report",
                        str(accepted_lifecycle),
                        "--window-start-ms",
                        "1000",
                        "--window-end-ms",
                        "2000",
                        "--window-reason",
                        "unit_test_window",
                        "--excluded-window-reason",
                        "unit_test_exclusion",
                        "--phase1-join-output",
                        str(phase1_join),
                        "--label-coverage-output",
                        str(label_coverage),
                        "--dataset-manifest-output",
                        str(dataset_manifest),
                    ]
                )
            )
            coverage = json.loads(phase1_join.read_text(encoding="utf-8"))
            label_payload = json.loads(label_coverage.read_text(encoding="utf-8"))

        self.assertEqual(manifest["phase1_status"], "PASS")
        self.assertEqual(manifest["scope_kind"], "windowed")
        self.assertEqual(manifest["window_start_ts_ms"], 1000)
        self.assertEqual(manifest["window_end_ts_ms"], 2000)
        self.assertEqual(manifest["window_reason"], "unit_test_window")
        self.assertEqual(manifest["denominator_source"], "event_artifact_only")
        self.assertFalse(manifest["r2_labels_built"])
        self.assertFalse(manifest["selector_training_view_built"])
        self.assertFalse(manifest["baseline_built"])
        self.assertFalse(manifest["shadow_only_emit"]["enabled"])
        self.assertEqual(coverage["scope_kind"], "windowed")
        self.assertEqual(coverage["accepted_lifecycle_join_completeness"], 1.0)
        self.assertEqual(coverage["accepted_rows_joined"], 1)
        self.assertEqual(label_payload["r2_status"], "not_built_in_phase1")

    def test_accepted_lifecycle_r1_timestop_below_target_is_explicit_negative(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "lifecycle.jsonl"
            output = root / "accepted_lifecycle_v1.jsonl"
            write_jsonl(
                source,
                [
                    {
                        "analysis_status": "ok",
                        "candidate_id": "c1",
                        "position_id": "p1",
                        "mint_id": "mint1",
                        "pool_id": "pool1",
                        "close_reason": "TimeStop",
                        "truth_status": "resolved",
                        "truth_source": "canonical_account_state_snapshot",
                        "timing": {"position_duration_ms": 60_000},
                        "shadow": {"final_pnl_pct": 12.0},
                    }
                ],
            )

            accepted.run(
                accepted.build_parser().parse_args(
                    [
                        "--lifecycle-report",
                        str(source),
                        "--output",
                        str(output),
                        "--pnl-target-net-pct",
                        "40",
                    ]
                )
            )
            row = read_jsonl(output)[0]

        self.assertTrue(row["execution_realized"])
        self.assertEqual(row["lifecycle_status"], "resolved")
        self.assertTrue(row["label_resolved"])
        self.assertEqual(row["r1_label"], "negative")
        self.assertEqual(row["r1_label_reason"], "time_stop_below_target")
        self.assertIsNone(row["r1_gray_reason"])

    def test_accepted_lifecycle_window_filters_by_decision_or_entry_time(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "lifecycle.jsonl"
            output = root / "accepted_lifecycle_v1.jsonl"
            write_jsonl(
                source,
                [
                    {
                        "analysis_status": "ok",
                        "candidate_id": "before",
                        "mint_id": "mint_before",
                        "pool_id": "pool_before",
                        "close_reason": "TimeStop",
                        "truth_status": "resolved",
                        "timing": {"decision_ts_ms": 900},
                        "shadow": {"final_pnl_pct": -1.0},
                    },
                    {
                        "analysis_status": "ok",
                        "candidate_id": "inside",
                        "mint_id": "mint_inside",
                        "pool_id": "pool_inside",
                        "close_reason": "Target",
                        "truth_status": "resolved",
                        "timing": {"entry_execution_ts_ms": 1_500},
                        "shadow": {"final_pnl_pct": 60.0},
                    },
                ],
            )
            summary = accepted.run(
                accepted.build_parser().parse_args(
                    [
                        "--lifecycle-report",
                        str(source),
                        "--output",
                        str(output),
                        "--pnl-target-net-pct",
                        "50",
                        "--window-start-ms",
                        "1000",
                        "--window-end-ms",
                        "2000",
                    ]
                )
            )
            rows = read_jsonl(output)

        self.assertEqual(summary["scope_kind"], "windowed")
        self.assertEqual(summary["rows_read"], 2)
        self.assertEqual(summary["rows_written"], 1)
        self.assertEqual(summary["window_skipped_counts"]["before_window"], 1)
        self.assertEqual(rows[0]["candidate_id"], "inside")
        self.assertEqual(rows[0]["lifecycle_status"], "resolved")

    def test_r1_target_stop_nonpositive_excluded_and_gray_cases(self) -> None:
        base = {"truth_status": "resolved", "analysis_status": "ok"}
        target = dict(base, close_reason="Target", final_pnl_pct=5.0)
        stop = dict(base, close_reason="StopLoss", final_pnl_pct=-10.0)
        non_positive = dict(base, close_reason="TimeStop", final_pnl_pct=0.0)
        gray = dict(base, close_reason="Other", final_pnl_pct=5.0)
        unresolved = {"truth_status": "partial", "close_reason": "Target", "final_pnl_pct": 50.0}

        self.assertEqual(common.classify_r1(target, pnl_target_net_pct=40)["r1_label"], "positive")
        self.assertEqual(common.classify_r1(stop, pnl_target_net_pct=40)["r1_label_reason"], "stop_loss")
        self.assertEqual(
            common.classify_r1(non_positive, pnl_target_net_pct=40)["r1_label_reason"],
            "non_positive_pnl",
        )
        self.assertEqual(common.classify_r1(gray, pnl_target_net_pct=40)["r1_gray_reason"], "positive_below_target")
        self.assertEqual(
            common.classify_r1(unresolved, pnl_target_net_pct=40)["r1_excluded_reason"],
            "truth_status_not_resolved",
        )

    def test_feature_snapshot_has_cutoff_metadata_and_no_outcome_leakage(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            events = root / "events.jsonl"
            output = root / "feature_snapshots_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 20_000,
                    }
                ],
            )
            write_jsonl(
                events,
                [
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 2_000,
                        "slot": 10,
                        "side": "buy",
                        "signer": "buyer1",
                        "quote_amount_sol": 2.0,
                        "bonding_curve_progress": 0.10,
                        "final_pnl_pct": 999.0,
                    },
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 3_000,
                        "slot": 11,
                        "side": "sell",
                        "signer": "seller1",
                        "quote_amount_sol": 1.0,
                    },
                ],
            )

            snapshots.run(
                snapshots.build_parser().parse_args(
                    [
                        "--candidate-universe",
                        str(candidates),
                        "--events",
                        str(events),
                        "--output",
                        str(output),
                        "--snapshot-kind",
                        "birth+5s",
                    ]
                )
            )
            row = read_jsonl(output)[0]

        self.assertEqual(row["snapshot_kind"], "birth+5s")
        self.assertEqual(row["feature_cutoff_ts_ms"], 6_000)
        self.assertEqual(row["feature_cutoff_slot"], 11)
        self.assertEqual(row["feature_snapshot_status"], "ok")
        self.assertEqual(row["feature_source_max_ts_ms"], 3_000)
        self.assertNotIn("final_pnl_pct", row)
        self.assertEqual(row["unique_buyers"], 1)
        self.assertAlmostEqual(row["sell_share"], 0.5)

    def test_training_view_keeps_unmatured_horizon_out_of_r2_negative(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            lifecycle = root / "accepted_lifecycle_v1.jsonl"
            features = root / "feature_snapshots_v1.jsonl"
            paths = root / "price_paths.jsonl"
            output = root / "selector_training_view_v1.jsonl"
            coverage = root / "coverage.json"
            audit = root / "audit.json"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 5_000,
                    }
                ],
            )
            write_jsonl(lifecycle, [])
            write_jsonl(
                features,
                [
                    {
                        "candidate_id": "c1",
                        "snapshot_kind": "decision",
                        "feature_cutoff_ts_ms": 5_000,
                        "feature_cutoff_slot": 42,
                        "feature_source": "selector_offline_event_rollup",
                        "feature_observed_lag_ms": 0,
                        "feature_source_max_ts_ms": 5_000,
                        "feature_snapshot_status": "ok",
                        "feature_time_provenance_ok": True,
                    }
                ],
            )
            write_jsonl(
                paths,
                [
                    {
                        "candidate_id": "c1",
                        "path_source": "DIAG_ACCOUNT_UPDATE_RELAY",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": False,
                        "samples": [{"offset_ms": 1_000, "return_pct": 5.0}],
                    }
                ],
            )

            training.run(
                training.build_parser().parse_args(
                    [
                        "--candidate-universe",
                        str(candidates),
                        "--accepted-lifecycle",
                        str(lifecycle),
                        "--feature-snapshots",
                        str(features),
                        "--price-paths",
                        str(paths),
                        "--output",
                        str(output),
                        "--label-coverage-output",
                        str(coverage),
                        "--leakage-audit-output",
                        str(audit),
                        "--target-net-pct",
                        "40",
                        "--stop-net-pct",
                        "40",
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            row = read_jsonl(output)[0]
            audit_payload = json.loads(audit.read_text(encoding="utf-8"))

        self.assertIsNone(row["r2_label"])
        self.assertEqual(row["r2_status"], "horizon_unmatured")
        self.assertFalse(row["label_resolved"])
        self.assertEqual(audit_payload["status"], "PASS")

    def test_training_view_r2_no_target_is_negative_only_with_matured_coverage(self) -> None:
        path = {
            "path_source": "yellowstone_account_update",
            "path_status": "ok",
            "path_coverage_ok": True,
            "horizon_matured": True,
            "samples": [{"offset_ms": 60_000, "return_pct": 5.0}],
        }
        r2 = common.classify_r2(path, target_net_pct=40.0, stop_net_pct=40.0, horizon_ms=60_000)
        self.assertEqual(r2["r2_label"], "negative")
        self.assertEqual(r2["r2_label_reason"], "no_target_by_horizon")

    def test_rpc_backfill_only_is_not_r2_ssot(self) -> None:
        path = {
            "path_source": "rpc_tx",
            "rpc_backfill": True,
            "path_status": "ok",
            "path_coverage_ok": True,
            "horizon_matured": True,
            "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
        }
        r2 = common.classify_r2(path, target_net_pct=40.0, stop_net_pct=40.0, horizon_ms=60_000)
        self.assertIsNone(r2["r2_label"])
        self.assertEqual(r2["r2_excluded_reason"], "rpc_backfill_only_not_r2_ssot")

    def test_rpc_canonical_mixed_source_is_not_r2_ssot(self) -> None:
        path = {
            "path_source": "rpc_canonical_account_state",
            "path_status": "ok",
            "path_coverage_ok": True,
            "horizon_matured": True,
            "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
        }
        r2 = common.classify_r2(path, target_net_pct=40.0, stop_net_pct=40.0, horizon_ms=60_000)
        self.assertIsNone(r2["r2_label"])
        self.assertEqual(r2["r2_source_provenance"], "rpc_backfill_only")

    def test_r2_target_before_stop_and_stop_before_target(self) -> None:
        target_first = {
            "path_source": "geyser_account_update",
            "path_status": "ok",
            "path_coverage_ok": True,
            "horizon_matured": True,
            "samples": [
                {"offset_ms": 1_000, "return_pct": 45.0},
                {"offset_ms": 2_000, "return_pct": -45.0},
            ],
        }
        stop_first = {
            "path_source": "geyser_account_update",
            "path_status": "ok",
            "path_coverage_ok": True,
            "horizon_matured": True,
            "samples": [
                {"offset_ms": 1_000, "return_pct": -45.0},
                {"offset_ms": 2_000, "return_pct": 45.0},
            ],
        }
        self.assertEqual(
            common.classify_r2(target_first, target_net_pct=40, stop_net_pct=40, horizon_ms=60_000)[
                "r2_label_reason"
            ],
            "target_before_stop",
        )
        self.assertEqual(
            common.classify_r2(stop_first, target_net_pct=40, stop_net_pct=40, horizon_ms=60_000)[
                "r2_label_reason"
            ],
            "stop_before_target",
        )

    def test_feature_snapshot_without_cutoff_events_is_audit_no_go(self) -> None:
        snapshot = common.build_feature_snapshot(
            {
                "candidate_id": "c1",
                "base_mint": "mint1",
                "pool_id": "pool1",
                "bonding_curve": "curve1",
                "quote_mint": "SOL",
                "birth_ts_ms": 1_000,
            },
            [],
            snapshot_kind="birth+5s",
            cutoff_ts_ms=6_000,
        )
        audit = training.leakage_audit([snapshot])
        self.assertEqual(snapshot["feature_snapshot_status"], "feature_snapshot_incomplete")
        self.assertEqual(audit["status"], "NO-GO")

    def test_training_view_excludes_incomplete_feature_snapshot_from_model_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidate_universe = root / "candidate_universe_v1.jsonl"
            accepted_lifecycle = root / "accepted_lifecycle_v1.jsonl"
            feature_snapshots = root / "feature_snapshots_v1.jsonl"
            r2_paths_file = root / "r2_market_paths_v1.jsonl"
            write_jsonl(
                candidate_universe,
                [
                    {
                        "candidate_id": "complete",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint-complete",
                        "pool_id": "pool-complete",
                        "bonding_curve": "curve-complete",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 9_000,
                    },
                    {
                        "candidate_id": "incomplete",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint-incomplete",
                        "pool_id": "pool-incomplete",
                        "bonding_curve": "curve-incomplete",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 2_000,
                        "decision_ts_ms": 10_000,
                    },
                ],
            )
            write_jsonl(accepted_lifecycle, [])
            write_jsonl(
                feature_snapshots,
                [
                    {
                        "candidate_id": "complete",
                        "snapshot_kind": "decision",
                        "feature_snapshot_status": "ok",
                        "feature_cutoff_ts_ms": 9_000,
                        "feature_cutoff_slot": 1,
                        "feature_source": "unit_test_feature_snapshot",
                        "feature_source_max_ts_ms": 9_000,
                        "feature_observed_lag_ms": 0,
                    },
                    {
                        "candidate_id": "incomplete",
                        "snapshot_kind": "decision",
                        "feature_snapshot_status": "feature_snapshot_incomplete",
                        "feature_snapshot_incomplete_reason": "missing_cutoff_events",
                        "feature_cutoff_ts_ms": None,
                        "feature_cutoff_slot": None,
                        "feature_source": "unit_test_feature_snapshot",
                        "feature_source_max_ts_ms": None,
                        "feature_observed_lag_ms": None,
                    },
                ],
            )
            write_jsonl(
                r2_paths_file,
                [
                    {
                        "candidate_id": "complete",
                        "base_mint": "mint-complete",
                        "pool_id": "pool-complete",
                        "bonding_curve": "curve-complete",
                        "path_source": "yellowstone_accountupdate",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [
                            {"offset_ms": 0, "return_pct": 0.0},
                            {"offset_ms": 60_000, "return_pct": 45.0},
                        ],
                    },
                    {
                        "candidate_id": "incomplete",
                        "base_mint": "mint-incomplete",
                        "pool_id": "pool-incomplete",
                        "bonding_curve": "curve-incomplete",
                        "path_source": "yellowstone_accountupdate",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [
                            {"offset_ms": 0, "return_pct": 0.0},
                            {"offset_ms": 60_000, "return_pct": -45.0},
                        ],
                    },
                ],
            )

            rows, coverage, audit = training.build_training_view(
                candidate_universe=candidate_universe,
                accepted_lifecycle=accepted_lifecycle,
                feature_snapshots=feature_snapshots,
                price_paths=r2_paths_file,
                target_net_pct=40.0,
                stop_net_pct=40.0,
                horizon_ms=60_000,
                snapshot_kind="decision",
                fallback_snapshot_kind="birth+30s",
                split_denominator="resolved_r2",
            )

        by_id = {row["candidate_id"]: row for row in rows}
        self.assertTrue(by_id["complete"]["r2_only_training_denominator"])
        self.assertEqual(by_id["complete"]["training_row_status"], "model_eligible")
        self.assertFalse(by_id["incomplete"]["r2_only_training_denominator"])
        self.assertFalse(by_id["incomplete"]["model_eligible"])
        self.assertEqual(
            by_id["incomplete"]["training_row_status"],
            "excluded_feature_snapshot_incomplete",
        )
        self.assertEqual(coverage["r2_training_denominator_rows"], 1)
        self.assertEqual(coverage["effective_r2_training_denominator_rows"], 1)
        self.assertEqual(coverage["feature_snapshot_incomplete_excluded_rows"], 1)
        self.assertEqual(coverage["missing_feature_cutoff_excluded_rows"], 1)
        self.assertEqual(audit["status"], "PASS")
        self.assertEqual(audit["rows_excluded_from_model_audit"], 1)
        self.assertEqual(audit["violation_count"], 0)

    def test_feature_snapshot_does_not_use_decision_logs_by_default(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            decisions = root / "decisions.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(
                decisions,
                [
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 2_000,
                        "slot": 5,
                        "side": "buy",
                        "quote_amount_sol": 10.0,
                    }
                ],
            )
            rows, manifest = snapshots.build_feature_snapshots(
                candidate_universe=candidates,
                event_paths=[],
                decision_paths=[decisions],
                snapshot_kinds=["birth+5s"],
            )

        self.assertEqual(manifest["decision_context_rows_loaded"], 0)
        self.assertEqual(rows[0]["source_event_count"], 0)
        self.assertEqual(rows[0]["feature_snapshot_status"], "feature_snapshot_incomplete")

    def test_feature_snapshot_rolls_up_pool_transaction_events(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            events = root / "events.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(
                events,
                [
                    {
                        "kind": {
                            "type": "NewPoolDetected",
                            "payload": {
                                "is_birth_event": True,
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "bonding_curve": "curve1",
                                "quote_mint": "SOL",
                                "birth_ts_ms": 1_000,
                                "slot": 10,
                            },
                        }
                    },
                    {
                        "kind": {
                            "type": "PoolTransaction",
                            "payload": {
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "bonding_curve": "curve1",
                                "quote_mint": "SOL",
                                "timestamp_ms": 2_000,
                                "slot": 11,
                                "side": "buy",
                                "is_buy": True,
                                "success": True,
                                "signer": "buyer-a",
                                "wallet": "buyer-a",
                                "quote_amount_sol": 1.5,
                            },
                        }
                    },
                    {
                        "kind": {
                            "type": "PoolTransaction",
                            "payload": {
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "bonding_curve": "curve1",
                                "quote_mint": "SOL",
                                "timestamp_ms": 3_000,
                                "slot": 12,
                                "side": "sell",
                                "is_buy": False,
                                "success": True,
                                "signer": "seller-b",
                                "wallet": "seller-b",
                                "quote_amount_sol": 0.5,
                            },
                        }
                    },
                    {
                        "kind": {
                            "type": "PoolTransaction",
                            "payload": {
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "bonding_curve": "curve1",
                                "quote_mint": "SOL",
                                "timestamp_ms": 4_000,
                                "slot": 13,
                                "side": "buy",
                                "is_buy": True,
                                "success": False,
                                "signer": "failed-buyer",
                                "wallet": "failed-buyer",
                                "quote_amount_sol": 9.0,
                            },
                        }
                    },
                ],
            )
            rows, manifest = snapshots.build_feature_snapshots(
                candidate_universe=candidates,
                event_paths=[events],
                decision_paths=[],
                snapshot_kinds=["birth+5s"],
            )

        self.assertEqual(manifest["status"], "ok")
        self.assertEqual(len(rows), 1)
        row = rows[0]
        self.assertEqual(row["feature_snapshot_status"], "ok")
        self.assertEqual(row["source_event_count"], 4)
        self.assertEqual(row["tx_event_count"], 2)
        self.assertEqual(row["unique_buyers"], 1)
        self.assertAlmostEqual(row["net_quote_in_15s"], 1.0)
        self.assertAlmostEqual(row["net_quote_in_30s"], 1.0)
        self.assertAlmostEqual(row["trade_rate"], 0.4)
        self.assertAlmostEqual(row["sell_share"], 0.5)
        self.assertAlmostEqual(row["top1_wallet_share"], 0.75)
        self.assertAlmostEqual(row["buyer_hhi"], 1.0)
        self.assertEqual(row["curve_progress_status"], "unavailable_missing_curve_state_source")

    def test_feature_snapshot_decision_missing_cutoff_emits_incomplete_row(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            output = root / "feature_snapshots_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            snapshots.run(
                snapshots.build_parser().parse_args(
                    [
                        "--candidate-universe",
                        str(candidates),
                        "--output",
                        str(output),
                        "--snapshot-kind",
                        "decision",
                    ]
                )
            )
            row = read_jsonl(output)[0]

        self.assertEqual(row["snapshot_kind"], "decision")
        self.assertEqual(row["feature_snapshot_status"], "feature_snapshot_incomplete")
        self.assertEqual(row["feature_snapshot_excluded_reason"], "missing_decision_cutoff")
        self.assertIsNone(row["feature_cutoff_ts_ms"])

    def test_feature_snapshot_decision_context_provenance_is_explicit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            events = root / "events.jsonl"
            output = root / "feature_snapshots_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 2_000,
                        "decision_context_join_key": "mint_pool:mint1:pool1",
                    }
                ],
            )
            write_jsonl(
                events,
                [
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 1_500,
                        "slot": 9,
                    }
                ],
            )
            snapshots.run(
                snapshots.build_parser().parse_args(
                    [
                        "--candidate-universe",
                        str(candidates),
                        "--events",
                        str(events),
                        "--output",
                        str(output),
                        "--snapshot-kind",
                        "decision",
                    ]
                )
            )
            row = read_jsonl(output)[0]

        self.assertEqual(row["feature_snapshot_status"], "ok")
        self.assertTrue(row["decision_context_source"])
        self.assertTrue(row["decision_context_not_denominator"])
        self.assertEqual(row["decision_cutoff_source"], "candidate_universe_decision_ts_ms_context_join")

    def test_candidate_universe_excludes_non_birth_events_and_non_sol_is_out_of_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            output = root / "candidate_universe_v1.jsonl"
            write_jsonl(
                events,
                [
                    {
                        "type": "Trade",
                        "candidate_id": "skip",
                        "base_mint": "mint_skip",
                        "bonding_curve": "curve_skip",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    },
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "non_sol",
                        "base_mint": "mint_non_sol",
                        "bonding_curve": "curve_non_sol",
                        "quote_mint": "USDC",
                        "birth_ts_ms": 2_000,
                    },
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "sol",
                        "base_mint": "mint_sol",
                        "bonding_curve": "curve_sol",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 3_000,
                    },
                ],
            )
            summary = universe.run(
                universe.build_parser().parse_args(
                    ["--events", str(events), "--output", str(output)]
                )
            )
            rows = read_jsonl(output)

        self.assertEqual(summary["event_load"]["skipped_counts"]["non_birth_create_event"], 1)
        self.assertEqual(len(rows), 2)
        by_id = {row["candidate_id"]: row for row in rows}
        self.assertFalse(by_id["non_sol"]["cohort_in_scope"])
        self.assertTrue(by_id["sol"]["cohort_in_scope"])

    def test_candidate_universe_ignores_pool_transaction_denominator_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            output = root / "candidate_universe_v1.jsonl"
            write_jsonl(
                events,
                [
                    {
                        "kind": {
                            "type": "NewPoolDetected",
                            "payload": {
                                "is_birth_event": True,
                                "candidate_id": "birth-candidate",
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "bonding_curve": "curve1",
                                "quote_mint": "SOL",
                                "birth_ts_ms": 1_000,
                            },
                        }
                    },
                    {
                        "kind": {
                            "type": "PoolTransaction",
                            "payload": {
                                "candidate_id": "trade-only",
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "bonding_curve": "curve1",
                                "quote_mint": "SOL",
                                "timestamp_ms": 1_500,
                                "side": "buy",
                                "signer": "wallet1",
                                "quote_amount_sol": 1.0,
                                "success": True,
                            },
                        }
                    },
                ],
            )
            summary = universe.run(
                universe.build_parser().parse_args(
                    ["--events", str(events), "--output", str(output)]
                )
            )
            rows = read_jsonl(output)

        self.assertEqual(summary["status"], "ok")
        self.assertEqual(summary["event_load"]["skipped_counts"]["non_birth_create_event"], 1)
        self.assertEqual(summary["event_denominator_rows_after_dedupe"], 1)
        self.assertEqual(summary["decision_logs_created_denominator_rows"], 0)
        self.assertEqual(summary["candidate_ids_from_decision_only"], 0)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["candidate_id"], "birth-candidate")

    def test_candidate_universe_identity_collision_is_no_go(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            output = root / "candidate_universe_v1.jsonl"
            write_jsonl(
                events,
                [
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    },
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "c1",
                        "base_mint": "mint2",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    },
                ],
            )
            summary = universe.run(
                universe.build_parser().parse_args(["--events", str(events), "--output", str(output)])
            )

        self.assertEqual(summary["status"], "NO-GO")
        self.assertTrue(summary["identity_collisions"])

    def test_candidate_universe_joins_decision_context_by_mint_pool_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            decisions = root / "decisions.jsonl"
            output = root / "candidate_universe_v1.jsonl"
            write_jsonl(
                events,
                [
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "mint1:pool1:1000",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "pool1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(
                decisions,
                [
                    {
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "decision_ts_ms": 11_000,
                        "decision_verdict_buy": True,
                        "verdict_type": "BUY",
                        "v25_shadow_confidence": 0.7,
                        "v3_shadow_verdict": "BUY_CANDIDATE",
                        "v3_shadow_confidence": 0.8,
                    }
                ],
            )
            summary = universe.run(
                universe.build_parser().parse_args(
                    ["--events", str(events), "--decisions", str(decisions), "--output", str(output)]
                )
            )
            rows = read_jsonl(output)

        self.assertEqual(summary["status"], "ok")
        self.assertEqual(summary["decision_context_rows_joined"], 1)
        self.assertEqual(summary["decision_only_rows_skipped"], 0)
        self.assertEqual(rows[0]["candidate_id"], "mint1:pool1:1000")
        self.assertTrue(rows[0]["decision_verdict_buy"])
        self.assertEqual(rows[0]["gatekeeper_v3_verdict"], "BUY_CANDIDATE")
        self.assertEqual(rows[0]["gatekeeper_v25_score"], 0.7)

    def test_training_view_reports_accepted_join_completeness_no_go(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            lifecycle = root / "accepted_lifecycle_v1.jsonl"
            features = root / "feature_snapshots_v1.jsonl"
            paths = root / "price_paths.jsonl"
            output = root / "selector_training_view_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(lifecycle, [{"candidate_id": "missing", "truth_status": "resolved"}])
            write_jsonl(
                features,
                [
                    {
                        "candidate_id": "c1",
                        "snapshot_kind": "decision",
                        "feature_cutoff_ts_ms": 5_000,
                        "feature_cutoff_slot": 9,
                        "feature_source": "selector_offline_event_rollup",
                        "feature_observed_lag_ms": 0,
                        "feature_source_max_ts_ms": 5_000,
                        "feature_snapshot_status": "ok",
                    }
                ],
            )
            write_jsonl(
                paths,
                [
                    {
                        "candidate_id": "c1",
                        "path_source": "yellowstone_account_update",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
                    }
                ],
            )
            _rows, coverage, _audit = training.build_training_view(
                candidate_universe=candidates,
                accepted_lifecycle=lifecycle,
                feature_snapshots=features,
                price_paths=paths,
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
                snapshot_kind="decision",
                fallback_snapshot_kind="decision",
            )

        self.assertEqual(coverage["accepted_lifecycle_join_gate"]["status"], "NO-GO")
        self.assertEqual(coverage["status"], "NO-GO")

    def test_training_view_joins_accepted_lifecycle_by_mint_pool_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            lifecycle = root / "accepted_lifecycle_v1.jsonl"
            features = root / "feature_snapshots_v1.jsonl"
            paths = root / "price_paths.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "mint1:pool1:1000",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "pool1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(
                lifecycle,
                [
                    {
                        "candidate_id": "mint1_pool1_11000",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "decision_ts_ms": 11_000,
                        "truth_status": "resolved",
                        "r1_label": "positive",
                    }
                ],
            )
            write_jsonl(
                features,
                [
                    {
                        "candidate_id": "mint1:pool1:1000",
                        "snapshot_kind": "decision",
                        "feature_cutoff_ts_ms": 5_000,
                        "feature_cutoff_slot": 9,
                        "feature_source": "selector_offline_event_rollup",
                        "feature_observed_lag_ms": 0,
                        "feature_source_max_ts_ms": 5_000,
                        "feature_snapshot_status": "ok",
                    }
                ],
            )
            write_jsonl(
                paths,
                [
                    {
                        "candidate_id": "mint1:pool1:1000",
                        "path_source": "yellowstone_account_update",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
                    }
                ],
            )
            rows, coverage, _audit = training.build_training_view(
                candidate_universe=candidates,
                accepted_lifecycle=lifecycle,
                feature_snapshots=features,
                price_paths=paths,
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
                snapshot_kind="decision",
                fallback_snapshot_kind="decision",
            )

        self.assertTrue(rows[0]["accepted_lifecycle_joined"])
        self.assertEqual(rows[0]["accepted_lifecycle_join_key"], "mint_pool:mint1:pool1")
        self.assertEqual(rows[0]["accepted_lifecycle_candidate_id"], "mint1_pool1_11000")
        self.assertEqual(coverage["accepted_lifecycle_exact_candidate_id_joined"], 0)
        self.assertEqual(coverage["accepted_lifecycle_joined"], 1)
        self.assertEqual(coverage["accepted_lifecycle_join_gate"]["status"], "PASS")

    def test_training_view_excludes_out_of_scope_lifecycle_rows_from_join_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            lifecycle = root / "accepted_lifecycle_v1.jsonl"
            features = root / "feature_snapshots_v1.jsonl"
            paths = root / "price_paths.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "birth_ts_ms": 1_000_000,
                    }
                ],
            )
            write_jsonl(
                lifecycle,
                [
                    {
                        "candidate_id": "old",
                        "base_mint": "old_mint",
                        "pool_id": "old_pool",
                        "decision_ts_ms": 1_000,
                    }
                ],
            )
            write_jsonl(
                features,
                [
                    {
                        "candidate_id": "c1",
                        "snapshot_kind": "decision",
                        "feature_cutoff_ts_ms": 1_005_000,
                        "feature_cutoff_slot": 9,
                        "feature_source": "selector_offline_event_rollup",
                        "feature_observed_lag_ms": 0,
                        "feature_source_max_ts_ms": 1_005_000,
                        "feature_snapshot_status": "ok",
                    }
                ],
            )
            write_jsonl(
                paths,
                [
                    {
                        "candidate_id": "c1",
                        "path_source": "yellowstone_account_update",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
                    }
                ],
            )
            _rows, coverage, _audit = training.build_training_view(
                candidate_universe=candidates,
                accepted_lifecycle=lifecycle,
                feature_snapshots=features,
                price_paths=paths,
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
                snapshot_kind="decision",
                fallback_snapshot_kind="decision",
            )

        self.assertEqual(coverage["accepted_lifecycle_rows"], 1)
        self.assertEqual(coverage["accepted_lifecycle_join_scope_rows"], 0)
        self.assertEqual(coverage["accepted_lifecycle_out_of_scope_rows"], 1)
        self.assertEqual(coverage["accepted_lifecycle_join_gate"]["status"], "PASS")

    def test_gatekeeper_compare_reports_missing_v3_replay_without_pseudo_score(self) -> None:
        rows = [
            {
                "candidate_id": "c1",
                "split": "holdout",
                "eligible": True,
                "cohort_in_scope": True,
                "stream_completeness_ok": True,
                "label_resolved": True,
                "r2_label": "positive",
                "snapshot_kind": "decision",
                "feature_cutoff_ts_ms": 5_000,
                "observation_window_ms": 60_000,
                "gatekeeper_v25_replay_artifact_version": "replay-1",
                "decision_verdict_buy": True,
                "gatekeeper_v25_score": 0.8,
            }
        ]
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            training_view = root / "training.jsonl"
            output = root / "compare.json"
            write_jsonl(training_view, rows)
            report = compare.run(
                compare.build_parser().parse_args(["--training-view", str(training_view), "--output", str(output)])
            )

        by_model = {item["model"]: item for item in report["models"]}
        self.assertEqual(by_model["gatekeeper_v25"]["status"], "ok")
        self.assertEqual(by_model["gatekeeper_v3"]["status"], "replay_input_missing")
        self.assertFalse(report["comparison_contract"]["same_candidate_set"])
        self.assertFalse(report["model_candidate_sets"]["same_model_candidate_set"])

    def test_gatekeeper_compare_empty_eligible_rows_is_not_replay_input_missing(self) -> None:
        rows = [
            {
                "candidate_id": "c1",
                "split": "holdout",
                "eligible": True,
                "cohort_in_scope": True,
                "stream_completeness_ok": False,
                "label_resolved": False,
                "r2_status": "missing_path",
            }
        ]
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            training_view = root / "training.jsonl"
            output = root / "compare.json"
            write_jsonl(training_view, rows)
            report = compare.run(
                compare.build_parser().parse_args(["--training-view", str(training_view), "--output", str(output)])
            )

        by_model = {item["model"]: item for item in report["models"]}
        self.assertEqual(by_model["gatekeeper_v25"]["status"], "no_comparison_rows")
        self.assertEqual(by_model["gatekeeper_v3"]["status"], "no_comparison_rows")
        self.assertIn("no_comparison_eligible_rows", report["contract_checks"]["fail_reasons"])
        self.assertNotIn("model_candidate_set_mismatch", report["contract_checks"]["fail_reasons"])

    def test_baseline_sample_gate_blocks_promotion_on_tiny_holdout(self) -> None:
        rows = []
        for i in range(10):
            split = "train" if i < 6 else "validation" if i < 8 else "holdout"
            rows.append(
                {
                    "candidate_id": f"c{i}",
                    "eligible": True,
                    "stream_completeness_ok": True,
                    "label_resolved": True,
                    "r2_label": "positive" if i % 2 == 0 else "negative",
                    "split": split,
                    "decision_verdict_buy": True,
                    "quote_mint_is_sol": True,
                    "unique_buyers": 10 + i,
                    "trade_rate": 1.0 + i,
                    "net_quote_in_15s": float(i),
                    "net_quote_in_30s": float(i * 2),
                }
            )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            training_view = root / "training.jsonl"
            output = root / "baseline.json"
            leakage = root / "leakage.json"
            write_jsonl(training_view, rows)
            leakage.write_text(json.dumps({"status": "PASS", "violation_count": 0}), encoding="utf-8")
            report = baseline.run(
                baseline.build_parser().parse_args(
                    [
                        "--training-view",
                        str(training_view),
                        "--output",
                        str(output),
                        "--min-first-baseline-accepted",
                        "1",
                        "--min-comparison-accepted",
                        "1",
                        "--min-eligible",
                        "1",
                        "--min-holdout-accepted",
                        "50",
                        "--leakage-audit",
                        str(leakage),
                    ]
                )
            )

        self.assertEqual(report["status"], "ok")
        self.assertIn("permutation_importance_holdout", report["methods"]["rules"])
        self.assertEqual(report["methods"]["rules"]["promotion_gate"]["status"], "NO-GO")

    def test_dataset_orchestrator_writes_public_layout_and_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events.jsonl"
            lifecycle = root / "lifecycle.jsonl"
            paths = root / "paths.jsonl"
            write_jsonl(
                events,
                [
                    {
                        "type": "NewPoolDetected",
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 5_000,
                    },
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 2_000,
                        "slot": 10,
                        "side": "buy",
                        "signer": "buyer1",
                        "quote_amount_sol": 1.0,
                    },
                ],
            )
            write_jsonl(lifecycle, [])
            write_jsonl(
                paths,
                [
                    {
                        "candidate_id": "c1",
                        "path_source": "yellowstone_account_update",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
                    }
                ],
            )
            manifest = dataset.build_dataset(
                dataset.build_parser().parse_args(
                    [
                        "--scope",
                        "unit",
                        "--root",
                        str(root),
                        "--events",
                        str(events),
                        "--lifecycle-report",
                        str(lifecycle),
                        "--price-paths",
                        str(paths),
                        "--pnl-target-net-pct",
                        "40",
                        "--target-net-pct",
                        "40",
                        "--stop-net-pct",
                        "40",
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            dataset_dir = root / "datasets" / "selector" / "unit"
            report_dir = root / "reports" / "selector" / "unit"
            self.assertTrue((dataset_dir / "candidate_universe_v1.jsonl").exists())
            self.assertTrue((dataset_dir / "accepted_lifecycle_v1.jsonl").exists())
            self.assertTrue((dataset_dir / "feature_snapshots_v1.jsonl").exists())
            self.assertTrue((dataset_dir / "selector_training_view_v1.jsonl").exists())
            self.assertTrue((report_dir / "dataset_manifest_v1.json").exists())
            self.assertIn("dataset_manifest_v1", manifest["outputs"])

    def test_phase2_orchestrator_writes_features_and_r2_missing_paths_without_scoring(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "unit"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            candidates = dataset_dir / "candidate_universe_v1.jsonl"
            events = root / "events.jsonl"
            manifest_path = report_dir / "dataset_manifest_v1.json"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 2_000,
                    }
                ],
            )
            write_jsonl(
                events,
                [
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 1_500,
                        "slot": 9,
                        "type": "NewPoolDetected",
                    }
                ],
            )
            manifest_path.write_text(
                json.dumps(
                    {
                        "artifact": "dataset_manifest_v1",
                        "scope": scope,
                        "status": "PASS",
                        "phase1_status": "PASS",
                        "denominator_source": "event_artifact_only",
                        "r2_labels_built": False,
                        "outputs": {
                            "candidate_universe_v1": {
                                "path": str(candidates),
                                "exists": True,
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )

            manifest = phase2.build_phase2(
                phase2.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--events",
                        str(events),
                        "--target-net-pct",
                        "40",
                        "--stop-net-pct",
                        "40",
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            self.assertTrue((dataset_dir / "feature_snapshots_v1.jsonl").exists())
            self.assertTrue((report_dir / "feature_snapshots_manifest_v1.json").exists())
            self.assertFalse((dataset_dir / "selector_training_view_v1.jsonl").exists())
            self.assertFalse((report_dir / "selector_baseline_v1.json").exists())
            self.assertFalse((report_dir / "gatekeeper_compare_v25_v3_v1.json").exists())
            self.assertTrue((dataset_dir / "r2_market_paths_v1.jsonl").exists())
            self.assertTrue((report_dir / "r2_market_path_coverage_v1.json").exists())
            r2_rows = read_jsonl(dataset_dir / "r2_market_paths_v1.jsonl")
            self.assertEqual(len(r2_rows), 1)
            self.assertEqual(r2_rows[0]["r2_status"], "missing_path")
            self.assertEqual(r2_rows[0]["r2_excluded_reason"], "no_canonical_market_path")
            self.assertEqual(manifest["phase2_stage_status"], "P2B_PENDING_R2_DENOMINATOR")
            self.assertEqual(manifest["phase2_status"], "NO-GO/PENDING_R2_DENOMINATOR")
            self.assertEqual(manifest["r2_config"]["profile"], "r2_40_40_60s_v1")
            self.assertTrue(manifest["r2_market_paths_built"])
            self.assertTrue(manifest["r2_label_projection_built"])
            self.assertFalse(manifest["r2_resolved_denominator_built"])
            self.assertFalse(manifest["selector_training_view_built"])
            self.assertFalse(manifest["baseline_built"])
            self.assertFalse(manifest["shadow_only_emit"]["enabled"])

    def test_phase2_accepts_r2_universe_only_phase1_but_keeps_phase3_no_go(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "unit-r2-only"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            candidates = dataset_dir / "candidate_universe_v1.jsonl"
            events = root / "events.jsonl"
            manifest_path = report_dir / "dataset_manifest_v1.json"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 2_000,
                    }
                ],
            )
            write_jsonl(
                events,
                [
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 1_500,
                        "slot": 9,
                        "type": "NewPoolDetected",
                    }
                ],
            )
            manifest_path.write_text(
                json.dumps(
                    {
                        "artifact": "dataset_manifest_v1",
                        "scope": scope,
                        "status": "PASS_FOR_R2_UNIVERSE_ONLY",
                        "phase1_status": "PASS_FOR_R2_UNIVERSE_ONLY",
                        "phase3_precision_readiness": "NO-GO_NO_ACCEPTED_LIFECYCLE",
                        "denominator_source": "event_artifact_only",
                        "r2_labels_built": False,
                        "outputs": {
                            "candidate_universe_v1": {
                                "path": str(candidates),
                                "exists": True,
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )

            manifest = phase2.build_phase2(
                phase2.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--events",
                        str(events),
                        "--target-net-pct",
                        "40",
                        "--stop-net-pct",
                        "40",
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            self.assertEqual(manifest["phase1_status"], "PASS_FOR_R2_UNIVERSE_ONLY")
            self.assertEqual(manifest["phase3_precision_readiness"], "NO-GO_NO_ACCEPTED_LIFECYCLE")
            self.assertTrue(manifest["r2_market_paths_built"])
            self.assertFalse(manifest["selector_training_view_built"])
            self.assertFalse(manifest["baseline_built"])
            self.assertFalse(manifest["shadow_only_emit"]["enabled"])

    def test_phase2_writes_label_coverage_without_phase3_when_lifecycle_is_absent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "unit-r2-label-coverage"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            candidates = dataset_dir / "candidate_universe_v1.jsonl"
            accepted_lifecycle = dataset_dir / "accepted_lifecycle_v1.jsonl"
            events = root / "events.jsonl"
            canonical = root / "canonical_r2_source_v1.jsonl"
            manifest_path = report_dir / "dataset_manifest_v1.json"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "quote_mint": "SOL",
                        "birth_ts_ms": 1_000,
                        "decision_ts_ms": 2_000,
                    }
                ],
            )
            write_jsonl(accepted_lifecycle, [])
            write_jsonl(
                events,
                [
                    {
                        "candidate_id": "c1",
                        "timestamp_ms": 1_500,
                        "slot": 9,
                        "type": "NewPoolDetected",
                    }
                ],
            )
            write_jsonl(
                canonical,
                [
                    {
                        "base_mint": "mint1",
                        "bonding_curve": "curve1",
                        "path_source": "DIAG_ACCOUNT_UPDATE_RELAY",
                        "samples": [
                            {"offset_ms": 0, "ts_ms": 2_000, "slot": 10, "return_pct": 0.0},
                            {"offset_ms": 10_000, "ts_ms": 12_000, "slot": 11, "return_pct": -50.0},
                            {"offset_ms": 60_000, "ts_ms": 62_000, "slot": 12, "return_pct": -50.0},
                        ],
                    }
                ],
            )
            manifest_path.write_text(
                json.dumps(
                    {
                        "artifact": "dataset_manifest_v1",
                        "scope": scope,
                        "status": "PASS_FOR_R2_UNIVERSE_ONLY",
                        "phase1_status": "PASS_FOR_R2_UNIVERSE_ONLY",
                        "phase3_precision_readiness": "NO-GO_NO_ACCEPTED_LIFECYCLE",
                        "denominator_source": "event_artifact_only",
                        "r2_labels_built": False,
                        "outputs": {
                            "candidate_universe_v1": {
                                "path": str(candidates),
                                "exists": True,
                            },
                            "accepted_lifecycle_v1": {
                                "path": str(accepted_lifecycle),
                                "exists": True,
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )

            manifest = phase2.build_phase2(
                phase2.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--events",
                        str(events),
                        "--canonical-snapshot-jsonl",
                        str(canonical),
                        "--target-net-pct",
                        "40",
                        "--stop-net-pct",
                        "40",
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            label_coverage = json.loads((report_dir / "label_coverage_v1.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["phase2_stage_status"], "P2C_PASS")
            self.assertEqual(manifest["phase2_status"], "P2C_PASS_LABEL_COVERAGE_R2_ONLY")
            self.assertEqual(manifest["phase3_precision_readiness"], "NO-GO_NO_ACCEPTED_LIFECYCLE")
            self.assertEqual(label_coverage["phase"], "phase2")
            self.assertEqual(label_coverage["status"], "PASS_FOR_R2_COVERAGE_ONLY")
            self.assertEqual(label_coverage["r2_resolved_rows"], 1)
            self.assertEqual(label_coverage["r2_negative_rows"], 1)
            self.assertEqual(label_coverage["accepted_lifecycle_rows"], 0)
            self.assertEqual(label_coverage["phase3_precision_readiness"], "NO-GO_NO_ACCEPTED_LIFECYCLE")
            self.assertFalse(manifest["selector_training_view_built"])
            self.assertFalse(manifest["baseline_built"])
            self.assertFalse(manifest["shadow_only_emit"]["enabled"])

    def test_phase3_r2only_builds_training_view_without_baseline_claims(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-phase3-r2only-test"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            candidates = dataset_dir / "candidate_universe_v1.jsonl"
            accepted_lifecycle = dataset_dir / "accepted_lifecycle_v1.jsonl"
            features = dataset_dir / "feature_snapshots_v1.jsonl"
            r2_paths = dataset_dir / "r2_market_paths_v1.jsonl"

            candidate_rows = []
            feature_rows = []
            path_rows = []
            for idx in range(20):
                candidate_id = f"c{idx:02d}"
                ts_ms = 1_000 + idx * 1_000
                candidate_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "candidate_universe_status": "ok",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "base_mint": f"mint{idx}",
                        "pool_id": f"pool{idx}",
                        "bonding_curve": f"curve{idx}",
                        "quote_mint": "SOL",
                        "birth_ts_ms": ts_ms,
                        "decision_ts_ms": ts_ms + 500,
                        "decision_verdict_buy": idx % 3 == 0,
                    }
                )
                feature_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "snapshot_kind": "decision",
                        "feature_cutoff_ts_ms": ts_ms + 500,
                        "feature_cutoff_slot": idx + 10,
                        "feature_source": "selector_offline_event_rollup",
                        "feature_observed_lag_ms": 0,
                        "feature_source_max_ts_ms": ts_ms + 500,
                        "feature_snapshot_status": "ok",
                        "feature_time_provenance_ok": True,
                        "unique_buyers": idx + 1,
                    }
                )
                return_pct = 45.0 if idx % 2 == 0 else -45.0
                path_rows.append(
                    {
                        "candidate_id": candidate_id,
                        "base_mint": f"mint{idx}",
                        "pool_id": f"pool{idx}",
                        "bonding_curve": f"curve{idx}",
                        "path_source": "DIAG_ACCOUNT_UPDATE_RELAY",
                        "path_status": "ok",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": return_pct}],
                    }
                )

            write_jsonl(candidates, candidate_rows)
            write_jsonl(accepted_lifecycle, [])
            write_jsonl(features, feature_rows)
            write_jsonl(r2_paths, path_rows)
            (report_dir / "dataset_manifest_v1.json").write_text(
                json.dumps(
                    {
                        "phase2_status": "P2C_PASS_LABEL_COVERAGE_R2_ONLY",
                        "denominator_source": "event_artifact_only",
                        "r2_resolved_denominator_built": True,
                        "selector_training_view_built": False,
                        "baseline_built": False,
                        "gatekeeper_compare_built": False,
                        "outputs": {
                            "candidate_universe_v1": {
                                "path": str(candidates),
                                "exists": True,
                            },
                            "accepted_lifecycle_v1": {
                                "path": str(accepted_lifecycle),
                                "exists": True,
                            },
                            "feature_snapshots_v1": {
                                "path": str(features),
                                "exists": True,
                            },
                            "r2_market_paths_v1": {
                                "path": str(r2_paths),
                                "exists": True,
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )

            phase3_manifest = phase3_r2only.run(
                phase3_r2only.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--target-net-pct",
                        "40",
                        "--stop-net-pct",
                        "40",
                        "--horizon-ms",
                        "60000",
                        "--min-resolved-rows",
                        "10",
                    ]
                )
            )
            training_rows = read_jsonl(dataset_dir / "selector_training_view_v1.jsonl")
            training_manifest = json.loads(
                (report_dir / "selector_training_view_manifest_v1.json").read_text(
                    encoding="utf-8"
                )
            )

        self.assertEqual(phase3_manifest["status"], "PASS_R2_ONLY_DRAFT")
        self.assertEqual(phase3_manifest["phase3_precision_readiness"], "R2_ONLY_READY")
        self.assertEqual(phase3_manifest["dataset_kind"], "r2_only")
        self.assertEqual(
            phase3_manifest["universe_source_class"],
            "ghost_observed_birth_universe",
        )
        self.assertEqual(
            phase3_manifest["universe_completeness_claim"],
            "system_observed_not_archive_complete",
        )
        self.assertEqual(
            phase3_manifest["precision_claim_scope"],
            "observed_birth_universe_only",
        )
        self.assertFalse(phase3_manifest["market_recall_claim_allowed"])
        self.assertFalse(phase3_manifest["production_promotion_allowed"])
        self.assertFalse(phase3_manifest["execution_success_claim_allowed"])
        self.assertFalse(phase3_manifest["realized_pnl_available"])
        self.assertFalse(phase3_manifest["claim_boundaries"]["r1_lifecycle_claim"])
        self.assertFalse(phase3_manifest["claim_boundaries"]["realized_pnl_claim"])
        self.assertFalse(phase3_manifest["baseline_built"])
        self.assertFalse(phase3_manifest["gatekeeper_compare_built"])
        self.assertEqual(phase3_manifest["training_rows"], 20)
        self.assertEqual(phase3_manifest["r2_training_denominator_rows"], 20)
        self.assertEqual(phase3_manifest["r2_positive_rows"], 10)
        self.assertEqual(phase3_manifest["r2_negative_rows"], 10)
        self.assertEqual(training_manifest["r2_training_denominator_rows"], 20)
        self.assertEqual(training_manifest["dataset_kind"], "r2_only")
        self.assertEqual(
            training_manifest["universe_source_class"],
            "ghost_observed_birth_universe",
        )
        self.assertFalse(training_manifest["market_recall_claim_allowed"])
        self.assertFalse(training_manifest["production_promotion_allowed"])
        self.assertEqual(
            training_manifest["r2_training_denominator_split_counts"]["train"],
            14,
        )
        self.assertEqual(
            training_manifest["r2_training_denominator_split_counts"]["validation"],
            3,
        )
        self.assertEqual(
            training_manifest["r2_training_denominator_split_counts"]["holdout"],
            3,
        )
        self.assertEqual(training_rows[0]["phase3_dataset_kind"], "r2_only")
        self.assertTrue(training_rows[0]["r2_only_training_denominator"])
        self.assertIn("selector_accept_context", training_rows[0])
        self.assertEqual(
            training_rows[0]["execution_feasibility_status"],
            "not_available_r2_only",
        )

    def test_r2only_baseline_report_is_draft_only_and_uses_resolved_denominator(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3b-r2only-test"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            training_view = dataset_dir / "selector_training_view_v1.jsonl"
            phase3_manifest = report_dir / "phase3_r2only_manifest_v1.json"

            rows = []
            for idx in range(20):
                split = "train" if idx < 14 else "validation" if idx < 17 else "holdout"
                rows.append(
                    {
                        "candidate_id": f"c{idx:02d}",
                        "split": split,
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "feature_snapshot_status": "ok",
                        "r2_label": "positive" if idx % 2 == 0 else "negative",
                        "r2_status": "resolved",
                        "r2_path_coverage_ok": True,
                        "r2_horizon_matured": True,
                        "r2_only_training_denominator": True,
                        "decision_verdict_buy": idx % 3 == 0,
                        "gatekeeper_v25_score": float(20 - idx),
                        "birth_ts_ms": idx,
                        "unique_buyers": idx + 1,
                        "quote_mint_is_sol": True,
                        "execution_feasibility_status": "not_available_r2_only",
                    }
                )
            rows.append(
                {
                    "candidate_id": "excluded-unmatured",
                    "split": "holdout",
                    "cohort_in_scope": True,
                    "stream_completeness_ok": True,
                    "feature_snapshot_status": "ok",
                    "r2_label": None,
                    "r2_status": "horizon_unmatured",
                    "r2_path_coverage_ok": True,
                    "r2_horizon_matured": False,
                    "r2_only_training_denominator": False,
                    "decision_verdict_buy": True,
                }
            )
            write_jsonl(training_view, rows)
            phase3_manifest.write_text(
                json.dumps(
                    {
                        "status": "PASS_R2_ONLY_DRAFT",
                        "dataset_kind": "r2_only",
                        "market_recall_claim_allowed": False,
                        "production_promotion_allowed": False,
                        "leakage_audit_status": "PASS",
                    }
                ),
                encoding="utf-8",
            )

            report = r2only_baseline.run(
                r2only_baseline.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--min-resolved-rows",
                        "10",
                        "--min-holdout-resolved-rows",
                        "3",
                        "--bootstrap-samples",
                        "50",
                    ]
                )
            )

        self.assertEqual(report["status"], "P3B_PASS_R2_ONLY_BASELINE_DRAFT")
        self.assertEqual(report["dataset_kind"], "r2_only")
        self.assertEqual(report["resolved_denominator_count"], 20)
        self.assertEqual(report["positive_rows"], 10)
        self.assertEqual(report["negative_rows"], 10)
        self.assertEqual(report["split_counts"]["holdout"]["positive"], 1)
        self.assertEqual(report["split_counts"]["holdout"]["negative"], 2)
        self.assertEqual(
            report["selector_accept_context"]["precision_at_accept"]["selected_count"],
            1,
        )
        self.assertEqual(
            report["selector_accept_context"]["precision_at_accept"]["precision_r2"],
            1.0,
        )
        self.assertTrue(report["baseline_built"])
        self.assertFalse(report["market_recall_claim_allowed"])
        self.assertFalse(report["production_promotion_allowed"])
        self.assertFalse(report["claim_boundaries"]["production_promotion_claim"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuning_started"])
        self.assertEqual(report["exclusions"]["horizon_unmatured"], 1)
        self.assertTrue(report["outputs"]["selector_r2only_baseline_report_v1"]["exists"])
        self.assertTrue(report["outputs"]["selector_r2only_baseline_by_bucket_v1"]["exists"])

    def test_r2only_feature_audit_flags_missing_tx_event_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3c-feature-audit-test"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            event_dir = root / "datasets" / "events" / "source"
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            event_dir.mkdir(parents=True)
            training_view = dataset_dir / "selector_training_view_v1.jsonl"
            feature_snapshots = dataset_dir / "feature_snapshots_v1.jsonl"
            r2_market_paths = dataset_dir / "r2_market_paths_v1.jsonl"
            events = event_dir / "events.jsonl"
            feature_manifest = report_dir / "feature_snapshots_manifest_v1.json"

            write_jsonl(
                training_view,
                [
                    {
                        "candidate_id": "c1",
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "feature_snapshot_status": "ok",
                        "r2_label": "positive",
                        "r2_status": "resolved",
                        "r2_path_coverage_ok": True,
                        "r2_horizon_matured": True,
                        "buyer_hhi": None,
                        "top1_wallet_share": None,
                        "sell_share": None,
                        "curve_progress_pct": None,
                    }
                ],
            )
            write_jsonl(
                feature_snapshots,
                [
                    {
                        "candidate_id": "c1",
                        "snapshot_kind": "decision",
                        "feature_snapshot_status": "ok",
                        "source_event_count": 1,
                        "tx_event_count": 0,
                        "buyer_hhi": None,
                        "top1_wallet_share": None,
                        "sell_share": None,
                        "curve_progress_pct": None,
                    }
                ],
            )
            write_jsonl(r2_market_paths, [{"candidate_id": "c1"}])
            write_jsonl(
                events,
                [
                    {
                        "envelope": {"candidate_id": "c1", "event_time_ms": 1_000},
                        "kind": {
                            "type": "NewPoolDetected",
                            "payload": {
                                "is_birth_event": True,
                                "base_mint": "mint1",
                                "pool_id": "pool1",
                                "timestamp_ms": 1_000,
                            },
                        },
                    }
                ],
            )
            feature_manifest.write_text(
                json.dumps({"input_event_paths": [str(events)]}),
                encoding="utf-8",
            )

            report = r2only_feature_audit.run(
                r2only_feature_audit.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

        self.assertEqual(report["status"], "P3C_PASS_DIAGNOSTIC_ONLY")
        self.assertEqual(report["source_event_probe"]["event_type_counts"]["NewPoolDetected"], 1)
        self.assertEqual(report["snapshot_summary"]["tx_event_count_nonzero_rows"], 0)
        buyer_hhi = {
            item["feature"]: item for item in report["feature_reports"]
        }["buyer_hhi"]
        self.assertIn(
            "source_event_artifacts_lack_pool_transaction_rows",
            buyer_hhi["root_cause_candidates"],
        )
        self.assertIn("no_buy_side_detected", buyer_hhi["root_cause_candidates"])
        self.assertTrue(report["claim_boundaries"]["diagnostic_only"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuning_started"])

    def test_r2only_ablation_report_is_diagnostic_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3c-ablation-test"
            dataset_dir = root / "datasets" / "selector" / scope
            report_dir = root / "reports" / "selector" / scope
            dataset_dir.mkdir(parents=True)
            report_dir.mkdir(parents=True)
            training_view = dataset_dir / "selector_training_view_v1.jsonl"
            feature_audit = report_dir / "selector_r2only_feature_audit_v1.json"

            rows = []
            for idx in range(20):
                split = "train" if idx < 14 else "validation" if idx < 17 else "holdout"
                rows.append(
                    {
                        "candidate_id": f"c{idx:02d}",
                        "split": split,
                        "cohort_in_scope": True,
                        "stream_completeness_ok": True,
                        "feature_snapshot_status": "ok",
                        "r2_label": "positive" if idx % 2 == 0 else "negative",
                        "r2_status": "resolved",
                        "r2_path_coverage_ok": True,
                        "r2_horizon_matured": True,
                        "decision_verdict_buy": idx % 4 == 0,
                        "net_quote_in_15s": float(idx),
                        "net_quote_in_30s": float(idx * 2),
                        "trade_rate": float(idx + 1),
                        "unique_buyers": idx + 1,
                        "quote_mint_is_sol": True,
                    }
                )
            write_jsonl(training_view, rows)
            feature_audit.write_text(json.dumps({"status": "P3C_PASS_DIAGNOSTIC_ONLY"}), encoding="utf-8")

            report = r2only_ablation.run(
                r2only_ablation.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--top-k",
                        "2",
                        "3",
                    ]
                )
            )

        self.assertEqual(report["status"], "P3C_PASS_DIAGNOSTIC_ONLY")
        self.assertEqual(report["resolved_denominator_count"], 20)
        self.assertIn("net_quote_in_15s", report["available_features_used"])
        self.assertIn("holdout_precision_at_top_k", report["simple_available_feature_score"])
        self.assertFalse(report["claim_boundaries"]["model_ready"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuning_started"])
        self.assertFalse(report["claim_boundaries"]["production_promotion_claim"])

    def write_feature_contribution_fixture(self, root: Path, scope: str) -> list[dict]:
        dataset_dir = root / "datasets" / "selector" / scope
        report_dir = root / "reports" / "selector" / scope
        dataset_dir.mkdir(parents=True)
        report_dir.mkdir(parents=True)
        rows: list[dict] = []
        labels = [
            ("train", "positive", True, 1.0, 0),
            ("train", "negative", False, 2.0, 1),
            ("train", "positive", False, 3.0, 2),
            ("train", "negative", True, 4.0, 3),
            ("train", "positive", False, 5.0, 4),
            ("validation", "positive", False, 1.5, 5),
            ("validation", "negative", True, 4.5, 6),
            ("holdout", "positive", False, 1.2, 7),
            ("holdout", "negative", True, 4.8, 8),
        ]
        for split, label, accepted, net_quote, idx in labels:
            rows.append(
                {
                    "candidate_id": f"fc{idx}",
                    "base_mint": f"mint{idx}",
                    "birth_ts_ms": 1_000 + idx,
                    "split": split,
                    "cohort_in_scope": True,
                    "stream_completeness_ok": True,
                    "feature_snapshot_status": "ok",
                    "r2_label": label,
                    "r2_status": "resolved",
                    "r2_path_coverage_ok": True,
                    "r2_horizon_matured": True,
                    "decision_verdict_buy": accepted,
                    "net_quote_in_15s": net_quote,
                    "net_quote_in_30s": net_quote * 2.0,
                    "trade_rate": float(10 - idx),
                    "unique_buyers": 2 + idx,
                    "sell_share": 0.10 + (idx * 0.01),
                    "top1_wallet_share": 0.20 + (idx * 0.01),
                    "buyer_hhi": 0.30 + (idx * 0.01),
                    "gk_context_status": "ok",
                    "gk_cutoff_status": "same_decision_time",
                    "gk_observation_profile": "observation_8s_10s",
                    "gk_log_schema_version": 25,
                    "gk_decision_plane": "v25_shadow",
                    "gk_bonding_progress_pct": 10.0 + idx,
                    "gk_current_market_cap_sol": 20.0 + (idx * 2.0),
                    "gk_price_change_ratio": 1.0 + (idx * 0.1),
                }
            )
        rows.append(
            {
                "candidate_id": "unresolved",
                "base_mint": "mint-unresolved",
                "birth_ts_ms": 9_999,
                "split": "holdout",
                "cohort_in_scope": True,
                "stream_completeness_ok": True,
                "feature_snapshot_status": "ok",
                "r2_label": None,
                "r2_status": "horizon_unmatured",
                "r2_path_coverage_ok": True,
                "r2_horizon_matured": False,
                "decision_verdict_buy": True,
                "net_quote_in_15s": 99.0,
                "net_quote_in_30s": 99.0,
                "trade_rate": 99.0,
                "unique_buyers": 99,
                "sell_share": 0.99,
                "top1_wallet_share": 0.99,
                "buyer_hhi": 0.99,
                "gk_context_status": "ok",
                "gk_cutoff_status": "same_decision_time",
                "gk_observation_profile": "observation_8s_10s",
                "gk_log_schema_version": 25,
                "gk_decision_plane": "v25_shadow",
                "gk_bonding_progress_pct": 99.0,
                "gk_current_market_cap_sol": 99.0,
                "gk_price_change_ratio": 9.9,
            }
        )
        write_jsonl(dataset_dir / "selector_training_view_v1.jsonl", rows)
        for path, payload in {
            "selector_r2only_baseline_report_v1.json": {"status": "P3B_PASS_R2_ONLY_BASELINE_DRAFT"},
            "selector_r2only_feature_audit_v1.json": {"status": "P3C_PASS_DIAGNOSTIC_ONLY"},
            "selector_r2only_ablation_report_v1.json": {"status": "P3C_PASS_DIAGNOSTIC_ONLY"},
            "dataset_manifest_v1.json": {"phase2_status": "PASS", "leakage_precheck": "PASS"},
        }.items():
            (report_dir / path).write_text(json.dumps(payload), encoding="utf-8")
        (report_dir / "FEATURE_RICH_R2_BASELINE_DECISION.md").write_text(
            "P3E_PASS_FEATURE_RICH_R2_BASELINE_DRAFT\n",
            encoding="utf-8",
        )
        (report_dir / "gatekeeper_feature_context_manifest_v1.json").write_text(
            json.dumps(
                {
                    "status": "PASS",
                    "feature_columns": [
                        "gk_log_schema_version",
                        "gk_decision_plane",
                        "gk_observation_profile",
                        "gk_context_status",
                        "gk_cutoff_status",
                        "gk_bonding_progress_pct",
                        "gk_current_market_cap_sol",
                        "gk_price_change_ratio",
                    ],
                    "model_feature_columns": [
                        "gk_bonding_progress_pct",
                        "gk_current_market_cap_sol",
                        "gk_price_change_ratio",
                    ],
                }
            ),
            encoding="utf-8",
        )
        return rows

    def test_feature_contribution_report_builds_diagnostic_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3f-feature-contribution-test"
            self.write_feature_contribution_fixture(root, scope)

            report = r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )
            output_json_exists = Path(report["outputs"]["selector_r2only_feature_contribution_v1"]).exists()
            output_md_exists = Path(report["outputs"]["FEATURE_RICH_R2_FEATURE_CONTRIBUTION"]).exists()

        self.assertEqual(report["status"], "P3F_PASS_FEATURE_CONTRIBUTION_DIAGNOSTIC")
        self.assertEqual(report["resolved_denominator_rows"], 9)
        self.assertEqual(report["positive_rows"], 5)
        self.assertEqual(report["negative_rows"], 4)
        self.assertTrue(report["claim_boundaries"]["diagnostic_only"])
        self.assertFalse(report["claim_boundaries"]["model_ready"])
        self.assertFalse(report["claim_boundaries"]["production_ready"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuned"])
        self.assertFalse(report["claim_boundaries"]["threshold_changes"])
        self.assertFalse(report["claim_boundaries"]["runtime_changed"])
        self.assertIn("net_quote_in_15s", report["available_features_used"])
        self.assertTrue(output_json_exists)
        self.assertTrue(output_md_exists)

    def test_feature_contribution_uses_resolved_r2_denominator_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3f-resolved-only-test"
            self.write_feature_contribution_fixture(root, scope)

            report = r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

        self.assertEqual(report["training_rows"], 10)
        self.assertEqual(report["resolved_denominator_rows"], 9)
        self.assertNotIn("unresolved", json.dumps(report["examples"], sort_keys=True))
        self.assertEqual(report["split_counts"]["holdout"]["positive"], 1)
        self.assertEqual(report["split_counts"]["holdout"]["negative"], 1)

    def test_feature_contribution_bins_are_train_derived(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3f-train-bins-test"
            self.write_feature_contribution_fixture(root, scope)

            report = r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

        edges = report["feature_bins"]["net_quote_in_15s"]["train_edges"]
        self.assertEqual(edges, [1.0, 1.8, 2.6, 3.4, 4.2, 5.0])
        holdout_bins = report["feature_bins"]["net_quote_in_15s"]["splits"]["holdout"]
        self.assertEqual(sum(item["rows"] for item in holdout_bins), 2)
        self.assertEqual(holdout_bins[0]["rows"], 1)
        self.assertEqual(holdout_bins[4]["rows"], 1)

    def test_feature_contribution_does_not_claim_model_or_production(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3f-claims-test"
            self.write_feature_contribution_fixture(root, scope)

            report = r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

        self.assertIn("recommended_next_step", report["interpretation"])
        self.assertEqual(report["phase"], "phase3")
        self.assertEqual(report["dataset_kind"], "r2_only")
        self.assertTrue(report["claim_boundaries"]["diagnostic_only"])
        self.assertFalse(any(
            report["claim_boundaries"][key]
            for key in (
                "model_ready",
                "production_ready",
                "gatekeeper_tuned",
                "threshold_changes",
                "runtime_changed",
            )
        ))

    def test_feature_contribution_accept_vs_feature_score_overlap(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3f-overlap-test"
            self.write_feature_contribution_fixture(root, scope)

            report = r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

        top10 = report["gatekeeper_vs_feature_score"]["holdout"]["top10"]
        self.assertEqual(top10["bucket_metrics"]["overlap"]["rows"], 1)
        self.assertEqual(top10["bucket_metrics"]["feature_only"]["rows"], 1)
        self.assertEqual(top10["bucket_metrics"]["gatekeeper_only"]["rows"], 0)
        self.assertEqual(top10["feature_top_rejected_by_gatekeeper"], 1)
        self.assertEqual(top10["gatekeeper_accepted_outside_feature_top"], 0)
        self.assertEqual(
            top10["label_matrix"]["gatekeeper_accept_false|feature_top_true"]["positive"],
            1,
        )

    def test_model_candidate_report_builds_diagnostic_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3g-model-candidate-test"
            self.write_feature_contribution_fixture(root, scope)
            r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

            report = r2only_model_candidate.build_report(
                r2only_model_candidate.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--bootstrap-samples",
                        "50",
                        "--logistic-epochs",
                        "20",
                    ]
                )
            )
            output_json_exists = Path(report["outputs"]["selector_r2only_model_candidate_v1"]).exists()
            output_md_exists = Path(report["outputs"]["FEATURE_RICH_R2_MODEL_CANDIDATE"]).exists()

        self.assertIn(
            report["status"],
            {
                "P3G_PASS_DIAGNOSTIC_MODEL_CANDIDATE",
                "P3G_DIAGNOSTIC_NO_GO_OR_NEEDS_MORE_DATA",
            },
        )
        self.assertEqual(report["resolved_denominator_rows"], 9)
        self.assertTrue(report["claim_boundaries"]["diagnostic_only"])
        self.assertFalse(report["claim_boundaries"]["model_ready"])
        self.assertFalse(report["claim_boundaries"]["production_ready"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuned"])
        self.assertTrue(output_json_exists)
        self.assertTrue(output_md_exists)

    def test_model_candidate_uses_resolved_r2_denominator_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3g-resolved-only-test"
            self.write_feature_contribution_fixture(root, scope)
            r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

            report = r2only_model_candidate.build_report(
                r2only_model_candidate.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--bootstrap-samples",
                        "50",
                        "--logistic-epochs",
                        "20",
                    ]
                )
            )

        self.assertEqual(report["training_rows"], 10)
        self.assertEqual(report["resolved_denominator_rows"], 9)
        self.assertEqual(report["split_counts"]["holdout"]["positive"], 1)
        self.assertEqual(report["split_counts"]["holdout"]["negative"], 1)
        self.assertNotIn("unresolved", json.dumps(report["candidates"], sort_keys=True))

    def test_model_candidate_includes_simple_single_feature_and_logistic_candidates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3g-candidate-families-test"
            self.write_feature_contribution_fixture(root, scope)
            r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    ["--scope", scope, "--root", str(root)]
                )
            )

            report = r2only_model_candidate.build_report(
                r2only_model_candidate.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--bootstrap-samples",
                        "50",
                        "--logistic-epochs",
                        "20",
                    ]
                )
            )

        candidate_ids = {candidate["candidate_id"] for candidate in report["candidates"]}
        self.assertIn("simple_feature_score_v1", candidate_ids)
        self.assertIn("logistic_sanity_baseline", candidate_ids)
        self.assertIn("single_feature_ranker:net_quote_in_15s", candidate_ids)
        logistic = {
            candidate["candidate_id"]: candidate for candidate in report["candidates"]
        }["logistic_sanity_baseline"]
        holdout_top10 = next(
            item for item in logistic["by_split"]["holdout"]["precision_at_top_k"] if item["k"] == 10
        )
        self.assertIn("bootstrap_ci_precision", holdout_top10)
        self.assertFalse(report["claim_boundaries"]["threshold_changes"])
        self.assertFalse(report["claim_boundaries"]["runtime_changed"])

    def test_model_candidate_supports_flow_gk_combined_feature_sets(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3g-flow-gk-combined-test"
            self.write_feature_contribution_fixture(root, scope)
            p3f_report = r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--feature-set",
                        "flow",
                        "--feature-set",
                        "gk",
                        "--feature-set",
                        "combined",
                    ]
                )
            )

            report = r2only_model_candidate.build_report(
                r2only_model_candidate.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--feature-set",
                        "flow",
                        "--feature-set",
                        "gk",
                        "--feature-set",
                        "combined",
                        "--bootstrap-samples",
                        "50",
                        "--logistic-epochs",
                        "20",
                    ]
                )
            )

        self.assertEqual(set(p3f_report["feature_set_reports"]), {"flow", "gk", "combined"})
        self.assertEqual(set(report["feature_set_reports"]), {"flow", "gk", "combined"})
        self.assertIn("gk_bonding_progress_pct", report["features_used_by_set"]["gk"])
        self.assertIn("net_quote_in_15s", report["features_used_by_set"]["flow"])
        self.assertIn("gk_bonding_progress_pct", report["features_used_by_set"]["combined"])
        for forbidden in (
            "gk_log_schema_version",
            "gk_decision_plane",
            "gk_observation_profile",
            "gk_context_status",
            "gk_cutoff_status",
        ):
            self.assertNotIn(forbidden, report["features_used_by_set"]["gk"])
            self.assertNotIn(forbidden, report["features_used_by_set"]["combined"])
        candidate_ids = {candidate["candidate_id"] for candidate in report["candidates"]}
        self.assertIn("flow:simple_feature_score_v1", candidate_ids)
        self.assertIn("gk:simple_feature_score_v1", candidate_ids)
        self.assertIn("combined:simple_feature_score_v1", candidate_ids)
        self.assertIn("gatekeeper_accept_context", report)
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuned"])
        self.assertFalse(report["claim_boundaries"]["runtime_changed"])

    def test_candidate_selection_builds_offline_threshold_and_stability_reports(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3j-candidate-selection-test"
            source_scope = "shadow-burnin-v3-selector-dataset-p3j-test"
            self.write_feature_contribution_fixture(root, scope)
            report_dir = root / "reports" / "selector" / scope
            dataset_manifest_path = report_dir / "dataset_manifest_v1.json"
            dataset_manifest = json.loads(dataset_manifest_path.read_text(encoding="utf-8"))
            dataset_manifest["source_scope"] = source_scope
            dataset_manifest_path.write_text(json.dumps(dataset_manifest), encoding="utf-8")
            (report_dir / "phase3_r2only_manifest_v1.json").write_text(
                json.dumps(
                    {
                        "status": "PASS_R2_ONLY_DRAFT",
                        "phase3_precision_readiness": "R2_ONLY_READY",
                        "leakage_audit_status": "PASS",
                        "fail_reasons": [],
                    }
                ),
                encoding="utf-8",
            )
            r2only_feature_contribution.build_report(
                r2only_feature_contribution.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--feature-set",
                        "flow",
                        "--feature-set",
                        "gk",
                        "--feature-set",
                        "combined",
                    ]
                )
            )
            simcov_report_dir = root / "reports" / "selector" / source_scope
            simcov_report_dir.mkdir(parents=True)
            (simcov_report_dir / "buy_simulation_coverage_audit_v1.json").write_text(
                json.dumps(
                    {
                        "metrics": {
                            "buy_rows": 20,
                            "shadow_simulation_attempted_rows": 20,
                            "shadow_simulated_rows": 19,
                            "not_executable_route_rows": 0,
                        },
                        "critical_regression_markers": {
                            "AccountNotFound": 0,
                            "unsupported_legacy_buy_layout_requires_bcv2": 0,
                            "ResourceExhausted": 0,
                        },
                        "failure_classes": {
                            "UNKNOWN_UNCLASSIFIED": {"count": 0},
                            "LEGACY_BC_V2_TAIL_RESOLVER_FAILED": {"count": 0},
                        },
                    }
                ),
                encoding="utf-8",
            )

            report = r2only_candidate_selection.build_report(
                r2only_candidate_selection.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--feature-set",
                        "combined",
                        "--bootstrap-samples",
                        "25",
                        "--logistic-epochs",
                        "20",
                    ]
                )
            )
            output_json_exists = Path(report["outputs"]["selector_r2only_candidate_selection_v1"]).exists()
            output_md_exists = Path(report["outputs"]["FEATURE_RICH_R2_CANDIDATE_SELECTION"]).exists()
            threshold_csv_exists = Path(report["outputs"]["selector_r2only_threshold_grid_v1"]).exists()
            stability_csv_exists = Path(report["outputs"]["selector_r2only_candidate_stability_v1"]).exists()

        self.assertEqual(report["status"], "P3J_NO-GO_UNSTABLE_SIGNAL")
        self.assertIn("r2_training_denominator_below_1440", report["acceptance"]["fail_reasons"])
        self.assertEqual(report["simcov_operational_gate"]["status"], "PASS")
        self.assertTrue(report["claim_boundaries"]["diagnostic_only"])
        self.assertFalse(report["claim_boundaries"]["production_promotion_allowed"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuning_started"])
        self.assertFalse(report["claim_boundaries"]["runtime_changed"])
        self.assertEqual(report["ev_proxy"]["claim"], "R2 market-opportunity EV proxy, not live PnL")
        candidate_ids = {candidate["candidate_id"] for candidate in report["candidates"]}
        self.assertIn("combined:simple_feature_score_v1", candidate_ids)
        self.assertIn("gk_context_only:simple_feature_score_v1", candidate_ids)
        self.assertIn("combined:logistic_sanity_baseline", candidate_ids)
        self.assertIn("flow_only", report["comparators"])
        self.assertIn("gatekeeper_accept", report["comparators"])
        self.assertIn("combined:simple_feature_score_v1", report["threshold_grid"])
        self.assertIn("gk_concentration_available", report["stability"])
        self.assertIn("minus_concentration_features", report["feature_ablation"])
        self.assertTrue(output_json_exists)
        self.assertTrue(output_md_exists)
        self.assertTrue(threshold_csv_exists)
        self.assertTrue(stability_csv_exists)

    def build_shadow_score_contract_fixture(
        self,
        root: Path,
        scope: str,
        *,
        missing_core_candidate_id: str | None = None,
    ) -> dict:
        source_scope = f"{scope}-source"
        rows = self.write_feature_contribution_fixture(root, scope)
        dataset_path = root / "datasets" / "selector" / scope / "selector_training_view_v1.jsonl"
        if missing_core_candidate_id:
            for row in rows:
                if row.get("candidate_id") == missing_core_candidate_id:
                    row.pop("gk_bonding_progress_pct", None)
            write_jsonl(dataset_path, rows)
        report_dir = root / "reports" / "selector" / scope
        dataset_manifest_path = report_dir / "dataset_manifest_v1.json"
        dataset_manifest = json.loads(dataset_manifest_path.read_text(encoding="utf-8"))
        dataset_manifest["source_scope"] = source_scope
        dataset_manifest_path.write_text(json.dumps(dataset_manifest), encoding="utf-8")
        (report_dir / "phase3_r2only_manifest_v1.json").write_text(
            json.dumps(
                {
                    "status": "PASS_R2_ONLY_DRAFT",
                    "phase3_precision_readiness": "R2_ONLY_READY",
                    "leakage_audit_status": "PASS",
                    "fail_reasons": [],
                }
            ),
            encoding="utf-8",
        )
        r2only_feature_contribution.build_report(
            r2only_feature_contribution.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--feature-set",
                    "flow",
                    "--feature-set",
                    "gk",
                    "--feature-set",
                    "combined",
                ]
            )
        )
        r2only_model_candidate.build_report(
            r2only_model_candidate.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--feature-set",
                    "flow",
                    "--feature-set",
                    "gk",
                    "--feature-set",
                    "combined",
                    "--bootstrap-samples",
                    "25",
                    "--logistic-epochs",
                    "20",
                ]
            )
        )
        simcov_report_dir = root / "reports" / "selector" / source_scope
        simcov_report_dir.mkdir(parents=True)
        (simcov_report_dir / "buy_simulation_coverage_audit_v1.json").write_text(
            json.dumps(
                {
                    "metrics": {
                        "buy_rows": 20,
                        "shadow_simulation_attempted_rows": 20,
                        "shadow_simulated_rows": 19,
                        "not_executable_route_rows": 0,
                    },
                    "critical_regression_markers": {
                        "AccountNotFound": 0,
                        "unsupported_legacy_buy_layout_requires_bcv2": 0,
                        "ResourceExhausted": 0,
                    },
                    "failure_classes": {
                        "UNKNOWN_UNCLASSIFIED": {"count": 0},
                        "LEGACY_BC_V2_TAIL_RESOLVER_FAILED": {"count": 0},
                    },
                }
            ),
            encoding="utf-8",
        )
        r2only_candidate_selection.build_report(
            r2only_candidate_selection.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--feature-set",
                    "combined",
                    "--bootstrap-samples",
                    "25",
                    "--logistic-epochs",
                    "20",
                ]
            )
        )
        return shadow_score_contract.build_report(
            shadow_score_contract.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                ]
            )
        )

    def test_shadow_score_contract_builds_from_p3j_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3k-contract-test"
            report = self.build_shadow_score_contract_fixture(root, scope)
            output_json_exists = Path(report["outputs"]["selector_shadow_score_contract_v1"]).exists()
            output_md_exists = Path(report["outputs"]["SELECTOR_SHADOW_SCORE_CONTRACT"]).exists()
            output_scores_exists = Path(report["outputs"]["selector_shadow_scores_v1"]).exists()
            output_thresholds_exists = Path(report["outputs"]["selector_shadow_score_thresholds_v1"]).exists()

        self.assertEqual(report["status"], "P3K_PASS_SHADOW_SCORE_CONTRACT_DRAFT")
        self.assertEqual(report["candidate_contract"]["candidate_id"], "combined:simple_feature_score_v1")
        self.assertEqual(report["candidate_contract"]["score_version"], "selector_shadow_score_combined_simple_v1")
        self.assertIn("flow", report["feature_groups"])
        self.assertIn("gk_curve_market_core", report["feature_groups"])
        self.assertTrue(output_json_exists)
        self.assertTrue(output_md_exists)
        self.assertTrue(output_scores_exists)
        self.assertTrue(output_thresholds_exists)

    def test_shadow_score_contract_preserves_non_claims(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.build_shadow_score_contract_fixture(root, "selector-p3k-non-claims-test")

        self.assertTrue(report["claim_boundaries"]["diagnostic_only"])
        self.assertTrue(report["claim_boundaries"]["shadow_only"])
        self.assertFalse(report["claim_boundaries"]["production_promotion_allowed"])
        self.assertFalse(report["claim_boundaries"]["gatekeeper_tuning_started"])
        self.assertFalse(report["claim_boundaries"]["runtime_changed"])
        self.assertFalse(report["claim_boundaries"]["active_execution_changed"])
        self.assertFalse(report["claim_boundaries"]["send_path_changed"])
        self.assertFalse(report["candidate_contract"]["production_ready"])
        self.assertFalse(report["candidate_contract"]["gatekeeper_tuning_ready"])
        self.assertFalse(report["candidate_contract"]["runtime_active"])

    def test_shadow_score_contract_marks_missing_concentration_degraded(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.build_shadow_score_contract_fixture(root, "selector-p3k-concentration-test")
            score_rows = read_jsonl(Path(report["outputs"]["selector_shadow_scores_v1"]))

        self.assertGreater(
            report["score_validity_status_counts"].get("score_degraded_missing_concentration", 0),
            0,
        )
        self.assertTrue(any(row["concentration_available"] is False for row in score_rows))
        self.assertTrue(report["missing_policy"]["missing_not_safe"])
        self.assertTrue(report["missing_policy"]["missing_not_negative_automatically"])

    def test_shadow_score_contract_invalidates_missing_core_curve_market(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "selector-p3k-core-missing-test"
            candidate_id = "fc0"
            report = self.build_shadow_score_contract_fixture(
                root,
                scope,
                missing_core_candidate_id=candidate_id,
            )
            score_rows = read_jsonl(Path(report["outputs"]["selector_shadow_scores_v1"]))
            row = next(item for item in score_rows if item["candidate_id"] == candidate_id)

        self.assertEqual(row["score_validity_status"], "score_invalid_missing_core_curve_market")
        self.assertIn("gk_bonding_progress_pct", row["reason_vector"]["missing"])
        self.assertGreaterEqual(row["required_feature_missing_count"], 1)

    def test_shadow_score_contract_reproduces_topk_metrics(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.build_shadow_score_contract_fixture(root, "selector-p3k-repro-test")

        self.assertEqual(report["acceptance"]["topk_reproduction_status"], "PASS")
        for split_payload in report["topk_reproduction"].values():
            for payload in split_payload.values():
                self.assertEqual(payload["status"], "PASS")
                self.assertEqual(payload["delta"], 0.0)

    def test_shadow_score_contract_writes_reason_vector(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = self.build_shadow_score_contract_fixture(root, "selector-p3k-reason-vector-test")
            score_rows = read_jsonl(Path(report["outputs"]["selector_shadow_scores_v1"]))

        top_rows = [row for row in score_rows if row["threshold_pass_top10_equiv"]]
        self.assertTrue(top_rows)
        for row in top_rows:
            self.assertIsInstance(row["reason_vector"], dict)
            self.assertIn("positive", row["reason_vector"])
            self.assertIn("negative", row["reason_vector"])
            self.assertIn("missing", row["reason_vector"])
            self.assertFalse(row["non_claims"]["changes_gatekeeper_decision"])
            self.assertFalse(row["non_claims"]["changes_execution"])
            self.assertFalse(row["non_claims"]["production_signal"])

    def test_shadow_score_sidecar_audit_validates_terminal_decision_coverage(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "shadow-score-sidecar-audit-test"
            decision_dir = (
                root
                / "logs"
                / "rollout"
                / scope
                / "decisions"
                / scope
                / "v2.2"
                / "legacy_live"
                / "fixture"
            )
            decisions = [
                {"candidate_id": "c1", "verdict_type": "BUY"},
                {"candidate_id": "c2", "verdict_type": "REJECT_HARD_FAIL"},
                {"candidate_id": "c3", "verdict_type": "TIMEOUT_PHASE1_NO_DATA"},
            ]
            score_rows = []
            for row in decisions:
                score_rows.append(
                    {
                        "schema_version": "selector_shadow_score_v1",
                        "score_version": "selector_shadow_score_combined_simple_v1",
                        "score_candidate_id": "combined:simple_feature_score_v1",
                        "candidate_id": row["candidate_id"],
                        "gatekeeper_verdict_type": row["verdict_type"],
                        "selector_shadow_score": 0.42,
                        "score_validity_status": "score_valid",
                        "feature_availability": {
                            "feature_mapping_status": "partial_runtime_mapping_missing_flow_features"
                        },
                        "thresholds": {"top10_equiv_pass": False},
                        "reason_vector": {"positive": [], "negative": [], "missing": []},
                        "claim_boundaries": {
                            "diagnostic_only": True,
                            "shadow_only": True,
                            "production_promotion_allowed": False,
                            "gatekeeper_tuning_started": False,
                            "changes_gatekeeper_decision": False,
                            "changes_execution": False,
                            "send_path_changed": False,
                        },
                    }
                )
            write_jsonl(decision_dir / "gatekeeper_v2_decisions.jsonl", decisions)
            write_jsonl(decision_dir / "selector_shadow_score_v1.jsonl", score_rows)

            report = shadow_score_sidecar_audit.build_report(
                shadow_score_sidecar_audit.build_parser().parse_args(
                    [
                        "--scope",
                        scope,
                        "--root",
                        str(root),
                        "--decision-plane",
                        "legacy_live",
                        "--min-score-coverage",
                        "0.95",
                    ]
                )
            )

        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["decision_rows"], 3)
        self.assertEqual(report["score_rows"], 3)
        self.assertEqual(report["numeric_score_rows"], 3)
        self.assertEqual(report["claim_boundary_violation_rows"], 0)
        self.assertEqual(report["decision_influence_claim_rows"], 0)
        self.assertEqual(report["execution_influence_claim_rows"], 0)
        self.assertEqual(report["send_path_changed_claim_rows"], 0)

    def write_shadow_score_parity_fixture(
        self,
        root: Path,
        *,
        scope: str = "shadow-score-parity-test",
        score_delta: float = 0.0,
        threshold_mismatch: bool = False,
        claim_boundary_violation: bool = False,
    ) -> tuple[Path, Path]:
        decision_dir = (
            root
            / "logs"
            / "rollout"
            / scope
            / "decisions"
            / scope
            / "v2.2"
            / "legacy_live"
            / "fixture"
        )
        rust_source = Path(__file__).resolve().parents[1] / "ghost-brain/src/oracle/decision_logger.rs"
        specs, thresholds = shadow_score_parity_audit.parse_runtime_spec(rust_source)
        decision = {
            "execution_candidate_id": "candidate-1",
            "pool_id": "pool-1",
            "base_mint": "mint-1",
            "verdict_type": "BUY",
            "observation_end_ts_ms": 1_000,
            "curve_data_known": True,
            "curve_wait_elapsed_ms": 10_010,
            "bonding_progress_pct": 50.0,
            "current_market_cap_sol": 100.0,
            "price_change_ratio": 2.0,
            "hhi": 0.2,
            "top3_volume_pct": 0.3,
            "total_tx_evaluated": 10,
            "unique_signers_evaluated": 5,
            "buy_count": 4,
            "buy_ratio": 0.5,
            "sell_buy_ratio": 0.2,
            "total_volume_sol": 5.0,
            "avg_tx_sol": 0.5,
            "net_quote_in_15s": 3.0,
            "net_quote_in_30s": 3.0,
            "trade_rate": 1.0,
            "unique_buyers": 4,
            "sell_share": 0.2,
            "top1_wallet_share": 0.35,
            "buyer_hhi": 0.25,
        }
        expected = shadow_score_parity_audit.recompute(decision, specs, thresholds)
        score_row = {
            "schema_version": "selector_shadow_score_v1",
            "score_version": "selector_shadow_score_combined_simple_v1",
            "score_candidate_id": "combined:simple_feature_score_v1",
            "candidate_id": "candidate-1",
            "pool_id": "pool-1",
            "base_mint": "mint-1",
            "gatekeeper_verdict_type": "BUY",
            "selector_shadow_score": expected["selector_shadow_score"] + score_delta,
            "score_validity_status": expected["score_validity_status"],
            "feature_availability": expected["feature_availability"],
            "thresholds": dict(expected["thresholds"]),
            "reason_vector": {"positive": [], "negative": [], "missing": []},
            "claim_boundaries": {
                "diagnostic_only": True,
                "shadow_only": True,
                "production_promotion_allowed": False,
                "gatekeeper_tuning_started": False,
                "changes_gatekeeper_decision": False,
                "changes_execution": claim_boundary_violation,
                "send_path_changed": False,
            },
        }
        if threshold_mismatch:
            score_row["thresholds"]["top10_equiv_pass"] = not score_row["thresholds"][
                "top10_equiv_pass"
            ]
        write_jsonl(decision_dir / "gatekeeper_v2_decisions.jsonl", [decision])
        write_jsonl(decision_dir / "selector_shadow_score_v1.jsonl", [score_row])
        return decision_dir, rust_source

    def run_shadow_score_parity_fixture(self, root: Path, scope: str, rust_source: Path) -> dict:
        return shadow_score_parity_audit.build_report(
            shadow_score_parity_audit.build_parser().parse_args(
                [
                    "--runtime-scope",
                    scope,
                    "--root",
                    str(root),
                    "--decision-plane",
                    "legacy_live",
                    "--rust-source",
                    str(rust_source),
                ]
            )
        )

    def test_shadow_score_parity_recomputes_runtime_mapped_subset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "shadow-score-parity-pass"
            _, rust_source = self.write_shadow_score_parity_fixture(root, scope=scope)
            report = self.run_shadow_score_parity_fixture(root, scope, rust_source)

        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["matched_rows"], 1)
        self.assertEqual(report["runtime_formula_parity"]["score_mismatch_rows"], 0)
        self.assertEqual(report["runtime_formula_parity"]["threshold_pass_mismatch_count"], 0)
        self.assertEqual(report["runtime_formula_parity"]["validity_status_mismatch_count"], 0)

    def test_shadow_score_parity_detects_score_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "shadow-score-parity-score-mismatch"
            _, rust_source = self.write_shadow_score_parity_fixture(
                root, scope=scope, score_delta=0.01
            )
            report = self.run_shadow_score_parity_fixture(root, scope, rust_source)

        self.assertEqual(report["status"], "FAIL")
        self.assertIn("score_mismatch_rows=1", report["fail_reasons"])

    def test_shadow_score_parity_detects_threshold_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "shadow-score-parity-threshold-mismatch"
            _, rust_source = self.write_shadow_score_parity_fixture(
                root, scope=scope, threshold_mismatch=True
            )
            report = self.run_shadow_score_parity_fixture(root, scope, rust_source)

        self.assertEqual(report["status"], "FAIL")
        self.assertIn("threshold_pass_mismatch_count=1", report["fail_reasons"])

    def test_shadow_score_parity_reports_complete_flow_mapping(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "shadow-score-parity-drift"
            _, rust_source = self.write_shadow_score_parity_fixture(root, scope=scope)
            report = self.run_shadow_score_parity_fixture(root, scope, rust_source)

        self.assertEqual(
            report["mapped_only_drift"]["status"],
            "NO_RUNTIME_MAPPING_DRIFT_FULL_MAPPING_AVAILABLE",
        )
        self.assertEqual(report["feature_spec"]["missing_runtime_mapping_features"], [])
        self.assertGreater(report["feature_spec"]["mapped_features"], 0)

    def test_shadow_score_parity_requires_non_claim_boundaries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "shadow-score-parity-boundary"
            _, rust_source = self.write_shadow_score_parity_fixture(
                root, scope=scope, claim_boundary_violation=True
            )
            report = self.run_shadow_score_parity_fixture(root, scope, rust_source)

        self.assertEqual(report["status"], "FAIL")
        self.assertIn("claim_boundary_violation_rows=1", report["fail_reasons"])

    def test_selector_lifecycle_launcher_accepts_build_freshness_flag(self) -> None:
        args = lifecycle_launcher.build_parser().parse_args(
            [
                "--scope",
                "shadow-burnin-v3-selector-dataset-r11-simcov-route2-smoke",
                "--config",
                "configs/rollout/shadow-burnin-v3-selector-dataset-r11-simcov-route2-smoke.toml",
                "--tmux-session",
                "selector_dataset_r11_simcov_route2",
                "--build-release-before-start",
                "--dry-run",
            ]
        )

        self.assertTrue(args.build_release_before_start)
        self.assertTrue(args.dry_run)

    def test_r12_nln_route_evidence_profile_uses_exact_two_program_streams(self) -> None:
        config_path = (
            Path(__file__).resolve().parents[1]
            / "configs"
            / "rollout"
            / "shadow-burnin-v3-selector-dataset-r12-simcov-evidence.toml"
        )
        with config_path.open("rb") as fh:
            config = tomllib.load(fh)
        program_streams = config["seer"]["program_streams"]
        enabled_topics = program_streams["enabled_topics"]
        disabled_streams = program_streams["disabled_streams"]
        eventstream_policy = json.loads(program_streams["eventstream_policy_header"])

        self.assertEqual(program_streams["endpoint"], "events.nln.clr3.org:443")
        self.assertEqual(program_streams["max_streams"], 2)
        self.assertEqual(
            enabled_topics,
            ["solana.pump_fun.buy", "solana.pump_fun.buy_exact_sol_in"],
        )
        self.assertEqual(
            eventstream_policy["allowed_topics"],
            ["solana.pump_fun.buy", "solana.pump_fun.buy_exact_sol_in"],
        )
        self.assertIn("prod.rpc.solana.pumpfun.trade", disabled_streams)
        self.assertIn("prod.rpc.solana.system.transfers", disabled_streams)
        self.assertTrue(set(enabled_topics).isdisjoint(disabled_streams))
        self.assertLessEqual(len(enabled_topics), 2)

    def test_r2_market_paths_writes_one_row_per_candidate_and_missing_path_is_unresolved(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    },
                    {
                        "candidate_id": "c2",
                        "base_mint": "mint2",
                        "pool_id": "pool2",
                        "bonding_curve": "curve2",
                        "decision_ts_ms": 1_000,
                    },
                ],
            )
            rows, coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )

        self.assertEqual(len(rows), 2)
        self.assertEqual({row["r2_status"] for row in rows}, {"missing_path"})
        self.assertEqual(coverage["status"], "NO-GO/PENDING_R2_DENOMINATOR")
        self.assertEqual(coverage["r2_missing_path_rows"], 2)
        self.assertEqual(coverage["r2_resolved_rows"], 0)

    def test_r2_market_paths_target_stop_and_no_target_labels(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            source = root / "account_updates.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "target",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    },
                    {
                        "candidate_id": "stop",
                        "base_mint": "mint2",
                        "pool_id": "pool2",
                        "bonding_curve": "curve2",
                        "decision_ts_ms": 1_000,
                    },
                    {
                        "candidate_id": "flat",
                        "base_mint": "mint3",
                        "pool_id": "pool3",
                        "bonding_curve": "curve3",
                        "decision_ts_ms": 1_000,
                    },
                ],
            )
            write_jsonl(
                source,
                [
                    {
                        "candidate_id": "target",
                        "path_source": "yellowstone_account_update",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [
                            {"offset_ms": 1_000, "return_pct": 45.0},
                            {"offset_ms": 60_000, "return_pct": -45.0},
                        ],
                    },
                    {
                        "candidate_id": "stop",
                        "path_source": "DIAG_ACCOUNT_UPDATE_RELAY",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [
                            {"offset_ms": 1_000, "return_pct": -45.0},
                            {"offset_ms": 60_000, "return_pct": 45.0},
                        ],
                    },
                    {
                        "candidate_id": "flat",
                        "path_source": "canonical_account_state_snapshot",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 5.0}],
                    },
                ],
            )
            rows, coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[source],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )

        by_id = {row["candidate_id"]: row for row in rows}
        self.assertEqual(by_id["target"]["r2_label"], "positive")
        self.assertEqual(by_id["target"]["r2_status"], "positive")
        self.assertEqual(by_id["target"]["r2_label_reason"], "target_before_stop")
        self.assertEqual(by_id["stop"]["r2_label"], "negative")
        self.assertEqual(by_id["stop"]["r2_label_reason"], "stop_before_target")
        self.assertEqual(by_id["flat"]["r2_label"], "negative")
        self.assertEqual(by_id["flat"]["r2_label_reason"], "no_target_by_horizon")
        self.assertEqual(coverage["status"], "PASS")
        self.assertEqual(coverage["r2_resolved_rows"], 3)

    def test_r2_market_paths_unresolved_statuses_do_not_become_negative(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            source = root / "account_updates.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "incomplete",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    },
                    {
                        "candidate_id": "unmatured",
                        "base_mint": "mint2",
                        "pool_id": "pool2",
                        "bonding_curve": "curve2",
                        "decision_ts_ms": 1_000,
                    },
                ],
            )
            write_jsonl(
                source,
                [
                    {
                        "candidate_id": "incomplete",
                        "path_source": "yellowstone_account_update",
                        "path_coverage_ok": False,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 5.0}],
                    },
                    {
                        "candidate_id": "unmatured",
                        "path_source": "yellowstone_account_update",
                        "path_coverage_ok": True,
                        "horizon_matured": False,
                        "samples": [{"offset_ms": 1_000, "return_pct": 5.0}],
                    },
                ],
            )
            rows, _coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[source],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )

        by_id = {row["candidate_id"]: row for row in rows}
        self.assertIsNone(by_id["incomplete"]["r2_label"])
        self.assertEqual(by_id["incomplete"]["r2_status"], "stream_incomplete")
        self.assertIsNone(by_id["unmatured"]["r2_label"])
        self.assertEqual(by_id["unmatured"]["r2_status"], "horizon_unmatured")

    def test_r2_market_paths_reject_nln_and_rpc_as_canonical_sources(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            source = root / "account_updates.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "nln",
                        "base_mint": "mint1",
                        "pool_id": "pool1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    },
                    {
                        "candidate_id": "rpc",
                        "base_mint": "mint2",
                        "pool_id": "pool2",
                        "bonding_curve": "curve2",
                        "decision_ts_ms": 1_000,
                    },
                ],
            )
            write_jsonl(
                source,
                [
                    {
                        "candidate_id": "nln",
                        "path_source": "nln_program_stream_pumpfun_trade",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
                    },
                    {
                        "candidate_id": "rpc",
                        "path_source": "rpc_canonical_account_state",
                        "path_coverage_ok": True,
                        "horizon_matured": True,
                        "samples": [{"offset_ms": 60_000, "return_pct": 50.0}],
                    },
                ],
            )
            rows, coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[source],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )

        self.assertEqual({row["r2_status"] for row in rows}, {"noncanonical_source"})
        self.assertTrue(all(row["r2_label"] is None for row in rows))
        self.assertEqual(coverage["r2_noncanonical_source_rows"], 2)
        self.assertEqual(coverage["r2_resolved_rows"], 0)

    def test_canonical_r2_source_exports_diag_rows_compatible_with_r2_builder(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            diag_log = root / "system.log"
            source_output = root / "canonical_r2_source_v1.jsonl"
            manifest_output = root / "canonical_r2_source_manifest_v1.json"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "curve1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    }
                ],
            )
            diag_log.write_text(
                "\n".join(
                    [
                        (
                            "1970-01-01T00:00:01.000Z INFO "
                            "DIAG_ACCOUNT_UPDATE_RELAY base_mint=mint1 bonding_curve=curve1 "
                            "slot=10 sol_reserves=1000000000 token_reserves=1000000 "
                            "complete=0 curve_finality=confirmed"
                        ),
                        (
                            "1970-01-01T00:01:01.000Z INFO "
                            "DIAG_ACCOUNT_UPDATE_RELAY base_mint=mint1 bonding_curve=curve1 "
                            "slot=11 sol_reserves=1500000000 token_reserves=1000000 "
                            "complete=0 curve_finality=confirmed"
                        ),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            source_manifest = canonical_r2.run(
                canonical_r2.build_parser().parse_args(
                    [
                        "--root",
                        str(root),
                        "--candidate-universe",
                        str(candidates),
                        "--diag-log",
                        str(diag_log),
                        "--output",
                        str(source_output),
                        "--manifest-output",
                        str(manifest_output),
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            rows, coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[source_output],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )
            source_rows = read_jsonl(source_output)
            manifest_exists = manifest_output.exists()

        self.assertEqual(source_manifest["status"], "PASS")
        self.assertEqual(source_manifest["source_rows_written"], 1)
        self.assertEqual(source_manifest["candidate_ok_rows"], 1)
        self.assertEqual(len(source_rows), 1)
        self.assertTrue(manifest_exists)
        self.assertEqual(rows[0]["path_source"], "DIAG_ACCOUNT_UPDATE_RELAY")
        self.assertEqual(rows[0]["r2_label"], "positive")
        self.assertEqual(rows[0]["r2_label_reason"], "target_before_stop")
        self.assertEqual(coverage["status"], "PASS")

    def test_canonical_r2_source_allows_one_ms_horizon_flooring(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            diag_log = root / "system.log"
            source_output = root / "canonical_r2_source_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "curve1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    }
                ],
            )
            diag_log.write_text(
                "\n".join(
                    [
                        (
                            "1970-01-01T00:00:01.000Z INFO "
                            "DIAG_ACCOUNT_UPDATE_RELAY base_mint=mint1 bonding_curve=curve1 "
                            "slot=10 sol_reserves=1000000000 token_reserves=1000000 "
                            "complete=0 curve_finality=confirmed"
                        ),
                        (
                            "1970-01-01T00:01:00.999Z INFO "
                            "DIAG_ACCOUNT_UPDATE_RELAY base_mint=mint1 bonding_curve=curve1 "
                            "slot=11 sol_reserves=900000000 token_reserves=1000000 "
                            "complete=0 curve_finality=confirmed"
                        ),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            source_manifest = canonical_r2.run(
                canonical_r2.build_parser().parse_args(
                    [
                        "--root",
                        str(root),
                        "--candidate-universe",
                        str(candidates),
                        "--diag-log",
                        str(diag_log),
                        "--output",
                        str(source_output),
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            source_rows = read_jsonl(source_output)
            rows, coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[source_output],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )

        self.assertEqual(source_manifest["status"], "PASS")
        self.assertEqual(source_manifest["horizon_tolerance_ms"], 1)
        self.assertEqual(source_manifest["effective_horizon_ms"], 59_999)
        self.assertEqual(source_manifest["candidate_ok_rows"], 1)
        self.assertEqual(source_rows[0]["path_status"], "ok")
        self.assertTrue(source_rows[0]["horizon_matured"])
        self.assertEqual(source_rows[0]["horizon_tolerance_ms"], 1)
        self.assertEqual(rows[0]["r2_status"], "negative")
        self.assertEqual(rows[0]["r2_label"], "negative")
        self.assertEqual(rows[0]["r2_label_reason"], "no_target_by_horizon")
        self.assertEqual(coverage["status"], "PASS")

    def test_canonical_r2_source_post_horizon_grace_is_maturity_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            diag_log = root / "system.log"
            source_output = root / "canonical_r2_source_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "curve1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    }
                ],
            )
            diag_log.write_text(
                "\n".join(
                    [
                        (
                            "1970-01-01T00:00:01.000Z INFO "
                            "DIAG_ACCOUNT_UPDATE_RELAY base_mint=mint1 bonding_curve=curve1 "
                            "slot=10 sol_reserves=1000000000 token_reserves=1000000 "
                            "complete=0 curve_finality=confirmed"
                        ),
                        (
                            "1970-01-01T00:01:01.500Z INFO "
                            "DIAG_ACCOUNT_UPDATE_RELAY base_mint=mint1 bonding_curve=curve1 "
                            "slot=11 sol_reserves=2000000000 token_reserves=1000000 "
                            "complete=0 curve_finality=confirmed"
                        ),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            source_manifest = canonical_r2.run(
                canonical_r2.build_parser().parse_args(
                    [
                        "--root",
                        str(root),
                        "--candidate-universe",
                        str(candidates),
                        "--diag-log",
                        str(diag_log),
                        "--output",
                        str(source_output),
                        "--horizon-ms",
                        "60000",
                        "--post-horizon-grace-ms",
                        "2000",
                    ]
                )
            )
            source_rows = read_jsonl(source_output)
            rows, coverage = r2_paths.build_r2_market_paths(
                candidate_universe=candidates,
                account_update_paths=[],
                diag_account_update_paths=[],
                canonical_snapshot_paths=[source_output],
                target_net_pct=40,
                stop_net_pct=40,
                horizon_ms=60_000,
            )

        self.assertEqual(source_manifest["status"], "PASS")
        self.assertEqual(source_manifest["post_horizon_grace_ms"], 2_000)
        self.assertEqual(source_manifest["candidate_ok_rows"], 1)
        self.assertEqual(source_rows[0]["path_status"], "ok")
        self.assertTrue(source_rows[0]["horizon_matured"])
        self.assertEqual(source_rows[0]["post_horizon_grace_ms"], 2_000)
        self.assertEqual(rows[0]["r2_status"], "negative")
        self.assertEqual(rows[0]["r2_label"], "negative")
        self.assertEqual(rows[0]["r2_label_reason"], "no_target_by_horizon")
        self.assertEqual(coverage["status"], "PASS")

    def test_canonical_r2_source_fails_closed_without_matching_diag_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            candidates = root / "candidate_universe_v1.jsonl"
            diag_log = root / "system.log"
            source_output = root / "canonical_r2_source_v1.jsonl"
            write_jsonl(
                candidates,
                [
                    {
                        "candidate_id": "c1",
                        "base_mint": "mint1",
                        "pool_id": "curve1",
                        "bonding_curve": "curve1",
                        "decision_ts_ms": 1_000,
                    }
                ],
            )
            diag_log.write_text(
                (
                    "1970-01-01T00:00:01.000Z INFO "
                    "DIAG_ACCOUNT_UPDATE_RELAY base_mint=other bonding_curve=curve1 "
                    "slot=10 sol_reserves=1000000000 token_reserves=1000000 "
                    "complete=0 curve_finality=confirmed\n"
                ),
                encoding="utf-8",
            )

            source_manifest = canonical_r2.run(
                canonical_r2.build_parser().parse_args(
                    [
                        "--root",
                        str(root),
                        "--candidate-universe",
                        str(candidates),
                        "--diag-log",
                        str(diag_log),
                        "--output",
                        str(source_output),
                        "--horizon-ms",
                        "60000",
                    ]
                )
            )
            source_rows = read_jsonl(source_output)

        self.assertEqual(source_manifest["status"], "NO-GO/PENDING_R2_SOURCE")
        self.assertIn("no_candidate_matched_canonical_source", source_manifest["fail_reasons"])
        self.assertEqual(source_manifest["source_rows_written"], 0)
        self.assertEqual(source_rows, [])

    def write_simcov_fixture(
        self,
        root: Path,
        *,
        scope: str,
        buy_rows: list[dict],
        shadow_rows: list[dict],
    ) -> None:
        decision_dir = (
            root
            / "logs"
            / "rollout"
            / scope
            / "decisions"
            / scope
            / "v2.2"
            / "legacy_live"
            / "fixture"
        )
        shadow_dir = root / "logs" / "shadow_run" / scope
        decision_rows = [
            {
                **row,
                "log_schema_version": 25,
                "decision_plane": "legacy_live",
                "gatekeeper_version": "v2.2",
                "decision_verdict_buy": True,
                "verdict_type": "BUY",
            }
            for row in buy_rows
        ]
        write_jsonl(decision_dir / "gatekeeper_v2_decisions.jsonl", decision_rows)
        write_jsonl(decision_dir / "gatekeeper_v2_buys.jsonl", decision_rows)
        write_jsonl(root / "logs" / "shadow_run" / f"{scope}-buys.jsonl", shadow_rows)
        write_jsonl(shadow_dir / "shadow_entries.jsonl", shadow_rows)
        write_jsonl(shadow_dir / "shadow_lifecycle.jsonl", shadow_rows)
        rollout_dir = root / "logs" / "rollout" / scope
        rollout_dir.mkdir(parents=True, exist_ok=True)
        (rollout_dir / "system.log").write_text("", encoding="utf-8")
        (rollout_dir / "oracle.log").write_text("", encoding="utf-8")

    def run_simcov_audit(self, root: Path, scope: str) -> dict:
        return simcov_audit.build_audit(
            simcov_audit.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--decision-plane",
                    "legacy_live",
                    "--max-unknown-rate",
                    "1.0",
                ]
            )
        )

    def r18c_regression_fixture_dir(self) -> Path:
        return (
            Path(__file__).resolve().parents[1]
            / "tests"
            / "fixtures"
            / "selector"
            / "r18c_bcv2_handoff_regression"
        )

    def run_selector_regression_gate(self, audit_json: Path, jsonl: Path) -> dict:
        return selector_regression_gates.build_report(
            selector_regression_gates.build_parser().parse_args(
                [
                    "--scope",
                    "r18c-bcv2-handoff-regression-fixture",
                    "--root",
                    str(Path(__file__).resolve().parents[1]),
                    "--audit-json",
                    str(audit_json),
                    "--jsonl",
                    str(jsonl),
                    "--require-attempted-equals-buy",
                    "--require-not-executable-zero",
                    "--min-attempt-coverage",
                    "0.95",
                ]
            )
        )

    def test_selector_regression_gate_accepts_r18c_bcv2_handoff_fixture(self) -> None:
        fixture = self.r18c_regression_fixture_dir()

        report = self.run_selector_regression_gate(
            fixture / "audit_pass.json",
            fixture / "shadow_buys.jsonl",
        )

        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["metrics"]["buy_rows"], 2)
        self.assertEqual(report["metrics"]["attempted_rows"], 2)
        self.assertEqual(report["metrics"]["not_executable_route_rows"], 0)
        self.assertGreaterEqual(report["metrics"]["attempt_coverage"], 0.95)
        self.assertTrue(report["config_guard"]["normal_bonding_curve_load_required"])
        self.assertFalse(report["fail_reasons"])

    def test_selector_regression_gate_rejects_not_executable_r18c_fixture(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        audit = read_jsonl(fixture / "shadow_buys.jsonl")
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            audit_json = json.loads((fixture / "audit_pass.json").read_text(encoding="utf-8"))
            audit_json["metrics"]["shadow_simulation_attempted_rows"] = 1
            audit_json["metrics"]["not_executable_route_rows"] = 1
            audit_json["metrics"]["simulation_attempt_coverage"] = 0.5
            audit_path = tmp / "audit.json"
            audit_path.write_text(json.dumps(audit_json), encoding="utf-8")
            write_jsonl(tmp / "rows.jsonl", audit)

            report = self.run_selector_regression_gate(audit_path, tmp / "rows.jsonl")

        self.assertEqual(report["status"], "FAIL")
        self.assertIn("not_executable_route_rows_nonzero", report["fail_reasons"])
        self.assertIn("attempted_rows_not_equal_buy_rows", report["fail_reasons"])
        self.assertIn("attempt_coverage_below_minimum", report["fail_reasons"])

    def test_selector_regression_gate_operational_profile_allows_bounded_not_executable(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        rows = read_jsonl(fixture / "shadow_buys.jsonl")
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            audit_json = json.loads((fixture / "audit_pass.json").read_text(encoding="utf-8"))
            audit_json["metrics"]["buy_rows"] = 100
            audit_json["metrics"]["shadow_simulation_attempted_rows"] = 95
            audit_json["metrics"]["not_executable_route_rows"] = 5
            audit_json["metrics"]["simulation_attempt_coverage"] = 0.95
            audit_path = tmp / "audit.json"
            audit_path.write_text(json.dumps(audit_json), encoding="utf-8")
            write_jsonl(tmp / "rows.jsonl", rows)

            report = selector_regression_gates.build_report(
                selector_regression_gates.build_parser().parse_args(
                    [
                        "--scope",
                        "r19-operational-fixture",
                        "--root",
                        str(Path(__file__).resolve().parents[1]),
                        "--audit-json",
                        str(audit_path),
                        "--jsonl",
                        str(tmp / "rows.jsonl"),
                        "--gate-profile",
                        "operational",
                        "--min-attempt-coverage",
                        "0.95",
                        "--max-not-executable-rate",
                        "0.05",
                        "--max-unknown-unclassified",
                        "1",
                    ]
                )
            )

        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["metrics"]["gate_profile"], "operational")
        self.assertEqual(report["metrics"]["attempted_rows"], 95)
        self.assertEqual(report["metrics"]["not_executable_route_rows"], 5)
        self.assertFalse(report["fail_reasons"])

    def test_selector_regression_gate_rejects_selected_fallback_without_route_kind(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        rows = read_jsonl(fixture / "shadow_buys.jsonl")
        rows[0]["selected_route_kind"] = None
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            write_jsonl(tmp / "rows.jsonl", rows)

            report = self.run_selector_regression_gate(
                fixture / "audit_pass.json",
                tmp / "rows.jsonl",
            )

        self.assertEqual(report["status"], "FAIL")
        self.assertIn(
            "forbidden_marker_present:selected_route_kind=None for selected_fallback_route_execution_handoff",
            report["fail_reasons"],
        )

    def test_selector_regression_gate_rejects_stale_bcv2_reason_when_fatal(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        rows = read_jsonl(fixture / "shadow_buys.jsonl")
        rows[0]["fatal_reasons_after_final_manifest_validation"] = [
            "primary_route_bcv2_missing"
        ]
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            write_jsonl(tmp / "rows.jsonl", rows)

            report = self.run_selector_regression_gate(
                fixture / "audit_pass.json",
                tmp / "rows.jsonl",
            )

        self.assertEqual(report["status"], "FAIL")
        self.assertIn(
            "forbidden_marker_present:primary_route_bcv2_missing fatal after final handoff",
            report["fail_reasons"],
        )

    def test_selector_regression_gate_rejects_bcv2_missing_rpc_precheck(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        rows = read_jsonl(fixture / "shadow_buys.jsonl")
        rows[0]["simulation_account_manifest"][1][
            "precheck_rpc_load_status"
        ] = "missing_on_rpc_precheck"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            write_jsonl(tmp / "rows.jsonl", rows)

            report = self.run_selector_regression_gate(
                fixture / "audit_pass.json",
                tmp / "rows.jsonl",
            )

        self.assertEqual(report["status"], "FAIL")
        self.assertIn(
            "forbidden_marker_present:missing_on_rpc_precheck for bonding_curve_v2",
            report["fail_reasons"],
        )

    def test_selector_regression_gate_rejects_meta_only_on_normal_bonding_curve(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        rows = read_jsonl(fixture / "shadow_buys.jsonl")
        rows[0]["simulation_account_manifest"][0][
            "precheck_rpc_load_status"
        ] = "BCV2_LOAD_NOT_REQUIRED"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            write_jsonl(tmp / "rows.jsonl", rows)

            report = self.run_selector_regression_gate(
                fixture / "audit_pass.json",
                tmp / "rows.jsonl",
            )

        self.assertEqual(report["status"], "FAIL")
        self.assertFalse(report["config_guard"]["normal_bonding_curve_load_required"])
        self.assertIn(
            "forbidden_marker_present:BCV2 meta-only applied to normal bonding_curve",
            report["fail_reasons"],
        )

    def test_selector_regression_gate_rejects_can_unlock_execution(self) -> None:
        fixture = self.r18c_regression_fixture_dir()
        rows = read_jsonl(fixture / "shadow_buys.jsonl")
        rows[0]["can_unlock_execution"] = True
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            write_jsonl(tmp / "rows.jsonl", rows)

            report = self.run_selector_regression_gate(
                fixture / "audit_pass.json",
                tmp / "rows.jsonl",
            )

        self.assertEqual(report["status"], "FAIL")
        self.assertIn(
            "forbidden_marker_present:can_unlock_execution=true",
            report["fail_reasons"],
        )

    def run_route_evidence_join_report(self, root: Path, scope: str) -> dict:
        return route_evidence_join.build_report(
            route_evidence_join.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--decision-plane",
                    "legacy_live",
                ]
            )
        )

    def run_route_manifest_reuse_projection(self, root: Path, scope: str) -> dict:
        return route_manifest_reuse.build_report(
            route_manifest_reuse.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--decision-plane",
                    "legacy_live",
                    "--raw-transaction-evidence-glob",
                    f"datasets/events/{scope}/raw_route_evidence.jsonl",
                ]
            )
        )

    def run_coverage_breakthrough_projection(self, root: Path, scope: str) -> dict:
        if coverage_breakthrough is None:
            self.skipTest("build_selector_coverage_breakthrough_projection.py is not tracked")
        return coverage_breakthrough.build_report(
            coverage_breakthrough.build_parser().parse_args(
                [
                    "--scope",
                    scope,
                    "--root",
                    str(root),
                    "--decision-plane",
                    "legacy_live",
                    "--raw-transaction-evidence-glob",
                    f"datasets/events/{scope}/raw_route_evidence.jsonl",
                ]
            )
        )

    def write_route_evidence_candidates(self, root: Path, scope: str, rows: list[dict]) -> None:
        write_jsonl(
            root / "logs" / "nln_capture" / scope / "route_manifest_evidence_candidates_v1.jsonl",
            rows,
        )

    def write_raw_route_evidence(self, root: Path, scope: str, rows: list[dict]) -> None:
        write_jsonl(root / "datasets" / "events" / scope / "raw_route_evidence.jsonl", rows)

    def write_program_stream_raw(self, root: Path, scope: str, *, topic_file: str, rows: list[dict]) -> None:
        write_jsonl(root / "logs" / "nln_capture" / scope / topic_file, rows)

    def route_evidence_candidate(
        self,
        *,
        signature: str | None = "sig1",
        slot: int | None = 10,
        ix_index: int | None = 2,
        remaining_accounts: list[str] | None = None,
        account_manifest_hash: str = "manifest1",
    ) -> dict:
        if remaining_accounts is None:
            remaining_accounts = ["buyback_fee", "buyback_quote"]
        return {
            "artifact": "route_manifest_evidence_candidate_v1",
            "parse_status": "OK",
            "topic": "solana.pump_fun.buy",
            "route_kind": "legacy_buy",
            "signature": signature,
            "slot": slot,
            "ix_index": ix_index,
            "tx_index": None,
            "account_manifest_hash": account_manifest_hash,
            "instruction_evidence_hash": "instruction1",
            "remaining_accounts_count": len(remaining_accounts),
            "remaining_accounts": [
                {"index": index, "pubkey": value}
                for index, value in enumerate(remaining_accounts)
            ],
            "has_legacy_tail": len(remaining_accounts) == 2,
            "can_unlock_execution": False,
            "named_accounts": [
                {"role": "global", "pubkey": "global1"},
                {"role": "mint", "pubkey": "mint1"},
                {"role": "bonding_curve", "pubkey": "pool1"},
                {"role": "associated_bonding_curve", "pubkey": "abc1"},
                {"role": "user", "pubkey": "user1"},
                {"role": "program", "pubkey": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"},
            ],
        }

    def raw_route_evidence(
        self,
        *,
        signature: str = "sig1",
        slot: int = 10,
        ix_index: int = 2,
        associated_bonding_curve: str = "abc1",
        remaining_accounts: list[str] | None = None,
        resolver_validation_status: str = "PASS",
    ) -> dict:
        if remaining_accounts is None:
            remaining_accounts = ["buyback_fee", "buyback_quote"]
        ordered_accounts = [
            "global1",
            "mint1",
            "pool1",
            associated_bonding_curve,
            "user1",
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            *remaining_accounts,
        ]
        account_keys = ["unused0", *ordered_accounts]
        return {
            "artifact": "raw_pumpfun_instruction_evidence_v1",
            "signature": signature,
            "slot": slot,
            "tx_index": None,
            "ix_index": ix_index,
            "route_kind": "legacy_buy",
            "mint": "mint1",
            "program_id": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            "account_manifest_hash": "manifest1",
            "account_keys": account_keys,
            "compiled_instruction_account_indices": list(range(1, len(account_keys))),
            "remaining_accounts": remaining_accounts,
            "remaining_accounts_count": len(remaining_accounts),
            "has_legacy_tail": len(remaining_accounts) == 2,
            "resolver_validation_status": resolver_validation_status,
            "can_unlock_execution": False,
            "named_accounts": [
                {"role": "global", "pubkey": "global1"},
                {"role": "mint", "pubkey": "mint1"},
                {"role": "bonding_curve", "pubkey": "pool1"},
                {"role": "associated_bonding_curve", "pubkey": associated_bonding_curve},
                {"role": "user", "pubkey": "user1"},
                {"role": "token_program", "pubkey": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"},
                {"role": "program", "pubkey": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"},
            ],
        }

    def test_route_evidence_join_complete_projects_attempt_without_success_claim(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-evidence-complete"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "signature": "sig1",
                "slot": 10,
                "ix_index": 2,
                "route_kind": "legacy_buy",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "signature": "sig1",
                "slot": 10,
                "ix_index": 2,
                "route_kind": "legacy_buy",
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_route_evidence_candidates(root, scope, [self.route_evidence_candidate()])
            self.write_raw_route_evidence(root, scope, [self.raw_route_evidence()])

            report = self.run_route_evidence_join_report(root, scope)
            joined = read_jsonl(Path(report["outputs"]["joined"]))
            blocker_rows = read_jsonl(Path(report["outputs"]["blocker_table"]))

        self.assertEqual(report["join_evidence"]["complete_rows"], 1)
        self.assertEqual(joined[0]["status"], "complete")
        self.assertFalse(joined[0]["can_unlock_execution"])
        self.assertEqual(report["baseline"]["buy_rows"], 1)
        self.assertEqual(report["buy_blocker_rows"], 1)
        self.assertEqual(report["baseline"]["shadow_simulation_attempted_rows"], 0)
        self.assertEqual(report["evidence_enabled"]["shadow_simulation_attempted_rows"], 1)
        self.assertEqual(report["evidence_enabled"]["shadow_simulation_success_rows"], 0)
        self.assertTrue(
            report["projection_meta"]["success_rows_not_projected_without_runtime_simulation"]
        )
        self.assertEqual(
            report["evidence_enabled"]["simulation_attempt_coverage"]["display"],
            "1 / 1 = 100.00%",
        )
        self.assertEqual(blocker_rows[0]["exact_blocker_reason"], "not_executable_route:ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING:complete_join_available_offline_only")

    def test_route_evidence_join_outlier_tail_len_3_missing_join_key_is_pending(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-evidence-tail3"
            self.write_simcov_fixture(root, scope=scope, buy_rows=[], shadow_rows=[])
            self.write_route_evidence_candidates(
                root,
                scope,
                [
                    self.route_evidence_candidate(
                        signature=None,
                        slot=None,
                        ix_index=None,
                        remaining_accounts=["tail1", "tail2", "tail3"],
                    )
                ],
            )
            report = self.run_route_evidence_join_report(root, scope)
            joined = read_jsonl(Path(report["outputs"]["joined"]))
            outliers = read_jsonl(Path(report["outputs"]["outliers"]))

        self.assertEqual(joined[0]["status"], "pending_join")
        self.assertIn("tail_len_3", joined[0]["taxonomy"])
        self.assertIn("missing_signature", joined[0]["taxonomy"])
        self.assertIn("missing_ix_index", joined[0]["taxonomy"])
        self.assertEqual(outliers[0]["tail_class"], "tail_len_3")
        self.assertEqual(outliers[0]["raw_gRPC_match_status"], "missing_join_key")

    def test_route_evidence_join_key_audit_confirms_absence_despite_remaining_index(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-evidence-join-key-audit"
            self.write_simcov_fixture(root, scope=scope, buy_rows=[], shadow_rows=[])
            self.write_route_evidence_candidates(root, scope, [])
            self.write_program_stream_raw(
                root,
                scope,
                topic_file="nln_pumpfun_buy_raw_v1.jsonl",
                rows=[
                    {
                        "artifact": "nln_program_stream_raw_v1",
                        "payload": {
                            "accounts": {
                                "remaining_accounts": [
                                    {"index": 0, "pubkey": "tail1"},
                                    {"index": 1, "pubkey": "tail2"},
                                ]
                            }
                        },
                    }
                ],
            )
            report = self.run_route_evidence_join_report(root, scope)
            audit = report["program_stream_join_key_audit"]

        self.assertTrue(audit["PROGRAM_STREAM_JOIN_KEY_ABSENT_CONFIRMED"])
        self.assertEqual(audit["field_group_counts"]["signature_like"], 0)
        self.assertEqual(audit["field_group_counts"]["slot_like"], 0)
        self.assertEqual(audit["field_group_counts"]["tx_index_like"], 0)
        self.assertEqual(audit["field_group_counts"]["ix_index_like"], 0)
        self.assertGreater(audit["field_group_counts"]["generic_index_ambiguous"], 0)

    def test_route_evidence_historical_comparison_uses_attempt_rate_not_dispatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-evidence-historical"
            old_scope = "old-93-attempt-coverage"
            self.write_simcov_fixture(root, scope=scope, buy_rows=[], shadow_rows=[])
            self.write_route_evidence_candidates(root, scope, [])
            common.write_json(
                root / "reports" / "selector" / old_scope / "buy_simulation_coverage_audit_v1.json",
                {
                    "artifact": "buy_simulation_coverage_audit_v1",
                    "scope": old_scope,
                    "metrics": {
                        "buy_rows": 100,
                        "shadow_dispatch_rows": 100,
                        "shadow_simulated_rows": 80,
                        "simulation_attempt_coverage": 0.93,
                        "simulation_success_coverage": 0.80,
                        "simulation_failed_rows": 13,
                        "not_executable_route_rows": 7,
                    },
                },
            )

            report = self.run_route_evidence_join_report(root, scope)
            historical = report["historical_coverage_comparison"]

        self.assertEqual(historical["old_93_coverage_claim_status"], "CONFIRMED_IN_LOCAL_ARTIFACTS")
        matching = historical["old_93_matching_rows"]
        self.assertEqual(len(matching), 1)
        self.assertEqual(matching[0]["scope"], old_scope)
        self.assertEqual(matching[0]["attempted_rows"], 93)
        self.assertEqual(matching[0]["attempt_coverage"]["display"], "93 / 100 = 93.00%")

    def test_route_evidence_join_conflict_emits_field_level_diff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-evidence-conflict"
            self.write_simcov_fixture(root, scope=scope, buy_rows=[], shadow_rows=[])
            self.write_route_evidence_candidates(root, scope, [self.route_evidence_candidate()])
            self.write_raw_route_evidence(
                root,
                scope,
                [self.raw_route_evidence(associated_bonding_curve="different_abc")],
            )
            report = self.run_route_evidence_join_report(root, scope)
            joined = read_jsonl(Path(report["outputs"]["joined"]))
            outliers = read_jsonl(Path(report["outputs"]["outliers"]))

        self.assertEqual(report["join_evidence"]["status_counts"]["conflicted"], 1)
        self.assertEqual(joined[0]["status"], "conflicted")
        self.assertEqual(joined[0]["conflict_field"], "associated_bonding_curve")
        self.assertEqual(outliers[0]["program_stream_value"], "abc1")
        self.assertEqual(outliers[0]["raw_gRPC_value"], "different_abc")

    def test_route_manifest_reuse_projects_tail_recovery_without_unlock(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-manifest-reuse-tail"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 2000,
                "rpc_slot": 20,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "fallback_route_kind": "legacy_buy",
                "fallback_route_attempted": True,
                "legacy_buy_curve_pubkey": "pool1",
                "legacy_buy_associated_bonding_curve_pubkey": "abc1",
                "selected_route_account_set_roles": [
                    "bonding_curve:pool1:account_state_core",
                    "associated_bonding_curve:abc1:account_overrides",
                    "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
                ],
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_raw_route_evidence(root, scope, [self.raw_route_evidence(slot=10)])

            report = self.run_route_manifest_reuse_projection(root, scope)
            rows = read_jsonl(Path(report["outputs"]["projection_rows"]))
            store = read_jsonl(Path(report["outputs"]["manifest_store"]))

        self.assertEqual(report["baseline"]["buy_rows"], 1)
        self.assertEqual(report["manifest_store"]["clean_manifest_rows"], 1)
        self.assertEqual(report["projection"]["not_executable_rows_matched_by_manifest"], 1)
        self.assertEqual(report["projection"]["LEGACY_TAIL_MISSING_rows_recoverable"], 1)
        self.assertEqual(report["projection"]["projected_attempt_coverage"]["display"], "1 / 1 = 100.00%")
        self.assertEqual(rows[0]["manifest_lookup_status"], "exact_pool_route_manifest_found")
        self.assertEqual(rows[0]["projected_attempt_status"], "would_be_route_materializable_offline")
        self.assertTrue(rows[0]["recoverable_by_manifest"])
        self.assertFalse(rows[0]["can_unlock_execution"])
        self.assertFalse(store[0]["can_unlock_execution"])
        self.assertFalse(report["claim_boundaries"]["observed_manifest_store_can_unlock_execution"])

    def test_route_manifest_reuse_keeps_state_readiness_as_separate_blocker(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "route-manifest-reuse-state"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 2000,
                "rpc_slot": 20,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "primary_route_kind": "legacy_buy",
                "legacy_buy_curve_pubkey": "pool1",
                "legacy_buy_associated_bonding_curve_pubkey": "abc1",
                "selected_route_account_set_roles": [
                    "bonding_curve:pool1:account_state_core",
                    "associated_bonding_curve:abc1:account_overrides",
                    "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
                ],
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_simulation_load_not_ready:bonding_curve:pool1"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_raw_route_evidence(root, scope, [self.raw_route_evidence(slot=10)])
            (root / "logs" / "rollout" / scope / "system.log").write_text(
                "DIAG_ACCOUNT_UPDATE_RELAY pool1\n",
                encoding="utf-8",
            )

            report = self.run_route_manifest_reuse_projection(root, scope)
            rows = read_jsonl(Path(report["outputs"]["projection_rows"]))
            state_rows = read_jsonl(Path(report["outputs"]["state_readiness_audit"]))

        self.assertEqual(report["baseline"]["root_cause_counts"]["ROUTE_INCOMPLETE_STATE_NOT_READY"], 1)
        self.assertEqual(report["projection"]["not_executable_rows_matched_by_manifest"], 0)
        self.assertEqual(report["projection"]["STATE_NOT_READY_rows_recoverable"], 0)
        self.assertEqual(report["projection"]["rows_blocked_by_state_readiness"], 1)
        self.assertEqual(rows[0]["manifest_lookup_status"], "exact_pool_route_manifest_found")
        self.assertEqual(rows[0]["projected_attempt_status"], "blocked_by_state_readiness")
        self.assertFalse(rows[0]["recoverable_by_manifest"])
        self.assertEqual(report["state_readiness"]["state_rows_with_diag_update"], 1)
        self.assertEqual(state_rows[0]["state_readiness_diagnosis"], "diag_update_seen_but_timing_unverified")

    def test_coverage_breakthrough_projects_state_hold_windows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "coverage-breakthrough-state"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 1780824550000,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "primary_route_kind": "legacy_buy",
                "legacy_buy_curve_pubkey": "pool1",
                "legacy_buy_associated_bonding_curve_pubkey": "abc1",
                "selected_route_account_set_roles": [
                    "bonding_curve:pool1:account_state_core",
                    "associated_bonding_curve:abc1:account_overrides",
                    "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
                ],
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_simulation_load_not_ready:bonding_curve:pool1"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_raw_route_evidence(root, scope, [self.raw_route_evidence(slot=10)])
            (root / "logs" / "rollout" / scope / "system.log").write_text(
                "2026-06-07T09:29:10.040Z INFO DIAG_ACCOUNT_UPDATE_RELAY "
                "base_mint=mint1 bonding_curve=pool1 slot=20 sol_reserves=1 token_reserves=2 complete=0\n",
                encoding="utf-8",
            )

            report = self.run_coverage_breakthrough_projection(root, scope)
            state_rows = read_jsonl(Path(report["outputs"]["state_rows"]))

        self.assertEqual(report["state_projection"]["state_not_ready_rows"], 1)
        self.assertEqual(
            state_rows[0]["projected_recoverability"],
            "STATE_UPDATE_AFTER_DECISION_WITHIN_50MS",
        )
        self.assertFalse(state_rows[0]["recoverable_with_hold_ms"]["25"])
        self.assertTrue(state_rows[0]["recoverable_with_hold_ms"]["50"])
        self.assertEqual(report["state_projection"]["hold_windows"]["25ms"]["recovered_rows"], 0)
        self.assertEqual(report["state_projection"]["hold_windows"]["50ms"]["recovered_rows"], 1)
        self.assertEqual(report["combined_projection"]["hold_windows"]["50ms"]["attempted"]["display"], "1 / 1 = 100.00%")
        self.assertEqual(report["claim_boundaries"]["can_unlock_execution_true_rows"], 0)

    def test_coverage_breakthrough_role_split_ignores_user_derived_conflict(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "coverage-breakthrough-role-split"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 1780824550000,
                "rpc_slot": 30,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "fallback_route_kind": "legacy_buy",
                "legacy_buy_curve_pubkey": "pool1",
                "legacy_buy_associated_bonding_curve_pubkey": "abc1",
                "selected_route_account_set_roles": [
                    "bonding_curve:pool1:account_state_core",
                    "associated_bonding_curve:abc1:account_overrides",
                    "token_program:TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb:token_program",
                ],
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
                ),
            }
            raw_a = self.raw_route_evidence(signature="sig1", slot=10)
            raw_b = self.raw_route_evidence(signature="sig2", slot=11)
            raw_a["named_accounts"].append({"role": "user_volume_accumulator", "pubkey": "uva1"})
            raw_b["named_accounts"].append({"role": "user_volume_accumulator", "pubkey": "uva2"})
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            self.write_raw_route_evidence(root, scope, [raw_a, raw_b])

            report = self.run_coverage_breakthrough_projection(root, scope)
            conflict_rows = read_jsonl(Path(report["outputs"]["conflict_rows"]))

        self.assertEqual(report["baseline"]["projected_after_R14_attempted"]["display"], "0 / 1 = 0.00%")
        self.assertEqual(report["conflict_role_split"]["conflict_rows_before_role_split"], 1)
        self.assertEqual(report["conflict_role_split"]["blocked_by_conflict_recoverable_after_role_split"], 1)
        self.assertEqual(report["conflict_role_split"]["attempted_after_role_split"]["display"], "1 / 1 = 100.00%")
        self.assertEqual(conflict_rows[0]["role_split_lookup_status"], "pool_static_manifest_found")
        self.assertTrue(conflict_rows[0]["recoverable_after_role_split"])
        self.assertFalse(conflict_rows[0]["can_unlock_execution"])

    def test_buy_simulation_audit_classifies_not_executable_legacy_tail_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-legacy-tail"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 1000,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(report["metrics"]["buy_rows"], 1)
        self.assertEqual(report["metrics"]["not_executable_route_rows"], 1)
        self.assertEqual(
            report["failure_classes"]["ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING"]["count"],
            1,
        )
        self.assertEqual(samples[0]["classification"], "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING")
        self.assertEqual(samples[0]["legacy_buy_remaining_account_count"], 0)

    def test_buy_simulation_audit_reports_route_manifest_cache_lookup_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-route-cache-status"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 1000,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            rollout_dir = root / "logs" / "rollout" / scope
            (rollout_dir / "system.log").write_text(
                "INFO pool=pool1 base_mint=mint1 phase=after_wait "
                "manifest_cache_lookup_status=ROUTE_CACHE_MISS_NO_PRIOR_MANIFEST "
                "manifest_cache_candidate_count=0 prior_complete_legacy_manifest_age_ms=0 "
                "has_prior_complete_legacy_manifest_in_session=false "
                "route_account_manifest_source=missing_observed_legacy_manifest "
                "ACTIVE_BUY_ROUTE_MANIFEST_CACHE_LOOKUP\n",
                encoding="utf-8",
            )
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(
            report["route_manifest_cache"]["classes"]["ROUTE_CACHE_MISS_NO_PRIOR_MANIFEST"]["count"],
            1,
        )
        self.assertEqual(
            samples[0]["manifest_cache_lookup_status"],
            "ROUTE_CACHE_MISS_NO_PRIOR_MANIFEST",
        )
        self.assertEqual(samples[0]["manifest_cache_candidate_count"], 0)

    def test_buy_simulation_audit_route_missing_with_rpc_status_not_provider(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-route-rpc-status"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 1000,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "legacy_buy_curve_rpc_load_status": "present_on_rpc_precheck",
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(
            report["failure_classes"]["ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING"]["count"],
            1,
        )
        self.assertEqual(report["failure_classes"].get("SIM_FAIL_PROVIDER", {}).get("count", 0), 0)
        self.assertEqual(samples[0]["classification"], "ROUTE_INCOMPLETE_LEGACY_TAIL_MISSING")

    def test_buy_simulation_audit_classifies_bcv2_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-bcv2"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "decision_ts_ms": 1000,
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "primary_route_bcv2_missing:bonding_curve_v2:bcv2"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(
            report["failure_classes"]["ROUTE_INCOMPLETE_BCV2_MISSING"]["count"],
            1,
        )
        self.assertTrue(samples[0]["primary_route_bcv2_missing"])

    def test_buy_simulation_audit_classifies_legacy_curve_load_not_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-load-not-ready"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_simulation_load_not_ready:bonding_curve:pool1"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(
            report["failure_classes"]["ROUTE_INCOMPLETE_STATE_NOT_READY"]["count"],
            1,
        )
        self.assertEqual(report["failure_classes"]["UNKNOWN_UNCLASSIFIED"]["count"], 0)
        self.assertEqual(samples[0]["classification"], "ROUTE_INCOMPLETE_STATE_NOT_READY")

    def test_buy_simulation_audit_requires_latch_marker_for_state_not_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-load-not-ready-no-latch-marker"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_simulation_load_not_ready:bonding_curve:pool1"
                ),
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)

        self.assertIn("STATE_LATCH_MARKER_MISSING_FOR_STATE_NOT_READY", report["fail_reasons"])
        self.assertEqual(report["state_latch_contract"]["state_not_ready_rows"], 1)
        self.assertEqual(
            report["state_latch_contract"]["state_not_ready_latch_marker_missing_rows"],
            1,
        )
        self.assertEqual(report["state_latch_contract"]["contract_status"], "FAIL")

    def test_buy_simulation_audit_accepts_latch_marker_for_state_not_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-load-not-ready-with-latch-marker"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_unknown_error",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "dispatch_status": "not_dispatched",
                "simulation_outcome": "not_attempted",
                "execution_feasibility_status": "not_executable_route",
                "route_resolution_status": "no_executable_route_account_set",
                "dispatch_attempted": False,
                "simulation_attempted": False,
                "precheck_failure_reason": (
                    "no_executable_route_account_set:"
                    "legacy_buy_simulation_load_not_ready:bonding_curve:pool1"
                ),
                "state_latch_eligibility_marker": "STATE_LATCH_ELIGIBILITY_CHECKED",
                "state_latch_eligibility_checked": True,
                "state_latch_attempted": False,
                "state_latch_outcome": "STATE_LATCH_SKIPPED_BONDING_CURVE_MISSING",
                "state_latch_skip_reason": "STATE_LATCH_SKIPPED_BONDING_CURVE_MISSING",
                "state_latch_eligible": False,
                "can_unlock_execution": False,
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)

        self.assertNotIn(
            "STATE_LATCH_MARKER_MISSING_FOR_STATE_NOT_READY",
            report["fail_reasons"],
        )
        self.assertEqual(report["state_latch_contract"]["state_not_ready_rows"], 1)
        self.assertEqual(
            report["state_latch_contract"]["state_latch_eligibility_checked_rows"],
            1,
        )
        self.assertEqual(report["state_latch_contract"]["state_latch_skipped_rows"], 1)
        self.assertEqual(report["state_latch_contract"]["contract_status"], "PASS")

    def test_buy_simulation_audit_classifies_custom_program_errors(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-custom"
            buys = []
            shadows = []
            for code in ("2006", "6024", "6002"):
                buys.append(
                    {
                        "pool_id": f"pool{code}",
                        "base_mint": f"mint{code}",
                        "ab_record_id": f"pool{code}:mint{code}:BUY",
                        "shadow_execution_outcome": "shadow_simulation_error",
                    }
                )
                shadows.append(
                    {
                        "record_type": "shadow_dispatch",
                        "pool_id": f"pool{code}",
                        "mint_id": f"mint{code}",
                        "ab_record_id": f"pool{code}:mint{code}:BUY",
                        "candidate_id": f"mint{code}_pool{code}_1000",
                        "decision_plane": "legacy_live",
                        "dispatch_status": "failed",
                        "simulation_outcome": "failed",
                        "dispatch_attempted": True,
                        "simulation_attempted": True,
                        "execution_feasibility_status": "executable",
                        "route_resolution_status": "primary_route_ready",
                        "err": f"InstructionError(3, Custom({code}))",
                        "logs_excerpt": [f"custom {code}"],
                        "retry_count": 0,
                        "payer_provenance": "configured",
                        "simulation_account_manifest": [{"role": "bonding_curve"}],
                    }
                )
            self.write_simcov_fixture(root, scope=scope, buy_rows=buys, shadow_rows=shadows)
            report = self.run_simcov_audit(root, scope)

        self.assertEqual(report["failure_classes"]["SIM_FAIL_CUSTOM_2006"]["count"], 1)
        self.assertEqual(report["failure_classes"]["SIM_FAIL_CUSTOM_6024"]["count"], 1)
        self.assertEqual(report["failure_classes"]["SIM_FAIL_CUSTOM_6002"]["count"], 1)

    def test_buy_simulation_audit_ignores_feature_timeout_for_provider_failure(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-provider"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_transport_error",
                "iwim_fetch_status": "TIMEOUT",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "dispatch_status": "failed",
                "simulation_outcome": "failed",
                "dispatch_attempted": True,
                "simulation_attempted": True,
                "execution_feasibility_status": "executable",
                "route_resolution_status": "primary_route_ready",
                "err": "Failed to fetch payer account: HTTP status client error (429 Too Many Requests)",
                "error_class": "network_provider_problem",
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(report["failure_classes"]["SIM_FAIL_PROVIDER"]["count"], 1)
        self.assertEqual(report["failure_classes"].get("SIM_FAIL_TIMEOUT", {}).get("count", 0), 0)
        self.assertEqual(samples[0]["classification"], "SIM_FAIL_PROVIDER")

    def test_buy_simulation_audit_classifies_position_limit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scope = "simcov-position-limit"
            buy = {
                "pool_id": "pool1",
                "base_mint": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "shadow_execution_outcome": "shadow_position_limit_reached",
            }
            shadow = {
                "record_type": "shadow_dispatch",
                "pool_id": "pool1",
                "mint_id": "mint1",
                "ab_record_id": "pool1:mint1:BUY",
                "candidate_id": "mint1_pool1_1000",
                "decision_plane": "legacy_live",
                "dispatch_status": "failed",
                "simulation_outcome": "failed",
                "dispatch_attempted": True,
                "simulation_attempted": True,
                "execution_feasibility_status": "executable",
                "route_resolution_status": "primary_route_ready",
                "err": "Max concurrent positions reached: active=5, max=5",
            }
            self.write_simcov_fixture(root, scope=scope, buy_rows=[buy], shadow_rows=[shadow])
            report = self.run_simcov_audit(root, scope)
            samples = read_jsonl(Path(report["outputs"]["samples"]))

        self.assertEqual(report["metrics"]["position_limit_rows"], 1)
        self.assertEqual(report["failure_classes"]["POSITION_LIMIT_REACHED"]["count"], 1)
        self.assertEqual(samples[0]["active_positions"], 5)
        self.assertEqual(samples[0]["max_concurrent_positions"], 5)


if __name__ == "__main__":
    unittest.main()
