#!/usr/bin/env python3
"""PR-RUG-MARKUP-A0 offline proof.

This script is research-only. It reads local rollout/shadow evidence, writes
CSV/Markdown reports, and does not change runtime, configs, logs, Gatekeeper,
selector, TX/Jito/live paths, alpha hooks, or sidecars.

The first output artifact is always:

    reports/selector/rug_markup_a0_evidence_inventory.csv

If no requested scope has both decision-time Gatekeeper/materialized feature
rows and shadow_exit_replay_v1, the proof stops as RUG_MARKUP_BLOCKED_BY_DATA.
If only one requested scope is evaluable, the strongest allowed verdict is
RUG_MARKUP_SINGLE_SCOPE_DIAGNOSTIC.
"""

from __future__ import annotations

import argparse
import bisect
import csv
import json
import math
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Mapping

sys.dont_write_bytecode = True

import time_stop_v2_counterfactual_lab as lab


R49_SCOPE = "shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1"
R50_SCOPE = "shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1"

REPORT_DIR = Path("reports/selector")
INVENTORY_CSV = REPORT_DIR / "rug_markup_a0_evidence_inventory.csv"
SUMMARY_CSV = REPORT_DIR / "rug_markup_a0_summary.csv"
COST_CSV = REPORT_DIR / "rug_markup_a0_cost_sensitivity.csv"
STABILITY_CSV = REPORT_DIR / "rug_markup_a0_stability.csv"
TAIL_CSV = REPORT_DIR / "rug_markup_a0_tail_audit.csv"
THRESHOLD_CSV = REPORT_DIR / "rug_markup_a0_threshold_manifest.csv"
REPORT_MD = Path("PLANS/AUDYT/RAPORT_RUG_MARKUP_A0_OFFLINE_PROOF_20260629.md")
ADR_MD = Path("docs/ADR/ADR_8D_RUG_MARKUP_A0_RESULT_20260629.md")

LOCAL_LOGS_ROOT = Path("logs")
VOLUME_LOGS_ROOT = Path("/mnt/HC_Volume_105935807/logs")

TARGET_BPS_GRID = (1000, 1500, 2000, 2500)
STOP_BPS_GRID = (-300, -500, -700, -1000)
MAX_HOLD_MS_GRID = (20000, 30000, 40000)
COSTS_BPS = (100, 200)
CLASSIFIERS = (
    "R0_BROAD",
    "R1_DEV_SIGNER_CONCENTRATION",
    "R2_BUY_BURST_MARKUP",
    "R3_SCAMBOT_COORDINATION",
    "R4_MARKUP_WITH_DUMP_RISK",
)
SEGMENTS = ("train", "validation", "holdout")

VERDICT_BLOCKED = "RUG_MARKUP_BLOCKED_BY_DATA"
VERDICT_REJECTED = "RUG_MARKUP_REJECTED_FOR_RUNTIME"
VERDICT_SINGLE_SCOPE = "RUG_MARKUP_SINGLE_SCOPE_DIAGNOSTIC"
VERDICT_PROMISING = "RUG_MARKUP_PROMISING_OFFLINE_ONLY_NEED_SECOND_SCOPE"

PRE_ENTRY_FIELDS = (
    "buy_count",
    "total_tx",
    "total_volume_sol",
    "sol_buy_ratio",
    "buy_ratio",
    "current_market_cap_sol",
    "bonding_progress_pct",
    "price_change_ratio",
    "max_single_tx_price_impact_pct_observed",
    "unique_ratio",
    "hhi",
    "buyer_hhi",
    "top3_signer_volume_ratio",
    "top3_volume_pct",
    "same_ms_tx_ratio",
    "burst_ratio",
    "early_top3_buy_volume_pct_3s",
    "early_slot_volume_dominance_buy",
    "dev_tx_ratio",
    "dev_volume_ratio",
    "dev_buy_total_sol",
    "dev_buyer_infrastructure_affinity",
    "dev_has_sold",
    "dev_sold_within_3s",
    "dev_sold_within_5s",
    "signer_cross_pool_velocity",
    "cpv_other_pool_activity",
    "flipper_presence_ratio",
    "delta_buy_count_1s_to_2s",
    "delta_buy_count_1s_to_3s",
    "delta_tx_count_1s_to_2s",
    "delta_tx_count_1s_to_3s",
    "delta_unique_signers_1s_to_2s",
    "delta_unique_signers_1s_to_3s",
    "delta_burstratio_1s_to_2s",
    "delta_burstratio_1s_to_3s",
    "delta_price_pct_1s_to_2s",
    "delta_price_pct_1s_to_3s",
)

FORBIDDEN_FEATURES = (
    "final_pnl",
    "final_pnl_bps",
    "target",
    "stop",
    "timeout",
    "target_label",
    "stop_label",
    "timeout_label",
    "exit_result",
    "pool_id",
    "mint",
    "base_mint",
    "signer",
    "signer_id",
    "dev_wallet",
    "dev_wallet_id",
    "path_bps",
)


@dataclass(frozen=True)
class ScopeEvidence:
    scope: str
    role: str
    decision_paths: tuple[Path, ...]
    preferred_decision_path: Path | None
    has_materialized_snapshot: bool
    materialized_sample_rows: int
    pre_entry_fields_found: tuple[str, ...]
    replay_path: Path | None
    shadow_lifecycle_path: Path | None
    probe_lifecycle_path: Path | None

    @property
    def has_decision_evidence(self) -> bool:
        return self.preferred_decision_path is not None

    @property
    def has_replay_evidence(self) -> bool:
        return self.replay_path is not None

    @property
    def has_full_evidence(self) -> bool:
        return self.has_decision_evidence and self.has_replay_evidence and (
            self.has_materialized_snapshot or bool(self.pre_entry_fields_found)
        )


@dataclass
class DecisionLoad:
    rows: int
    malformed_rows: int
    rows_with_join_key: int
    rows_with_timestamp: int
    rows_with_materialized_snapshot: int
    index: dict[tuple[str, str], list[dict[str, Any]]]


@dataclass
class ReplayLoad:
    rows: int
    malformed_rows: int
    rows_with_identity: int
    rows_with_path: int
    records: list[dict[str, Any]]


@dataclass
class JoinedScope:
    scope: str
    evidence: ScopeEvidence
    replay_rows: int
    decision_rows: int
    joined_records: list[dict[str, Any]]
    unjoined_replay_rows: int
    join_rate: float
    malformed_decision_rows: int
    malformed_replay_rows: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline PR-RUG-MARKUP-A0 proof.")
    parser.add_argument("--r49-scope", default=R49_SCOPE)
    parser.add_argument("--r50-scope", default=R50_SCOPE)
    parser.add_argument("--local-logs-root", type=Path, default=LOCAL_LOGS_ROOT)
    parser.add_argument("--volume-logs-root", type=Path, default=VOLUME_LOGS_ROOT)
    parser.add_argument("--reports-dir", type=Path, default=REPORT_DIR)
    return parser.parse_args()


def path_size(path: Path | None) -> int:
    if path is None:
        return 0
    try:
        return path.stat().st_size
    except OSError:
        return 0


