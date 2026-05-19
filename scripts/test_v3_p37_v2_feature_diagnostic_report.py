import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_shadow_lifecycle_feature_availability as availability
import v3_p37_v2_feature_diagnostic_report as diagnostic


def row(candidate: str, quality: str, value: float, *, category: str = "BUY") -> diagnostic.DiagnosticRow:
    label = {
        "candidate_id": candidate,
        "pool_id": "pool",
        "base_mint": "mint",
        "buy_quality_class": quality,
        "market_outcome_class": "market_good_clean" if quality == "buy_quality_dirty_good" else "market_bad_clean",
        "gatekeeper_buy_context_found": True,
        "close_reason": "Target" if quality == "buy_quality_dirty_good" else "StopLoss",
        "truth_gap_class": "truth_gap_clean",
        "final_pnl_pct": 10.0 if quality == "buy_quality_dirty_good" else -10.0,
    }
    decision_row = {
        "pool_id": "pool",
        "base_mint": "mint",
        "decision_plane": "v25_shadow",
        "gatekeeper_version": "v2.5",
        "config_hash": "hash",
        "buy_ratio": value,
        "decision_reason": category,
    }
    decision = availability.DecisionRow(
        row=decision_row,
        path=Path("gatekeeper_v2_buys.jsonl"),
        log_kind="buy",
        timestamp_ms=1000,
        feature_groups=availability.detect_feature_groups(decision_row),
    )
    return diagnostic.DiagnosticRow(
        label=label,
        decision=decision,
        match_time_delta_ms=1,
        numeric={"buy_ratio": value},
        categorical={"decision_reason": category},
    )


class P37V2FeatureDiagnosticReportTests(unittest.TestCase):
    def test_numeric_separation_reports_auc_and_rank_biserial(self) -> None:
        rows = [
            row("g1", "buy_quality_dirty_good", 0.9),
            row("g2", "buy_quality_dirty_good", 0.8),
            row("b1", "buy_quality_bad", 0.1),
            row("b2", "buy_quality_bad", 0.2),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            summary = diagnostic.build_comparison(
                diagnostic.comparison_specs()[0],
                rows,
                Path(tmp),
                min_rows=1,
            )

        top = summary["top_numeric_by_auc"][0]
        self.assertEqual(top["feature"], "buy_ratio")
        self.assertEqual(top["auc_good_gt_bad"], 1.0)
        self.assertEqual(top["rank_biserial"], 1.0)

    def test_forbidden_label_fields_are_not_predictive_features(self) -> None:
        rows = [
            row("g1", "buy_quality_dirty_good", 0.9),
            row("b1", "buy_quality_bad", 0.1),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            summary = diagnostic.build_comparison(
                diagnostic.comparison_specs()[0],
                rows,
                Path(tmp),
                min_rows=1,
            )

        feature_names = {item["feature"] for item in summary["feature_coverage"]["numeric"]}
        self.assertNotIn("final_pnl_pct", feature_names)
        self.assertNotIn("close_reason", feature_names)
        self.assertNotIn("truth_gap_class", feature_names)

    def test_categorical_feature_reports_odds_ratio(self) -> None:
        rows = [
            row("g1", "buy_quality_dirty_good", 0.9, category="BUY_STRONG"),
            row("g2", "buy_quality_dirty_good", 0.8, category="BUY_STRONG"),
            row("b1", "buy_quality_bad", 0.1, category="BUY_WEAK"),
            row("b2", "buy_quality_bad", 0.2, category="BUY_WEAK"),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            summary = diagnostic.build_comparison(
                diagnostic.comparison_specs()[0],
                rows,
                Path(tmp),
                min_rows=1,
            )

        categorical = summary["top_categorical"][0]
        self.assertEqual(categorical["feature"], "decision_reason")
        categories = {item["category"]: item for item in categorical["top_categories"]}
        self.assertGreater(categories["BUY_STRONG"]["odds_ratio_dirty_good"], 1.0)


if __name__ == "__main__":
    unittest.main()
