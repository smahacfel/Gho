#!/usr/bin/env python3
"""Build minimal cutoff-safe buyer quality context for selector candidates.

P4E-A is offline-only. It materializes buyer participation history that was
observable before each candidate cutoff. It does not compute buyer PnL, buyer R2
success rates, runtime scores, Gatekeeper policy, execution, or send behavior.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

import selector_pipeline_common as common


ARTIFACT = "buyer_quality_context_v1"
MANIFEST_ARTIFACT = "buyer_quality_context_manifest_v1"
DEFAULT_FIRST_N = 5


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default="/root/Gho")
    parser.add_argument("--scope", required=True)
    parser.add_argument("--runtime-scope", required=True)
    parser.add_argument("--candidate-universe", type=Path)
    parser.add_argument("--events-glob", action="append")
    parser.add_argument("--coordination-glob", action="append")
    parser.add_argument("--first-n", type=int, default=DEFAULT_FIRST_N)
    parser.add_argument("--max-event-bytes-for-wallet-history", type=int, default=2_000_000_000)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser


def candidate_universe_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / "candidate_universe_v1.jsonl"


def output_path(root: Path, scope: str) -> Path:
    return root / "datasets" / "selector" / scope / f"{ARTIFACT}.jsonl"


def manifest_path(root: Path, scope: str) -> Path:
    return root / "reports" / "selector" / scope / f"{MANIFEST_ARTIFACT}.json"


def event_paths(root: Path, runtime_scope: str, globs: list[str] | None) -> list[Path]:
    patterns = globs or [f"datasets/events/{runtime_scope}/*.jsonl"]
    paths: list[Path] = []
    for pattern in patterns:
        paths.extend(root.glob(pattern))
    return sorted(set(paths))


def coordination_paths(root: Path, runtime_scope: str, globs: list[str] | None) -> list[Path]:
    patterns = globs or [
        f"logs/rollout/{runtime_scope}/decisions/**/coordination_risk_evidence.jsonl"
    ]
    paths: list[Path] = []
    for pattern in patterns:
        paths.extend(root.glob(pattern))
    return sorted(set(paths))


def nested_get(value: Any, *keys: str) -> Any:
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def event_kind(row: dict[str, Any]) -> str:
    kind = row.get("kind")
    if isinstance(kind, dict):
        return str(kind.get("type") or "")
    return str(row.get("type") or row.get("event_type") or "")


def event_payload(row: dict[str, Any]) -> dict[str, Any]:
    payload = nested_get(row, "kind", "payload")
    if isinstance(payload, dict):
        return payload
    payload = row.get("payload")
    return payload if isinstance(payload, dict) else row


def event_ts_ms(row: dict[str, Any], payload: dict[str, Any]) -> int | None:
    return common.int_or_none(
        payload.get("event_ts_ms")
        or payload.get("timestamp_ms")
        or payload.get("decision_ts_ms")
        or nested_get(row, "envelope", "event_time_ms")
        or row.get("event_time_ms")
    )


def is_successful_buy(payload: dict[str, Any]) -> bool:
    side = str(payload.get("side") or payload.get("direction") or "").lower()
    is_buy = payload.get("is_buy")
    success = payload.get("success")
    return (side == "buy" or is_buy is True) and success is not False


def iter_pool_transaction_rows(path: Path):
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8", errors="ignore") as fh:
        for line in fh:
            if "PoolTransaction" not in line:
                continue
            raw = line.strip()
            if not raw:
                continue
            index = 0
            while index < len(raw):
                try:
                    obj, next_index = decoder.raw_decode(raw, index)
                except json.JSONDecodeError:
                    break
                if isinstance(obj, dict):
                    yield obj
                index = next_index
                while index < len(raw) and raw[index].isspace():
                    index += 1


def parse_pool_transactions(
    paths: list[Path],
    *,
    wanted_pools: set[str] | None = None,
    wanted_mints: set[str] | None = None,
    wanted_wallets: set[str] | None = None,
) -> list[dict[str, Any]]:
    txs: list[dict[str, Any]] = []
    for path in paths:
        for row in iter_pool_transaction_rows(path):
            if event_kind(row) and event_kind(row) != "PoolTransaction":
                continue
            payload = event_payload(row)
            if not payload or not is_successful_buy(payload):
                continue
            ts_ms = event_ts_ms(row, payload)
            wallet = common.str_or_none(payload.get("wallet")) or common.str_or_none(payload.get("signer"))
            pool_id = common.str_or_none(payload.get("pool_id")) or common.str_or_none(payload.get("pool_amm_id"))
            base_mint = (
                common.str_or_none(payload.get("base_mint"))
                or common.str_or_none(payload.get("mint_id"))
                or common.str_or_none(payload.get("token_mint"))
            )
            if ts_ms is None or not wallet or not (pool_id or base_mint):
                continue
            if wanted_wallets is not None and wallet not in wanted_wallets:
                continue
            if wanted_pools is not None or wanted_mints is not None:
                pool_match = bool(pool_id and wanted_pools is not None and pool_id in wanted_pools)
                mint_match = bool(base_mint and wanted_mints is not None and base_mint in wanted_mints)
                if not pool_match and not mint_match:
                    continue
            txs.append(
                {
                    "ts_ms": ts_ms,
                    "slot": common.int_or_none(payload.get("slot") or payload.get("event_slot")),
                    "wallet": wallet,
                    "pool_id": pool_id,
                    "base_mint": base_mint,
                    "volume_sol": common.float_or_none(
                        payload.get("volume_sol") or payload.get("quote_amount_sol")
                    ),
                }
            )
    return sorted(txs, key=lambda item: (item["ts_ms"], item.get("slot") or 0, item["wallet"]))


def candidate_identity_sets(candidates: list[dict[str, Any]]) -> tuple[set[str], set[str]]:
    pools: set[str] = set()
    mints: set[str] = set()
    for candidate in candidates:
        pool_id = common.str_or_none(candidate.get("pool_id"))
        mint = common.str_or_none(candidate.get("base_mint")) or common.str_or_none(candidate.get("mint_id"))
        if pool_id:
            pools.add(pool_id)
        if mint:
            mints.add(mint)
    return pools, mints


def candidate_id_identity(candidate_id: str | None, *, order: str) -> tuple[str, str] | None:
    if not candidate_id:
        return None
    parts = candidate_id.split(":")
    if len(parts) < 2:
        return None
    if order == "mint_pool":
        return parts[0], parts[1]
    if order == "pool_mint":
        return parts[1], parts[0]
    raise ValueError(order)


def parse_coordination_buyer_proxy(paths: list[Path]) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    for path in paths:
        for row in common.iter_json_objects(path):
            candidate_id = common.str_or_none(row.get("candidate_id"))
            if not candidate_id:
                continue
            sample = row.get("sample_summary") if isinstance(row.get("sample_summary"), dict) else {}
            velocity = common.float_or_none(
                common.find_first_key(row, ("signer_cross_pool_velocity",), depth=0)
            )
            payload = {
                "buyer_sample_count": common.int_or_none(sample.get("successful_buy_txs")),
                "unique_buyer_count": common.int_or_none(sample.get("unique_buyers")),
                "cross_pool_velocity": velocity,
            }
            out[f"candidate:{candidate_id}"] = payload
            identity = candidate_id_identity(candidate_id, order="pool_mint")
            if identity:
                out[f"mint_pool:{identity[0]}:{identity[1]}"] = payload
    return out


def proxy_context_row(
    candidate: dict[str, Any],
    proxy: dict[str, Any] | None,
    *,
    first_n: int,
) -> dict[str, Any]:
    candidate_id = common.str_or_none(candidate.get("candidate_id")) or ""
    sample = common.int_or_none(proxy.get("buyer_sample_count")) if proxy else None
    unique = common.int_or_none(proxy.get("unique_buyer_count")) if proxy else None
    velocity = common.float_or_none(proxy.get("cross_pool_velocity")) if proxy else None
    reasons = ["wallet_history_scan_skipped_large_event_scope"]
    if proxy is None:
        reasons.append("missing_coordination_buyer_proxy")
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "candidate_id": candidate_id,
        "base_mint": candidate.get("base_mint") or candidate.get("mint_id"),
        "pool_id": candidate.get("pool_id"),
        "bq_cutoff_ts_ms": candidate_cutoff(candidate),
        "bq_buyer_sample_count": sample,
        "bq_unique_buyer_count": unique,
        "bq_repeat_buyer_count": None,
        "bq_repeat_buyer_share": None,
        "bq_prior_pool_participation_count_sum": None,
        "bq_prior_pool_participation_count_mean": None,
        "bq_prior_pool_participation_count_max": None,
        "bq_cross_pool_velocity_mean": velocity,
        "bq_cross_pool_velocity_max": velocity,
        "bq_first_n_buyer_count": min(unique, first_n) if unique is not None else None,
        "bq_first_n_repeat_buyer_count": None,
        "bq_first_n_repeat_buyer_share": None,
        "bq_context_status": "proxy_status_only" if proxy else "unknown",
        "bq_context_reasons": reasons,
        "bq_uses_r2_labels": False,
        "bq_uses_future_activity": False,
    }


def candidate_cutoff(candidate: dict[str, Any]) -> int | None:
    return common.int_or_none(
        candidate.get("feature_cutoff_ts_ms")
        or candidate.get("decision_ts_ms")
        or candidate.get("birth_ts_ms")
    )


def tx_matches_candidate(tx: dict[str, Any], candidate: dict[str, Any]) -> bool:
    pool_id = common.str_or_none(candidate.get("pool_id"))
    base_mint = common.str_or_none(candidate.get("base_mint")) or common.str_or_none(candidate.get("mint_id"))
    return bool(
        (pool_id and tx.get("pool_id") == pool_id)
        or (base_mint and tx.get("base_mint") == base_mint)
    )


def build_tx_identity_index(txs: list[dict[str, Any]]) -> dict[tuple[str, str], list[dict[str, Any]]]:
    indexed: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for tx in txs:
        pool_id = common.str_or_none(tx.get("pool_id"))
        base_mint = common.str_or_none(tx.get("base_mint"))
        if pool_id:
            indexed[("pool", pool_id)].append(tx)
        if base_mint:
            indexed[("mint", base_mint)].append(tx)
    return indexed


def current_candidate_txs(
    candidate: dict[str, Any],
    tx_index: dict[tuple[str, str], list[dict[str, Any]]],
) -> list[dict[str, Any]]:
    pool_id = common.str_or_none(candidate.get("pool_id"))
    base_mint = common.str_or_none(candidate.get("base_mint")) or common.str_or_none(candidate.get("mint_id"))
    candidates: list[dict[str, Any]] = []
    seen: set[tuple[int, str, str | None]] = set()
    for key in (("pool", pool_id), ("mint", base_mint)):
        if key[1] is None:
            continue
        for tx in tx_index.get((key[0], key[1]), []):
            marker = (
                int(tx["ts_ms"]),
                str(tx["wallet"]),
                common.str_or_none(tx.get("pool_id")),
            )
            if marker in seen:
                continue
            seen.add(marker)
            candidates.append(tx)
    return sorted(candidates, key=lambda tx: (tx["ts_ms"], tx.get("slot") or 0, tx["wallet"]))


def prior_counts_for_wallet(
    wallet: str,
    cutoff_ms: int,
    current_pool: str | None,
    txs_by_wallet: dict[str, list[dict[str, Any]]],
) -> tuple[int, int, int | None]:
    prior_pools: set[str] = set()
    prior_txs = 0
    first_seen: int | None = None
    for tx in txs_by_wallet.get(wallet, []):
        ts_ms = common.int_or_none(tx.get("ts_ms"))
        if ts_ms is None or ts_ms >= cutoff_ms:
            continue
        pool = common.str_or_none(tx.get("pool_id"))
        if current_pool and pool == current_pool:
            continue
        prior_txs += 1
        if pool:
            prior_pools.add(pool)
        first_seen = ts_ms if first_seen is None else min(first_seen, ts_ms)
    return len(prior_pools), prior_txs, first_seen


def mean(values: list[float]) -> float | None:
    return sum(values) / len(values) if values else None


def build_context_row(
    candidate: dict[str, Any],
    tx_index: dict[tuple[str, str], list[dict[str, Any]]],
    txs_by_wallet: dict[str, list[dict[str, Any]]],
    *,
    first_n: int,
) -> dict[str, Any]:
    candidate_id = common.str_or_none(candidate.get("candidate_id")) or ""
    cutoff_ms = candidate_cutoff(candidate)
    pool_id = common.str_or_none(candidate.get("pool_id"))
    base_mint = common.str_or_none(candidate.get("base_mint")) or common.str_or_none(candidate.get("mint_id"))
    reasons: list[str] = []
    if cutoff_ms is None:
        reasons.append("missing_cutoff_ts_ms")
    current_buys = [
        tx
        for tx in current_candidate_txs(candidate, tx_index)
        if cutoff_ms is not None
        and common.int_or_none(tx.get("ts_ms")) is not None
        and common.int_or_none(tx.get("ts_ms")) <= cutoff_ms
    ]
    current_buys.sort(key=lambda tx: (tx["ts_ms"], tx.get("slot") or 0, tx["wallet"]))
    buyers = []
    seen: set[str] = set()
    for tx in current_buys:
        wallet = common.str_or_none(tx.get("wallet"))
        if wallet and wallet not in seen:
            buyers.append(wallet)
            seen.add(wallet)
    first_buyers = buyers[:first_n]
    prior_pool_counts: list[int] = []
    prior_tx_counts: list[int] = []
    prior_history_by_wallet: dict[str, tuple[int, int, int | None]] = {}
    repeat_buyers = 0
    for wallet in buyers:
        if cutoff_ms is None:
            continue
        prior_pool_count, prior_tx_count, first_seen = prior_counts_for_wallet(
            wallet,
            cutoff_ms,
            pool_id,
            txs_by_wallet,
        )
        prior_history_by_wallet[wallet] = (prior_pool_count, prior_tx_count, first_seen)
        prior_pool_counts.append(prior_pool_count)
        prior_tx_counts.append(prior_tx_count)
        if prior_pool_count > 0:
            repeat_buyers += 1
    first_repeat = 0
    for wallet in first_buyers:
        if cutoff_ms is None:
            continue
        prior_pool_count, _prior_tx_count, _first_seen = prior_counts_for_wallet(
            wallet,
            cutoff_ms,
            pool_id,
            txs_by_wallet,
        )
        if prior_pool_count > 0:
            first_repeat += 1
    velocity_values: list[float] = []
    for wallet in buyers:
        if cutoff_ms is None or wallet not in prior_history_by_wallet:
            continue
        count, _prior_tx_count, first_seen = prior_history_by_wallet[wallet]
        if first_seen is None:
            continue
        hours = max((cutoff_ms - first_seen) / 3_600_000.0, 1.0)
        velocity_values.append(count / hours)
    if not current_buys:
        reasons.append("no_current_pool_buy_evidence_before_cutoff")
    if not buyers:
        reasons.append("no_unique_buyer_evidence_before_cutoff")
    context_status = (
        "unknown"
        if "missing_cutoff_ts_ms" in reasons
        else "unknown_no_buyer_evidence"
        if not buyers
        else "clean"
        if repeat_buyers > 0
        else "no_prior_history_observed"
    )
    return {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": ARTIFACT,
        "candidate_id": candidate_id,
        "base_mint": base_mint,
        "pool_id": pool_id,
        "bq_cutoff_ts_ms": cutoff_ms,
        "bq_buyer_sample_count": len(current_buys),
        "bq_unique_buyer_count": len(buyers),
        "bq_repeat_buyer_count": repeat_buyers,
        "bq_repeat_buyer_share": repeat_buyers / len(buyers) if buyers else None,
        "bq_prior_pool_participation_count_sum": sum(prior_pool_counts) if buyers else None,
        "bq_prior_pool_participation_count_mean": mean([float(v) for v in prior_pool_counts]),
        "bq_prior_pool_participation_count_max": max(prior_pool_counts) if prior_pool_counts else None,
        "bq_cross_pool_velocity_mean": mean(velocity_values),
        "bq_cross_pool_velocity_max": max(velocity_values) if velocity_values else None,
        "bq_first_n_buyer_count": len(first_buyers),
        "bq_first_n_repeat_buyer_count": first_repeat,
        "bq_first_n_repeat_buyer_share": first_repeat / len(first_buyers) if first_buyers else None,
        "bq_context_status": context_status,
        "bq_context_reasons": reasons,
        "bq_uses_r2_labels": False,
        "bq_uses_future_activity": False,
    }


def build_context(args: argparse.Namespace) -> dict[str, Any]:
    root = Path(args.root)
    candidates_path = args.candidate_universe or candidate_universe_path(root, args.scope)
    out = args.output or output_path(root, args.scope)
    manifest_out = args.manifest_output or manifest_path(root, args.scope)
    candidates = list(common.iter_json_objects(candidates_path))
    paths = event_paths(root, args.runtime_scope, args.events_glob)
    total_event_bytes = sum(path.stat().st_size for path in paths if path.exists())
    history_source = "event_wallet_history"
    current_txs: list[dict[str, Any]] = []
    prior_txs: list[dict[str, Any]] = []
    buyer_wallets: set[str] = set()
    if total_event_bytes > args.max_event_bytes_for_wallet_history:
        history_source = "coordination_proxy_status_only"
        proxy_by_candidate = parse_coordination_buyer_proxy(
            coordination_paths(root, args.runtime_scope, args.coordination_glob)
        )
        rows = []
        for candidate in candidates:
            candidate_id = common.str_or_none(candidate.get("candidate_id"))
            if not candidate_id:
                continue
            identity = candidate_id_identity(candidate_id, order="mint_pool")
            proxy = proxy_by_candidate.get(f"candidate:{candidate_id}")
            if proxy is None and identity:
                proxy = proxy_by_candidate.get(f"mint_pool:{identity[0]}:{identity[1]}")
            rows.append(proxy_context_row(candidate, proxy, first_n=args.first_n))
    else:
        wanted_pools, wanted_mints = candidate_identity_sets(candidates)
        current_txs = parse_pool_transactions(
            paths,
            wanted_pools=wanted_pools,
            wanted_mints=wanted_mints,
        )
        buyer_wallets = {str(tx["wallet"]) for tx in current_txs}
        prior_txs = parse_pool_transactions(paths, wanted_wallets=buyer_wallets) if buyer_wallets else []
        txs_by_wallet: dict[str, list[dict[str, Any]]] = defaultdict(list)
        for tx in prior_txs:
            txs_by_wallet[str(tx["wallet"])].append(tx)
        tx_index = build_tx_identity_index(current_txs)
        rows = [
            build_context_row(candidate, tx_index, txs_by_wallet, first_n=args.first_n)
            for candidate in candidates
            if common.str_or_none(candidate.get("candidate_id"))
        ]
    common.write_jsonl(out, rows)
    status_counts = Counter(str(row.get("bq_context_status") or "unknown") for row in rows)
    manifest = {
        "selector_schema_version": common.SCHEMA_VERSION,
        "artifact": MANIFEST_ARTIFACT,
        "status": "PASS" if rows else "NO-GO",
        "fail_reasons": [] if rows else ["no_candidate_rows"],
        "scope": args.scope,
        "runtime_scope": args.runtime_scope,
        "offline_only": True,
        "changes_runtime": False,
        "changes_gatekeeper": False,
        "changes_execution": False,
        "changes_send_path": False,
        "candidate_rows": len(candidates),
        "rows_written": len(rows),
        "event_files": [str(path) for path in paths],
        "event_bytes": total_event_bytes,
        "history_source": history_source,
        "max_event_bytes_for_wallet_history": args.max_event_bytes_for_wallet_history,
        "candidate_pool_transaction_buy_rows": len(current_txs),
        "buyer_history_transaction_rows": len(prior_txs),
        "buyer_wallets_observed": len(buyer_wallets),
        "status_counts": common.counter_dict(status_counts),
        "output": str(out),
    }
    common.write_json(manifest_out, manifest)
    return manifest


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    manifest = build_context(args)
    if args.json:
        print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0 if manifest["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
