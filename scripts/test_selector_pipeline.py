#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import build_selector_accepted_lifecycle as accepted
import build_selector_candidate_universe as universe
import build_selector_canonical_r2_source as canonical_r2
import build_selector_dataset as dataset
import build_selector_feature_snapshots as snapshots
import build_selector_phase1_report as phase1_report
import build_selector_phase2 as phase2
import build_selector_phase3_r2only as phase3_r2only
import build_selector_r2_market_paths as r2_paths
import build_selector_r2only_baseline_report as r2only_baseline
import build_selector_r2only_ablation_report as r2only_ablation
import build_selector_r2only_feature_audit as r2only_feature_audit
import build_selector_training_view as training
import compare_selector_gatekeepers as compare
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


if __name__ == "__main__":
    unittest.main()