def safe_div(numerator: float, denominator: float) -> float:
    return numerator / denominator if denominator else 0.0


def as_float(value: Any) -> float | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    return parsed if math.isfinite(parsed) else None


def as_int(value: Any) -> int | None:
    parsed = as_float(value)
    if parsed is None:
        return None
    return int(round(parsed))


def median(values: list[float]) -> float:
    return float(statistics.median(values)) if values else 0.0


def mean(values: list[float]) -> float:
    return float(sum(values) / len(values)) if values else 0.0


def first_existing(paths: Iterable[Path]) -> Path | None:
    seen: set[str] = set()
    for path in paths:
        key = str(path)
        if key in seen:
            continue
        seen.add(key)
        if path.exists() and path.is_file():
            return path
    return None


def unique_paths(paths: Iterable[Path]) -> tuple[Path, ...]:
    out: dict[str, Path] = {}
    for path in paths:
        try:
            key = str(path.resolve())
        except OSError:
            key = str(path)
        out[key] = path
    return tuple(out.values())


def decision_paths_for_scope(scope: str, local_logs_root: Path, volume_logs_root: Path) -> tuple[Path, ...]:
    bases = [
        local_logs_root / "rollout" / scope / "decisions" / scope,
        volume_logs_root / "rollout" / scope / "decisions" / scope,
    ]
    candidates: list[Path] = []
    for base in bases:
        if base.exists():
            candidates.extend(base.glob("**/gatekeeper_v2_decisions.jsonl"))
    paths = list(unique_paths(candidates))
    paths.sort(
        key=lambda path: (
            "/v2.5/v25_shadow/" not in str(path),
            "/v2.2/legacy_live/" not in str(path),
            str(path),
        )
    )
    return tuple(paths)


def shadow_base_candidates(scope: str, local_logs_root: Path, volume_logs_root: Path) -> list[Path]:
    return [
        local_logs_root / "shadow_run" / scope,
        volume_logs_root / "shadow_run" / scope,
    ]


def replay_path_for_scope(scope: str, local_logs_root: Path, volume_logs_root: Path) -> Path | None:
    return first_existing(base / "shadow_exit_replay_v1.jsonl" for base in shadow_base_candidates(scope, local_logs_root, volume_logs_root))


def lifecycle_path_for_scope(scope: str, filename: str, local_logs_root: Path, volume_logs_root: Path) -> Path | None:
    return first_existing(base / filename for base in shadow_base_candidates(scope, local_logs_root, volume_logs_root))


def discover_scope_names(local_logs_root: Path, volume_logs_root: Path, requested: tuple[str, str]) -> list[str]:
    names: set[str] = set(requested)
    for root in (
        local_logs_root / "rollout",
        local_logs_root / "shadow_run",
        volume_logs_root / "rollout",
        volume_logs_root / "shadow_run",
    ):
        if not root.exists():
            continue
        for child in root.iterdir():
            if child.is_dir():
                names.add(child.name)
    return sorted(names)


def sample_decision_path(path: Path | None, max_rows: int = 25) -> tuple[bool, int, tuple[str, ...]]:
    if path is None:
        return False, 0, ()
    fields_found: set[str] = set()
    materialized_rows = 0
    sampled = 0
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as handle:
            for line in handle:
                if sampled >= max_rows:
                    break
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if not isinstance(row, dict):
                    continue
                sampled += 1
                snapshot = row.get("materialized_feature_snapshot")
                if isinstance(snapshot, dict):
                    materialized_rows += 1
                for field in PRE_ENTRY_FIELDS:
                    if row.get(field) is not None or (isinstance(snapshot, dict) and snapshot.get(field) is not None):
                        fields_found.add(field)
    except OSError:
        return False, 0, ()
    return materialized_rows > 0, materialized_rows, tuple(sorted(fields_found))


def build_evidence_inventory(
    r49_scope: str,
    r50_scope: str,
    local_logs_root: Path,
    volume_logs_root: Path,
) -> list[ScopeEvidence]:
    requested = (r49_scope, r50_scope)
    evidences: list[ScopeEvidence] = []
    for scope in discover_scope_names(local_logs_root, volume_logs_root, requested):
        decision_paths = decision_paths_for_scope(scope, local_logs_root, volume_logs_root)
        preferred_decision_path = decision_paths[0] if decision_paths else None
        has_materialized, materialized_rows, fields_found = sample_decision_path(preferred_decision_path)
        role = "primary_r49" if scope == r49_scope else "primary_r50" if scope == r50_scope else "other_local_scope"
        evidences.append(
            ScopeEvidence(
                scope=scope,
                role=role,
                decision_paths=decision_paths,
                preferred_decision_path=preferred_decision_path,
                has_materialized_snapshot=has_materialized,
                materialized_sample_rows=materialized_rows,
                pre_entry_fields_found=fields_found,
                replay_path=replay_path_for_scope(scope, local_logs_root, volume_logs_root),
                shadow_lifecycle_path=lifecycle_path_for_scope(scope, "shadow_lifecycle.jsonl", local_logs_root, volume_logs_root),
                probe_lifecycle_path=lifecycle_path_for_scope(scope, "probe_shadow_lifecycle.jsonl", local_logs_root, volume_logs_root),
            )
        )
    return evidences


