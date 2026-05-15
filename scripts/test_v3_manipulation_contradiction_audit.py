#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_manipulation_contradiction_audit as audit


def row(
    *,
    reason: str = audit.TARGET_REASON,
    evidence_status: str = "degraded",
    dev_volume_ratio: float = 0.31,
    max_dev_volume_ratio: float = 0.23,
    active_reason: str = "REJECT_PDD_ENTRY_DRIFT",
) -> dict:
    return {
        "ab_record_id": "pool:1000:11000:REJECT",
        "pool_id": "pool",
        "decision_plane": "v25_shadow",
        "reason_code": active_reason,
        "v3_shadow_reason_code": reason,
        "v3_shadow_verdict": "REJECT",
        "v3_shadow_stage": "RISK",
        "v3_policy_config_hash": "policy-a",
        "v3_feature_snapshot_hash": "snapshot-a",
        "v3_shadow_manipulation_contradictions": {
            "status": evidence_status,
            "reasons": ["timing_bundle_concentration"],
            "timing_bundle_concentration": True,
            "early_top3_concentration": True,
            "dev_has_sold": True,
            "sybil_evidence_degraded": evidence_status == "degraded",
            "dev_volume_ratio": dev_volume_ratio,
            "top3_volume_pct": 0.4,
            "hhi": 0.04,
            "bundle_suspicion_ratio": 0.2,
            "same_ms_tx_ratio": 0.1,
            "contradiction_score": 0.2,
            "fee_topology_diversity_index": 0.3,
            "spend_fraction_divergence": 0.1,
            "max_tx_per_signer": 3,
            "signer_cross_pool_velocity": 0.0,
        },
        "v3_shadow_evidence_status": {
            "manipulation_contradiction": {
                "status": evidence_status,
                "degraded_reasons": ["manipulation_contradiction_partial"]
                if evidence_status == "degraded"
                else [],
            },
            "manipulation": {"status": "clean"},
            "sybil": {"status": "degraded"},
            "fsc": {"status": "degraded"},
            "organic_broadening": {"status": "insufficient_sample"},
            "pdd_sequence": {"status": "insufficient_sample"},
            "tx_segments": {"status": "insufficient_sample"},
        },
        "v3_actionability": {
            "profile": "normal",
            "groups": {
                "manipulation_contradiction": {
                    "actionability": "not_actionable"
                    if evidence_status == "degraded"
                    else "actionable"
                }
            },
            "stages": {"risk": "not_actionable" if evidence_status == "degraded" else "actionable"},
        },
        "v3_component_scores": {
            "risk": {"penalty": 1.0, "status": "ACTIONABLE"},
            "opportunity": {"score": 0.5, "status": "UNAVAILABLE"},
            "confidence": {"raw": 0.5, "final": 0.0, "cap_reasons": ["hard_risk"]},
        },
        "v3_stage_thresholds": {
            "profiles": {
                "normal": {
                    "risk": {
                        "reject_on_dev_sell": False,
                        "hard_fail_same_ms_tx_ratio": 0.6,
                        "hard_fail_top3_volume_pct": 0.7,
                        "hard_fail_hhi": 0.1,
                        "max_dev_volume_ratio": max_dev_volume_ratio,
                        "max_tx_per_signer": 999999,
                        "max_signer_cross_pool_velocity": 9999.0,
                        "max_funding_source_concentration": 0.99,
                    }
                }
            }
        },
    }


class V3ManipulationContradictionAuditTests(unittest.TestCase):
    def test_hard_risk_trigger_detects_dev_volume_threshold(self) -> None:
        self.assertEqual(audit.hard_risk_triggers(row()), ["dev_volume_ratio_threshold"])

    def test_hard_risk_trigger_supports_legacy_flat_threshold_payload(self) -> None:
        value = row()
        value["v3_stage_thresholds"] = {
            "risk": {
                "reject_on_dev_sell": False,
                "hard_fail_same_ms_tx_ratio": 0.6,
                "hard_fail_top3_volume_pct": 0.7,
                "hard_fail_hhi": 0.1,
                "max_dev_volume_ratio": 0.23,
                "max_tx_per_signer": 999999,
                "max_signer_cross_pool_velocity": 9999.0,
                "max_funding_source_concentration": 0.99,
            }
        }

        self.assertEqual(audit.hard_risk_triggers(value), ["dev_volume_ratio_threshold"])

    def test_hard_risk_trigger_reports_missing_thresholds(self) -> None:
        value = row()
        value.pop("v3_stage_thresholds")

        self.assertEqual(audit.hard_risk_triggers(value), ["thresholds_missing"])

    def test_dataset_summary_splits_degraded_evidence_from_hard_trigger(self) -> None:
        rows = [row(), row(evidence_status="clean", dev_volume_ratio=0.35, active_reason="REJECT_PDD_WHALE")]

        summary = audit.dataset_summary("test", Path("decisions.jsonl"), rows, 0, sample_limit=1)

        self.assertEqual(summary["target_rows"], 2)
        self.assertEqual(summary["hard_risk_triggers"]["dev_volume_ratio_threshold"], 2)
        self.assertEqual(
            summary["manipulation_contradiction_clean_vs_degraded"],
            {"clean": 1, "degraded": 1, "other": 0},
        )
        self.assertEqual(summary["active_reason_codes"]["REJECT_PDD_ENTRY_DRIFT"], 1)
        self.assertEqual(summary["active_reason_codes"]["REJECT_PDD_WHALE"], 1)

    def test_certification_keeps_bucket_blocked_when_degraded_evidence_exists(self) -> None:
        summary = audit.dataset_summary("test", Path("decisions.jsonl"), [row()], 0, sample_limit=1)

        cert = audit.certification(summary)

        self.assertEqual(cert["p3_1_status"], "keep_blocked_needs_full_replay")
        self.assertFalse(cert["promotion_ready"])
        self.assertIn(
            "dominant_bucket_has_degraded_manipulation_contradiction_evidence",
            cert["insufficient_evidence_gates"],
        )

    def test_cross_dataset_summary_uses_dominant_counts_not_sort_order(self) -> None:
        datasets = [
            {
                "name": "primary",
                "target_rows": 3,
                "target_share_of_v3_rows": 1.0,
                "active_reason_codes": {"A_SMALL": 1, "Z_BIG": 2},
                "hard_risk_triggers": {"A_SMALL": 1, "Z_BIG": 2},
            }
        ]

        result = audit.cross_dataset_summary(datasets)

        self.assertEqual(result["dominant_active_reason_by_dataset"]["primary"], ("Z_BIG", 2))
        self.assertEqual(result["dominant_hard_trigger_by_dataset"]["primary"], ("Z_BIG", 2))


if __name__ == "__main__":
    unittest.main()
