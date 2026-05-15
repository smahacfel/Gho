#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_shadow_report


def v3_row(
    *,
    ab_record_id: str = "pool:1000:11000:REJECT",
    plane: str = "v25_shadow",
    active_buy: bool | None = False,
    active_type: str = "REJECT_CORE_FAIL",
    v3_verdict: str = "PENDING",
    reason: str = "PENDING_V3_WAIT_EVIDENCE",
    confidence: float = 0.0,
    execution_outcome: str | None = None,
    config_hash: str = "v2-hash-a",
    policy_hash: str = "v3-policy-a",
    snapshot_hash: str = "v3-snapshot-a",
) -> dict:
    row = {
        "pool_id": "pool",
        "join_key": "pool:mint:1000",
        "observation_start_ts_ms": 1000,
        "ab_record_id": ab_record_id,
        "decision_plane": plane,
        "decision_verdict_buy": active_buy,
        "verdict_type": active_type,
        "config_hash": config_hash,
        "v3_shadow_schema_version": 1,
        "v3_shadow_verdict": v3_verdict,
        "v3_shadow_stage": "EVIDENCE",
        "v3_shadow_reason_code": reason,
        "v3_shadow_reason_chain": [reason],
        "v3_shadow_secondary_reason_codes": [],
        "v3_shadow_risk_status": "DEGRADED",
        "v3_shadow_risk_penalty": 0.0,
        "v3_shadow_opportunity_status": "DEGRADED",
        "v3_shadow_opportunity_score": 0.0,
        "v3_shadow_confidence_raw": confidence,
        "v3_shadow_confidence_after_risk": confidence,
        "v3_shadow_confidence_after_stage": confidence,
        "v3_shadow_confidence_cap": confidence,
        "v3_shadow_confidence_cap_reasons": ["insufficient_evidence"],
        "v3_shadow_confidence_final": confidence,
        "v3_shadow_confidence": confidence,
        "v3_policy_config_hash": policy_hash,
        "v3_feature_snapshot_hash": snapshot_hash,
        "v3_materialization_version": 1,
        "v3_policy_version": 1,
        "v3_stage_thresholds": {
            "evidence": {"min_tx_count": 12},
            "risk": {"hard_fail_hhi": 0.1},
            "opportunity": {"min_buy_ratio": 0.8},
            "confidence": {"execution_not_run_confidence_cap": 0.8},
        },
        "v3_component_scores": {
            "risk": {"status": "DEGRADED", "penalty": 0.0},
            "opportunity": {"status": "DEGRADED", "score": 0.0},
            "confidence": {
                "raw": confidence,
                "after_risk": confidence,
                "after_stage": confidence,
                "cap": confidence,
                "cap_reasons": ["insufficient_evidence"],
                "final": confidence,
            },
            "final_confidence": confidence,
        },
        "v3_actionability": {
            "stages": {
                "evidence": "blocked",
                "risk": "actionable",
                "opportunity": "blocked",
                "confidence": "blocked",
            },
            "groups": {
                "tx_intel": {"status": "Clean", "actionability": "actionable"},
                "tx_segments": {"status": "Unavailable", "actionability": "blocked"},
                "curve": {"status": "Degraded", "actionability": "degraded"},
            },
        },
        "v3_shadow_evidence_status": {
            "tx_intel": {"status": "clean"},
            "tx_segments": {
                "status": "unavailable",
                "unavailable_reasons": ["segment_sequence_missing"],
            },
            "curve": {
                "status": "degraded",
                "degraded_reasons": ["curve_evidence_partial"],
            },
        },
        "v3_shadow_organic_broadening": {
            "sequence_available": False,
            "tx_count_growth_ratio": 0.0,
            "unique_signer_growth_ratio": 0.0,
        },
        "v3_shadow_manipulation_contradictions": {
            "dev_has_sold": False,
            "high_hhi": False,
            "sybil_evidence_degraded": False,
        },
    }
    if execution_outcome is not None:
        row["shadow_execution_outcome"] = execution_outcome
    return row


