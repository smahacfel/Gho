import argparse
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_shadow_lifecycle_labeler as labeler


def args() -> argparse.Namespace:
    return argparse.Namespace(
        entry_truth_gap_clean_ms=1500,
        entry_truth_gap_degraded_acceptable_ms=10000,
        exit_truth_gap_clean_ms=5000,
        exit_truth_gap_timestop_acceptable_ms=45000,
        exit_truth_gap_other_acceptable_ms=15000,
        entry_drift_acceptable_abs_pct=15.0,
        exit_drift_acceptable_abs_pct=5.0,
    )


def report_row(
    *,
    final_pnl_pct: float,
    close_reason: str = "Target",
    entry_gap_ms: int = 500,
    exit_gap_ms: int = 1000,
    finality: str = "speculative",
    gatekeeper_context: bool = True,
) -> dict:
    return {
        "analysis_status": "ok",
        "truth_dataset_kind": "shadow_burnin_lifecycle_onchain",
        "collection_plane": "active_shadow",
        "execution_verification_class_hint": "shadow_onchain_speculative_snapshot_verified",
        "candidate_id": "mint_pool_1000",
        "position_id": "pool:mint:1000",
        "mint_id": "mint",
        "pool_id": "pool",
        "ab_record_id": "ab-1",
        "v3_feature_snapshot_hash": "mfs-hash",
        "v3_policy_config_hash": "policy-hash",
        "decision_plane": "v25_shadow",
        "rollout_namespace": "unit-ns",
        "close_reason": close_reason,
        "truth_status": "resolved",
        "truth_source": "canonical_account_state_snapshot",
        "sample_price_state": "Valid",
        "timing": {
            "decision_ts_ms": 1000,
            "entry_execution_ts_ms": 1200,
            "close_ts_ms": 4200,
            "position_duration_ms": 3000,
            "decision_to_execution_ms": 200,
            "detection_to_execution_ms": 8200,
            "gatekeeper_buy_context_found": gatekeeper_context,
        },
        "shadow": {
            "execution_outcome": "shadow_simulated",
            "final_pnl_sol": final_pnl_pct / 100.0,
            "final_pnl_pct": final_pnl_pct,
            "gross_pnl_sol": final_pnl_pct / 100.0,
            "net_pnl_sol": final_pnl_pct / 100.0,
            "estimated_costs_sol": 0.0,
            "total_exits": 1,
        },
        "onchain": {
            "entry": {"match_delta_ms": entry_gap_ms, "curve_finality": finality},
            "exit": {"max_abs_truth_gap_ms": exit_gap_ms},
        },
        "drift_pct": {
            "entry_vs_onchain_executable": 0.5,
            "exit_vs_onchain_executable": 0.0,
            "entry_vs_onchain_spot": 0.8,
            "exit_vs_onchain_spot": 0.0,
        },
        "exit_fills": [
            {
                "onchain_match_delta_ms": exit_gap_ms,
                "onchain_curve_finality": finality,
            }
        ],
    }


