#!/usr/bin/env python3
from __future__ import annotations

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p36_calibration_report


def f64_bits(value: float) -> str:
    return f"{int.from_bytes(struct.pack('>d', value), 'big'):016x}"


def quality(ab_record_id: str, label: str, verdict: str = "REJECT") -> dict:
    return {
        "ab_record_id": ab_record_id,
        "outcome_label": label,
        "v3_verdict": verdict,
    }


def v3_row(
    ab_record_id: str,
    *,
    reason: str = "PENDING_V3_WAIT_EVIDENCE",
    verdict: str = "PENDING",
    evidence_status: dict | None = None,
    manipulation: dict | None = None,
) -> dict:
    return {
        "ab_record_id": ab_record_id,
        "v3_shadow_reason_code": reason,
        "v3_shadow_verdict": verdict,
        "v3_shadow_evidence_status": evidence_status or {},
        "v3_shadow_manipulation_contradictions": manipulation or {},
        "v3_materialized_feature_snapshot": {
            "session_metadata": {"observation_duration_ms": 3000},
            "organic_broadening": {
                "sequence_available": True,
                "total_tx_count": 12,
                "total_unique_signers": 8,
                "buy_ratio_min": 0.10,
                "buy_ratio_max": 0.95,
                "tx_count_growth_ratio": 0.80,
                "unique_signer_growth_ratio": 0.90,
                "max_segment_hhi": 0.20,
                "t1_vs_t0_unique_signer_delta": 0,
                "t2_vs_t1_unique_signer_delta": 0,
            },
            "tx_intel_features": {"buy_count": 6},
        },
        "v3_policy_config_payload": {
            "early_window_ms": 2000,
            "evidence_requirements": {
                "fsc": True,
                "sybil": True,
                "alpha": True,
            },
            "profiles": {
                "normal": {
                    "hard_fail_same_ms_tx_ratio_bits": f64_bits(0.60),
                    "hard_fail_top3_volume_pct_bits": f64_bits(0.70),
                    "hard_fail_hhi_bits": f64_bits(0.10),
                    "max_dev_volume_ratio_bits": f64_bits(0.23),
                    "max_signer_cross_pool_velocity_bits": f64_bits(9999.0),
                    "min_tx_count": 20,
                    "min_unique_signers": 10,
                    "min_buy_count": 8,
                    "min_buy_ratio_bits": f64_bits(0.20),
                    "max_buy_ratio_bits": f64_bits(0.90),
                    "organic_min_tx_count_growth_ratio_bits": f64_bits(1.05),
                    "organic_min_unique_signer_growth_ratio_bits": f64_bits(1.05),
                    "max_hhi_bits": f64_bits(0.12),
                }
            },
        },
    }


