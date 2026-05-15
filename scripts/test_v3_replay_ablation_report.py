#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_replay_ablation_report


def row(
    *,
    ab_record_id: str = "pool:1000:11000:REJECT",
    reason: str = "REJECT_V3_MANIPULATION_CONTRADICTION",
    v3_verdict: str = "REJECT",
    confidence: float = 0.0,
    policy_hash: str = "policy-a",
    snapshot_hash: str = "snapshot-a",
    outcome: str | None = None,
    active_type: str = "REJECT_CORE_FAIL",
) -> dict:
    value = {
        "ab_record_id": ab_record_id,
        "pool_id": "pool",
        "decision_plane": "v25_shadow",
        "decision_verdict_buy": False,
        "verdict_type": active_type,
        "v3_shadow_schema_version": 1,
        "v3_shadow_verdict": v3_verdict,
        "v3_shadow_reason_code": reason,
        "v3_shadow_confidence": confidence,
        "v3_policy_config_hash": policy_hash,
        "v3_feature_snapshot_hash": snapshot_hash,
        "v3_materialization_version": 1,
        "v3_shadow_confidence_cap_reasons": [],
        "v3_shadow_evidence_status": {
            "tx_intel": {"status": "clean"},
            "fsc": {"status": "degraded", "degraded_reasons": ["fsc_evidence_partial"]},
        },
    }
    if outcome is not None:
        value["shadow_execution_outcome"] = outcome
    return value


class V3ReplayAblationReportTests(unittest.TestCase):
    def test_hash_only_replay_yields_insufficient_data_not_pass(self) -> None:
        rows = [row(), row(ab_record_id="pool2", reason="PENDING_V3_WAIT_SAMPLE", v3_verdict="PENDING")]

        replay = v3_replay_ablation_report.replay_parity(rows, 0)
        calibration = v3_replay_ablation_report.calibration_buckets(rows)
        ablation = v3_replay_ablation_report.ablation_proxy(rows)
        cert = v3_replay_ablation_report.certification(rows, replay, calibration, ablation)

        self.assertEqual(replay["status"], "hash_only")
        self.assertEqual(cert["p3_status"], "insufficient_data")
        self.assertEqual(cert["promotion_ready_gates"], [])
        self.assertTrue(cert["no_p2_promotion"])
        self.assertIn("replay_hash_only", cert["insufficient_evidence_gates"])

    def test_duplicate_ab_record_conflict_is_reported(self) -> None:
        rows = [
            row(snapshot_hash="snapshot-a", v3_verdict="REJECT"),
            row(snapshot_hash="snapshot-b", v3_verdict="PENDING"),
        ]

        replay = v3_replay_ablation_report.replay_parity(rows, 0)

        self.assertEqual(replay["duplicate_ab_record_conflict_count"], 1)
        self.assertIn("pool:1000:11000:REJECT", replay["duplicate_ab_record_conflicts"])

    def test_calibration_buckets_report_unknown_outcome_ratio(self) -> None:
        rows = [
            row(confidence=0.0),
            row(ab_record_id="pool2", confidence=0.82, outcome="confirmed"),
        ]

        buckets = v3_replay_ablation_report.calibration_buckets(rows)

        self.assertEqual(buckets["0"]["count"], 1)
        self.assertEqual(buckets["0"]["unknown_outcome_ratio"], 1.0)
        self.assertEqual(buckets["0_75_to_1_00"]["count"], 1)
        self.assertEqual(buckets["0_75_to_1_00"]["outcome_label_coverage"], 1.0)

    def test_manipulation_dominance_blocks_promotion_readiness(self) -> None:
        rows = [
            row(ab_record_id="pool1"),
            row(ab_record_id="pool2"),
            row(ab_record_id="pool3", reason="PENDING_V3_WAIT_EVIDENCE", v3_verdict="PENDING"),
        ]

        replay = v3_replay_ablation_report.replay_parity(rows, 0)
        calibration = v3_replay_ablation_report.calibration_buckets(rows)
        ablation = v3_replay_ablation_report.ablation_proxy(rows)
        cert = v3_replay_ablation_report.certification(rows, replay, calibration, ablation)

        self.assertEqual(ablation["reason_group_counts"]["manipulation_contradiction"], 2)
        self.assertIn(
            "dominant_manipulation_contradiction_requires_more_evidence",
            cert["insufficient_evidence_gates"],
        )

    def test_missing_explicit_compare_log_fails_closed(self) -> None:
        with self.assertRaises(FileNotFoundError):
            v3_replay_ablation_report.resolve_compare_log(
                Path("logs/rollout/definitely_missing_v3_p3_compare.jsonl")
            )

    def test_unimplemented_lifecycle_and_events_inputs_fail_closed(self) -> None:
        with self.assertRaisesRegex(NotImplementedError, "--shadow-lifecycle"):
            v3_replay_ablation_report.validate_unimplemented_inputs(Path("lifecycle.jsonl"), None)

        with self.assertRaisesRegex(NotImplementedError, "--events-dir"):
            v3_replay_ablation_report.validate_unimplemented_inputs(None, Path("events"))


if __name__ == "__main__":
    unittest.main()