class P37ShadowLifecycleLabelerTests(unittest.TestCase):
    def test_speculative_positive_is_dirty_good_not_clean_good(self) -> None:
        row = report_row(final_pnl_pct=25.0, finality="speculative")

        label = labeler.build_label(row, args())

        self.assertEqual(label["market_outcome_class"], "market_good_clean")
        self.assertEqual(
            label["execution_verification_class"],
            "shadow_onchain_speculative_snapshot_verified",
        )
        self.assertEqual(label["buy_quality_class"], "buy_quality_dirty_good")
        self.assertIn("speculative_curve_finality", label["degraded_reasons"])

    def test_finalized_positive_clean_context_can_be_good(self) -> None:
        row = report_row(final_pnl_pct=10.0, finality="finalized")

        label = labeler.build_label(row, args())

        self.assertEqual(label["execution_verification_class"], "shadow_onchain_finalized_verified")
        self.assertEqual(label["truth_gap_class"], "truth_gap_clean")
        self.assertEqual(label["buy_quality_class"], "buy_quality_good")

    def test_negative_resolved_row_is_buy_quality_bad(self) -> None:
        row = report_row(final_pnl_pct=-12.0, finality="speculative", gatekeeper_context=False)

        label = labeler.build_label(row, args())

        self.assertEqual(label["market_outcome_class"], "market_bad_clean")
        self.assertEqual(label["buy_quality_class"], "buy_quality_bad")
        self.assertIn("missing_gatekeeper_buy_context", label["degraded_reasons"])

    def test_no_executable_route_is_not_buy_quality_bad(self) -> None:
        row = report_row(final_pnl_pct=-12.0, finality="speculative", gatekeeper_context=False)
        row["route_resolution_status"] = "no_executable_route_account_set"
        row["execution_feasibility_status"] = "not_executable_route"
        row["execution_feasibility_reason"] = "no_executable_route_account_set"
        row["route_resolution_terminal_reason"] = "no_executable_route_account_set"
        row["lifecycle_label_eligibility"] = "not_lifecycle_label_eligible"
        row["shadow"]["execution_outcome"] = "no_executable_route_account_set"

        label = labeler.build_label(row, args())

        self.assertEqual(label["market_outcome_class"], "market_bad_clean")
        self.assertEqual(label["execution_verification_class"], "shadow_execution_infeasible")
        self.assertEqual(label["buy_quality_class"], "buy_quality_not_executable")
        self.assertEqual(label["label_quality"], "not_executable")
        self.assertEqual(label["execution_feasibility_status"], "not_executable_route")
        self.assertEqual(label["execution_feasibility_reason"], "no_executable_route_account_set")
        summary = labeler.build_summary(
            [label],
            source_path=Path("shadow_onchain.jsonl"),
            output_path=Path("labels.jsonl"),
            args=args(),
        )
        self.assertEqual(summary["execution_feasibility_reject_rows"], 1)
        self.assertEqual(summary["buy_quality_denominator_rows"], 0)
        self.assertEqual(
            summary["execution_feasibility_status_counts"],
            {"not_executable_route": 1},
        )

    def test_timestop_exit_gap_can_be_degraded_acceptable(self) -> None:
        row = report_row(
            final_pnl_pct=1.0,
            close_reason="TimeStop",
            entry_gap_ms=900,
            exit_gap_ms=30107,
            finality="speculative",
        )

        label = labeler.build_label(row, args())

        self.assertEqual(label["entry_truth_gap_class"], "truth_gap_clean")
        self.assertEqual(label["exit_truth_gap_class"], "truth_gap_degraded_acceptable")
        self.assertEqual(label["truth_gap_class"], "truth_gap_degraded_acceptable")
        self.assertEqual(label["buy_quality_class"], "buy_quality_dirty_good")

    def test_target_exit_gap_above_other_threshold_is_too_large(self) -> None:
        row = report_row(
            final_pnl_pct=1.0,
            close_reason="Target",
            entry_gap_ms=900,
            exit_gap_ms=30107,
            finality="speculative",
        )

        label = labeler.build_label(row, args())

        self.assertEqual(label["exit_truth_gap_class"], "truth_gap_too_large")
        self.assertEqual(label["truth_gap_class"], "truth_gap_too_large")
        self.assertEqual(label["buy_quality_class"], "buy_quality_unknown")

    def test_reporter_proof_metadata_is_propagated_to_labels_and_summary(self) -> None:
        row = report_row(final_pnl_pct=10.0, finality="finalized")
        row["execution_verification_class_hint"] = "shadow_onchain_finalized_verified"

        label = labeler.build_label(row, args())

        self.assertEqual(label["truth_dataset_kind"], "shadow_burnin_lifecycle_onchain")
        self.assertEqual(label["collection_plane"], "active_shadow")
        self.assertEqual(label["execution_verification_class_hint"], "shadow_onchain_finalized_verified")
        self.assertEqual(label["ab_record_id"], "ab-1")
        self.assertEqual(label["v3_feature_snapshot_hash"], "mfs-hash")
        self.assertEqual(label["v3_policy_config_hash"], "policy-hash")
        self.assertEqual(label["decision_plane"], "v25_shadow")
        self.assertEqual(label["rollout_namespace"], "unit-ns")

        summary = labeler.build_summary(
            [label],
            source_path=Path("shadow_onchain.jsonl"),
            output_path=Path("labels.jsonl"),
            args=args(),
        )
        self.assertEqual(
            summary["truth_dataset_kind_counts"],
            {"shadow_burnin_lifecycle_onchain": 1},
        )
        self.assertEqual(summary["collection_plane_counts"], {"active_shadow": 1})
        self.assertEqual(
            summary["execution_verification_class_hint_counts"],
            {"shadow_onchain_finalized_verified": 1},
        )


if __name__ == "__main__":
    unittest.main()
