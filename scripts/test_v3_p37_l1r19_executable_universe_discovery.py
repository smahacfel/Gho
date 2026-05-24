import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_l1r19_executable_universe_discovery as l1r19


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")


def write_config(root: Path, namespace: str) -> Path:
    config = root / "configs/rollout/run.toml"
    config.parent.mkdir(parents=True)
    config.write_text(
        f"""
[oracle]
decision_log_path = "../../logs/rollout/{namespace}/decisions"

[trigger.shadow_run]
output_path = "../../logs/shadow_run/{namespace}/buys.jsonl"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/{namespace}/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/{namespace}/shadow_lifecycle.jsonl"

[p37_shadow_probe]
namespace = "{namespace}"
selection_log_path = "../../logs/shadow_run/{namespace}/probe_selection.jsonl"
skip_log_path = "../../logs/shadow_run/{namespace}/probe_skips.jsonl"
transport_log_path = "../../logs/shadow_run/{namespace}/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/{namespace}/probe_shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/{namespace}/probe_shadow_lifecycle.jsonl"
""".strip()
        + "\n",
        encoding="utf-8",
    )
    return config


class P37L1R19ExecutableUniverseDiscoveryTests(unittest.TestCase):
    def test_infers_executable_subset_from_successful_lifecycle_labels(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            namespace = "fixture-executable"
            config = write_config(root, namespace)
            decision_row = {"ab_record_id": "ab1", "candidate_id": "c1", "v3_replay_payload_schema_version": 1}
            write_jsonl(
                root / f"logs/rollout/{namespace}/decisions/gatekeeper_v2_decisions.jsonl",
                [decision_row],
            )
            write_jsonl(root / f"logs/shadow_run/{namespace}/probe_selection.jsonl", [decision_row])
            write_jsonl(
                root / f"logs/shadow_run/{namespace}/p3_7_probe_shadow_lifecycle_labels.jsonl",
                [
                    {
                        **decision_row,
                        "buy_quality_class": "buy_quality_dirty_good",
                        "execution_feasibility_status": "executable",
                    }
                ],
            )

            report = l1r19.build_discovery_report([config])

        self.assertEqual(report["final_decision"], "GO_L2_EXECUTABLE_SUBSET")
        run = report["runs"][0]
        self.assertEqual(run["route_executable_rows"], 1)
        self.assertEqual(run["route_executable_evidence"], "inferred_from_successful_entry_or_lifecycle_label")
        self.assertEqual(run["buy_quality_denominator_rows"], 1)
        self.assertEqual(run["buy_quality_dirty_good"], 1)

    def test_blocks_when_only_execution_feasibility_rejects(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            namespace = "fixture-route-blocked"
            config = write_config(root, namespace)
            row = {
                "ab_record_id": "ab1",
                "candidate_id": "c1",
                "route_resolution_status": "no_executable_route_account_set",
                "execution_feasibility_status": "not_executable_route",
                "execution_feasibility_reason": "no_executable_route_account_set",
                "lifecycle_label_eligibility": "not_lifecycle_eligible",
            }
            write_jsonl(
                root / f"logs/rollout/{namespace}/decisions/gatekeeper_v2_decisions.jsonl",
                [{**row, "v3_replay_payload_schema_version": 1}],
            )
            write_jsonl(root / f"logs/shadow_run/{namespace}/probe_selection.jsonl", [row])
            write_jsonl(root / f"logs/shadow_run/{namespace}/probe_skips.jsonl", [row])

            report = l1r19.build_discovery_report([config])

        self.assertEqual(report["final_decision"], "BLOCK_L2_ROUTE_SUPPORT_REQUIRED")
        run = report["runs"][0]
        self.assertEqual(run["audit_evidence_status"], "execution_route_support_blocked")
        self.assertEqual(run["route_non_executable_rows"], 1)
        self.assertEqual(run["execution_feasibility_reject_rows"], 1)

    def test_blocks_audit_gap_without_route_or_label_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            namespace = "fixture-audit-gap"
            config = write_config(root, namespace)
            row = {"ab_record_id": "ab1", "candidate_id": "c1", "v3_replay_payload_schema_version": 1}
            write_jsonl(
                root / f"logs/rollout/{namespace}/decisions/gatekeeper_v2_decisions.jsonl",
                [row],
            )
            write_jsonl(root / f"logs/shadow_run/{namespace}/probe_selection.jsonl", [row])

            report = l1r19.build_discovery_report([config])

        self.assertEqual(report["final_decision"], "BLOCK_L2_AUDIT_GAP")
        run = report["runs"][0]
        self.assertEqual(run["audit_evidence_status"], "audit_gap")
        self.assertIn("no_execution_feasibility_or_successful_entry_evidence", run["audit_gap_reasons"])


if __name__ == "__main__":
    unittest.main()
