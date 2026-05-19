import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_shadow_lifecycle_feature_availability as availability


def label(candidate: str, quality: str, ts: int, *, context: bool = True) -> dict:
    return {
        "candidate_id": candidate,
        "pool_id": "pool",
        "base_mint": "mint",
        "decision_ts_ms": ts,
        "buy_quality_class": quality,
        "market_outcome_class": "market_good_clean" if quality == "buy_quality_dirty_good" else "market_bad_clean",
        "gatekeeper_buy_context_found": context,
        "close_reason": "Target" if quality == "buy_quality_dirty_good" else "StopLoss",
        "truth_gap_class": "truth_gap_clean",
    }


def decision(path: Path, ts: int, *, with_v3_mfs: bool = False) -> availability.DecisionRow:
    row = {
        "timestamp": "2026-05-06T21:52:20.000000+00:00",
        "pool_id": "pool",
        "base_mint": "mint",
        "first_seen_ts_ms": ts - 8000,
        "observation_end_ts_ms": ts - 100,
        "decision_plane": "v25_shadow",
        "decision_verdict_buy": True,
        "shadow_execution_outcome": "shadow_simulated",
        "gatekeeper_version": "v2.5",
        "phase2_passed": True,
        "phases_passed": 6,
        "block0_sniped_supply_pct": 0.1,
        "priority_fee_surge_slope": 1.0,
        "curve_t0_event_ts_ms": ts - 8000,
        "curve_finality": "speculative",
        "pdd_score": 1.0,
        "pdd_price_anchor_available": True,
        "tas_available": False,
        "observation_stage": "Extended",
        "v25_shadow_verdict_type": "BUY",
        "legacy_live_reason_chain": "BUY",
        "config_hash": "hash",
    }
    if with_v3_mfs:
        row["v3_materialized_feature_snapshot"] = {
            "checkpoint_features": {"price_trajectory": [1.0, 1.1]},
            "account_features": {"curve_finality": "confirmed"},
        }
    return availability.DecisionRow(
        row=row,
        path=path,
        log_kind="buy",
        timestamp_ms=ts,
        feature_groups=availability.detect_feature_groups(row),
    )


def decision_without_features(path: Path, ts: int) -> availability.DecisionRow:
    row = {
        "timestamp": "2026-05-06T21:52:20.000000+00:00",
        "pool_id": "pool",
        "base_mint": "mint",
    }
    return availability.DecisionRow(
        row=row,
        path=path,
        log_kind="decision",
        timestamp_ms=ts,
        feature_groups=availability.detect_feature_groups(row),
    )


class P37ShadowLifecycleFeatureAvailabilityTests(unittest.TestCase):
    def test_v2_features_available_for_gatekeeper_context_classes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "gatekeeper_v2_buys.jsonl"
            labels = [
                label("good", "buy_quality_dirty_good", 1000),
                label("bad", "buy_quality_bad", 2000),
                label("good2", "buy_quality_dirty_good", 3000),
                label("bad2", "buy_quality_bad", 4000),
            ]
            decisions = [decision(path, 1000), decision(path, 2000), decision(path, 3000), decision(path, 4000)]

            report = availability.build_report(
                labels=labels,
                raw_rows=[],
                decision_rows=decisions,
                decision_logs=[path],
                max_match_drift_ms=60_000,
                min_feature_label_rows=1,
                min_temporal_split_class_rows=1,
                source_labels=Path("labels.jsonl"),
                source_raw=Path("raw.jsonl"),
                config_path=Path("config.toml"),
            )

        self.assertEqual(report["feature_availability_status"], "v2_features_available")
        self.assertTrue(report["phase_b_possible"])
        self.assertFalse(report["v3_selector_prototype_possible"])
        self.assertEqual(report["feature_group_matrix"]["tx_intel_fields"]["gatekeeper_context_dirty_good"], 2)
        self.assertEqual(report["feature_group_matrix"]["pdd_fields"]["gatekeeper_context_bad"], 2)

    def test_v3_mfs_takes_precedence_when_available(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "gatekeeper_v2_buys.jsonl"
            labels = [
                label("good", "buy_quality_dirty_good", 1000),
                label("bad", "buy_quality_bad", 2000),
                label("good2", "buy_quality_dirty_good", 3000),
                label("bad2", "buy_quality_bad", 4000),
            ]
            decisions = [
                decision(path, 1000, with_v3_mfs=True),
                decision(path, 2000, with_v3_mfs=True),
                decision(path, 3000, with_v3_mfs=True),
                decision(path, 4000, with_v3_mfs=True),
            ]

            report = availability.build_report(
                labels=labels,
                raw_rows=[],
                decision_rows=decisions,
                decision_logs=[path],
                max_match_drift_ms=60_000,
                min_feature_label_rows=1,
                min_temporal_split_class_rows=1,
                source_labels=Path("labels.jsonl"),
                source_raw=Path("raw.jsonl"),
                config_path=Path("config.toml"),
            )

        self.assertEqual(report["feature_availability_status"], "v3_features_available")
        self.assertTrue(report["v3_selector_prototype_possible"])

    def test_unmatched_labels_are_lifecycle_only(self) -> None:
        report = availability.build_report(
            labels=[label("good", "buy_quality_dirty_good", 1000)],
            raw_rows=[],
            decision_rows=[],
            decision_logs=[],
            max_match_drift_ms=60_000,
            min_feature_label_rows=1,
            min_temporal_split_class_rows=1,
            source_labels=Path("labels.jsonl"),
            source_raw=Path("raw.jsonl"),
            config_path=Path("config.toml"),
        )

        self.assertEqual(report["feature_availability_status"], "lifecycle_only")
        self.assertFalse(report["phase_b_possible"])
        self.assertEqual(report["join_quality_counts"]["unmatched"], 1)

    def test_matched_rows_without_feature_groups_are_not_feature_available(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "gatekeeper_v2_decisions.jsonl"
            report = availability.build_report(
                labels=[label("good", "buy_quality_dirty_good", 1000)],
                raw_rows=[],
                decision_rows=[decision_without_features(path, 1000)],
                decision_logs=[path],
                max_match_drift_ms=60_000,
                min_feature_label_rows=1,
                min_temporal_split_class_rows=1,
                source_labels=Path("labels.jsonl"),
                source_raw=Path("raw.jsonl"),
                config_path=Path("config.toml"),
            )

        self.assertEqual(report["join_quality_counts"]["matched_by_pool_mint_time_window"], 1)
        self.assertEqual(report["rows_with_any_decision_time_features"], {})
        self.assertEqual(report["feature_availability_status"], "lifecycle_only")


if __name__ == "__main__":
    unittest.main()
