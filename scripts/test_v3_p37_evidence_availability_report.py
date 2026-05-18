import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_evidence_availability_report as report


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")


class P37EvidenceAvailabilityReportTest(unittest.TestCase):
    def test_decision_vectors_do_not_unblock_outcome_truth(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decision = root / "decisions.jsonl"
            threshold = root / "threshold.jsonl"
            labels = root / "labels.jsonl"
            feasibility = root / "feasibility.jsonl"
            write_jsonl(
                decision,
                [
                    {
                        "ab_record_id": "a",
                        "vectors_prices": [1.0, 1.1],
                        "vectors_ts_offsets_ms": [0, 100],
                        "v3_materialized_feature_snapshot": {
                            "checkpoint_features": {"price_trajectory": [1.0, 1.1]}
                        },
                    }
                ],
            )
            write_jsonl(threshold, [{"ab_record_id": "a", "threshold_window_max_return_pct": 50.0}])
            write_jsonl(labels, [{"ab_record_id": "a", "market_outcome_class": "good_dirty"}])
            write_jsonl(
                feasibility,
                [{"ab_record_id": "a", "decision_quality_class": "good_not_executable", "execution_quality_class": "no_dispatch_expected"}],
            )

            built = report.build_report(
                [
                    ("r11", decision, threshold, labels, feasibility, None),
                    ("r13", decision, threshold, labels, feasibility, None),
                ]
            )

        self.assertEqual(built["p3_7_evidence_status"], "blocked")
        self.assertIn("no_post_decision_price_path_rows", built["gate"]["blockers"])
        self.assertEqual(built["runs"][0]["decision_time_inputs"]["decision_vector_rows"], 1)
        self.assertTrue(built["runs"][0]["decision_time_inputs"]["not_outcome_truth_source"])
        self.assertEqual(built["runs"][0]["outcome_truth_evidence"]["post_decision_price_path_rows"], 0)
        self.assertTrue(built["runs"][0]["outcome_truth_evidence"]["threshold_summary_is_not_price_path"])

    def test_price_path_and_good_executable_can_unblock(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            decision = root / "decisions.jsonl"
            threshold = root / "threshold.jsonl"
            labels = root / "labels.jsonl"
            feasibility = root / "feasibility.jsonl"
            write_jsonl(decision, [{"ab_record_id": "a"}])
            write_jsonl(
                threshold,
                [
                    {
                        "ab_record_id": "a",
                        "price_path_samples": [{"ts_ms": 1, "return_pct": 1.0}],
                    }
                ],
            )
            write_jsonl(labels, [{"ab_record_id": "a", "market_outcome_class": "good_clean", "mfe_pct_10s": 45.0}])
            write_jsonl(
                feasibility,
                [{"ab_record_id": "a", "decision_quality_class": "good_executable", "execution_quality_class": "execution_feasible_clean"}],
            )

            built = report.build_report(
                [
                    ("r11", decision, threshold, labels, feasibility, None),
                    ("r13", decision, threshold, labels, feasibility, None),
                ]
            )

        self.assertEqual(built["p3_7_evidence_status"], "evidence_ready_for_temporal_target")
        self.assertEqual(built["gate"]["blockers"], [])

    def test_event_schema_sample_marks_candidate_events_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            events = root / "events"
            write_jsonl(
                events / "events.jsonl",
                [
                    {
                        "envelope": {"candidate_id": "a"},
                        "kind": {"type": "Candidate", "payload": {"source": "gatekeeper_v2"}},
                    }
                ],
            )
            sample = report.sample_event_schema(events)

        self.assertEqual(sample["classification"], "sampled_candidate_events_only")
        self.assertEqual(sample["file_count"], 1)
        self.assertEqual(sample["price_path_like_rows"], 0)

    def test_markdown_states_phase_b_block(self) -> None:
        built = {
            "gate": {
                "status": "blocked",
                "blockers": ["no_good_clean_rows"],
                "required_next_step": "obtain_or_derive_post_decision_price_path_or_lifecycle_evidence",
            },
            "runs": [
                {
                    "name": "r11",
                    "row_counts": {"label_v2_rows": 1},
                    "outcome_truth_evidence": {"post_decision_price_path_rows": 0, "label_v2_mfe_mae_rows": 0},
                    "decision_time_inputs": {"decision_vector_rows": 1, "checkpoint_price_trajectory_rows": 1},
                    "market_outcome_class_counts": {"good_clean": 0},
                    "decision_quality_class_counts": {"good_executable": 0},
                    "status": "blocked",
                    "event_dataset_schema_sample": {
                        "file_count": 1,
                        "sampled_rows": 1,
                        "classification": "sampled_candidate_events_only",
                        "price_path_like_rows": 0,
                    },
                }
            ],
        }

        markdown = report.render_markdown(built)

        self.assertIn("NO FEATURE PROTOTYPE", markdown)
        self.assertIn("Decision logs maja decision-time vectors", markdown)


if __name__ == "__main__":
    unittest.main()