class V3ShadowReportTests(unittest.TestCase):
    def test_no_v3_fields_returns_no_v3_status(self) -> None:
        rows = [
            {
                "pool_id": "pool",
                "decision_plane": "legacy_live",
                "decision_verdict_buy": False,
                "verdict_type": "REJECT_CORE_FAIL",
            }
        ]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["status"], "no_v3_fields")
        self.assertEqual(report["counts"]["deduped_rows"], 1)
        self.assertEqual(report["counts"]["v3_rows"], 0)

    def test_mixed_legacy_and_v25_rows_do_not_double_count(self) -> None:
        rows = [
            v3_row(plane="legacy_live", v3_verdict="BUY_CANDIDATE"),
            v3_row(plane="v25_shadow", v3_verdict="PENDING"),
        ]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["status"], "ok")
        self.assertEqual(report["counts"]["raw_rows"], 2)
        self.assertEqual(report["counts"]["deduped_rows"], 1)
        self.assertEqual(report["counts"]["duplicate_rows_removed"], 1)
        self.assertEqual(report["active_vs_v3_verdict"]["REJECT"]["PENDING"], 1)
        self.assertEqual(report["hash_coverage"]["v3_policy_config_hash"]["coverage"], 1.0)

    def test_v3_verdict_matrix_aggregation(self) -> None:
        rows = [
            v3_row(active_buy=True, active_type="BUY", v3_verdict="BUY_CANDIDATE"),
            v3_row(
                ab_record_id="pool2:1000:11000:TIMEOUT",
                active_buy=None,
                active_type="TIMEOUT_PHASE1",
                v3_verdict="TIMEOUT",
            ),
        ]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["active_vs_v3_verdict"]["BUY"]["BUY_CANDIDATE"], 1)
        self.assertEqual(report["active_vs_v3_verdict"]["TIMEOUT"]["TIMEOUT"], 1)

    def test_confidence_and_evidence_distribution_aggregation(self) -> None:
        rows = [v3_row(confidence=0.82)]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["confidence_buckets"]["0_75_to_1_00"], 1)
        self.assertEqual(report["component_score_buckets"]["confidence_final"]["0_75_to_1_00"], 1)
        self.assertEqual(report["v3_stages"]["EVIDENCE"], 1)
        self.assertEqual(report["v3_risk_statuses"]["DEGRADED"], 1)
        self.assertEqual(report["v3_opportunity_statuses"]["DEGRADED"], 1)
        self.assertEqual(report["confidence_cap_reasons"]["insufficient_evidence"], 1)
        self.assertEqual(report["evidence_status_by_feature"]["tx_intel"]["clean"], 1)
        self.assertEqual(report["evidence_status_by_feature"]["tx_segments"]["unavailable"], 1)
        self.assertEqual(report["evidence_status_by_feature"]["curve"]["degraded"], 1)
        self.assertEqual(
            report["missing_degraded_evidence"]["unavailable:segment_sequence_missing"],
            1,
        )
        self.assertEqual(
            report["missing_degraded_evidence"]["degraded:curve_evidence_partial"],
            1,
        )

    def test_no_dispatch_and_missing_execution_are_not_success(self) -> None:
        rows = [
            v3_row(execution_outcome="no_dispatch_rejected"),
            v3_row(ab_record_id="pool2:1000:11000:REJECT"),
        ]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["execution"]["success_count"], 0)
        self.assertEqual(report["execution"]["outcomes"]["no_dispatch_rejected"], 1)
        self.assertEqual(report["execution"]["outcomes"]["missing"], 1)

    def test_submitted_execution_is_not_success(self) -> None:
        rows = [v3_row(execution_outcome="submitted")]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["execution"]["success_count"], 0)
        self.assertEqual(report["execution"]["outcomes"]["submitted"], 1)

    def test_hash_actionability_and_replay_status_are_reported(self) -> None:
        rows = [
            v3_row(snapshot_hash="snapshot-a"),
            v3_row(
                ab_record_id="pool2:1000:11000:REJECT",
                config_hash="v2-hash-b",
                policy_hash="v3-policy-b",
                snapshot_hash="snapshot-a",
            ),
            v3_row(
                ab_record_id="pool3:1000:11000:REJECT",
                policy_hash="",
                snapshot_hash="",
            ),
        ]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["replay_status"], "hash_only")
        self.assertEqual(report["hash_coverage"]["v3_policy_config_hash"]["present"], 2)
        self.assertEqual(report["hash_coverage"]["v3_feature_snapshot_hash"]["present"], 2)
        self.assertEqual(report["snapshot_uniqueness"]["duplicates"]["snapshot-a"], 2)
        self.assertEqual(
            report["config_hash_matrix"]["v2-hash-b"]["v3-policy-b"],
            1,
        )
        self.assertEqual(report["actionability_summary"]["stages"]["evidence"]["blocked"], 3)
        self.assertEqual(
            report["actionability_summary"]["groups"]["tx_segments"]["blocked"],
            3,
        )

    def test_pre_dedupe_conflicting_duplicates_are_reported(self) -> None:
        rows = [
            v3_row(v3_verdict="PENDING", reason="PENDING_V3_WAIT_EVIDENCE", snapshot_hash="a"),
            v3_row(v3_verdict="REJECT", reason="REJECT_V3_MANIPULATION_CONTRADICTION", snapshot_hash="b"),
        ]

        report = v3_shadow_report.build_report_from_rows(rows)

        self.assertEqual(report["counts"]["deduped_rows"], 1)
        self.assertEqual(report["pre_dedupe_conflicts"]["duplicate_groups"], 1)
        self.assertEqual(report["pre_dedupe_conflicts"]["conflict_groups"], 1)
        self.assertEqual(report["pre_dedupe_conflicts"]["conflict_rows"], 2)
        conflict = next(iter(report["pre_dedupe_conflicts"]["conflicts"].values()))
        self.assertEqual(conflict["snapshot_hashes"], ["a", "b"])

    def test_build_report_marks_old_decision_log_as_stale_against_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decisions_dir = root / "decisions"
            decisions_dir.mkdir()
            decisions_log = decisions_dir / v3_shadow_report.DECISIONS_LOG_NAME
            decisions_log.write_text(json.dumps(v3_row()) + "\n", encoding="utf-8")
            config_path = root / "shadow-burnin.toml"
            config_path.write_text(
                '[oracle]\ndecision_log_path = "decisions"\n',
                encoding="utf-8",
            )

            os.utime(decisions_log, (100.0, 100.0))
            os.utime(config_path, (200.0, 200.0))

            report = v3_shadow_report.build_report(config_path)

            self.assertEqual(report["status"], "stale_artifacts")
            self.assertTrue(report["artifact_freshness"]["stale_against_config"])


if __name__ == "__main__":
    unittest.main()
