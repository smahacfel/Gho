#!/usr/bin/env python3
from __future__ import annotations

import sys
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
    reason: str = "V3_SHADOW_PENDING_INSUFFICIENT_EVIDENCE",
    confidence: float = 0.0,
    execution_outcome: str | None = None,
) -> dict:
    row = {
        "pool_id": "pool",
        "join_key": "pool:mint:1000",
        "observation_start_ts_ms": 1000,
        "ab_record_id": ab_record_id,
        "decision_plane": plane,
        "decision_verdict_buy": active_buy,
        "verdict_type": active_type,
        "v3_shadow_schema_version": 1,
        "v3_shadow_verdict": v3_verdict,
        "v3_shadow_reason_code": reason,
        "v3_shadow_reason_chain": [reason],
        "v3_shadow_confidence": confidence,
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


if __name__ == "__main__":
    unittest.main()
