#!/usr/bin/env python3
"""P3.7-R17 replay-ready diagnostic preflight.

This preflight protects R17 from becoming another blind runtime smoke. It checks
the rollout config, the Ghost brain V3 replay payload config, and the current
runtime source support for the Gatekeeper V2 replay-input contract.

The temporal standard-window axis is intentionally fail-closed: a config marker
is not enough. Current runtime code must actually emit decision_eval_snapshots.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python 3.10 fallback only
    import tomli as tomllib  # type: ignore


SCHEMA_VERSION = 1
REPLAY_INPUT_SCHEMA_VERSION = 1
EXPECTED_NAMESPACE = "shadow-burnin-v3-p37-r17-replay-ready-diagnostic"
DEFAULT_CONFIG = Path(
    "configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml"
)
REQUIRED_VERDICT_TYPES = {"BUY", "REJECT", "PENDING"}
REQUIRED_TEMPORAL_SNAPSHOT_MS = {2000, 5000, 7000}


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def get_path(data: dict[str, Any], keys: list[str], default: Any = None) -> Any:
    value: Any = data
    for key in keys:
        if not isinstance(value, dict) or key not in value:
            return default
        value = value[key]
    return value


def bool_path(data: dict[str, Any], keys: list[str], default: bool = False) -> bool:
    return bool(get_path(data, keys, default))


def str_path(data: dict[str, Any], keys: list[str], default: str = "") -> str:
    value = get_path(data, keys, default)
    return str(value) if value is not None else default


def int_path(data: dict[str, Any], keys: list[str], default: int = 0) -> int:
    value = get_path(data, keys, default)
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def flatten_strings(data: Any) -> list[str]:
    values: list[str] = []
    if isinstance(data, dict):
        for value in data.values():
            values.extend(flatten_strings(value))
    elif isinstance(data, list):
        for value in data:
            values.extend(flatten_strings(value))
    elif isinstance(data, str):
        values.append(data)
    return values


def resolve_path(raw: str, base: Path) -> Path:
    path = Path(raw)
    if path.is_absolute():
        return path
    return (base / path).resolve()


def ghost_brain_config_path(config: dict[str, Any], config_path: Path) -> Path:
    raw = str_path(config, ["ghost_brain_config_path"])
    return resolve_path(raw, config_path.parent) if raw else Path()


def load_ghost_brain_config(config: dict[str, Any], config_path: Path) -> tuple[dict[str, Any] | None, str]:
    path = ghost_brain_config_path(config, config_path)
    if not path:
        return None, "missing_ghost_brain_config_path"
    if not path.exists():
        return None, f"ghost_brain_config_missing:{path}"
    try:
        return load_toml(path), "ok"
    except Exception as err:  # pragma: no cover - exact TOML errors vary
        return None, f"ghost_brain_config_parse_error:{err}"


def ghost_brain_replay_payload_enabled(config: dict[str, Any], config_path: Path) -> tuple[bool, str]:
    brain, reason = load_ghost_brain_config(config, config_path)
    if brain is None:
        return False, reason
    enabled = bool_path(brain, ["gatekeeper_v3", "replay_payload_enabled"])
    return enabled, "ok" if enabled else "gatekeeper_v3_replay_payload_disabled"


def gatekeeper_v2_snapshot_contract(
    config: dict[str, Any], config_path: Path, snapshot_ms: set[int]
) -> tuple[list[str], dict[str, Any]]:
    blockers: list[str] = []
    brain, reason = load_ghost_brain_config(config, config_path)
    if brain is None:
        return [reason], {"ghost_brain_config_reason": reason}

    gatekeeper_v2 = get_path(brain, ["gatekeeper_v2"], {})
    if not isinstance(gatekeeper_v2, dict):
        return ["gatekeeper_v2_config_missing"], {"ghost_brain_config_reason": "gatekeeper_v2_config_missing"}

    max_wait_time_ms = int_path(gatekeeper_v2, ["max_wait_time_ms"])
    dow = get_path(gatekeeper_v2, ["dow"], {})
    if not isinstance(dow, dict):
        dow = {}
    dow_enabled = bool_path(gatekeeper_v2, ["dow", "enabled"])
    normal_window_ms = int_path(gatekeeper_v2, ["dow", "normal_window_ms"])
    extended_window_ms = int_path(gatekeeper_v2, ["dow", "extended_window_ms"])

    if max_wait_time_ms <= 0:
        blockers.append("gatekeeper_v2_max_wait_time_missing")
    for target_ms in sorted(snapshot_ms):
        if max_wait_time_ms > 0 and target_ms > max_wait_time_ms:
            blockers.append(
                f"temporal_snapshot_target_exceeds_gatekeeper_window:{target_ms}:max:{max_wait_time_ms}"
            )
    if dow_enabled and normal_window_ms > 0 and max_wait_time_ms > 0 and normal_window_ms > max_wait_time_ms:
        blockers.append(
            f"gatekeeper_dow_normal_window_exceeds_max_wait_time:{normal_window_ms}:max:{max_wait_time_ms}"
        )
    if dow_enabled and extended_window_ms > 0 and normal_window_ms > 0 and extended_window_ms < normal_window_ms:
        blockers.append(
            f"gatekeeper_dow_extended_window_before_normal_window:{extended_window_ms}:normal:{normal_window_ms}"
        )

    return blockers, {
        "gatekeeper_v2_mode": str_path(gatekeeper_v2, ["mode"]),
        "gatekeeper_v2_max_wait_time_ms": max_wait_time_ms,
        "gatekeeper_v2_dow_enabled": dow_enabled,
        "gatekeeper_v2_dow_normal_window_ms": normal_window_ms,
        "gatekeeper_v2_dow_extended_window_ms": extended_window_ms,
    }


def runtime_source_support(repo_root: Path) -> dict[str, Any]:
    decision_logger_path = repo_root / "ghost-brain/src/oracle/decision_logger.rs"
    gatekeeper_path = repo_root / "ghost-launcher/src/components/gatekeeper.rs"

    decision_logger = decision_logger_path.read_text(encoding="utf-8") if decision_logger_path.exists() else ""
    gatekeeper = gatekeeper_path.read_text(encoding="utf-8") if gatekeeper_path.exists() else ""

    decision_logger_has_v22_fields = all(
        token in decision_logger
        for token in [
            "GATEKEEPER_BUY_LOG_SCHEMA_VERSION: u32 = 22",
            "gatekeeper_v2_replay_input_schema_version",
            "gatekeeper_v2_replay_ready_non_temporal",
            "gatekeeper_v2_replay_ready_temporal",
            "gatekeeper_v2_phase_pass_vector",
            "decision_eval_snapshots",
        ]
    )
    gatekeeper_emits_non_temporal_contract = all(
        token in gatekeeper
        for token in [
            "gatekeeper_v2_replay_input_schema_version: Some(1)",
            "gatekeeper_v2_replay_ready_non_temporal",
            "gatekeeper_v2_phase_pass_vector",
            "pdd_soft_penalty_points",
            "hard_fail_hhi_threshold",
            "observed_mode",
            "observed_window_ms",
            "observed_stage",
        ]
    )
    gatekeeper_emits_temporal_snapshots = "decision_eval_snapshots: Some" in gatekeeper
    gatekeeper_hardcodes_temporal_snapshots_none = "decision_eval_snapshots: None" in gatekeeper

    return {
        "decision_logger_has_v22_fields": decision_logger_has_v22_fields,
        "gatekeeper_emits_non_temporal_contract": gatekeeper_emits_non_temporal_contract,
        "gatekeeper_emits_temporal_snapshots": gatekeeper_emits_temporal_snapshots,
        "gatekeeper_hardcodes_temporal_snapshots_none": gatekeeper_hardcodes_temporal_snapshots_none,
        "decision_logger_path": str(decision_logger_path),
        "gatekeeper_path": str(gatekeeper_path),
    }


def add_if(blockers: list[str], condition: bool, code: str) -> None:
    if condition:
        blockers.append(code)


def validate_config(config: dict[str, Any], config_path: Path, repo_root: Path) -> dict[str, Any]:
    blockers: list[str] = []
    warnings: list[str] = []

    namespace = str_path(config, ["p37_shadow_probe", "namespace"])
    contract = get_path(config, ["r17_replay_ready_contract"], {})
    if not isinstance(contract, dict):
        contract = {}

    add_if(blockers, namespace != EXPECTED_NAMESPACE, "namespace_mismatch")
    add_if(blockers, str_path(config, ["execution", "execution_mode"]) != "shadow", "execution_mode_not_shadow")
    add_if(blockers, str_path(config, ["trigger", "entry_mode"]) != "shadow_only", "trigger_entry_mode_not_shadow_only")
    add_if(blockers, not bool_path(config, ["trigger", "enabled"]), "trigger_disabled")
    add_if(blockers, not bool_path(config, ["trigger", "shadow_run", "enabled"]), "shadow_run_disabled")
    add_if(blockers, str_path(config, ["trigger", "shadow_run", "payer_strategy"]) != "ephemeral", "shadow_payer_not_ephemeral")
    add_if(blockers, int_path(config, ["trigger", "shadow_run", "max_concurrent"], 99) > 1, "shadow_run_max_concurrent_gt_1")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "enabled"]), "p37_shadow_probe_disabled")
    add_if(blockers, int_path(config, ["p37_shadow_probe", "max_concurrent"], 99) > 1, "probe_max_concurrent_gt_1")
    add_if(blockers, int_path(config, ["p37_shadow_probe", "max_probes_per_run"], 9999) > 15, "probe_cap_gt_15")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_ab_record_id"]), "ab_record_id_not_required")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_materialized_feature_set"]), "mfs_not_required")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_v3_replay_payload"]), "v3_replay_payload_not_required")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_v3_feature_snapshot_hash"]), "v3_feature_snapshot_hash_not_required")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_v3_policy_config_hash"]), "v3_policy_config_hash_not_required")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_execution_route_identity"]), "execution_route_identity_not_required")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_curve_account_state"]), "curve_account_state_not_required")
    add_if(blockers, bool_path(config, ["p37_shadow_probe", "append"]), "probe_append_enabled")
    add_if(blockers, not bool_path(config, ["p37_shadow_probe", "require_unique_namespace"]), "unique_namespace_not_required")
    add_if(blockers, bool_path(config, ["r17_replay_ready_contract", "allow_p2_live"]), "p2_live_allowed")
    add_if(blockers, bool_path(config, ["r17_replay_ready_contract", "allow_phase_b"]), "phase_b_allowed")
    add_if(blockers, bool_path(config, ["r17_replay_ready_contract", "allow_threshold_tuning"]), "threshold_tuning_allowed")

    include_verdicts = set(get_path(config, ["p37_shadow_probe", "include_verdict_types"], []) or [])
    add_if(blockers, not REQUIRED_VERDICT_TYPES.issubset(include_verdicts), "probe_verdict_types_incomplete")
    add_if(blockers, bool_path(config, ["p37_shadow_probe", "exclude_active_buy_rows"], True), "active_buy_rows_excluded")

    replay_payload_enabled, replay_reason = ghost_brain_replay_payload_enabled(config, config_path)
    add_if(blockers, not replay_payload_enabled, replay_reason)

    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "enabled"]), "r17_replay_contract_disabled")
    add_if(
        blockers,
        int_path(config, ["r17_replay_ready_contract", "gatekeeper_v2_replay_input_schema_version"]) != REPLAY_INPUT_SCHEMA_VERSION,
        "r17_replay_contract_schema_mismatch",
    )
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "require_gatekeeper_v2_replay_contract"]), "v2_replay_contract_not_required")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "require_non_temporal_axes"]), "non_temporal_axes_not_required")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "require_temporal_decision_eval_snapshots"]), "temporal_snapshots_not_required")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "decision_eval_snapshots_enabled"]), "decision_eval_snapshots_disabled")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "include_terminal_snapshot"]), "terminal_snapshot_not_required")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "require_gatekeeper_v3_payload"]), "gatekeeper_v3_payload_not_required")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "require_execution_feasibility_fields"]), "execution_feasibility_fields_not_required")
    add_if(blockers, not bool_path(config, ["r17_replay_ready_contract", "require_route_executable_universe_fields"]), "route_executable_universe_fields_not_required")

    snapshot_ms = set(get_path(config, ["r17_replay_ready_contract", "decision_eval_snapshot_elapsed_ms"], []) or [])
    add_if(blockers, not REQUIRED_TEMPORAL_SNAPSHOT_MS.issubset(snapshot_ms), "temporal_snapshot_checkpoints_incomplete")
    gatekeeper_snapshot_blockers, gatekeeper_snapshot_contract = gatekeeper_v2_snapshot_contract(
        config, config_path, snapshot_ms
    )
    blockers.extend(gatekeeper_snapshot_blockers)

    namespace_path_violations = []
    for value in flatten_strings(config):
        if (
            ("../../logs/" in value or "../../datasets/" in value or "../../data/" in value)
            and EXPECTED_NAMESPACE not in value
        ):
            namespace_path_violations.append(value)
    if namespace_path_violations:
        blockers.append("output_path_namespace_mismatch")

    support = runtime_source_support(repo_root)
    add_if(blockers, not support["decision_logger_has_v22_fields"], "decision_logger_v22_replay_fields_missing")
    add_if(blockers, not support["gatekeeper_emits_non_temporal_contract"], "gatekeeper_non_temporal_replay_contract_missing")
    if bool_path(config, ["r17_replay_ready_contract", "require_temporal_decision_eval_snapshots"]):
        add_if(
            blockers,
            not support["gatekeeper_emits_temporal_snapshots"],
            "temporal_decision_eval_snapshots_runtime_not_implemented",
        )
        if support["gatekeeper_hardcodes_temporal_snapshots_none"]:
            warnings.append("gatekeeper_currently_emits_decision_eval_snapshots_none")

    return {
        "namespace": namespace,
        "blockers": sorted(set(blockers)),
        "warnings": sorted(set(warnings)),
        "ghost_brain_replay_payload_enabled": replay_payload_enabled,
        "ghost_brain_replay_payload_reason": replay_reason,
        "include_verdict_types": sorted(include_verdicts),
        "required_temporal_snapshot_ms": sorted(REQUIRED_TEMPORAL_SNAPSHOT_MS),
        "configured_temporal_snapshot_ms": sorted(snapshot_ms),
        "gatekeeper_snapshot_contract": gatekeeper_snapshot_contract,
        "runtime_support": support,
        "namespace_path_violations": namespace_path_violations,
    }


def build_preflight_report(config_path: Path, repo_root: Path | None = None) -> dict[str, Any]:
    repo = repo_root or Path(__file__).resolve().parents[1]
    config_path = config_path if config_path.is_absolute() else (repo / config_path)

    if not config_path.exists():
        return {
            "schema_version": SCHEMA_VERSION,
            "report_name": "P3.7-R17 Replay-Ready Diagnostic Preflight",
            "config_path": str(config_path),
            "preflight_status": "fail",
            "final_decision": "BLOCK_R17_REPLAY_READY_PREFLIGHT",
            "blockers": ["config_missing"],
            "recommended_next_path": "create_r17_replay_ready_config",
        }

    try:
        config = load_toml(config_path)
    except Exception as err:  # pragma: no cover - exact TOML errors vary
        return {
            "schema_version": SCHEMA_VERSION,
            "report_name": "P3.7-R17 Replay-Ready Diagnostic Preflight",
            "config_path": str(config_path),
            "preflight_status": "fail",
            "final_decision": "BLOCK_R17_REPLAY_READY_PREFLIGHT",
            "blockers": [f"config_parse_error:{err}"],
            "recommended_next_path": "repair_r17_config",
        }

    validation = validate_config(config, config_path, repo)
    blockers = validation["blockers"]
    if not blockers:
        final_decision = "GO_R17_REPLAY_READY_DIAGNOSTIC_RUN"
        recommended_next_path = "start_bounded_r17_replay_ready_diagnostic"
    elif "temporal_decision_eval_snapshots_runtime_not_implemented" in blockers:
        final_decision = "BLOCK_R17_TEMPORAL_SNAPSHOT_RUNTIME_GAP"
        recommended_next_path = "implement_temporal_decision_eval_snapshots_before_r17"
    else:
        final_decision = "BLOCK_R17_REPLAY_READY_PREFLIGHT"
        recommended_next_path = "repair_r17_config_or_replay_contract"

    return {
        "schema_version": SCHEMA_VERSION,
        "report_name": "P3.7-R17 Replay-Ready Diagnostic Preflight",
        "config_path": str(config_path),
        "preflight_status": "pass" if not blockers else "fail",
        "final_decision": final_decision,
        "blockers": blockers,
        "warnings": validation["warnings"],
        "namespace": validation["namespace"],
        "safety_contract": {
            "execution_mode": str_path(config, ["execution", "execution_mode"]),
            "trigger_entry_mode": str_path(config, ["trigger", "entry_mode"]),
            "shadow_payer_strategy": str_path(config, ["trigger", "shadow_run", "payer_strategy"]),
            "shadow_run_max_concurrent": int_path(config, ["trigger", "shadow_run", "max_concurrent"]),
            "probe_max_concurrent": int_path(config, ["p37_shadow_probe", "max_concurrent"]),
            "probe_cap": int_path(config, ["p37_shadow_probe", "max_probes_per_run"]),
        },
        "replay_contract": {
            "ghost_brain_replay_payload_enabled": validation["ghost_brain_replay_payload_enabled"],
            "ghost_brain_replay_payload_reason": validation["ghost_brain_replay_payload_reason"],
            "include_verdict_types": validation["include_verdict_types"],
            "required_temporal_snapshot_ms": validation["required_temporal_snapshot_ms"],
            "configured_temporal_snapshot_ms": validation["configured_temporal_snapshot_ms"],
            "gatekeeper_snapshot_contract": validation["gatekeeper_snapshot_contract"],
        },
        "runtime_support": validation["runtime_support"],
        "namespace_path_violations": validation["namespace_path_violations"],
        "recommended_next_path": recommended_next_path,
        "non_goals_enforced": [
            "no_threshold_tuning",
            "no_phase_b",
            "no_p2_live",
            "no_runtime_started_by_preflight",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--json", action="store_true", help="print JSON report to stdout")
    parser.add_argument("--output-json", type=Path, help="optional path to write JSON report")
    args = parser.parse_args()

    report = build_preflight_report(args.config)
    if args.output_json:
        args.output_json.parent.mkdir(parents=True, exist_ok=True)
        args.output_json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.json or not args.output_json:
        print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["preflight_status"] == "pass" else 2


if __name__ == "__main__":
    raise SystemExit(main())