def inventory_rows(evidences: Iterable[ScopeEvidence]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for evidence in evidences:
        missing: list[str] = []
        if not evidence.has_decision_evidence:
            missing.append("missing_gatekeeper_v2_decisions_jsonl")
        if not evidence.has_materialized_snapshot and not evidence.pre_entry_fields_found:
            missing.append("missing_materialized_or_pre_entry_fields")
        if not evidence.has_replay_evidence:
            missing.append("missing_shadow_exit_replay_v1")
        rows.append(
            {
                "scope": evidence.scope,
                "role": evidence.role,
                "has_gatekeeper_v2_decisions": evidence.has_decision_evidence,
                "decision_paths_count": len(evidence.decision_paths),
                "preferred_decision_path": str(evidence.preferred_decision_path or ""),
                "preferred_decision_size_bytes": path_size(evidence.preferred_decision_path),
                "has_materialized_feature_snapshot": evidence.has_materialized_snapshot,
                "materialized_sample_rows": evidence.materialized_sample_rows,
                "pre_entry_fields_found_count": len(evidence.pre_entry_fields_found),
                "pre_entry_fields_found": ";".join(evidence.pre_entry_fields_found),
                "has_shadow_exit_replay_v1": evidence.has_replay_evidence,
                "shadow_exit_replay_v1_path": str(evidence.replay_path or ""),
                "shadow_exit_replay_v1_size_bytes": path_size(evidence.replay_path),
                "has_shadow_lifecycle_jsonl": evidence.shadow_lifecycle_path is not None,
                "shadow_lifecycle_path": str(evidence.shadow_lifecycle_path or ""),
                "has_probe_shadow_lifecycle_jsonl": evidence.probe_lifecycle_path is not None,
                "probe_shadow_lifecycle_path": str(evidence.probe_lifecycle_path or ""),
                "full_evidence_for_rug_markup_a0": evidence.has_full_evidence,
                "blocking_reason": ";".join(missing),
            }
        )
    return rows


def write_csv(path: Path, rows: list[Mapping[str, Any]], fieldnames: list[str] | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fields = fieldnames or []
    if not fields:
        for row in rows:
            for key in row:
                if key not in fields:
                    fields.append(key)
    if not fields:
        fields = ["empty"]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow(dict(row))


def parse_join_ts(row: Mapping[str, Any]) -> int | None:
    for field in (
        "decision_ts_ms",
        "timestamp_ms",
        "ab_t_end_event_ts_ms",
        "event_ts_ms",
        "curve_t0_event_ts_ms",
        "first_seen_ts_ms",
    ):
        value = as_int(row.get(field))
        if value is not None:
            return value
    join_key = row.get("join_key")
    if isinstance(join_key, str):
        suffix = join_key.rsplit(":", 1)[-1]
        if suffix.isdigit():
            return int(suffix)
    return None


def feature_value(row: Mapping[str, Any], field: str) -> Any:
    value = row.get(field)
    if value is not None:
        return value
    snapshot = row.get("materialized_feature_snapshot")
    if isinstance(snapshot, dict):
        return snapshot.get(field)
    return None


def compact_features(row: Mapping[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for field in PRE_ENTRY_FIELDS:
        value = feature_value(row, field)
        if value is not None:
            out[field] = value
    return out


def load_decision_features(path: Path) -> DecisionLoad:
    index: dict[tuple[str, str], list[dict[str, Any]]] = {}
    rows = malformed = rows_with_join_key = rows_with_timestamp = rows_with_materialized = 0
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if not isinstance(row, dict):
                malformed += 1
                continue
            rows += 1
            pool_id = row.get("pool_id")
            base_mint = row.get("base_mint")
            if not pool_id or not base_mint:
                continue
            rows_with_join_key += 1
            ts_ms = parse_join_ts(row)
            if ts_ms is None:
                continue
            rows_with_timestamp += 1
            snapshot = row.get("materialized_feature_snapshot")
            if isinstance(snapshot, dict):
                rows_with_materialized += 1
            features = compact_features(row)
            if not features:
                continue
            index.setdefault((str(pool_id), str(base_mint)), []).append(
                {
                    "ts_ms": ts_ms,
                    "features": features,
                    "verdict_type": row.get("verdict_type") or row.get("decision") or "",
                    "reason_code": row.get("reason_code") or row.get("decision_reason") or "",
                }
            )
    for records in index.values():
        records.sort(key=lambda item: int(item["ts_ms"]))
    return DecisionLoad(
        rows=rows,
        malformed_rows=malformed,
        rows_with_join_key=rows_with_join_key,
        rows_with_timestamp=rows_with_timestamp,
        rows_with_materialized_snapshot=rows_with_materialized,
        index=index,
    )


def load_replay_rows(path: Path) -> ReplayLoad:
    records: list[dict[str, Any]] = []
    rows = malformed = rows_with_identity = rows_with_path = 0
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if not isinstance(row, dict):
                malformed += 1
                continue
            rows += 1
            if row.get("pool_id") and row.get("base_mint") and as_int(row.get("entry_ts_ms")) is not None:
                rows_with_identity += 1
            if isinstance(row.get("path_bps"), list) and row.get("path_bps"):
                rows_with_path += 1
            records.append(row)
    return ReplayLoad(
        rows=rows,
        malformed_rows=malformed,
        rows_with_identity=rows_with_identity,
        rows_with_path=rows_with_path,
        records=records,
    )


def latest_decision_before(records: list[dict[str, Any]], entry_ts_ms: int) -> dict[str, Any] | None:
    timestamps = [int(row["ts_ms"]) for row in records]
    index = bisect.bisect_right(timestamps, entry_ts_ms) - 1
    if index < 0:
        return None
    return records[index]


def chronological_segment(index: int, total: int) -> str:
    ratio = index / total if total else 0.0
    if ratio < 1 / 3:
        return "train"
    if ratio < 2 / 3:
        return "validation"
    return "holdout"


def join_scope(evidence: ScopeEvidence) -> JoinedScope:
    if evidence.preferred_decision_path is None or evidence.replay_path is None:
        return JoinedScope(evidence.scope, evidence, 0, 0, [], 0, 0.0, 0, 0)
    decisions = load_decision_features(evidence.preferred_decision_path)
    replays = load_replay_rows(evidence.replay_path)
    joined: list[dict[str, Any]] = []
    unjoined = 0
    replay_records = sorted(
        replays.records,
        key=lambda row: (
            as_int(row.get("entry_ts_ms")) or 0,
            str(row.get("pool_id") or ""),
            str(row.get("base_mint") or ""),
        ),
    )
    for replay in replay_records:
        pool_id = replay.get("pool_id")
        base_mint = replay.get("base_mint")
        entry_ts_ms = as_int(replay.get("entry_ts_ms"))
        if not pool_id or not base_mint or entry_ts_ms is None:
            unjoined += 1
            continue
        candidates = decisions.index.get((str(pool_id), str(base_mint)), [])
        decision = latest_decision_before(candidates, entry_ts_ms)
        if decision is None:
            unjoined += 1
            continue
        joined.append(
            {
                "scope": evidence.scope,
                "entry_ts_ms": entry_ts_ms,
                "replay": replay,
                "features": decision["features"],
                "decision_ts_ms": decision["ts_ms"],
            }
        )
    joined.sort(key=lambda row: (int(row["entry_ts_ms"]), str(row["replay"].get("pool_id") or "")))
    total_joined = len(joined)
    for idx, row in enumerate(joined):
        row["segment"] = chronological_segment(idx, total_joined)
    return JoinedScope(
        scope=evidence.scope,
        evidence=evidence,
        replay_rows=replays.rows,
        decision_rows=decisions.rows,
        joined_records=joined,
        unjoined_replay_rows=unjoined,
        join_rate=safe_div(float(len(joined)), float(replays.rows)),
        malformed_decision_rows=decisions.malformed_rows,
        malformed_replay_rows=replays.malformed_rows,
    )


def fv(features: Mapping[str, Any], field: str, default: float = 0.0) -> float:
    value = as_float(features.get(field))
    return default if value is None else value


def any_present(features: Mapping[str, Any], fields: Iterable[str]) -> bool:
    return any(as_float(features.get(field)) is not None for field in fields)


def classifier_r0(features: Mapping[str, Any]) -> bool:
    return bool(features)


def classifier_r1(features: Mapping[str, Any]) -> bool:
    dev_concentration = fv(features, "dev_tx_ratio") >= 0.15 or fv(features, "dev_volume_ratio") >= 0.30
    signer_concentration = (
        fv(features, "hhi") >= 0.10
        or fv(features, "buyer_hhi") >= 0.10
        or fv(features, "top3_signer_volume_ratio") >= 0.55
        or fv(features, "top3_volume_pct") >= 0.55
    )
    return dev_concentration and signer_concentration


def classifier_r2(features: Mapping[str, Any]) -> bool:
    buy_burst = fv(features, "buy_count") >= 8
    buy_skew = fv(features, "buy_ratio") >= 0.55 or fv(features, "sol_buy_ratio") >= 0.55
    burst = (
        fv(features, "burst_ratio") >= 0.25
        or fv(features, "same_ms_tx_ratio") >= 0.10
        or fv(features, "delta_buy_count_1s_to_2s") >= 3
        or fv(features, "delta_buy_count_1s_to_3s") >= 5
    )
    return buy_burst and buy_skew and burst


def classifier_r3(features: Mapping[str, Any]) -> bool:
    cross_pool = fv(features, "signer_cross_pool_velocity") >= 0.25 or fv(features, "cpv_other_pool_activity") >= 0.50
    coordination = (
        fv(features, "same_ms_tx_ratio") >= 0.10
        or fv(features, "hhi") >= 0.10
        or fv(features, "buyer_hhi") >= 0.10
        or fv(features, "top3_signer_volume_ratio") >= 0.50
        or fv(features, "top3_volume_pct") >= 0.50
    )
    return cross_pool and coordination


def classifier_r4(features: Mapping[str, Any]) -> bool:
    markup = classifier_r2(features)
    overextension = (
        fv(features, "price_change_ratio") >= 1.20
        or fv(features, "max_single_tx_price_impact_pct_observed") >= 20.0
        or fv(features, "bonding_progress_pct") >= 45.0
    )
    dump_risk = (
        fv(features, "dev_tx_ratio") >= 0.05
        or fv(features, "dev_volume_ratio") >= 0.10
        or fv(features, "hhi") >= 0.08
        or fv(features, "signer_cross_pool_velocity") >= 0.10
    )
    return markup and overextension and dump_risk


CLASSIFIER_FUNCS: dict[str, Callable[[Mapping[str, Any]], bool]] = {
    "R0_BROAD": classifier_r0,
    "R1_DEV_SIGNER_CONCENTRATION": classifier_r1,
    "R2_BUY_BURST_MARKUP": classifier_r2,
    "R3_SCAMBOT_COORDINATION": classifier_r3,
    "R4_MARKUP_WITH_DUMP_RISK": classifier_r4,
}


def max_consecutive(predicate_values: list[bool]) -> int:
    best = current = 0
    for value in predicate_values:
        if value:
            current += 1
            best = max(best, current)
        else:
            current = 0
    return best


def classify_result(result: str, pnl_bps: int) -> tuple[bool, bool, bool, bool]:
    is_target = result == lab.TARGET
    is_stop = result == lab.STOP
    is_timeout = result == lab.TIMEOUT
    negative_timeout = is_timeout and pnl_bps < 0
    return is_target, is_stop, is_timeout, negative_timeout


def evaluate_records(records: list[dict[str, Any]], target_bps: int, stop_bps: int, max_hold_ms: int) -> list[dict[str, Any]]:
    evaluated: list[dict[str, Any]] = []
    for record in records:
        replay = record["replay"]
        baseline = lab.simulate_baseline_cached(replay, target_bps, stop_bps, max_hold_ms)
        if baseline is None:
            continue
        is_target, is_stop, is_timeout, negative_timeout = classify_result(baseline.result, baseline.pnl_bps)
        evaluated.append(
            {
                "scope": record["scope"],
                "segment": record["segment"],
                "entry_ts_ms": record["entry_ts_ms"],
                "result": baseline.result,
                "pnl_bps": int(baseline.pnl_bps),
                "target": is_target,
                "stop": is_stop,
                "timeout": is_timeout,
                "negative_timeout": negative_timeout,
                "mfe_bps": as_int(replay.get("mfe_bps")),
                "mae_bps": as_int(replay.get("mae_bps")),
                "result_quality": baseline.result_quality,
                "pnl_quality": baseline.pnl_quality,
            }
        )
    evaluated.sort(key=lambda row: int(row["entry_ts_ms"]))
    return evaluated


def tail_metrics(pnls_after_cost: list[int], top_fraction: float) -> dict[str, Any]:
    if not pnls_after_cost:
        return {
            "top_fraction": top_fraction,
            "removed_count": 0,
            "removed_sum_bps": 0,
            "remaining_sum_bps": 0,
            "remaining_avg_bps": 0.0,
            "remaining_median_bps": 0.0,
            "tail_dependency_flag": False,
        }
    positives = sorted([pnl for pnl in pnls_after_cost if pnl > 0], reverse=True)
    remove_count = min(len(positives), math.ceil(len(pnls_after_cost) * top_fraction))
    removed = positives[:remove_count]
    removed_remaining = removed.copy()
    remaining: list[int] = []
    for pnl in sorted(pnls_after_cost, reverse=True):
        if pnl > 0 and removed_remaining and pnl == removed_remaining[0]:
            removed_remaining.pop(0)
            continue
        remaining.append(pnl)
    remaining_sum = int(sum(remaining))
    return {
        "top_fraction": top_fraction,
        "removed_count": remove_count,
        "removed_sum_bps": int(sum(removed)),
        "remaining_sum_bps": remaining_sum,
        "remaining_avg_bps": mean([float(pnl) for pnl in remaining]),
        "remaining_median_bps": median([float(pnl) for pnl in remaining]),
        "tail_dependency_flag": remaining_sum < 0,
    }


def base_metrics(evaluated: list[dict[str, Any]], cost_bps: int) -> dict[str, Any]:
    count = len(evaluated)
    gross = [int(row["pnl_bps"]) for row in evaluated]
    after_cost = [pnl - cost_bps for pnl in gross]
    nonloss = sum(1 for pnl in after_cost if pnl >= 0)
    target_count = sum(1 for row in evaluated if row["target"])
    stop_count = sum(1 for row in evaluated if row["stop"])
    timeout_count = sum(1 for row in evaluated if row["timeout"])
    negative_timeout_count = sum(1 for row in evaluated if row["negative_timeout"])
    return {
        "evaluated_count": count,
        "target_count": target_count,
        "target_rate": safe_div(float(target_count), float(count)),
        "stop_count": stop_count,
        "stop_rate": safe_div(float(stop_count), float(count)),
        "timeout_count": timeout_count,
        "timeout_rate": safe_div(float(timeout_count), float(count)),
        "negative_timeout_count": negative_timeout_count,
        "negative_timeout_rate": safe_div(float(negative_timeout_count), float(count)),
        "gross_sum_pnl_bps": int(sum(gross)),
        "gross_avg_pnl_bps": mean([float(value) for value in gross]),
        "gross_median_pnl_bps": median([float(value) for value in gross]),
        f"cost{cost_bps}_sum_pnl_bps": int(sum(after_cost)),
        f"cost{cost_bps}_avg_pnl_bps": mean([float(value) for value in after_cost]),
        f"cost{cost_bps}_median_pnl_bps": median([float(value) for value in after_cost]),
        f"cost{cost_bps}_nonloss_count": nonloss,
        f"cost{cost_bps}_precision": safe_div(float(nonloss), float(count)),
        f"cost{cost_bps}_wilson_lower95": lab.wilson_lower_bound(nonloss, count),
        f"cost{cost_bps}_max_consecutive_losses": max_consecutive([pnl < 0 for pnl in after_cost]),
        f"cost{cost_bps}_mfe_median_bps": median([float(row["mfe_bps"]) for row in evaluated if row.get("mfe_bps") is not None]),
        f"cost{cost_bps}_mae_median_bps": median([float(row["mae_bps"]) for row in evaluated if row.get("mae_bps") is not None]),
    }


def acceptance_failures(metrics: Mapping[str, Any], join_rate: float, selected_count: int, tail5: Mapping[str, Any]) -> list[str]:
    failures: list[str] = []
    if join_rate < 0.98:
        failures.append("join_quality_lt_98pct")
    if selected_count < 250:
        failures.append("selected_sample_lt_250")
    if float(metrics.get("cost100_precision") or 0.0) < 0.65:
        failures.append("precision_cost100_lt_65pct")
    if int(metrics.get("cost100_sum_pnl_bps") or 0) <= 0:
        failures.append("cost100_sum_not_positive")
    if int(metrics.get("cost200_sum_pnl_bps") or 0) <= 0:
        failures.append("cost200_sum_not_positive_for_promising")
    if float(metrics.get("cost100_median_pnl_bps") or 0.0) < 0.0:
        failures.append("median_cost100_negative")
    if float(metrics.get("cost200_median_pnl_bps") or 0.0) < 0.0:
        failures.append("median_cost200_negative_for_promising")
    if int(tail5.get("remaining_sum_bps") or 0) < 0:
        failures.append("top5_tail_removed_negative")
    return failures


def evaluate_scope(scope_data: JoinedScope) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    summary_rows: list[dict[str, Any]] = []
    cost_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    tail_rows: list[dict[str, Any]] = []
    replay_total = scope_data.replay_rows
    for classifier_name in CLASSIFIERS:
        predicate = CLASSIFIER_FUNCS[classifier_name]
        selected = [record for record in scope_data.joined_records if predicate(record["features"])]
        retained_pct = safe_div(float(len(selected)), float(replay_total))
        for target_bps in TARGET_BPS_GRID:
            for stop_bps in STOP_BPS_GRID:
                for max_hold_ms in MAX_HOLD_MS_GRID:
                    evaluated = evaluate_records(selected, target_bps, stop_bps, max_hold_ms)
                    metrics100 = base_metrics(evaluated, 100)
                    metrics200 = base_metrics(evaluated, 200)
                    metrics = {**metrics100, **metrics200}
                    tail5 = tail_metrics([int(row["pnl_bps"]) - 100 for row in evaluated], 0.05)
                    tail10 = tail_metrics([int(row["pnl_bps"]) - 100 for row in evaluated], 0.10)
                    failures = acceptance_failures(metrics, scope_data.join_rate, len(evaluated), tail5)
                    passes_single_scope = not failures
                    summary_rows.append(
                        {
                            "scope": scope_data.scope,
                            "classifier": classifier_name,
                            "target_bps": target_bps,
                            "stop_bps": stop_bps,
                            "max_hold_ms": max_hold_ms,
                            "retained_count": len(evaluated),
                            "retained_pct": retained_pct,
                            "selected_before_replay_eval_count": len(selected),
                            "exact_join_rate": scope_data.join_rate,
                            "decision_rows": scope_data.decision_rows,
                            "replay_rows": scope_data.replay_rows,
                            "unjoined_replay_rows": scope_data.unjoined_replay_rows,
                            "target_rate": metrics100["target_rate"],
                            "stop_rate": metrics100["stop_rate"],
                            "timeout_rate": metrics100["timeout_rate"],
                            "negative_timeout_rate": metrics100["negative_timeout_rate"],
                            "avg_pnl_bps": metrics100["gross_avg_pnl_bps"],
                            "median_pnl_bps": metrics100["gross_median_pnl_bps"],
                            "sum_pnl_bps": metrics100["gross_sum_pnl_bps"],
                            "precision_cost100": metrics100["cost100_precision"],
                            "wilson_lower95_cost100": metrics100["cost100_wilson_lower95"],
                            "cost100_sum_pnl_bps": metrics100["cost100_sum_pnl_bps"],
                            "cost100_avg_pnl_bps": metrics100["cost100_avg_pnl_bps"],
                            "cost100_median_pnl_bps": metrics100["cost100_median_pnl_bps"],
                            "cost200_sum_pnl_bps": metrics200["cost200_sum_pnl_bps"],
                            "cost200_avg_pnl_bps": metrics200["cost200_avg_pnl_bps"],
                            "cost200_median_pnl_bps": metrics200["cost200_median_pnl_bps"],
                            "max_consecutive_losses_cost100": metrics100["cost100_max_consecutive_losses"],
                            "mfe_median_bps": metrics100["cost100_mfe_median_bps"],
                            "mae_median_bps": metrics100["cost100_mae_median_bps"],
                            "after_top5_positive_removed_cost100_sum_bps": tail5["remaining_sum_bps"],
                            "after_top10_positive_removed_cost100_sum_bps": tail10["remaining_sum_bps"],
                            "passes_single_scope_gates": passes_single_scope,
                            "passes_promising_gate": False,
                            "no_leakage_assertion": True,
                            "acceptance_failures": ";".join(failures),
                        }
                    )
                    for cost in COSTS_BPS:
                        cost_metrics = metrics100 if cost == 100 else metrics200
                        cost_rows.append(
                            {
                                "scope": scope_data.scope,
                                "classifier": classifier_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "roundtrip_cost_bps": cost,
                                "retained_count": len(evaluated),
                                "sum_pnl_after_cost_bps": cost_metrics[f"cost{cost}_sum_pnl_bps"],
                                "avg_pnl_after_cost_bps": cost_metrics[f"cost{cost}_avg_pnl_bps"],
                                "median_pnl_after_cost_bps": cost_metrics[f"cost{cost}_median_pnl_bps"],
                                "nonloss_count": cost_metrics[f"cost{cost}_nonloss_count"],
                                "precision": cost_metrics[f"cost{cost}_precision"],
                                "wilson_lower95": cost_metrics[f"cost{cost}_wilson_lower95"],
                                "max_consecutive_losses": cost_metrics[f"cost{cost}_max_consecutive_losses"],
                            }
                        )
                    for segment in SEGMENTS:
                        segment_records = [row for row in evaluated if row["segment"] == segment]
                        seg100 = base_metrics(segment_records, 100)
                        seg200 = base_metrics(segment_records, 200)
                        stability_rows.append(
                            {
                                "scope": scope_data.scope,
                                "classifier": classifier_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "segment": segment,
                                "retained_count": len(segment_records),
                                "precision_cost100": seg100["cost100_precision"],
                                "wilson_lower95_cost100": seg100["cost100_wilson_lower95"],
                                "cost100_sum_pnl_bps": seg100["cost100_sum_pnl_bps"],
                                "cost100_avg_pnl_bps": seg100["cost100_avg_pnl_bps"],
                                "cost100_median_pnl_bps": seg100["cost100_median_pnl_bps"],
                                "cost200_sum_pnl_bps": seg200["cost200_sum_pnl_bps"],
                                "cost200_avg_pnl_bps": seg200["cost200_avg_pnl_bps"],
                                "cost200_median_pnl_bps": seg200["cost200_median_pnl_bps"],
                                "target_rate": seg100["target_rate"],
                                "stop_rate": seg100["stop_rate"],
                                "timeout_rate": seg100["timeout_rate"],
                                "negative_timeout_rate": seg100["negative_timeout_rate"],
                                "max_consecutive_losses_cost100": seg100["cost100_max_consecutive_losses"],
                            }
                        )
                    for top_fraction, tail in ((0.05, tail5), (0.10, tail10)):
                        tail_rows.append(
                            {
                                "scope": scope_data.scope,
                                "classifier": classifier_name,
                                "target_bps": target_bps,
                                "stop_bps": stop_bps,
                                "max_hold_ms": max_hold_ms,
                                "roundtrip_cost_bps": 100,
                                "top_positive_fraction_removed": top_fraction,
                                "retained_count_before_tail_removal": len(evaluated),
                                "removed_count": tail["removed_count"],
                                "removed_sum_bps": tail["removed_sum_bps"],
                                "sum_pnl_after_tail_removal_bps": tail["remaining_sum_bps"],
                                "avg_pnl_after_tail_removal_bps": tail["remaining_avg_bps"],
                                "median_pnl_after_tail_removal_bps": tail["remaining_median_bps"],
                                "tail_dependency_flag": tail["tail_dependency_flag"],
                            }
                        )
    return summary_rows, cost_rows, stability_rows, tail_rows


def threshold_manifest_rows() -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = [
        {
            "family": "classifier",
            "classifier": "R0_BROAD",
            "field_or_rule": "any joined replay row with decision-time feature snapshot",
            "operator": "exists",
            "threshold": "",
            "source": "PR-RUG-MARKUP-A0 fixed classifier family",
            "used_as_feature": True,
            "notes": "broad baseline; pool_id/base_mint used only for join",
        },
        {
            "family": "classifier",
            "classifier": "R1_DEV_SIGNER_CONCENTRATION",
            "field_or_rule": "(dev_tx_ratio >= 0.15 OR dev_volume_ratio >= 0.30) AND (hhi/buyer_hhi >= 0.10 OR top3 ratio >= 0.55)",
            "operator": "fixed_rule",
            "threshold": "",
            "source": "PR-RUG-MARKUP-A0 static A0 thresholds; no R50 tuning",
            "used_as_feature": True,
            "notes": "pre-entry/dev/signer concentration only",
        },
        {
            "family": "classifier",
            "classifier": "R2_BUY_BURST_MARKUP",
            "field_or_rule": "buy_count >= 8 AND (buy_ratio >= 0.55 OR sol_buy_ratio >= 0.55) AND burst/same-ms/delta-buy condition",
            "operator": "fixed_rule",
            "threshold": "",
            "source": "PR-RUG-MARKUP-A0 static A0 thresholds; no R50 tuning",
            "used_as_feature": True,
            "notes": "early buy burst markup profile",
        },
        {
            "family": "classifier",
            "classifier": "R3_SCAMBOT_COORDINATION",
            "field_or_rule": "(signer_cross_pool_velocity >= 0.25 OR cpv_other_pool_activity >= 0.50) AND coordination concentration/burst condition",
            "operator": "fixed_rule",
            "threshold": "",
            "source": "PR-RUG-MARKUP-A0 static A0 thresholds; no R50 tuning",
            "used_as_feature": True,
            "notes": "cross-pool/scambot coordination profile",
        },
        {
            "family": "classifier",
            "classifier": "R4_MARKUP_WITH_DUMP_RISK",
            "field_or_rule": "R2 plus overextension plus dump-risk concentration",
            "operator": "fixed_rule",
            "threshold": "",
            "source": "PR-RUG-MARKUP-A0 static A0 thresholds; no R50 tuning",
            "used_as_feature": True,
            "notes": "markup phase with dump risk profile",
        },
    ]
    for target_bps in TARGET_BPS_GRID:
        rows.append(
            {
                "family": "exit_grid",
                "classifier": "ALL",
                "field_or_rule": "target_bps",
                "operator": "fixed_value",
                "threshold": target_bps,
                "source": "user predeclared grid",
                "used_as_feature": False,
                "notes": "evaluation-only target; not input feature",
            }
        )
    for stop_bps in STOP_BPS_GRID:
        rows.append(
            {
                "family": "exit_grid",
                "classifier": "ALL",
                "field_or_rule": "stop_bps",
                "operator": "fixed_value",
                "threshold": stop_bps,
                "source": "user predeclared grid",
                "used_as_feature": False,
                "notes": "evaluation-only stop; not input feature",
            }
        )
    for hold_ms in MAX_HOLD_MS_GRID:
        rows.append(
            {
                "family": "exit_grid",
                "classifier": "ALL",
                "field_or_rule": "max_hold_ms",
                "operator": "fixed_value",
                "threshold": hold_ms,
                "source": "user predeclared grid",
                "used_as_feature": False,
                "notes": "evaluation-only hold; not input feature",
            }
        )
    for field in FORBIDDEN_FEATURES:
        rows.append(
            {
                "family": "forbidden_feature",
                "classifier": "ALL",
                "field_or_rule": field,
                "operator": "not_used",
                "threshold": "",
                "source": "PR-RUG-MARKUP-A0 leakage guard",
                "used_as_feature": False,
                "notes": "not used as classifier input",
            }
        )
    return rows


def best_rows(summary_rows: list[dict[str, Any]], limit: int = 8) -> list[dict[str, Any]]:
    return sorted(
        summary_rows,
        key=lambda row: (
            bool(row.get("passes_single_scope_gates")),
            int(row.get("cost100_sum_pnl_bps") or 0),
            float(row.get("precision_cost100") or 0.0),
            int(row.get("retained_count") or 0),
        ),
        reverse=True,
    )[:limit]


def markdown_table(rows: list[Mapping[str, Any]], fields: list[str]) -> str:
    out = ["| " + " | ".join(fields) + " |", "| " + " | ".join(["---"] * len(fields)) + " |"]
    for row in rows:
        out.append("| " + " | ".join(str(row.get(field, "")) for field in fields) + " |")
    return "\n".join(out)


def final_result(
    requested_evidences: list[ScopeEvidence],
    joined_scopes: list[JoinedScope],
    summary_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    full_requested = [evidence for evidence in requested_evidences if evidence.has_full_evidence]
    passing_by_rule: dict[tuple[str, int, int, int], set[str]] = {}
    for row in summary_rows:
        if row.get("passes_single_scope_gates") is True:
            key = (
                str(row.get("classifier")),
                int(row.get("target_bps") or 0),
                int(row.get("stop_bps") or 0),
                int(row.get("max_hold_ms") or 0),
            )
            passing_by_rule.setdefault(key, set()).add(str(row.get("scope")))
    passing_both = [key for key, scopes in passing_by_rule.items() if len(scopes) >= 2]
    passing_any = [key for key, scopes in passing_by_rule.items() if scopes]
    if not full_requested:
        verdict = VERDICT_BLOCKED
    elif len(full_requested) == 1 and passing_any:
        verdict = VERDICT_SINGLE_SCOPE
    elif len(full_requested) >= 2 and passing_both:
        verdict = VERDICT_PROMISING
    else:
        verdict = VERDICT_REJECTED
    best = best_rows(summary_rows, 1)
    return {
        "verdict": verdict,
        "full_evidence_scope_count": len(full_requested),
        "full_evidence_scopes": ";".join(evidence.scope for evidence in full_requested),
        "scope_count_evaluated": len(joined_scopes),
        "fixed_rules_tested_per_scope": len(CLASSIFIERS) * len(TARGET_BPS_GRID) * len(STOP_BPS_GRID) * len(MAX_HOLD_MS_GRID),
        "passing_fixed_rule_count": len(passing_both),
        "single_scope_signal_count": len(passing_any),
        "best_rule": "" if not best else f"{best[0]['classifier']}/{best[0]['target_bps']}/{best[0]['stop_bps']}/{best[0]['max_hold_ms']}",
        "best_scope": "" if not best else best[0]["scope"],
        "best_cost100_sum_bps": "" if not best else best[0]["cost100_sum_pnl_bps"],
        "best_cost100_precision": "" if not best else best[0]["precision_cost100"],
        "runtime_approval": False,
        "shadow_close_only_approval": False,
        "new_run_approval": False,
    }


def write_blocked_outputs(evidences: list[ScopeEvidence], result: Mapping[str, Any]) -> None:
    write_csv(
        SUMMARY_CSV,
        [
            {
                "verdict": result["verdict"],
                "blocking_reason": "no_requested_scope_has_both_gatekeeper_decisions_and_shadow_exit_replay",
                "runtime_approval": False,
                "shadow_close_only_approval": False,
            }
        ],
    )
    write_csv(COST_CSV, [])
    write_csv(STABILITY_CSV, [])
    write_csv(TAIL_CSV, [])
    write_csv(THRESHOLD_CSV, threshold_manifest_rows())
    write_reports(evidences, [], [], [], result)


def write_reports(
    evidences: list[ScopeEvidence],
    joined_scopes: list[JoinedScope],
    summary_rows: list[dict[str, Any]],
    stability_rows: list[dict[str, Any]],
    result: Mapping[str, Any],
) -> None:
    REPORT_MD.parent.mkdir(parents=True, exist_ok=True)
    ADR_MD.parent.mkdir(parents=True, exist_ok=True)
    requested = [row for row in inventory_rows(evidences) if row["role"] in ("primary_r49", "primary_r50")]
    inventory_md = markdown_table(
        requested,
        [
            "scope",
            "has_gatekeeper_v2_decisions",
            "has_materialized_feature_snapshot",
            "has_shadow_exit_replay_v1",
            "full_evidence_for_rug_markup_a0",
            "blocking_reason",
        ],
    )
    best_md = markdown_table(
        best_rows(summary_rows, 10),
        [
            "scope",
            "classifier",
            "target_bps",
            "stop_bps",
            "max_hold_ms",
            "retained_count",
            "precision_cost100",
            "wilson_lower95_cost100",
            "cost100_sum_pnl_bps",
            "cost200_sum_pnl_bps",
            "cost100_median_pnl_bps",
            "passes_single_scope_gates",
            "acceptance_failures",
        ],
    )
    scope_md = markdown_table(
        [
            {
                "scope": scope.scope,
                "decision_rows": scope.decision_rows,
                "replay_rows": scope.replay_rows,
                "joined_records": len(scope.joined_records),
                "exact_join_rate": scope.join_rate,
                "unjoined_replay_rows": scope.unjoined_replay_rows,
                "malformed_decision_rows": scope.malformed_decision_rows,
                "malformed_replay_rows": scope.malformed_replay_rows,
            }
            for scope in joined_scopes
        ],
        [
            "scope",
            "decision_rows",
            "replay_rows",
            "joined_records",
            "exact_join_rate",
            "unjoined_replay_rows",
            "malformed_decision_rows",
            "malformed_replay_rows",
        ],
    )
    if result["verdict"] == VERDICT_REJECTED:
        decision_line = "NO R51. CLOSE TRADING EDGE SEARCH."
    elif result["verdict"] == VERDICT_SINGLE_SCOPE:
        decision_line = "NO RUNTIME. NO SHADOW_CLOSE. ONLY SECOND LOGGING-ONLY SCOPE CAN VALIDATE."
    elif result["verdict"] == VERDICT_BLOCKED:
        decision_line = "NO R51 from this evidence set. The proof is blocked by missing full evidence."
    else:
        decision_line = "NO RUNTIME. NO SHADOW_CLOSE. Fresh predeclared validation would still be required."

    report = f"""# PR-RUG-MARKUP-A0: Offline proof

Data: `2026-06-29`

Final verdict: `{result['verdict']}`

## Decyzja

{decision_line}

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

`new_run_approval = false`

## Zakres i ograniczenia

To jest offline-only proof. Nie zmienia runtime, Gatekeepera, BUY/REJECT, selector runtime, `v25_confidence`, V3 promotion, TX builder/sender/Jito/live path, `shadow_close_only`, active close, sidecarow, `alpha_31100`, XGBoost ani zadnych progow runtime.

Skrypt nie uruchamia runu i nie wykonuje cleanupu. Surowe JSONL pozostaja lokalnym dowodem i nie sa przeznaczone do commita.

## Evidence inventory

Pierwszy wygenerowany artefakt:

`{INVENTORY_CSV}`

{inventory_md}

## Scope coverage

{scope_md if joined_scopes else "Brak scope z pelnym evidence do ewaluacji."}

Full evidence scopes: `{result['full_evidence_scopes']}`

Full evidence scope count: `{result['full_evidence_scope_count']}`

## Fixed classifier family

- `R0_BROAD`
- `R1_DEV_SIGNER_CONCENTRATION`
- `R2_BUY_BURST_MARKUP`
- `R3_SCAMBOT_COORDINATION`
- `R4_MARKUP_WITH_DUMP_RISK`

Klasyfikatory korzystaja tylko z decision-time/pre-entry fieldow z `gatekeeper_v2_decisions.jsonl` / `materialized_feature_snapshot`. `pool_id`, `base_mint` i identyfikatory sa uzyte tylko do joinu i sortowania chronologicznego, nie jako features.

## Fixed exit grid

- target_bps: `{', '.join(str(x) for x in TARGET_BPS_GRID)}`
- stop_bps: `{', '.join(str(x) for x in STOP_BPS_GRID)}`
- max_hold_ms: `{', '.join(str(x) for x in MAX_HOLD_MS_GRID)}`
- costs: `{', '.join(str(x) for x in COSTS_BPS)}`

Nie uzyto broad grid search, nowych masek ani strojenia R50.

## Best diagnostic rows

{best_md if summary_rows else "Brak metryk: proof blocked before evaluation."}

## Acceptance

Hard gates dla `PROMISING` wymagaja tej samej fixed rule na co najmniej dwoch niezaleznych scope. W obecnym evidence set `{result['full_evidence_scope_count']}` requested scope ma pelny pre-entry + replay evidence.

Passing fixed rules across two scopes: `{result['passing_fixed_rule_count']}`

Single-scope signal rows: `{result['single_scope_signal_count']}`

Best rule: `{result['best_rule']}` on `{result['best_scope']}`

## Leakage guard

Zakazane fieldy nie sa uzywane jako classifier inputs: final PnL, target/stop/timeout labels, future path after decision horizon, pool/mint/signer/dev wallet IDs oraz outcome-derived labels. `shadow_exit_replay_v1.path_bps` sluzy tylko do ewaluacji target/stop/hold i tail audit.

## Output files

- `{INVENTORY_CSV}`
- `{SUMMARY_CSV}`
- `{COST_CSV}`
- `{STABILITY_CSV}`
- `{TAIL_CSV}`
- `{THRESHOLD_CSV}`
- `{ADR_MD}`

## Runtime decision

Nie istnieje zgoda na runtime ani `shadow_close_only` z tego A0.
"""
    REPORT_MD.write_text(report, encoding="utf-8")

    adr = f"""# ADR-8D: PR-RUG-MARKUP-A0 offline result

Status: {result['verdict']} / NO_RUNTIME
Typ: ADR-8D / offline research result
Data: 2026-06-29
Zakres: PR-RUG-MARKUP-A0
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Zaimplementowano offline proof `scripts/rug_markup_a0_offline_proof.py`.

Final verdict: `{result['verdict']}`

{decision_line}

## 2. Runtime boundary

Nie wykonano i nie zatwierdzono:

- runtime change,
- Gatekeeper/BUY/REJECT change,
- selector runtime change,
- `v25_confidence` change,
- V3 promotion change,
- TX builder/sender/Jito/live path change,
- active close,
- `shadow_close_only`,
- sidecar,
- `alpha_31100`,
- XGBoost,
- nowego runu.

## 3. Evidence

Inventory jest pierwszym outputem skryptu: `{INVENTORY_CSV}`.

Requested scopes:

{inventory_md}

## 4. Metoda

Przetestowano tylko predeclared classifier family R0-R4 oraz fixed exit grid:

- target_bps: `{', '.join(str(x) for x in TARGET_BPS_GRID)}`
- stop_bps: `{', '.join(str(x) for x in STOP_BPS_GRID)}`
- max_hold_ms: `{', '.join(str(x) for x in MAX_HOLD_MS_GRID)}`
- cost: `{', '.join(str(x) for x in COSTS_BPS)}`

Nie dodano nowych progow runtime, masek, broad grid search ani R50 tuning.

## 5. Wynik

Full evidence scope count: `{result['full_evidence_scope_count']}`

Passing fixed rules across two scopes: `{result['passing_fixed_rule_count']}`

Single-scope signal rows: `{result['single_scope_signal_count']}`

Best diagnostic rule: `{result['best_rule']}`

## 6. Konsekwencje

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

`new_run_approval = false`

Surowe JSONL sa lokalnym dowodem i nie powinny byc commitowane.

## 7. Files

- `scripts/rug_markup_a0_offline_proof.py`
- `{INVENTORY_CSV}`
- `{SUMMARY_CSV}`
- `{COST_CSV}`
- `{STABILITY_CSV}`
- `{TAIL_CSV}`
- `{THRESHOLD_CSV}`
- `{REPORT_MD}`
"""
    ADR_MD.write_text(adr, encoding="utf-8")


def requested_evidences(evidences: list[ScopeEvidence], r49_scope: str, r50_scope: str) -> list[ScopeEvidence]:
    by_scope = {evidence.scope: evidence for evidence in evidences}
    return [by_scope[scope] for scope in (r49_scope, r50_scope) if scope in by_scope]


def main() -> int:
    args = parse_args()
    global REPORT_DIR, INVENTORY_CSV, SUMMARY_CSV, COST_CSV, STABILITY_CSV, TAIL_CSV, THRESHOLD_CSV
    REPORT_DIR = args.reports_dir
    INVENTORY_CSV = REPORT_DIR / "rug_markup_a0_evidence_inventory.csv"
    SUMMARY_CSV = REPORT_DIR / "rug_markup_a0_summary.csv"
    COST_CSV = REPORT_DIR / "rug_markup_a0_cost_sensitivity.csv"
    STABILITY_CSV = REPORT_DIR / "rug_markup_a0_stability.csv"
    TAIL_CSV = REPORT_DIR / "rug_markup_a0_tail_audit.csv"
    THRESHOLD_CSV = REPORT_DIR / "rug_markup_a0_threshold_manifest.csv"

    evidences = build_evidence_inventory(args.r49_scope, args.r50_scope, args.local_logs_root, args.volume_logs_root)
    write_csv(INVENTORY_CSV, inventory_rows(evidences))

    requested = requested_evidences(evidences, args.r49_scope, args.r50_scope)
    full_requested = [evidence for evidence in requested if evidence.has_full_evidence]
    if not full_requested:
        result = {
            "verdict": VERDICT_BLOCKED,
            "full_evidence_scope_count": 0,
            "full_evidence_scopes": "",
            "scope_count_evaluated": 0,
            "fixed_rules_tested_per_scope": len(CLASSIFIERS) * len(TARGET_BPS_GRID) * len(STOP_BPS_GRID) * len(MAX_HOLD_MS_GRID),
            "passing_fixed_rule_count": 0,
            "single_scope_signal_count": 0,
            "best_rule": "",
            "best_scope": "",
            "best_cost100_sum_bps": "",
            "best_cost100_precision": "",
            "runtime_approval": False,
            "shadow_close_only_approval": False,
            "new_run_approval": False,
        }
        write_blocked_outputs(requested, result)
        print(f"Final verdict: {VERDICT_BLOCKED}")
        print(f"Evidence inventory: {INVENTORY_CSV}")
        return 0

    joined_scopes = [join_scope(evidence) for evidence in full_requested]
    summary_rows: list[dict[str, Any]] = []
    cost_rows: list[dict[str, Any]] = []
    stability_rows: list[dict[str, Any]] = []
    tail_rows: list[dict[str, Any]] = []
    for scope_data in joined_scopes:
        scope_summary, scope_cost, scope_stability, scope_tail = evaluate_scope(scope_data)
        summary_rows.extend(scope_summary)
        cost_rows.extend(scope_cost)
        stability_rows.extend(scope_stability)
        tail_rows.extend(scope_tail)

    result = final_result(requested, joined_scopes, summary_rows)
    if result["verdict"] == VERDICT_PROMISING and len(full_requested) < 2:
        result["verdict"] = VERDICT_SINGLE_SCOPE
    write_csv(SUMMARY_CSV, summary_rows)
    write_csv(COST_CSV, cost_rows)
    write_csv(STABILITY_CSV, stability_rows)
    write_csv(TAIL_CSV, tail_rows)
    write_csv(THRESHOLD_CSV, threshold_manifest_rows())
    write_reports(requested, joined_scopes, summary_rows, stability_rows, result)

    print(f"Final verdict: {result['verdict']}")
    print(f"Evidence inventory: {INVENTORY_CSV}")
    print(f"Full evidence scopes: {result['full_evidence_scopes']}")
    print(f"Passing fixed rules across two scopes: {result['passing_fixed_rule_count']}")
    print(f"Single-scope signal rows: {result['single_scope_signal_count']}")
    print(f"Best diagnostic rule: {result['best_rule']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