class V3P36CalibrationReportTests(unittest.TestCase):
    def test_headline_keeps_neutral_separate_from_good_bad(self) -> None:
        rows = [
            quality("bad", "bad_entry", "REJECT"),
            quality("good", "good_entry", "PENDING"),
            quality("neutral", "neutral_entry", "REJECT"),
            quality("unknown", "unknown", "BUY_CANDIDATE"),
        ]

        headline = v3_p36_calibration_report.headline(rows)

        self.assertEqual(headline["known_rows"], 3)
        self.assertEqual(headline["bad_entry"], 1)
        self.assertEqual(headline["good_entry"], 1)
        self.assertEqual(headline["neutral_entry"], 1)
        self.assertEqual(headline["avoided_bad"], 1)
        self.assertEqual(headline["blocked_good"], 1)
        self.assertEqual(headline["protective_ratio"], 1.0)

    def test_pending_wait_evidence_reports_required_non_clean_groups(self) -> None:
        rows = [
            v3_row(
                "ab-1",
                evidence_status={
                    "fsc": {
                        "status": "degraded",
                        "degraded_reasons": ["fsc_evidence_partial"],
                    },
                    "alpha": {"status": "clean"},
                },
            )
        ]
        quality_by_ab = {"ab-1": quality("ab-1", "good_entry", "PENDING")}

        report = v3_p36_calibration_report.pending_wait_evidence_decomposition(
            rows, quality_by_ab
        )

        self.assertEqual(report["rows"], 1)
        self.assertEqual(report["required_non_clean_groups"]["fsc"]["degraded"], 1)
        self.assertEqual(
            report["required_non_clean_reasons"]["fsc"]["fsc_evidence_partial"], 1
        )
        self.assertEqual(report["outcome_split"]["fsc"]["good_entry"], 1)
        self.assertEqual(report["strict_effect"]["block"], 1)
        self.assertEqual(report["terminal_only_effect"]["pending_separate"], 1)

    def test_manipulation_decomposition_splits_subtriggers_and_combinations(self) -> None:
        rows = [
            v3_row(
                "ab-1",
                reason="REJECT_V3_MANIPULATION_CONTRADICTION",
                verdict="REJECT",
                manipulation={
                    "high_hhi": True,
                    "top3_volume_pct": 0.80,
                    "same_ms_tx_ratio": 0.10,
                    "bundle_suspicion_ratio": 0.0,
                },
            )
        ]
        quality_by_ab = {"ab-1": quality("ab-1", "bad_entry", "REJECT")}

        report = v3_p36_calibration_report.manipulation_decomposition(rows, quality_by_ab)

        self.assertEqual(report["subtrigger_outcome_split"]["hhi"]["bad_entry"], 1)
        self.assertEqual(report["subtrigger_outcome_split"]["top3_volume_pct"]["bad_entry"], 1)
        self.assertEqual(report["trigger_combinations"]["hhi+top3_volume_pct"], 1)

    def test_variant_quality_counts_good_recovered_and_safety_cost(self) -> None:
        variant = {
            "row_deltas": [
                {
                    "ab_record_id": "good",
                    "baseline_verdict": "PENDING",
                    "variant_verdict": "BUY_CANDIDATE",
                    "baseline_reason": "PENDING_V3_WAIT_EVIDENCE",
                    "variant_reason": "BUY_V3_NORMAL_CONFIRMED_OPPORTUNITY",
                    "baseline_stage": "EVIDENCE",
                    "variant_stage": "OPPORTUNITY",
                },
                {
                    "ab_record_id": "bad",
                    "baseline_verdict": "REJECT",
                    "variant_verdict": "BUY_CANDIDATE",
                    "baseline_reason": "REJECT_V3_MANIPULATION_CONTRADICTION",
                    "variant_reason": "BUY_V3_NORMAL_CONFIRMED_OPPORTUNITY",
                    "baseline_stage": "RISK",
                    "variant_stage": "OPPORTUNITY",
                },
            ]
        }
        quality_by_ab = {
            "good": quality("good", "good_entry", "PENDING"),
            "bad": quality("bad", "bad_entry", "REJECT"),
        }

        report = v3_p36_calibration_report.variant_quality(variant, quality_by_ab)

        self.assertEqual(report["good_unblocked"], 1)
        self.assertEqual(report["bad_unblocked"], 1)
        self.assertEqual(report["net_good_recovered"], 0)
        self.assertEqual(report["safety_cost"], 1)
        self.assertEqual(report["variant_protective_ratio"], None)
        self.assertEqual(
            report["transition_matrix"]["baseline_verdict_to_variant_verdict"]["PENDING"][
                "BUY_CANDIDATE"
            ],
            1,
        )

    def test_r12_gate_uses_candidate_ratio_not_baseline_ratio(self) -> None:
        headline = {
            "blocked_good": 10,
            "protective_ratio": 2.0,
        }
        runs = [{"ablation": {"status": "ok", "replay_status": "full_replay_ok"}}]
        variants = {
            v3_p36_calibration_report.CANDIDATE_VARIANT: {
                "status": "ok",
                "variant_blocked_bad": 4,
                "variant_blocked_good": 4,
                "variant_protective_ratio": 1.0,
                "variant_protective_precision": 0.5,
                "good_unblocked": 6,
                "bad_unblocked": 1,
                "unknown_unblocked": 0,
            }
        }

        gate = v3_p36_calibration_report.r12_gate(headline, runs, variants)

        self.assertEqual(gate["r12_gate_status"], "blocked")
        self.assertIn(
            "candidate_protective_ratio_below_1_30",
            gate["blocked_gates"],
        )

    def test_organic_decomposition_reports_candidate_low_organic_failures(self) -> None:
        rows = [v3_row("ab-1", reason="PENDING_V3_WAIT_EVIDENCE", verdict="PENDING")]
        variant = {
            "row_deltas": [
                {
                    "ab_record_id": "ab-1",
                    "variant_reason": "REJECT_V3_LOW_ORGANIC_BROADENING",
                }
            ]
        }
        quality_by_ab = {"ab-1": quality("ab-1", "good_entry", "PENDING")}

        report = v3_p36_calibration_report.organic_decomposition(
            rows, quality_by_ab, variant
        )

        self.assertEqual(report["rows"], 1)
        self.assertEqual(report["label_counts"]["good_entry"], 1)
        self.assertEqual(report["failure_counts"]["total_tx_count_below_min"], 1)
        self.assertEqual(report["failure_counts"]["buy_ratio_max_above_max"], 1)
        self.assertEqual(
            report["failure_outcome_split"]["max_segment_hhi_above_max"]["good_entry"],
            1,
        )

    def test_candidate_buy_analysis_reports_small_sample_and_features(self) -> None:
        rows = [
            v3_row(
                "ab-1",
                reason="PENDING_V3_WAIT_EVIDENCE",
                verdict="PENDING",
                manipulation={
                    "same_ms_tx_ratio": 0.75,
                    "bundle_suspicion_ratio": 0.20,
                    "dev_volume_ratio": 0.10,
                    "top3_volume_pct": 0.50,
                    "hhi": 0.03,
                },
            )
        ]
        variant = {
            "row_deltas": [
                {
                    "ab_record_id": "ab-1",
                    "variant_verdict": "BUY_CANDIDATE",
                }
            ]
        }
        quality_by_ab = {"ab-1": quality("ab-1", "good_entry", "PENDING")}

        report = v3_p36_calibration_report.candidate_buy_analysis(
            rows,
            quality_by_ab,
            variant,
            "test_variant",
        )

        self.assertEqual(report["status"], "ok")
        self.assertEqual(report["rows"], 1)
        self.assertTrue(report["sample_size_warning"])
        self.assertEqual(report["label_counts"]["good_entry"], 1)
        self.assertEqual(
            report["organic_failure_split"]["good_entry"]["total_tx_count_below_min"],
            1,
        )
        self.assertEqual(
            report["manip_trigger_split"]["good_entry"]["same_ms_bundle"],
            1,
        )
        self.assertEqual(
            report["feature_summary_by_label"]["good_entry"]["buy_count"]["median"],
            6.0,
        )


if __name__ == "__main__":
    unittest.main()
