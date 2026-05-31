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
import build_selector_dataset as dataset
import build_selector_feature_snapshots as snapshots
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
        self.assertEqual(summary["identity_collisions"], [])
        self.assertEqual(len(rows), 2)
        by_id = {row["candidate_id"]: row for row in rows}
        self.assertEqual(by_id["c1"]["candidate_universe_status"], "ok")
        self.assertFalse(by_id["c1"]["decision_verdict_buy"])
        self.assertEqual(by_id["c2"]["candidate_universe_status"], "universe_incomplete")
        self.assertIn("quote_mint", by_id["c2"]["candidate_identity_missing_fields"])

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
        self.assertEqual(row["r1_label"], "negative")
        self.assertEqual(row["r1_label_reason"], "time_stop_below_target")
        self.assertIsNone(row["r1_gray_reason"])

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


if __name__ == "__main__":
    unittest.main()
