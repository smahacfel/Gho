import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import v3_p37_r17_replay_ready_preflight as r17


EXPECTED_NAMESPACE = r17.EXPECTED_NAMESPACE


def write_file(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def source_tree(root: Path, temporal_snapshots: bool = False) -> None:
    write_file(
        root / "ghost-brain/src/oracle/decision_logger.rs",
        """
pub const GATEKEEPER_BUY_LOG_SCHEMA_VERSION: u32 = 22;
pub gatekeeper_v2_replay_input_schema_version: Option<u32>,
pub gatekeeper_v2_replay_ready_non_temporal: Option<bool>,
pub gatekeeper_v2_replay_ready_temporal: Option<bool>,
pub gatekeeper_v2_phase_pass_vector: Option<serde_json::Value>,
pub decision_eval_snapshots: Option<serde_json::Value>,
""",
    )
    snapshot_literal = "Some(serde_json::json!([]))" if temporal_snapshots else "None"
    write_file(
        root / "ghost-launcher/src/components/gatekeeper.rs",
        f"""
GatekeeperBuyLog {{
  gatekeeper_v2_replay_input_schema_version: Some(1),
  gatekeeper_v2_replay_ready_non_temporal: Some(true),
  gatekeeper_v2_phase_pass_vector: Some(serde_json::json!({{}})),
  pdd_soft_penalty_points: Some(0),
  hard_fail_hhi_threshold: Some(0.20),
  observed_mode: Some(config.mode.to_string()),
  observed_window_ms: Some(config.max_wait_time_ms),
  observed_stage: Some(\"terminal\".to_string()),
  decision_eval_snapshots: {snapshot_literal},
}}
""",
    )


def brain_config(
    root: Path,
    replay_payload_enabled: bool = True,
    max_wait_time_ms: int = 10_000,
    normal_window_ms: int = 7_000,
    extended_window_ms: int = 10_000,
) -> Path:
    path = root / "configs/rollout/ghost_brain.toml"
    raw = "true" if replay_payload_enabled else "false"
    write_file(
        path,
        f"""
[gatekeeper_v2]
mode = "long"
max_wait_time_ms = {max_wait_time_ms}

[gatekeeper_v2.dow]
enabled = true
early_entry_min_ms = 2000
early_entry_max_ms = 5000
normal_window_ms = {normal_window_ms}
extended_window_ms = {extended_window_ms}

[gatekeeper_v3]
replay_payload_enabled = {raw}
""",
    )
    return path


def r17_config(root: Path, **overrides: object) -> Path:
    namespace = str(overrides.get("namespace", EXPECTED_NAMESPACE))
    execution_mode = str(overrides.get("execution_mode", "shadow"))
    entry_mode = str(overrides.get("entry_mode", "shadow_only"))
    payer = str(overrides.get("payer_strategy", "ephemeral"))
    temporal_enabled = "true" if overrides.get("temporal_enabled", True) else "false"
    allow_p2_live = "true" if overrides.get("allow_p2_live", False) else "false"
    snapshot_targets = overrides.get("snapshot_targets", [2000, 5000, 7000, 10000])
    snapshot_targets_toml = ", ".join(str(target) for target in snapshot_targets)
    path = root / "configs/rollout/r17.toml"
    write_file(
        path,
        f"""
mode = "production"
ghost_brain_config_path = "ghost_brain.toml"

[trigger]
enabled = true
entry_mode = "{entry_mode}"

[trigger.shadow_run]
enabled = true
payer_strategy = "{payer}"
max_concurrent = 1
output_path = "../../logs/shadow_run/{namespace}/buys.jsonl"

[execution]
execution_mode = "{execution_mode}"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/{namespace}/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/{namespace}/shadow_lifecycle.jsonl"

[execution.events]
output_dir = "../../datasets/events/{namespace}"

[p37_shadow_probe]
enabled = true
namespace = "{namespace}"
run_id = "{namespace}"
session_id = "r17-replay-ready-diagnostic"
max_probes_per_run = 15
max_concurrent = 1
include_verdict_types = ["BUY", "REJECT", "PENDING"]
exclude_active_buy_rows = false
require_ab_record_id = true
require_materialized_feature_set = true
require_v3_replay_payload = true
require_v3_feature_snapshot_hash = true
require_v3_policy_config_hash = true
require_execution_route_identity = true
require_curve_account_state = true
append = false
require_unique_namespace = true
selection_log_path = "../../logs/shadow_run/{namespace}/probe_selection.jsonl"
skip_log_path = "../../logs/shadow_run/{namespace}/probe_skips.jsonl"
transport_log_path = "../../logs/shadow_run/{namespace}/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/{namespace}/probe_shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/{namespace}/probe_shadow_lifecycle.jsonl"

[r17_replay_ready_contract]
enabled = true
gatekeeper_v2_replay_input_schema_version = 1
require_gatekeeper_v2_replay_contract = true
require_non_temporal_axes = true
require_temporal_decision_eval_snapshots = true
decision_eval_snapshots_enabled = {temporal_enabled}
decision_eval_snapshot_elapsed_ms = [{snapshot_targets_toml}]
include_terminal_snapshot = true
require_gatekeeper_v3_payload = true
require_execution_feasibility_fields = true
require_route_executable_universe_fields = true
allow_threshold_tuning = false
allow_phase_b = false
allow_p2_live = {allow_p2_live}

[oracle]
decision_log_path = "../../logs/rollout/{namespace}/decisions"

[durability]
wal_dir = "../../data/rollout/{namespace}/wal"
snapshot_dir = "../../data/rollout/{namespace}/snapshots"
""",
    )
    return path


class P37R17ReplayReadyPreflightTests(unittest.TestCase):
    def test_blocks_when_temporal_snapshot_runtime_support_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=False)
            brain_config(root)
            config = r17_config(root)

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "fail")
        self.assertEqual(report["final_decision"], "BLOCK_R17_TEMPORAL_SNAPSHOT_RUNTIME_GAP")
        self.assertIn("temporal_decision_eval_snapshots_runtime_not_implemented", report["blockers"])

    def test_passes_when_config_and_runtime_support_full_replay_contract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=True)
            brain_config(root)
            config = r17_config(root)

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "pass")
        self.assertEqual(report["final_decision"], "GO_R17_REPLAY_READY_DIAGNOSTIC_RUN")

    def test_blocks_live_or_wrong_namespace(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=True)
            brain_config(root)
            config = r17_config(
                root,
                namespace="shadow-burnin-v3-p37-wrong",
                execution_mode="live",
                entry_mode="live",
                allow_p2_live=True,
            )

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "fail")
        self.assertIn("namespace_mismatch", report["blockers"])
        self.assertIn("execution_mode_not_shadow", report["blockers"])
        self.assertIn("trigger_entry_mode_not_shadow_only", report["blockers"])
        self.assertIn("p2_live_allowed", report["blockers"])

    def test_blocks_missing_v3_replay_payload(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=True)
            brain_config(root, replay_payload_enabled=False)
            config = r17_config(root)

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "fail")
        self.assertIn("gatekeeper_v3_replay_payload_disabled", report["blockers"])

    def test_blocks_disabled_snapshot_contract_even_if_runtime_supports_it(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=True)
            brain_config(root)
            config = r17_config(root, temporal_enabled=False)

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "fail")
        self.assertIn("decision_eval_snapshots_disabled", report["blockers"])

    def test_blocks_snapshot_target_beyond_gatekeeper_window(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=True)
            brain_config(root, max_wait_time_ms=5_000, normal_window_ms=7_000, extended_window_ms=5_000)
            config = r17_config(root)

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "fail")
        self.assertIn(
            "temporal_snapshot_target_exceeds_gatekeeper_window:7000:max:5000",
            report["blockers"],
        )
        self.assertIn(
            "gatekeeper_dow_normal_window_exceeds_max_wait_time:7000:max:5000",
            report["blockers"],
        )

    def test_blocks_missing_terminal_snapshot_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_tree(root, temporal_snapshots=True)
            brain_config(root)
            config = r17_config(root, snapshot_targets=[2000, 5000, 7000])

            report = r17.build_preflight_report(config, repo_root=root)

        self.assertEqual(report["preflight_status"], "fail")
        self.assertIn("terminal_snapshot_target_missing_from_config:10000", report["blockers"])


if __name__ == "__main__":
    unittest.main()
