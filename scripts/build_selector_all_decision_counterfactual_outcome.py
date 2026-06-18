#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

import audit_selector_business_target_rate as business_target_rate
import selector_pipeline_common as common


BUSINESS_LABELS = ("TARGET", "STOP", "TIMEOUT")
UNRESOLVED_LABELS = (
    "AMBIGUOUS_BARRIER_ORDER",
    "HORIZON_UNMATURED",
    "MISSING_PATH",
    "NO_SAMPLES",
    "NONCANONICAL_SOURCE",
    "STREAM_INCOMPLETE",
)


def resolve_rooted(root: Path, path: Path | None) -> Path | None:
    if path is None:
        return None
    return path if path.is_absolute() else root / path


def finite_float(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    return None


def stable_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def first_str(row: dict[str, Any], *fields: str) -> str | None:
    for field in fields:
        value = row.get(field)
        if isinstance(value, str) and value:
            return value
    return None


def first_int(row: dict[str, Any], *fields: str) -> int | None:
    for field in fields:
        value = common.int_or_none(row.get(field))
        if value is not None:
            return value
    return None


def parse_ab_record_id_timestamps(value: str | None) -> tuple[int | None, int | None]:
    if not isinstance(value, str) or not value:
        return None, None
    parts = value.rsplit(":", 3)
    if len(parts) != 4:
        return None, None
    first_seen_raw, decision_raw = parts[1], parts[2]
    if not first_seen_raw.isdigit() or not decision_raw.isdigit():
        return None, None
    return int(first_seen_raw), int(decision_raw)


def key_ab(row: dict[str, Any]) -> str | None:
    return first_str(row, "ab_record_id", "source_ab_record_id")


def decision_cutoff_ts_ms(row: dict[str, Any]) -> int | None:
    explicit = first_int(
        row,
        "decision_ts_ms",
        "feature_cutoff_ts_ms",
        "observation_end_ts_ms",
        "decision_timestamp_ms",
    )
    if explicit is not None:
        return explicit
    _first_seen_ts_ms, ab_decision_ts_ms = parse_ab_record_id_timestamps(key_ab(row))
    if ab_decision_ts_ms is not None:
        return ab_decision_ts_ms
    return first_int(row, "first_seen_ts_ms", "timestamp_ms")


def key_candidate(row: dict[str, Any]) -> str | None:
    return first_str(row, "candidate_id", "r2_candidate_id")


def key_pool_mint_ts(row: dict[str, Any]) -> str | None:
    pool_id = first_str(row, "pool_id", "pool_amm_id")
    base_mint = first_str(row, "base_mint", "mint_id")
    decision_ts_ms = decision_cutoff_ts_ms(row)
    if not pool_id or not base_mint or decision_ts_ms is None:
        return None
    return f"{pool_id}|{base_mint}|{decision_ts_ms}"


def key_pool_mint(row: dict[str, Any]) -> str | None:
    pool_id = first_str(row, "pool_id", "pool_amm_id")
    base_mint = first_str(row, "base_mint", "mint_id")
    if not pool_id or not base_mint:
        return None
    return f"{pool_id}|{base_mint}"


def all_decision_id(row: dict[str, Any]) -> tuple[str, str]:
    ab_record_id = key_ab(row)
    if ab_record_id:
        return f"ab_record_id:{ab_record_id}", "ab_record_id"
    pool_mint_ts = key_pool_mint_ts(row)
    if pool_mint_ts:
        return f"pool_mint_ts:{pool_mint_ts}", "pool_mint_ts"
    pool_mint = key_pool_mint(row)
    if pool_mint:
        return f"pool_mint:{pool_mint}", "pool_mint"
    digest = hashlib.sha256(stable_json(row).encode("utf-8")).hexdigest()
    return f"stable_json_sha256:{digest}", "stable_json_sha256"


def resolved_candidate_id(
    decision: dict[str, Any],
    *,
    r2_row: dict[str, Any] | None,
    selection: dict[str, Any] | None,
    transport: dict[str, Any] | None,
    entry: dict[str, Any] | None,
    lifecycle: dict[str, Any] | None,
    fallback_id: str,
) -> tuple[str, str]:
    closed = (lifecycle or {}).get("position_closed") if lifecycle is not None else None
    candidates = (
        ("decision_candidate_id", key_candidate(decision)),
        ("r2_candidate_id", key_candidate(r2_row or {})),
        ("probe_selection_candidate_id", key_candidate(selection or {})),
        ("probe_transport_candidate_id", key_candidate(transport or {})),
        ("probe_entry_candidate_id", key_candidate(entry or {})),
        ("probe_lifecycle_candidate_id", key_candidate(closed or {})),
    )
    for source, candidate_id in candidates:
        if candidate_id:
            return candidate_id, source
    return fallback_id, "synthesized_all_decision_id"


def event_time_key(sample: dict[str, Any], decision_ts_ms: int | None) -> int | None:
    offset_ms = common.int_or_none(sample.get("offset_ms"))
    if offset_ms is not None:
        return offset_ms
    ts_ms = common.int_or_none(sample.get("ts_ms"))
    if ts_ms is not None and decision_ts_ms is not None:
        return ts_ms - decision_ts_ms
    return ts_ms


def final_horizon_return_pct(row: dict[str, Any], horizon_ms: int) -> float | None:
    samples = row.get("samples")
    if not isinstance(samples, list):
        return None
    decision_ts_ms = common.int_or_none(row.get("decision_ts_ms"))
    best: tuple[int, float] | None = None
    for sample in samples:
        if not isinstance(sample, dict):
            continue
        key = event_time_key(sample, decision_ts_ms)
        ret = finite_float(sample.get("return_pct"))
        if key is None or ret is None or key < 0 or key > horizon_ms:
            continue
        if best is None or key > best[0]:
            best = (key, ret)
    return best[1] if best is not None else None


class UniqueIndex:
    def __init__(self) -> None:
        self._rows: dict[str, list[dict[str, Any]]] = defaultdict(list)

    def add(self, key: str | None, row: dict[str, Any]) -> None:
        if key:
            self._rows[key].append(row)

    def get(self, key: str | None) -> tuple[dict[str, Any] | None, bool]:
        if not key:
            return None, False
        rows = self._rows.get(key, [])
        if len(rows) == 1:
            return rows[0], False
        if len(rows) > 1:
            return None, True
        return None, False

    def unique_count(self) -> int:
        return sum(1 for rows in self._rows.values() if len(rows) == 1)

    def collision_count(self) -> int:
        return sum(1 for rows in self._rows.values() if len(rows) > 1)


def build_indexes(rows: Iterable[dict[str, Any]]) -> dict[str, UniqueIndex]:
    indexes = {
        "ab_record_id": UniqueIndex(),
        "candidate_id": UniqueIndex(),
        "pool_mint": UniqueIndex(),
        "pool_mint_ts": UniqueIndex(),
        "probe_id": UniqueIndex(),
    }
    for row in rows:
        indexes["ab_record_id"].add(key_ab(row), row)
        indexes["candidate_id"].add(key_candidate(row), row)
        indexes["pool_mint"].add(key_pool_mint(row), row)
        indexes["pool_mint_ts"].add(key_pool_mint_ts(row), row)
        indexes["probe_id"].add(first_str(row, "probe_id"), row)
    return indexes


def lookup(
    indexes: dict[str, UniqueIndex],
    row: dict[str, Any],
    order: tuple[str, ...],
) -> tuple[dict[str, Any] | None, str | None, bool]:
    key_fns = {
        "ab_record_id": key_ab,
        "candidate_id": key_candidate,
        "pool_mint": key_pool_mint,
        "pool_mint_ts": key_pool_mint_ts,
        "probe_id": lambda item: first_str(item, "probe_id"),
    }
    collision = False
    for name in order:
        matched, collided = indexes[name].get(key_fns[name](row))
        collision = collision or collided
        if matched is not None:
            return matched, name, collision
    return None, None, collision


def normalize_verdict_family(row: dict[str, Any]) -> str:
    for field in ("v3_shadow_verdict", "shadow_verdict"):
        value = row.get(field)
        if isinstance(value, str):
            upper = value.strip().upper()
            if upper in {"BUY", "REJECT", "PENDING", "TIMEOUT"}:
                return upper
    if row.get("decision_verdict_buy") is True:
        return "BUY"
    active = first_str(
        row,
        "verdict_type",
        "decision_verdict",
        "decision_result",
        "terminal_verdict",
        "active_verdict_type",
    )
    if not active:
        return "UNKNOWN"
    upper = active.strip().upper()
    if upper == "BUY":
        return "BUY"
    if upper.startswith("REJECT"):
        return "REJECT"
    if upper.startswith("TIMEOUT"):
        return "TIMEOUT"
    if upper.startswith("PENDING"):
        return "PENDING"
    return upper or "UNKNOWN"


def load_decision_rows(paths: list[Path]) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    per_path: list[dict[str, Any]] = []
    for path in paths:
        count = 0
        for offset, row in enumerate(common.iter_json_objects(path)):
            enriched = dict(row)
            enriched["_decision_source_path"] = str(path)
            enriched["_decision_source_row_offset"] = offset
            enriched["_decision_source_stable_json"] = stable_json(row)
            rows.append(enriched)
            count += 1
        per_path.append({"path": str(path), "rows": count})
    return rows, {"decision_source_paths": per_path, "decision_rows": len(rows)}


def find_decision_logs(root: Path, runtime_scope: str) -> list[Path]:
    base = root / "logs" / "rollout" / runtime_scope / "decisions"
    return sorted(path for path in base.glob("**/gatekeeper_v2_decisions.jsonl") if path.is_file())


def load_r2_labels(
    path: Path,
    *,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    counts: Counter[str] = Counter()
    for row in common.iter_json_objects(path):
        label = business_target_rate.first_barrier_label(
            row,
            target_net_pct=target_net_pct,
            stop_net_pct=stop_net_pct,
            horizon_ms=horizon_ms,
        )
        enriched = dict(row)
        enriched["_business_label"] = label
        enriched["_final_horizon_return_pct"] = final_horizon_return_pct(row, horizon_ms)
        counts[str(label.get("business_label") or "UNKNOWN")] += 1
        rows.append(enriched)
    resolved = sum(counts[label] for label in BUSINESS_LABELS)
    return rows, {
        "r2_market_paths_path": str(path),
        "r2_market_path_rows": len(rows),
        "r2_business_label_counts": common.counter_dict(counts),
        "r2_business_resolved_rows": resolved,
    }


def load_optional_rows(path: Path | None) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    if path is None or not path.exists():
        return [], {"path": str(path) if path is not None else None, "rows": 0, "exists": False}
    rows = list(common.iter_json_objects(path))
    return rows, {"path": str(path), "rows": len(rows), "exists": True}


def lifecycle_summary(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    by_candidate: dict[str, dict[str, Any]] = {}
    for row in rows:
        candidate_id = key_candidate(row)
        if not candidate_id:
            continue
        record_type = first_str(row, "record_type") or ""
        current = by_candidate.setdefault(candidate_id, {})
        if record_type == "position_closed":
            current["position_closed"] = row
        elif record_type == "exit_filled":
            current.setdefault("exit_fills", []).append(row)
    return by_candidate


def probe_entry_status(
    selection: dict[str, Any] | None,
    transport: dict[str, Any] | None,
    entry: dict[str, Any] | None,
) -> str:
    skip_reason = first_str(selection or {}, "probe_skip_reason")
    event_type = first_str(selection or {}, "event_type")
    if skip_reason or event_type == "probe_skipped":
        return "probe_skipped"
    if entry is not None:
        return "simulated_entry"
    if transport is not None and first_str(transport, "error_class", "failure_class"):
        return "probe_transport_error"
    if selection is not None:
        return "entry_missing"
    return "probe_not_observed"


def output_row(
    decision: dict[str, Any],
    *,
    r2_row: dict[str, Any] | None,
    r2_join_key: str | None,
    r2_collision: bool,
    selection: dict[str, Any] | None,
    selection_join_key: str | None,
    transport: dict[str, Any] | None,
    entry: dict[str, Any] | None,
    lifecycle: dict[str, Any] | None,
    runtime_scope: str,
    selector_scope: str,
    target_net_pct: float,
    stop_net_pct: float,
    horizon_ms: int,
) -> dict[str, Any]:
    decision_id, decision_id_source = all_decision_id(decision)
    candidate_id, candidate_id_source = resolved_candidate_id(
        decision,
        r2_row=r2_row,
        selection=selection,
        transport=transport,
        entry=entry,
        lifecycle=lifecycle,
        fallback_id=decision_id,
    )
    label = (
        r2_row.get("_business_label")
        if r2_row is not None and isinstance(r2_row.get("_business_label"), dict)
        else {
            "business_label": "MISSING_PATH",
            "business_label_resolved": False,
            "business_excluded_reason": "missing_r2_market_path",
            "target_hit_ts_ms": None,
            "stop_hit_ts_ms": None,
        }
    )
    closed = (lifecycle or {}).get("position_closed") if lifecycle is not None else None
    return {
        "schema_version": 1,
        "artifact": "all_decision_counterfactual_outcome_v1",
        "runtime_scope": runtime_scope,
        "selector_scope": selector_scope,
        "label_contract": "first_barrier_target_stop_timeout",
        "target_net_pct": target_net_pct,
        "stop_net_pct": stop_net_pct,
        "horizon_ms": horizon_ms,
        "all_decision_id": decision_id,
        "all_decision_id_source": decision_id_source,
        "decision_source_path": decision.get("_decision_source_path"),
        "decision_source_row_offset": decision.get("_decision_source_row_offset"),
        "decision_plane": first_str(decision, "decision_plane"),
        "rollout_namespace": first_str(decision, "rollout_namespace"),
        "ab_record_id": key_ab(decision),
        "candidate_id": candidate_id,
        "candidate_id_source": candidate_id_source,
        "pool_id": first_str(decision, "pool_id", "pool_amm_id"),
        "base_mint": first_str(decision, "base_mint", "mint_id"),
        "decision_ts_ms": decision_cutoff_ts_ms(decision),
        "original_gatekeeper_verdict_family": normalize_verdict_family(decision),
        "original_gatekeeper_verdict_type": first_str(
            decision,
            "verdict_type",
            "decision_verdict",
            "decision_result",
            "terminal_verdict",
        ),
        "original_decision_verdict_buy": common.bool_or_none(decision.get("decision_verdict_buy")),
        "original_reason_code": first_str(
            decision,
            "reason_code",
            "decision_reason_code",
            "active_reason_code",
            "v3_shadow_reason_code",
        ),
        "original_decision_reason": first_str(decision, "decision_reason", "reason"),
        "probe_join_key": selection_join_key,
        "probe_event_type": first_str(selection or {}, "event_type"),
        "probe_id": first_str(selection or {}, "probe_id"),
        "probe_selected_ts_ms": first_int(selection or {}, "probe_selected_ts_ms"),
        "probe_skip_reason": first_str(selection or {}, "probe_skip_reason"),
        "probe_bucket": first_str(selection or {}, "probe_bucket"),
        "probe_entry_status": probe_entry_status(selection, transport, entry),
        "probe_transport_error_class": first_str(transport or {}, "error_class", "failure_class"),
        "probe_execution_outcome": first_str(
            transport or {},
            "execution_outcome",
            "shadow_execution_outcome",
            "simulation_outcome",
        ),
        "probe_entry_price_sol": finite_float((entry or {}).get("entry_price")),
        "probe_entry_value_sol": finite_float((entry or {}).get("entry_value_sol")),
        "probe_lifecycle_close_reason": first_str(closed or {}, "close_reason"),
        "probe_lifecycle_final_pnl_pct": finite_float((closed or {}).get("final_pnl_pct")),
        "business_label": label.get("business_label"),
        "business_label_resolved": label.get("business_label_resolved"),
        "business_excluded_reason": label.get("business_excluded_reason"),
        "target_hit_ts_ms": label.get("target_hit_ts_ms"),
        "stop_hit_ts_ms": label.get("stop_hit_ts_ms"),
        "r2_join_key": r2_join_key,
        "r2_join_collision": r2_collision,
        "r2_candidate_id": key_candidate(r2_row or {}),
        "r2_path_coverage_ok": (r2_row or {}).get("path_coverage_ok"),
        "r2_horizon_matured": (r2_row or {}).get("horizon_matured"),
        "r2_status": (r2_row or {}).get("r2_status"),
        "price_at_decision": finite_float((r2_row or {}).get("price_at_decision")),
        "max_favorable_pnl_pct": finite_float((r2_row or {}).get("max_favorable_pnl_pct")),
        "max_adverse_pnl_pct": finite_float((r2_row or {}).get("max_adverse_pnl_pct")),
        "final_horizon_pnl_pct": (r2_row or {}).get("_final_horizon_return_pct"),
    }


def write_matrix(path: Path, rows: Iterable[dict[str, Any]], group_field: str) -> list[dict[str, Any]]:
    matrix: dict[str, Counter[str]] = defaultdict(Counter)
    totals: Counter[str] = Counter()
    for row in rows:
        group = str(row.get(group_field) or "UNKNOWN")
        label = str(row.get("business_label") or "UNKNOWN")
        matrix[group][label] += 1
        totals[group] += 1
    output: list[dict[str, Any]] = []
    for group in sorted(matrix):
        total = totals[group]
        item: dict[str, Any] = {group_field: group, "total": total}
        for label in (*BUSINESS_LABELS, *UNRESOLVED_LABELS, "UNKNOWN"):
            count = matrix[group][label]
            item[f"{label}_count"] = count
            item[f"{label}_rate"] = (count / total) if total else None
        output.append(item)
    path.parent.mkdir(parents=True, exist_ok=True)
    if output:
        with path.open("w", encoding="utf-8", newline="") as fh:
            writer = csv.DictWriter(fh, fieldnames=list(output[0].keys()))
            writer.writeheader()
            writer.writerows(output)
    else:
        path.write_text("", encoding="utf-8")
    return output


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    selector_scope = args.scope
    runtime_scope = args.runtime_scope
    dataset_dir = root / "datasets" / "selector" / selector_scope
    report_dir = root / "reports" / "selector" / selector_scope

    output_path = resolve_rooted(root, args.output) or (
        dataset_dir / "all_decision_counterfactual_outcome_v1.jsonl"
    )
    manifest_path = resolve_rooted(root, args.manifest_output) or (
        report_dir / "all_decision_counterfactual_outcome_manifest_v1.json"
    )
    verdict_matrix_path = resolve_rooted(root, args.verdict_matrix_output) or (
        report_dir / "all_decision_business_label_matrix_v1.csv"
    )
    reason_matrix_path = resolve_rooted(root, args.reason_matrix_output) or (
        report_dir / "all_decision_reason_business_label_matrix_v1.csv"
    )

    decision_logs = [resolve_rooted(root, path) for path in args.decision_log]
    decision_logs = [path for path in decision_logs if path is not None]
    if not decision_logs:
        decision_logs = find_decision_logs(root, runtime_scope)
    if not decision_logs:
        raise SystemExit(f"no gatekeeper_v2_decisions.jsonl files found for runtime scope {runtime_scope}")

    r2_market_paths = resolve_rooted(root, args.r2_market_paths) or (
        dataset_dir / "r2_market_paths_v1.jsonl"
    )
    probe_base = root / "logs" / "shadow_run" / runtime_scope
    probe_selection_path = resolve_rooted(root, args.probe_selection) or (
        probe_base / "probe_selection.jsonl"
    )
    probe_skips_path = resolve_rooted(root, args.probe_skips) or (probe_base / "probe_skips.jsonl")
    probe_transport_path = resolve_rooted(root, args.probe_transport) or (
        probe_base / "probe_transport.jsonl"
    )
    probe_entry_path = resolve_rooted(root, args.probe_entries) or (
        probe_base / "probe_shadow_entries.jsonl"
    )
    probe_lifecycle_path = resolve_rooted(root, args.probe_lifecycle) or (
        probe_base / "probe_shadow_lifecycle.jsonl"
    )

    decisions, decision_manifest = load_decision_rows(decision_logs)
    r2_rows, r2_manifest = load_r2_labels(
        r2_market_paths,
        target_net_pct=args.target_net_pct,
        stop_net_pct=args.stop_net_pct,
        horizon_ms=args.horizon_ms,
    )
    selection_rows, selection_manifest = load_optional_rows(probe_selection_path)
    skip_rows, skip_manifest = load_optional_rows(probe_skips_path)
    transport_rows, transport_manifest = load_optional_rows(probe_transport_path)
    entry_rows, entry_manifest = load_optional_rows(probe_entry_path)
    lifecycle_rows, lifecycle_manifest = load_optional_rows(probe_lifecycle_path)

    r2_indexes = build_indexes(r2_rows)
    selection_indexes = build_indexes([*selection_rows, *skip_rows])
    transport_indexes = build_indexes(transport_rows)
    entry_indexes = build_indexes(entry_rows)
    lifecycle_by_candidate = lifecycle_summary(lifecycle_rows)

    output_rows: list[dict[str, Any]] = []
    counts: Counter[str] = Counter()
    label_counts: Counter[str] = Counter()
    resolved_label_counts: Counter[str] = Counter()
    verdict_counts: Counter[str] = Counter()
    join_counts: Counter[str] = Counter()
    candidate_id_source_counts: Counter[str] = Counter()
    all_decision_id_source_counts: Counter[str] = Counter()
    for decision in decisions:
        r2_row, r2_key, r2_collision = lookup(
            r2_indexes, decision, ("candidate_id", "ab_record_id", "pool_mint_ts", "pool_mint")
        )
        selection, selection_key, selection_collision = lookup(
            selection_indexes,
            decision,
            ("ab_record_id", "candidate_id", "pool_mint_ts", "pool_mint"),
        )
        probe_lookup_row = selection or decision
        transport, _, transport_collision = lookup(
            transport_indexes,
            probe_lookup_row,
            ("probe_id", "candidate_id", "ab_record_id", "pool_mint_ts", "pool_mint"),
        )
        entry, _, entry_collision = lookup(
            entry_indexes,
            probe_lookup_row,
            ("probe_id", "candidate_id", "ab_record_id", "pool_mint_ts", "pool_mint"),
        )
        lifecycle = lifecycle_by_candidate.get(key_candidate(probe_lookup_row) or "")
        row = output_row(
            decision,
            r2_row=r2_row,
            r2_join_key=r2_key,
            r2_collision=r2_collision,
            selection=selection,
            selection_join_key=selection_key,
            transport=transport,
            entry=entry,
            lifecycle=lifecycle,
            runtime_scope=runtime_scope,
            selector_scope=selector_scope,
            target_net_pct=args.target_net_pct,
            stop_net_pct=args.stop_net_pct,
            horizon_ms=args.horizon_ms,
        )
        if selection_collision:
            row["probe_join_collision"] = True
        if transport_collision:
            row["probe_transport_join_collision"] = True
        if entry_collision:
            row["probe_entry_join_collision"] = True
        output_rows.append(row)
        counts["rows"] += 1
        label_counts[str(row.get("business_label") or "UNKNOWN")] += 1
        if row.get("business_label_resolved") is True:
            resolved_label_counts[str(row.get("business_label") or "UNKNOWN")] += 1
        verdict_counts[str(row.get("original_gatekeeper_verdict_family") or "UNKNOWN")] += 1
        candidate_id_source_counts[str(row.get("candidate_id_source") or "UNKNOWN")] += 1
        all_decision_id_source_counts[str(row.get("all_decision_id_source") or "UNKNOWN")] += 1
        join_counts[f"r2_join:{row.get('r2_join_key') or 'missing'}"] += 1
        join_counts[f"probe_join:{row.get('probe_join_key') or 'missing'}"] += 1
        join_counts[f"probe_entry:{row.get('probe_entry_status') or 'UNKNOWN'}"] += 1

    common.write_jsonl(output_path, output_rows)
    all_decision_id_counts = Counter(str(row.get("all_decision_id") or "") for row in output_rows)
    duplicate_all_decision_ids = {
        key: count for key, count in all_decision_id_counts.items() if key and count > 1
    }
    verdict_matrix = write_matrix(
        verdict_matrix_path, output_rows, "original_gatekeeper_verdict_family"
    )
    reason_matrix = write_matrix(reason_matrix_path, output_rows, "original_reason_code")
    manifest = {
        "schema_version": 1,
        "artifact": "all_decision_counterfactual_outcome_manifest_v1",
        "runtime_scope": runtime_scope,
        "selector_scope": selector_scope,
        "label_contract": {
            "target_net_pct": args.target_net_pct,
            "stop_net_pct": args.stop_net_pct,
            "horizon_ms": args.horizon_ms,
            "semantics": "first_barrier_TARGET_STOP_TIMEOUT",
            "timeout_is_not_target": True,
        },
        "outputs": {
            "outcome_jsonl": str(output_path),
            "manifest_json": str(manifest_path),
            "verdict_matrix_csv": str(verdict_matrix_path),
            "reason_matrix_csv": str(reason_matrix_path),
        },
        "inputs": {
            **decision_manifest,
            **r2_manifest,
            "probe_selection": selection_manifest,
            "probe_skips": skip_manifest,
            "probe_transport": transport_manifest,
            "probe_entries": entry_manifest,
            "probe_lifecycle": lifecycle_manifest,
        },
        "counts": {
            "output_rows": len(output_rows),
            "resolved_business_label_rows": sum(resolved_label_counts.values()),
            "unresolved_business_label_rows": len(output_rows) - sum(resolved_label_counts.values()),
            "business_label_counts": common.counter_dict(label_counts),
            "resolved_business_label_counts": common.counter_dict(resolved_label_counts),
            "original_gatekeeper_verdict_family_counts": common.counter_dict(verdict_counts),
            "candidate_id_source_counts": common.counter_dict(candidate_id_source_counts),
            "all_decision_id_source_counts": common.counter_dict(all_decision_id_source_counts),
            "decision_rows_missing_candidate_id": sum(
                1 for row in decisions if key_candidate(row) is None
            ),
            "output_rows_with_synthesized_candidate_id": candidate_id_source_counts.get(
                "synthesized_all_decision_id", 0
            ),
            "duplicate_all_decision_id_count": len(duplicate_all_decision_ids),
            "duplicate_all_decision_ids": duplicate_all_decision_ids,
            "join_counts": common.counter_dict(join_counts),
            "r2_index_collisions": {
                name: index.collision_count() for name, index in r2_indexes.items()
            },
            "probe_index_collisions": {
                name: index.collision_count() for name, index in selection_indexes.items()
            },
        },
        "matrices": {
            "verdict_matrix_rows": len(verdict_matrix),
            "reason_matrix_rows": len(reason_matrix),
        },
    }
    common.write_json(manifest_path, manifest)
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Build all-decision Gatekeeper outcome rows by joining terminal decisions, "
            "p37 counterfactual probe artifacts, and canonical R2 market path labels."
        )
    )
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument("--scope", required=True, help="Selector dataset scope.")
    parser.add_argument("--runtime-scope", required=True, help="Runtime/log scope.")
    parser.add_argument("--decision-log", type=Path, action="append", default=[])
    parser.add_argument("--r2-market-paths", type=Path)
    parser.add_argument("--probe-selection", type=Path)
    parser.add_argument("--probe-skips", type=Path)
    parser.add_argument("--probe-transport", type=Path)
    parser.add_argument("--probe-entries", type=Path)
    parser.add_argument("--probe-lifecycle", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument("--verdict-matrix-output", type=Path)
    parser.add_argument("--reason-matrix-output", type=Path)
    parser.add_argument("--target-net-pct", type=float, default=30.0)
    parser.add_argument("--stop-net-pct", type=float, default=30.0)
    parser.add_argument("--horizon-ms", type=int, default=60_000)
    parser.add_argument("--json", action="store_true")
    return parser


if __name__ == "__main__":
    args = build_parser().parse_args()
    report = build_report(args)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    else:
        print(
            "all_decision_counterfactual_outcome "
            f"rows={report['counts']['output_rows']} "
            f"output={report['outputs']['outcome_jsonl']} "
            f"manifest={report['outputs']['manifest_json']}"
        )
