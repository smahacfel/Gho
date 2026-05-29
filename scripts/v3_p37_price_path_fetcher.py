#!/usr/bin/env python3
"""
Build P3.7 price-path sample rows.

The tool is additive and offline. It does not modify decision logs, labels, or
runtime state. It can run in schema-only mode for fail-closed dry runs, or use a
bounded Solana RPC collector to build post-decision price path samples.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable
from urllib.parse import urlparse

import requests

import gatekeeper_outcome_labeler as v1


PRICE_PATH_SCHEMA_VERSION = 1
DEFAULT_WINDOW_S = 60.0
IMPLEMENTATION_STATUS = "schema_only_no_collector"
DEFAULT_RPC = "https://solana-mainnet.g.alchemy.com/v2/t3ipHfJnGWRbwo6i21IGu"
DEFAULT_WORKERS = 8
DEFAULT_MAX_RPS = 40.0
DEFAULT_MAX_PAGES = 20
DEFAULT_TIMEOUT_S = 30
DEFAULT_RPC_RETRIES = 4
RETRY_BACKOFF_S = 1.0
SIG_PAGE_LIMIT = 100
BLOCKTIME_RESOLUTION_MS = 1_000
LAMPORTS_PER_SOL = 1_000_000_000
PUMP_VIRTUAL_SOL_OFFSET = 30_000_000_000
PUMP_VIRTUAL_TOKEN_INITIAL_RAW = 1_073_000_191_000_000
PUMP_K = PUMP_VIRTUAL_SOL_OFFSET * PUMP_VIRTUAL_TOKEN_INITIAL_RAW
PRICE_MIN_SOL = 1e-14
PRICE_MAX_SOL = 1e-2

RPC_AUTH_HEADER_ENV = "GHOST_RPC_AUTH_HEADER"
RPC_AUTH_TOKEN_ENV = "GHOST_RPC_AUTH_TOKEN"
LEGACY_PROVIDER_AUTH_HEADER_ENV = "GHOST_SEER_GRPC_AUTH_HEADER"
LEGACY_PROVIDER_AUTH_TOKEN_ENV = "GHOST_SEER_GRPC_X_TOKEN"
DEFAULT_RPC_AUTH_HEADER = "x-api-key"
NLN_RPC_HOST = "rpc.nln.clr3.org"
NLN_RPC_HOST_SUFFIX = ".nln.clr3.org"


def _non_empty_env(name: str) -> str | None:
    value = os.environ.get(name)
    if value is None:
        return None
    value = value.strip()
    return value or None


def _is_nln_rpc_url(rpc_url: str) -> bool:
    host = (urlparse(rpc_url).hostname or "").lower()
    return host == NLN_RPC_HOST or host.endswith(NLN_RPC_HOST_SUFFIX)


def rpc_auth_headers(rpc_url: str) -> dict[str, str]:
    if not _is_nln_rpc_url(rpc_url):
        return {}
    token = _non_empty_env(RPC_AUTH_TOKEN_ENV) or _non_empty_env(LEGACY_PROVIDER_AUTH_TOKEN_ENV)
    if not token:
        return {}
    header = (
        _non_empty_env(RPC_AUTH_HEADER_ENV)
        or _non_empty_env(LEGACY_PROVIDER_AUTH_HEADER_ENV)
        or DEFAULT_RPC_AUTH_HEADER
    )
    return {header: token}

PATH_STATUS_OK = "ok"
PATH_STATUS_PARTIAL = "partial"
PATH_STATUS_UNAVAILABLE = "unavailable"
PATH_STATUS_ENTRY_INVALID = "entry_invalid"
PATH_STATUS_RPC_ERROR = "rpc_error"
PATH_STATUS_SCHEMA_ERROR = "schema_error"

DIAG_ACCOUNT_UPDATE_RELAY_RE = re.compile(
    r"^(?P<timestamp>\S+).*\bDIAG_ACCOUNT_UPDATE_RELAY\b "
    r"base_mint=(?P<base_mint>\S+) bonding_curve=(?P<bonding_curve>\S+) "
    r"slot=(?P<slot>\d+) sol_reserves=(?P<sol_reserves>\d+) "
    r"token_reserves=(?P<token_reserves>\d+) complete=(?P<complete>\d+) "
    r"curve_finality=(?P<curve_finality>\S+)"
)
NS_RE = re.compile(r"(\.\d{6})\d+")


def iter_jsonl(path: Path | None) -> Iterable[dict[str, Any]]:
    if path is None or not path.exists():
        return
    yield from v1.iter_json_objects(path)


def write_jsonl_row(path: Path, row: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def str_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def int_or_none(value: Any) -> int | None:
    return int(value) if isinstance(value, (int, float)) else None


def float_or_none(value: Any) -> float | None:
    return float(value) if isinstance(value, (int, float)) else None


def parse_timestamp_ms(timestamp: str) -> int:
    fixed = NS_RE.sub(r"\1", timestamp)
    if fixed.endswith("Z"):
        fixed = fixed[:-1] + "+00:00"
    parsed = datetime.fromisoformat(fixed)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return int(round(parsed.timestamp() * 1000.0))


def iter_system_log_paths(base_path: Path) -> list[Path]:
    candidates = [
        path
        for path in base_path.parent.glob(f"{base_path.name}*")
        if path.is_file() and path.name.startswith(base_path.name)
    ]
    return sorted(candidates, key=lambda path: path.name)


def diag_spot_price_sol(update: dict[str, Any]) -> float | None:
    sol_reserves = update.get("sol_reserves_lamports")
    token_reserves = update.get("token_reserves_raw")
    if not isinstance(sol_reserves, int) or not isinstance(token_reserves, int):
        return None
    if sol_reserves <= 0 or token_reserves <= 0:
        return None
    price = (sol_reserves / LAMPORTS_PER_SOL) / (token_reserves / 1_000_000.0)
    return price if PRICE_MIN_SOL <= price <= PRICE_MAX_SOL else None


def load_diag_timelines(
    system_log_base: Path | None,
    relevant_mints: set[str],
) -> dict[str, list[dict[str, Any]]]:
    if system_log_base is None or not relevant_mints:
        return {}
    timelines: dict[str, list[dict[str, Any]]] = {}
    for path in iter_system_log_paths(system_log_base):
        with path.open("r", encoding="utf-8", errors="ignore") as fh:
            for line in fh:
                if "DIAG_ACCOUNT_UPDATE_RELAY" not in line:
                    continue
                match = DIAG_ACCOUNT_UPDATE_RELAY_RE.match(line.rstrip())
                if not match:
                    continue
                base_mint = match.group("base_mint")
                if base_mint not in relevant_mints:
                    continue
                try:
                    timestamp_ms = parse_timestamp_ms(match.group("timestamp"))
                except Exception:
                    continue
                timelines.setdefault(base_mint, []).append(
                    {
                        "timestamp_ms": timestamp_ms,
                        "base_mint": base_mint,
                        "bonding_curve": match.group("bonding_curve"),
                        "slot": int(match.group("slot")),
                        "sol_reserves_lamports": int(match.group("sol_reserves")),
                        "token_reserves_raw": int(match.group("token_reserves")),
                        "complete": int(match.group("complete")),
                        "curve_finality": match.group("curve_finality"),
                    }
                )
    for rows in timelines.values():
        rows.sort(key=lambda row: int(row["timestamp_ms"]))
    return timelines


def relevant_base_mints(decisions_path: Path) -> set[str]:
    return {
        mint
        for row in v1.iter_json_objects(decisions_path)
        if (mint := str_or_none(row.get("base_mint"))) is not None
    }


class TokenBucket:
    def __init__(self, rate: float):
        self.rate = max(float(rate), 1.0)
        self.tokens = self.rate
        self.last = time.monotonic()
        self.lock = threading.Lock()

    def acquire(self) -> None:
        while True:
            with self.lock:
                now = time.monotonic()
                elapsed = now - self.last
                self.last = now
                self.tokens = min(self.rate, self.tokens + elapsed * self.rate)
                if self.tokens >= 1.0:
                    self.tokens -= 1.0
                    return
                wait_s = (1.0 - self.tokens) / self.rate
            time.sleep(wait_s)


class RpcClient:
    def __init__(
        self,
        rpc_url: str,
        *,
        max_rps: float,
        timeout_s: int,
        retries: int,
    ):
        self.rpc_url = rpc_url
        self.timeout_s = timeout_s
        self.retries = retries
        self.bucket = TokenBucket(max_rps)
        self.headers = rpc_auth_headers(rpc_url)

    def call(self, method: str, params: list[Any]) -> dict[str, Any]:
        payload = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        for attempt in range(1, self.retries + 2):
            self.bucket.acquire()
            try:
                response = requests.post(
                    self.rpc_url,
                    json=payload,
                    headers=self.headers,
                    timeout=self.timeout_s,
                )
            except requests.exceptions.RequestException as exc:
                if attempt > self.retries:
                    raise RuntimeError(f"rpc_network_error:{exc}") from exc
                time.sleep(RETRY_BACKOFF_S * attempt)
                continue
            if response.status_code == 429:
                retry_after_raw = response.headers.get("Retry-After")
                try:
                    retry_after_s = float(retry_after_raw) if retry_after_raw else None
                except ValueError:
                    retry_after_s = None
                time.sleep(max(RETRY_BACKOFF_S * attempt, retry_after_s or 0.0))
                continue
            if response.status_code >= 400:
                body = " ".join((response.text or "").strip().split())
                if len(body) > 240:
                    body = body[:240] + "..."
                raise RuntimeError(f"rpc_http_{response.status_code}:{body or response.reason}")
            data = response.json()
            if "error" in data:
                err = data.get("error") or {}
                raise RuntimeError(f"rpc_error_{err.get('code')}:{err.get('message')}")
            return data
        raise RuntimeError("rpc_retries_exhausted")


def row_identity(row: dict[str, Any]) -> str:
    for field in ("ab_record_id", "join_key"):
        value = str_or_none(row.get(field))
        if value:
            return f"{field}:{value}"
    pool_id = str_or_none(row.get("pool_id"))
    base_mint = str_or_none(row.get("base_mint"))
    entry_ts = int_or_none(row.get("entry_ts_ms") or row.get("hypothetical_entry_target_ts_ms"))
    if pool_id and base_mint and entry_ts is not None:
        return f"pool_mint_entry:{pool_id}:{base_mint}:{entry_ts}"
    raise ValueError("row is missing stable identity fields")


def processed_identities(*paths: Path | None) -> set[str]:
    done: set[str] = set()
    for path in paths:
        for row in iter_jsonl(path):
            try:
                done.add(row_identity(row))
            except ValueError:
                continue
    return done


def entry_match_confidence(label: dict[str, Any]) -> str:
    if label.get("entry_price_sol") is None:
        return "missing_entry_price"
    if label.get("entry_match_usable") and label.get("entry_match_causal"):
        return "usable_causal_match"
    if label.get("entry_match_usable"):
        return "usable_noncausal_match"
    return "unusable_match"


def base_row(
    decision: dict[str, Any],
    threshold: dict[str, Any] | None,
    *,
    target_pct: float,
    stop_pct: float,
    window_s: float,
) -> dict[str, Any]:
    label = v1.threshold_label(decision, threshold, target_pct, stop_pct)
    entry_price = float_or_none(label.get("entry_price_sol"))
    entry_ts_ms = int_or_none(label.get("entry_ts_ms"))
    label_valid = bool(label.get("label_valid"))
    path_status = PATH_STATUS_UNAVAILABLE if label_valid else PATH_STATUS_ENTRY_INVALID
    unknown_reason = IMPLEMENTATION_STATUS if label_valid else label.get("label_invalid_reason")
    return {
        "price_path_schema_version": PRICE_PATH_SCHEMA_VERSION,
        "ab_record_id": decision.get("ab_record_id"),
        "join_key": decision.get("join_key"),
        "pool_id": decision.get("pool_id"),
        "base_mint": decision.get("base_mint"),
        "entry_ts_ms": entry_ts_ms,
        "entry_price": entry_price,
        "entry_price_source": "threshold_hypothetical_entry" if entry_price is not None else "unavailable",
        "entry_match_confidence": entry_match_confidence(label),
        "path_source": "unavailable",
        "path_status": path_status,
        "samples": [],
        "sample_count": 0,
        "window_s": float(window_s),
        "mfe_pct_10s": None,
        "mae_pct_10s": None,
        "mfe_pct_30s": None,
        "mae_pct_30s": None,
        "mfe_pct_60s": None,
        "mae_pct_60s": None,
        "time_to_mfe_ms": None,
        "time_to_mae_ms": None,
        "drawdown_before_plus40": None,
        "unknown_reason": unknown_reason,
        "collector_status": IMPLEMENTATION_STATUS,
        "threshold_status": label.get("threshold_status"),
        "threshold_verdict": label.get("threshold_verdict"),
        "threshold_window_max_return_pct": label.get("max_executable_return_pct"),
        "threshold_window_min_return_pct": label.get("max_adverse_return_pct"),
        "threshold_summary_is_not_price_path": True,
    }


def compute_price_from_tx(
    tx: dict[str, Any],
    pool_id: str,
    base_mint: str,
) -> tuple[float | None, str, dict[str, Any]]:
    meta = tx.get("meta") or {}
    msg = (tx.get("transaction") or {}).get("message") or {}
    account_keys = msg.get("accountKeys") or []
    pre_balances = meta.get("preBalances") or []
    post_balances = meta.get("postBalances") or []
    pre_token = meta.get("preTokenBalances") or []
    post_token = meta.get("postTokenBalances") or []

    pool_idx: int | None = None
    for idx, account in enumerate(account_keys):
        pubkey = account.get("pubkey") if isinstance(account, dict) else account
        if pubkey == pool_id:
            pool_idx = idx
            break

    real_sol_post = (
        int(post_balances[pool_idx])
        if pool_idx is not None and pool_idx < len(post_balances)
        else None
    )
    real_sol_pre = (
        int(pre_balances[pool_idx])
        if pool_idx is not None and pool_idx < len(pre_balances)
        else None
    )

    def pool_token_raw(token_list: list[Any]) -> int | None:
        for item in token_list:
            if not isinstance(item, dict):
                continue
            if item.get("mint") != base_mint or item.get("owner") != pool_id:
                continue
            amount = (item.get("uiTokenAmount") or {}).get("amount")
            if amount is not None:
                return int(amount)
        return None

    token_post_raw = pool_token_raw(post_token)
    token_pre_raw = pool_token_raw(pre_token)

    if base_mint.endswith("pump") and real_sol_post is not None and real_sol_post >= 0:
        virtual_sol = real_sol_post + PUMP_VIRTUAL_SOL_OFFSET
        virtual_token = PUMP_K / virtual_sol
        price = virtual_sol / (1000.0 * virtual_token)
        if PRICE_MIN_SOL <= price <= PRICE_MAX_SOL:
            return price, "pump_virtual", {
                "real_sol_lamports": real_sol_post,
                "virtual_sol_lamports": virtual_sol,
                "virtual_token_raw": int(virtual_token),
            }

    if (
        real_sol_pre is not None
        and real_sol_post is not None
        and token_pre_raw is not None
        and token_post_raw is not None
    ):
        delta_sol = abs(real_sol_post - real_sol_pre)
        delta_token_raw = abs(token_post_raw - token_pre_raw)
        if delta_sol > 0 and delta_token_raw > 0:
            price = (delta_sol / LAMPORTS_PER_SOL) / (delta_token_raw / 1_000_000.0)
            if PRICE_MIN_SOL <= price <= PRICE_MAX_SOL:
                return price, "trade_delta", {
                    "delta_sol_lamports": delta_sol,
                    "delta_token_raw": delta_token_raw,
                }

    if real_sol_post is not None and token_post_raw is not None and token_post_raw > 0:
        price = (real_sol_post / LAMPORTS_PER_SOL) / (token_post_raw / 1_000_000.0)
        if PRICE_MIN_SOL <= price <= PRICE_MAX_SOL:
            return price, "reserve_ratio", {
                "pool_sol_lamports": real_sol_post,
                "pool_token_raw": token_post_raw,
            }

    return None, "price_failed", {"pool_idx_found": pool_idx is not None}


def collect_signatures_in_window(
    client: RpcClient,
    pool_id: str,
    start_ts_ms: int,
    end_ts_ms: int,
    *,
    max_pages: int,
) -> tuple[list[dict[str, Any]], int]:
    collected: list[dict[str, Any]] = []
    before: str | None = None
    pages = 0
    while pages < max_pages:
        params: list[Any] = [pool_id, {"limit": SIG_PAGE_LIMIT}]
        if before:
            params[1]["before"] = before
        result = client.call("getSignaturesForAddress", params).get("result") or []
        pages += 1
        if not result:
            break
        past_window = False
        for entry in result:
            if not isinstance(entry, dict):
                continue
            signature = str_or_none(entry.get("signature"))
            block_time = int_or_none(entry.get("blockTime"))
            slot = int_or_none(entry.get("slot"))
            if signature is None or block_time is None:
                continue
            ts_ms = block_time * 1000
            if ts_ms > end_ts_ms:
                continue
            if ts_ms + BLOCKTIME_RESOLUTION_MS <= start_ts_ms:
                past_window = True
                break
            if entry.get("err") is not None:
                continue
            collected.append({"signature": signature, "blockTime": block_time, "slot": slot})
        if past_window or len(result) < SIG_PAGE_LIMIT:
            break
        before = str_or_none(result[-1].get("signature")) if isinstance(result[-1], dict) else None
        if before is None:
            break
    collected.reverse()
    return collected, pages


def price_sample_for_signature(
    client: RpcClient,
    *,
    signature: str,
    block_time: int,
    slot: int | None,
    pool_id: str,
    base_mint: str,
    entry_ts_ms: int,
    entry_price: float,
) -> tuple[dict[str, Any] | None, str | None]:
    tx_data = client.call(
        "getTransaction",
        [signature, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}],
    )
    tx = tx_data.get("result")
    if not isinstance(tx, dict):
        return None, "tx_not_found"
    price, method, extra = compute_price_from_tx(tx, pool_id, base_mint)
    if price is None or price <= 0.0:
        return None, str(extra.get("status") or method or "price_failed")
    ts_ms = block_time * 1000
    if ts_ms < entry_ts_ms:
        return None, "pre_entry_blocktime_skipped"
    return {
        "ts_ms": ts_ms,
        "offset_ms": ts_ms - entry_ts_ms,
        "price_sol": price,
        "return_pct": ((price / entry_price) - 1.0) * 100.0,
        "source": f"rpc_tx:{method}",
        "signature": signature,
        "slot": slot,
    }, None


def summarize_samples(
    samples: list[dict[str, Any]],
    *,
    entry_ts_ms: int,
    target_pct: float,
) -> dict[str, Any]:
    def returns_until(ms: int) -> list[float]:
        end_ts = entry_ts_ms + ms
        return [float(row["return_pct"]) for row in samples if int(row["ts_ms"]) <= end_ts]

    def max_until(ms: int) -> float | None:
        values = returns_until(ms)
        return max(values) if values else None

    def min_until(ms: int) -> float | None:
        values = returns_until(ms)
        return min(values) if values else None

    if not samples:
        return {
            "mfe_pct_10s": None,
            "mae_pct_10s": None,
            "mfe_pct_30s": None,
            "mae_pct_30s": None,
            "mfe_pct_60s": None,
            "mae_pct_60s": None,
            "time_to_mfe_ms": None,
            "time_to_mae_ms": None,
            "drawdown_before_plus40": None,
        }

    max_sample = max(samples, key=lambda row: float(row["return_pct"]))
    min_sample = min(samples, key=lambda row: float(row["return_pct"]))
    plus40_index = next(
        (idx for idx, row in enumerate(samples) if float(row["return_pct"]) >= target_pct),
        None,
    )
    drawdown_before_plus40 = (
        min(float(row["return_pct"]) for row in samples[: plus40_index + 1])
        if plus40_index is not None
        else None
    )
    return {
        "mfe_pct_10s": max_until(10_000),
        "mae_pct_10s": min_until(10_000),
        "mfe_pct_30s": max_until(30_000),
        "mae_pct_30s": min_until(30_000),
        "mfe_pct_60s": max_until(60_000),
        "mae_pct_60s": min_until(60_000),
        "time_to_mfe_ms": int(max_sample["ts_ms"]) - entry_ts_ms,
        "time_to_mae_ms": int(min_sample["ts_ms"]) - entry_ts_ms,
        "drawdown_before_plus40": drawdown_before_plus40,
    }


def diag_samples_for_row(row: dict[str, Any], diag_timelines: dict[str, list[dict[str, Any]]]) -> list[dict[str, Any]]:
    base_mint = str_or_none(row.get("base_mint"))
    entry_ts_ms = int_or_none(row.get("entry_ts_ms"))
    entry_price = float_or_none(row.get("entry_price"))
    window_s = float_or_none(row.get("window_s")) or DEFAULT_WINDOW_S
    if base_mint is None or entry_ts_ms is None or entry_price is None or entry_price <= 0.0:
        return []
    end_ts_ms = entry_ts_ms + int(round(window_s * 1000.0))
    samples: list[dict[str, Any]] = []
    for update in diag_timelines.get(base_mint, []):
        ts_ms = int(update["timestamp_ms"])
        if ts_ms < entry_ts_ms:
            continue
        if ts_ms > end_ts_ms:
            break
        price = diag_spot_price_sol(update)
        if price is None:
            continue
        samples.append(
            {
                "ts_ms": ts_ms,
                "offset_ms": ts_ms - entry_ts_ms,
                "price_sol": price,
                "return_pct": ((price / entry_price) - 1.0) * 100.0,
                "source": "diag_update",
                "signature": None,
                "slot": update.get("slot"),
            }
        )
    return samples


class RpcPathCollector:
    def __init__(
        self,
        *,
        rpc_url: str,
        max_rps: float,
        timeout_s: int,
        retries: int,
        max_pages: int,
        target_pct: float,
        diag_timelines: dict[str, list[dict[str, Any]]] | None = None,
    ):
        self.client = RpcClient(rpc_url, max_rps=max_rps, timeout_s=timeout_s, retries=retries)
        self.max_pages = max_pages
        self.target_pct = target_pct
        self.diag_timelines = diag_timelines or {}

    def collect(self, row: dict[str, Any]) -> dict[str, Any]:
        if row["path_status"] == PATH_STATUS_ENTRY_INVALID:
            return row
        pool_id = str_or_none(row.get("pool_id"))
        base_mint = str_or_none(row.get("base_mint"))
        entry_ts_ms = int_or_none(row.get("entry_ts_ms"))
        entry_price = float_or_none(row.get("entry_price"))
        window_s = float_or_none(row.get("window_s")) or DEFAULT_WINDOW_S
        if pool_id is None or base_mint is None or entry_ts_ms is None or entry_price is None or entry_price <= 0.0:
            row.update({
                "path_status": PATH_STATUS_SCHEMA_ERROR,
                "path_source": "unavailable",
                "collector_status": "schema_error",
                "unknown_reason": "missing_pool_or_entry_fields",
            })
            return row
        diag_samples = diag_samples_for_row(row, self.diag_timelines)
        if diag_samples:
            row.update({
                "path_source": "diag_account_update",
                "path_status": PATH_STATUS_OK,
                "samples": diag_samples,
                "sample_count": len(diag_samples),
                "collector_status": "diag_collected",
                "unknown_reason": None,
            })
            row.update(summarize_samples(diag_samples, entry_ts_ms=entry_ts_ms, target_pct=self.target_pct))
            return row
        try:
            end_ts_ms = entry_ts_ms + int(round(window_s * 1000.0))
            signatures, pages = collect_signatures_in_window(
                self.client,
                pool_id,
                entry_ts_ms,
                end_ts_ms,
                max_pages=self.max_pages,
            )
            samples: list[dict[str, Any]] = []
            failures: list[str] = []
            for entry in signatures:
                try:
                    sample, failure = price_sample_for_signature(
                        self.client,
                        signature=str(entry["signature"]),
                        block_time=int(entry["blockTime"]),
                        slot=int_or_none(entry.get("slot")),
                        pool_id=pool_id,
                        base_mint=base_mint,
                        entry_ts_ms=entry_ts_ms,
                        entry_price=entry_price,
                    )
                except Exception as exc:  # fail closed per signature
                    sample = None
                    failure = f"tx_rpc_error:{exc}"
                if sample is not None:
                    samples.append(sample)
                elif failure:
                    failures.append(failure)
            samples.sort(key=lambda item: (int(item["ts_ms"]), str(item.get("signature") or "")))
            row.update({
                "path_source": "rpc_pool_signatures",
                "samples": samples,
                "sample_count": len(samples),
                "rpc_pages_fetched": pages,
                "rpc_signature_count": len(signatures),
                "rpc_price_failure_count": len(failures),
                "rpc_price_failure_reasons": sorted(set(failures))[:16],
                "collector_status": "rpc_collected",
                "unknown_reason": None,
            })
            row.update(summarize_samples(samples, entry_ts_ms=entry_ts_ms, target_pct=self.target_pct))
            if samples and failures:
                row["path_status"] = PATH_STATUS_PARTIAL
                row["unknown_reason"] = "partial_rpc_price_failures"
            elif samples:
                row["path_status"] = PATH_STATUS_OK
            elif failures:
                row["path_status"] = PATH_STATUS_RPC_ERROR
                row["unknown_reason"] = "all_rpc_price_fetches_failed"
            else:
                row["path_status"] = PATH_STATUS_UNAVAILABLE
                row["unknown_reason"] = "no_post_entry_signatures"
            return row
        except Exception as exc:
            row.update({
                "path_status": PATH_STATUS_RPC_ERROR,
                "path_source": "rpc_pool_signatures",
                "samples": [],
                "sample_count": 0,
                "collector_status": "rpc_error",
                "unknown_reason": str(exc),
            })
            return row


def load_threshold_index(threshold_path: Path) -> dict[str, dict[str, Any]]:
    return v1.index_rows(v1.iter_json_objects(threshold_path))


def prepare_candidate_rows(
    decisions_path: Path,
    threshold_hits_path: Path,
    *,
    target_pct: float,
    stop_pct: float,
    window_s: float,
    skip_identities: set[str] | None = None,
    limit: int | None = None,
) -> tuple[list[dict[str, Any]], dict[str, int]]:
    decisions = list(v1.iter_json_objects(decisions_path))
    if limit is not None:
        decisions = decisions[: max(0, limit)]
    threshold_by_key = load_threshold_index(threshold_hits_path)
    skip = skip_identities or set()
    rows: list[dict[str, Any]] = []
    counters = {
        "decisions": len(decisions),
        "written_candidates": 0,
        "skipped_existing": 0,
        "threshold_matched": 0,
        PATH_STATUS_OK: 0,
        PATH_STATUS_PARTIAL: 0,
        PATH_STATUS_RPC_ERROR: 0,
        PATH_STATUS_UNAVAILABLE: 0,
        PATH_STATUS_ENTRY_INVALID: 0,
        PATH_STATUS_SCHEMA_ERROR: 0,
    }
    for decision in decisions:
        threshold = v1.best_match(decision, threshold_by_key)
        if threshold is not None:
            counters["threshold_matched"] += 1
        row = base_row(
            decision,
            threshold,
            target_pct=target_pct,
            stop_pct=stop_pct,
            window_s=window_s,
        )
        identity = row_identity(row)
        if identity in skip:
            counters["skipped_existing"] += 1
            continue
        rows.append(row)
        counters["written_candidates"] += 1
    return rows, counters


def update_status_counter(counters: dict[str, int], row: dict[str, Any]) -> None:
    status = str(row.get("path_status") or "unknown")
    counters[status] = counters.get(status, 0) + 1


def build_rows(
    decisions_path: Path,
    threshold_hits_path: Path,
    *,
    target_pct: float,
    stop_pct: float,
    window_s: float,
    skip_identities: set[str] | None = None,
    collector: RpcPathCollector | None = None,
    workers: int = 1,
    limit: int | None = None,
) -> tuple[list[dict[str, Any]], dict[str, int]]:
    rows, counters = prepare_candidate_rows(
        decisions_path,
        threshold_hits_path,
        target_pct=target_pct,
        stop_pct=stop_pct,
        window_s=window_s,
        skip_identities=skip_identities,
        limit=limit,
    )
    if collector is not None and rows:
        if workers > 1:
            with ThreadPoolExecutor(max_workers=workers) as executor:
                rows = list(executor.map(collector.collect, rows))
        else:
            rows = [collector.collect(row) for row in rows]
    for row in rows:
        update_status_counter(counters, row)
    return rows, counters


def write_outputs(
    rows: list[dict[str, Any]],
    *,
    output_path: Path,
    checkpoint_path: Path | None,
) -> None:
    for row in rows:
        write_jsonl_row(output_path, row)
        if checkpoint_path is not None:
            write_jsonl_row(checkpoint_path, row)


def collect_and_write_outputs(
    decisions_path: Path,
    threshold_hits_path: Path,
    *,
    target_pct: float,
    stop_pct: float,
    window_s: float,
    skip_identities: set[str],
    collector: RpcPathCollector | None,
    workers: int,
    limit: int | None,
    output_path: Path,
    checkpoint_path: Path | None,
) -> dict[str, int]:
    rows, counters = prepare_candidate_rows(
        decisions_path,
        threshold_hits_path,
        target_pct=target_pct,
        stop_pct=stop_pct,
        window_s=window_s,
        skip_identities=skip_identities,
        limit=limit,
    )

    def persist(row: dict[str, Any]) -> None:
        update_status_counter(counters, row)
        write_jsonl_row(output_path, row)
        if checkpoint_path is not None:
            write_jsonl_row(checkpoint_path, row)

    if collector is None:
        for row in rows:
            persist(row)
    elif workers > 1:
        with ThreadPoolExecutor(max_workers=workers) as executor:
            future_to_row = {executor.submit(collector.collect, row): row for row in rows}
            for future in as_completed(future_to_row):
                try:
                    row = future.result()
                except Exception as exc:  # defensive fail-closed guard
                    row = future_to_row[future]
                    row.update(
                        {
                            "path_status": PATH_STATUS_RPC_ERROR,
                            "path_source": "rpc_pool_signatures",
                            "samples": [],
                            "sample_count": 0,
                            "collector_status": "worker_error",
                            "unknown_reason": str(exc),
                        }
                    )
                persist(row)
    else:
        for row in rows:
            persist(collector.collect(row))
    return counters


def run(args: argparse.Namespace) -> dict[str, Any]:
    skip = processed_identities(args.output, args.checkpoint) if args.resume else set()
    collector = None
    if not args.schema_only:
        diag_timelines = load_diag_timelines(
            args.system_log_base,
            relevant_base_mints(args.decisions),
        )
        collector = RpcPathCollector(
            rpc_url=args.rpc,
            max_rps=args.max_rps,
            timeout_s=args.timeout_s,
            retries=args.rpc_retries,
            max_pages=args.max_pages,
            target_pct=args.target_pct,
            diag_timelines=diag_timelines,
        )
    counters = collect_and_write_outputs(
        args.decisions,
        args.threshold_hits,
        target_pct=args.target_pct,
        stop_pct=args.stop_pct,
        window_s=args.window_s,
        skip_identities=skip,
        collector=collector,
        workers=max(1, args.workers),
        limit=args.limit,
        output_path=args.output,
        checkpoint_path=args.checkpoint,
    )
    return {
        "status": "ok",
        "collector_status": IMPLEMENTATION_STATUS if args.schema_only else "rpc_collector_enabled",
        "scope": {
            "schema_only": bool(args.schema_only),
            "rpc_collection_enabled": not bool(args.schema_only),
            "no_p2": True,
            "no_live": True,
            "no_threshold_tuning": True,
            "does_not_use_decision_vectors_as_outcome_truth": True,
            "default_rpc_provider": "nln_clr3",
            "diag_account_update_fallback_enabled": args.system_log_base is not None,
        },
        "paths": {
            "decisions": str(args.decisions),
            "threshold_hits": str(args.threshold_hits),
            "output": str(args.output),
            "checkpoint": str(args.checkpoint) if args.checkpoint else None,
        },
        "counts": counters,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--decisions", required=True, type=Path)
    parser.add_argument("--threshold-hits", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--checkpoint", type=Path)
    parser.add_argument("--rpc", default=DEFAULT_RPC, help="Solana JSON-RPC endpoint; defaults to archive-capable Alchemy endpoint")
    parser.add_argument("--workers", type=int, default=DEFAULT_WORKERS)
    parser.add_argument("--max-rps", type=float, default=DEFAULT_MAX_RPS)
    parser.add_argument("--max-pages", type=int, default=DEFAULT_MAX_PAGES)
    parser.add_argument("--timeout-s", type=int, default=DEFAULT_TIMEOUT_S)
    parser.add_argument("--rpc-retries", type=int, default=DEFAULT_RPC_RETRIES)
    parser.add_argument("--system-log-base", type=Path, help="optional system.log base for DIAG_ACCOUNT_UPDATE_RELAY fallback")
    parser.add_argument("--window-s", type=float, default=DEFAULT_WINDOW_S)
    parser.add_argument("--target-pct", type=float, default=v1.DEFAULT_TARGET_PCT)
    parser.add_argument("--stop-pct", type=float, default=v1.DEFAULT_STOP_PCT)
    parser.add_argument("--schema-only", action="store_true", help="emit fail-closed schema rows without RPC collection")
    parser.add_argument("--limit", type=int, help="process only the first N decision rows; useful for controlled smoke tests")
    parser.add_argument("--resume", action="store_true", help="skip identities already present in output/checkpoint")
    parser.add_argument("--json", action="store_true", help="print compact JSON summary")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    summary = run(args)
    if args.json:
        print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
