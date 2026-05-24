import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l2d1_non_temporal_axis_replay as l2d1


J4C = l2d1.J4C_NAMESPACE
R16 = l2d1.R16_R1_NAMESPACE
BLOCKED = "shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver"


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


def artifact(path: Path, role: str) -> dict:
    return {
        "exists": True,
        "path": str(path),
        "role": role,
        "sha256": l2d1.l2a.sha256_file(path),
    }


def label(ab_record_id: str, quality: str, namespace: str) -> dict:
    return {
        "source_ab_record_id": ab_record_id,
        "buy_quality_class": quality,
        "rollout_namespace": namespace,
        "run_id": namespace,
    }


def decision(
    ab_record_id: str,
    *,
    verdict: str = "REJECT",
    trace: list[dict] | None = None,
    extra: dict | None = None,
) -> dict:
    payload = {
        "ab_record_id": ab_record_id,
        "decision_verdict_buy": verdict == "BUY",
        "legacy_live_verdict_type": verdict,
        "reason_code": "BUY_NORMAL" if verdict == "BUY" else f"{verdict}_CORE_FAIL",
        "gatekeeper_terminal_gate": "buy" if verdict == "BUY" else "core",
        "gatekeeper_gate_trace": trace,
        "v3_materialized_feature_snapshot": {"session_metadata": {}},
        "v3_policy_config_payload": {"profiles": {}},
        "v3_shadow_verdict": "REJECT",
        "v3_shadow_reason_code": "REJECT_V3_MANIPULATION_CONTRADICTION",
        "v3_replay_payload_schema_version": 1,
        "v3_materialization_version": 1,
    }
    if extra:
        payload.update(extra)
    return payload


PDD_ONLY_FAIL_TRACE = [
    {"gate": "phase1_quantity", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "pdd", "status": "fail", "hard_or_soft": "hard"},
    {"gate": "core1", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core2", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core3", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "soft_budget", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "alpha", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "prosperity", "status": "pass", "hard_or_soft": "hard"},
]

HHI_AND_TOP3_FAIL_TRACE = [
    {"gate": "phase1_quantity", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "diversity_hhi_hard_fail", "status": "fail", "hard_or_soft": "hard"},
    {"gate": "diversity_top3_hard_fail", "status": "fail", "hard_or_soft": "hard"},
    {"gate": "pdd", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "core1", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core2", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core3", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "soft_budget", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "alpha", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "prosperity", "status": "pass", "hard_or_soft": "hard"},
]

PASS_TRACE = [
    {"gate": "phase1_quantity", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "pdd", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "core1", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core2", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "core3", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "soft_budget", "status": "pass", "hard_or_soft": "soft"},
    {"gate": "alpha", "status": "pass", "hard_or_soft": "hard"},
    {"gate": "prosperity", "status": "pass", "hard_or_soft": "hard"},
]


def build_manifest(
    root: Path,
    *,
    j4c_decision: dict,
    j4c_label: dict,
    r16_decision: dict | None = None,
    r16_label: dict | None = None,
) -> Path:
    j4c_label_path = root / "j4c_labels.jsonl"
    j4c_decision_path = root / "j4c_decisions.jsonl"
    r16_label_path = root / "r16_labels.jsonl"
    r16_decision_path = root / "r16_decisions.jsonl"
    manifest_path = root / "manifest.json"

    write_jsonl(j4c_label_path, [j4c_label])
    write_jsonl(j4c_decision_path, [j4c_decision])
    write_jsonl(r16_label_path, [r16_label] if r16_label else [])
    write_jsonl(r16_decision_path, [r16_decision] if r16_decision else [])

    dirty = sum(
        1
        for row in (j4c_label, r16_label)
        if row and row["buy_quality_class"] == "buy_quality_dirty_good"
    )
    rows = 1 + (1 if r16_label else 0)
    manifest = {
        "manifest_status": "pass",
        "final_decision": "GO_L2_INPUT_MANIFEST_LOCKED",
        "actual_contract": {
            "allowed_runs": [J4C, R16],
            "blocked_runs": [BLOCKED],
            "buy_quality_denominator_rows": rows,
            "buy_quality_dirty_good": dirty,
            "dirty_good_rate": dirty / rows,
        },
        "allowed_run_manifests": [
            {
                "namespace": J4C,
                "artifacts": [
                    artifact(j4c_decision_path, "decision"),
                    artifact(j4c_label_path, "lifecycle_label_file"),
                ],
            },
            {
                "namespace": R16,
                "artifacts": [
                    artifact(r16_decision_path, "decision"),
                    artifact(r16_label_path, "lifecycle_label_file"),
                ],
            },
        ],
    }
    write_json(manifest_path, manifest)
    return manifest_path


class P37L2D1NonTemporalAxisReplayTests(unittest.TestCase):
    def run_report(self, manifest: Path, axes: list[str], rows: int, dirty: int) -> dict:
        with patch.object(l2d1, "EXPECTED_DENOMINATOR", rows), patch.object(
            l2d1, "EXPECTED_DIRTY_GOOD", dirty
        ), patch.object(l2d1.l2c, "EXPECTED_DENOMINATOR", rows), patch.object(
            l2d1.l2c, "EXPECTED_DIRTY_GOOD", dirty
        ), patch.object(l2d1.l2a, "EXPECTED_DENOMINATOR", rows), patch.object(
            l2d1.l2a, "EXPECTED_DIRTY_GOOD", dirty
        ):
            return l2d1.build_report(manifest, axes)

    def test_blocks_when_baseline_j4c_row_lacks_gate_trace(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = build_manifest(
                root,
                j4c_decision=decision("j1", trace=None),
                j4c_label=label("j1", "buy_quality_bad", J4C),
            )
            report = self.run_report(manifest, ["soft_pdd_instead_of_hard_pdd"], 1, 0)

        self.assertEqual(report["analysis_status"], "pass")
        self.assertEqual(report["final_decision"], "BLOCK_L2D1_GATEKEEPER_V2_AXIS_REPLAY_INPUT_GAP")
        axis = report["axis_results"]["soft_pdd_instead_of_hard_pdd"]
        self.assertEqual(axis["axis_status"], "blocked_no_causal_replay_rows")
        self.assertTrue(
            any(
                status.startswith("unsupported_missing_fields:gatekeeper_gate_trace")
                for status in axis["row_status_counts"]
            )
        )

    def test_soft_pdd_axis_can_convert_baseline_reject_when_budget_allows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = build_manifest(
                root,
                j4c_decision=decision(
                    "j1",
                    trace=PDD_ONLY_FAIL_TRACE,
                    extra={
                        "pdd_hard_fail": "WHALE",
                        "soft_points": 2,
                        "max_soft_points": 8,
                        "pdd_soft_penalty_points": 3,
                    },
                ),
                j4c_label=label("j1", "buy_quality_dirty_good", J4C),
            )
            report = self.run_report(manifest, ["soft_pdd_instead_of_hard_pdd"], 1, 1)

        axis = report["axis_results"]["soft_pdd_instead_of_hard_pdd"]
        self.assertEqual(report["final_decision"], "GO_L2D1_NON_TEMPORAL_AXIS_REPLAY_RESULTS")
        self.assertEqual(axis["axis_evaluable_rows"], 1)
        self.assertEqual(axis["variant_buy_rows"], 1)
        self.assertEqual(axis["accepted_dirty_good"], 1)
        self.assertEqual(axis["changed_from_reject_to_buy"], 1)

    def test_hhi_relax_does_not_buy_when_top3_still_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = build_manifest(
                root,
                j4c_decision=decision(
                    "j1",
                    trace=HHI_AND_TOP3_FAIL_TRACE,
                    extra={
                        "hhi": 0.15,
                        "max_hhi": 0.155,
                        "top3_volume_pct": 0.99,
                        "same_ms_tx_ratio": 0.1,
                    },
                ),
                j4c_label=label("j1", "buy_quality_bad", J4C),
            )
            report = self.run_report(manifest, ["hhi_hard_fail_relaxed"], 1, 0)

        axis = report["axis_results"]["hhi_hard_fail_relaxed"]
        self.assertEqual(axis["axis_evaluable_rows"], 1)
        self.assertEqual(axis["variant_buy_rows"], 0)
        self.assertEqual(axis["accepted_bad"], 0)
        self.assertEqual(axis["avoided_bad"], 1)

    def test_r16_rows_are_not_forward_causal_for_already_applied_axes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = build_manifest(
                root,
                j4c_decision=decision(
                    "j1",
                    trace=PDD_ONLY_FAIL_TRACE,
                    extra={
                        "pdd_hard_fail": "WHALE",
                        "soft_points": 2,
                        "max_soft_points": 8,
                        "pdd_soft_penalty_points": 3,
                    },
                ),
                j4c_label=label("j1", "buy_quality_bad", J4C),
                r16_decision=decision("r1", verdict="BUY", trace=PASS_TRACE),
                r16_label=label("r1", "buy_quality_dirty_good", R16),
            )
            report = self.run_report(manifest, ["prosperity_filter_disabled"], 2, 1)

        axis = report["axis_results"]["prosperity_filter_disabled"]
        self.assertIn("unsupported_axis_already_applied_in_source_run", axis["row_status_counts"])
        self.assertIn("unsupported_missing_fields:prosperity_filter_enabled,aps_shadow_prosperity_would_pass", axis["row_status_counts"])

    def test_standard_axis_is_temporal_unsupported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = build_manifest(
                root,
                j4c_decision=decision("j1", trace=PASS_TRACE),
                j4c_label=label("j1", "buy_quality_bad", J4C),
            )
            report = self.run_report(manifest, ["standard_mode_shorter_window"], 1, 0)

        axis = report["axis_results"]["standard_mode_shorter_window"]
        self.assertEqual(axis["axis_status"], "unsupported_temporal_replay_required")
        self.assertIn("unsupported_temporal_replay_required", axis["row_status_counts"])


if __name__ == "__main__":
    unittest.main()
