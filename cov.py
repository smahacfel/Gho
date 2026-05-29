#!/usr/bin/env python3
"""
coverage_scanner.py — PumpFun Pool Historical Coverage Scanner
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Dla każdego rekordu z gatekeeper_v2_buys.jsonl pobiera przez RPC
trade history dla pool dokładnie w kanonicznym oknie decyzji Gatekeepera:

    • start: `observation_start_ts_ms` (fallback: `first_seen_ts_ms`)
    • end:   `observation_end_ts_ms`
              (fallback: `curve_t0_event_ts_ms + curve_wait_elapsed_ms`,
               potem wcześniejsze z `timestamp`, a na końcu start + observation window)

Następnie:

    • liczy on-chain non-dust trade tx w oknie: total / confirmed / failed
    • próbuje odczytać account data (curve / pool state)
    • oblicza coverage względem `total_tx_evaluated`
        (`dust_filtered_count` NIE należy do głównego numeratora coverage)
    • zapisuje wzbogacone rekordy do Ghost/logs/decisions.jsonl/coverages/
    • exact-window logi systemowe są używane wyłącznie pomocniczo:
        - dostają mały margines na skew zegara / flush logów,
        - ale finalny count nadal przechodzi przez on-chain tx fetch
          i filtr blockTime w oknie rekordu.

Semantyka coverage:
    • coverage_ratio_raw = total_tx_evaluated / rpc_total_tx
    • coverage_ratio     = min(1.0, coverage_ratio_raw)
    • `rpc_total_tx` ma odtwarzać ten sam zbiór, który Gatekeeper liczy do
      `total_tx_evaluated`:
        - liczymy trade-signatures (nie trade events)
        - wliczamy zarówno confirmed jak i failed
        - odrzucamy dust poniżej gatekeeperowego `min_sol_threshold`
    • coverage_ratio_all_tx jest aliasem głównej metryki total-vs-total,
      utrzymanym dla kompatybilności artefaktów
    • coverage_ratio_confirmed_only liczymy wyłącznie wtedy, gdy rekord wejściowy
        ma jawny split observed success/failed; wtedy porównujemy confirmed-vs-confirmed
        i failed-vs-failed na poziomie transakcji.

Przepustowość zależy od planu RPC oraz limitów specyficznych dla Solany.
Dostawca może pokazywać globalny limit planu, ale dla Solana Mainnet archival
`getTransaction` bardzo łatwo wpada w 429 przy burstach. Domyślny profil tego
skryptu zostawia konserwatywny zapas poniżej projektowego limitu operacyjnego.

Usage:
    python coverage_scanner.py [INPUT] [options]

    INPUT                   ścieżka do pliku JSONL
                            (domyślnie: <katalog_skryptu>/gatekeeper_v2_buys.jsonl)
    --rpc URL               Solana JSON-RPC endpoint
    --rps FLOAT             limit wszystkich żądań/s
    --concurrency INT       równoległe fetchery
    --tx-rps FLOAT          limit globalny tylko dla getTransaction
    --tx-fetch-concurrency INT  równoległe getTransaction per pool
    --tx-max-inflight INT   maksymalna liczba otwartych getTransaction globalnie
    --max-pages INT         max stron getSignaturesForAddress (domyślnie: 10)
    --include-account-info  pobieraj także getAccountInfo dla poola (wolniej)
    --target-ts-ms INT      wymusza konkretny target timestamp w ms
    --since-ms INT          filtruje input do rekordów z kohorty >= since_ms
    --run-id TEXT           opcjonalnie filtruje input do konkretnego run_id
    --output-dir PATH       katalog wyjściowy
    --log-level LEVEL       DEBUG / INFO / WARNING
"""

from __future__ import annotations

import argparse
import asyncio
from bisect import bisect_left, bisect_right
from collections import Counter
import gzip
import importlib
import json
import logging
import os
import random
import re
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlparse

try:
    aiohttp = importlib.import_module("aiohttp")
except ModuleNotFoundError:  # pragma: no cover - handled at runtime
    aiohttp = None


SCRIPT_DIR = Path(__file__).resolve().parent


def _find_repo_root(start_dir: Path) -> Path:
    for candidate in (start_dir, *start_dir.parents):
        if (candidate / "tools" / "fetch_pool_trade_counts.py").exists():
            return candidate
    return start_dir


REPO_ROOT = _find_repo_root(SCRIPT_DIR)
TOOLS_DIR = REPO_ROOT / "tools"
for import_dir in (TOOLS_DIR, SCRIPT_DIR):
    if import_dir.exists() and str(import_dir) not in sys.path:
        sys.path.insert(0, str(import_dir))

from fetch_pool_trade_counts import classify_trade


# ══════════════════════════════════════════════════════════════════════════════
# CONSTANTS
# ══════════════════════════════════════════════════════════════════════════════

DEFAULT_RPC_URL  = "https://solana-mainnet.g.alchemy.com/v2/t3ipHfJnGWRbwo6i21IGu"
DEFAULT_INPUT_PATH = str(SCRIPT_DIR / "gatekeeper_v2_buys.jsonl")
DEFAULT_RPS      = 24.0
DEFAULT_CONCUR   = 8
DEFAULT_TX_FETCH_CONCURRENCY = 4
DEFAULT_TX_RPS   = 12.0
DEFAULT_TX_INFLIGHT = 4
DEFAULT_INCLUDE_ACCOUNT_INFO = False
DEFAULT_MAXPAGES = 10          # max stron per pool (1000 sig/strona ⇒ 10 000 tx)
SIG_PAGE_LIMIT   = 1000        # max sygnatur na żądanie RPC
OUTPUT_BASE      = SCRIPT_DIR / "coverages"
DEFAULT_TX_CACHE_DIR = OUTPUT_BASE / ".tx_cache"
DEFAULT_WINDOW_MS = 10_000
EXACT_WINDOW_LOG_START_SKEW_MS = 100
EXACT_WINDOW_LOG_END_SKEW_MS = 100
SOLANA_MAINNET_PROVIDER_GUIDELINE_RPS = 50.0
MAX_SNAPSHOT_RETRIES = 4
SNAPSHOT_RETRY_BACKOFF_BASE_S = 0.5

RPC_AUTH_HEADER_ENV = "GHOST_RPC_AUTH_HEADER"
RPC_AUTH_TOKEN_ENV = "GHOST_RPC_AUTH_TOKEN"
LEGACY_PROVIDER_AUTH_HEADER_ENV = "GHOST_SEER_GRPC_AUTH_HEADER"
LEGACY_PROVIDER_AUTH_TOKEN_ENV = "GHOST_SEER_GRPC_X_TOKEN"
DEFAULT_RPC_AUTH_HEADER = "x-api-key"
NLN_RPC_HOST = "rpc.nln.clr3.org"
NLN_RPC_HOST_SUFFIX = ".nln.clr3.org"
ALCHEMY_RPC_HOST = "solana-mainnet.g.alchemy.com"


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
SNAPSHOT_RETRY_JITTER_S = 0.25
LAMPORTS_PER_SOL = 1_000_000_000
WSOL_MINT = "So11111111111111111111111111111111111111112"
PUMP_FUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMP_SWAP_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
DISC_BUY = bytes([0x66, 0x06, 0x3D, 0x12, 0x01, 0xDA, 0xEB, 0xEA])
DISC_SELL = bytes([0x33, 0xE6, 0x85, 0xA4, 0x01, 0x7F, 0x83, 0xAD])
DISC_EVENT_TRADE = bytes([0xBD, 0xDB, 0x7F, 0xD3, 0x4E, 0xE6, 0x61, 0xEE])
DISC_SWAP_OUTER_WRAPPER = bytes([0xE4, 0x45, 0xA5, 0x2E, 0x51, 0xCB, 0x9A, 0x1D])
DISC_SWAP_EVENT_BUY = bytes([0x67, 0xF4, 0x52, 0x1F, 0x2C, 0xF5, 0x77, 0x77])
DISC_SWAP_EVENT_SELL = bytes([0x3E, 0x2F, 0x37, 0x0A, 0xA5, 0x03, 0xDC, 0x2A])
DISC_SWAP_BUY_EXACT_QUOTE_IN = bytes([0xC6, 0x2E, 0x15, 0x52, 0xB4, 0xD9, 0xE8, 0x70])
DISC_PUMP_BUY_ROUTED = bytes([0x38, 0xFC, 0x74, 0x08, 0x9E, 0xDF, 0xCD, 0x5F])
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BASE58_INDEX = {ch: idx for idx, ch in enumerate(BASE58_ALPHABET)}
TOP_LEVEL_TRADE_DISCS = {
    DISC_BUY,
    DISC_SELL,
    DISC_SWAP_BUY_EXACT_QUOTE_IN,
    DISC_PUMP_BUY_ROUTED,
}
EVENT_TRADE_DISCS = {
    DISC_EVENT_TRADE,
    DISC_SWAP_EVENT_BUY,
    DISC_SWAP_EVENT_SELL,
}
SYSTEM_LOG_EMIT_RE = re.compile(
    r"^(?P<ts>\S+)\s+INFO\s+.*?Emitting PoolTransaction(?:\s*:)?\s+"
    r".*?\bsig=(?P<sig>\S+)\s+pool=(?P<pool>\S+)\b"
)
SYSTEM_LOG_GATEKEEPER_CONFIG_RE = re.compile(
    r"^(?P<ts>\S+)\s+INFO\s+.*?\bmin_sol_threshold=(?P<threshold>[0-9eE+.\-]+)\b"
)

# Known major tokens that should NEVER be treated as the pool base mint.
BLACKLIST_MINTS = {
    "So11111111111111111111111111111111111111112",   # WSOL
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",  # USDC
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",  # USDT
    "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",   # mSOL
    "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj",  # stSOL
    "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",  # BONK
    "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN",   # JUP
    "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs",  # ETH (Wormhole)
    "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1",   # bSOL
    "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn",  # jitoSOL
}

UNKNOWN_SENTINELS = {
    "unknown",
    "unk",
    "n/a",
    "na",
    "none",
    "null",
    "",
}


# ══════════════════════════════════════════════════════════════════════════════
# LOGGING
# ══════════════════════════════════════════════════════════════════════════════

logging.basicConfig(
    stream=sys.stderr,
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-8s  %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("coverage")
_RETRY_SENTINEL = object()
_SYSTEM_LOG_INDEX_CACHE: dict[Path, dict[str, list[tuple[int, str]]]] = {}
_SYSTEM_LOG_GATEKEEPER_CONFIG_CACHE: dict[Path, list[tuple[int, float]]] = {}
_REPO_CONFIG_MIN_SOL_THRESHOLD: float | None = None
_INPUT_BASE_DIR: Path | None = None
_EXTRA_SYSTEM_LOG_DIRS: list[Path] = []
_EXPLICIT_GHOST_BRAIN_CONFIG_PATH: Path | None = None


def _retry_backoff_s(attempt: int) -> float:
    backoff = SNAPSHOT_RETRY_BACKOFF_BASE_S * (2 ** (attempt - 1))
    jitter = random.random() * SNAPSHOT_RETRY_JITTER_S
    return backoff + jitter


def _parse_retry_delay_seconds(value: object) -> float | None:
    if value is None:
        return None
    if isinstance(value, (int, float)):
        return max(0.0, float(value))
    if not isinstance(value, str):
        return None

    candidate = value.strip().lower()
    try:
        if candidate.endswith("ms"):
            return max(0.0, float(candidate[:-2]) / 1000.0)
        if candidate.endswith("s"):
            return max(0.0, float(candidate[:-1]))
        return max(0.0, float(candidate))
    except ValueError:
        return None


def _parse_iso8601_ts_ms(value: str) -> int | None:
    try:
        normalized = value.replace("Z", "+00:00")
        dt = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1000)


def _unique_existing_dirs(paths: list[Path | None]) -> list[Path]:
    result: list[Path] = []
    seen: set[Path] = set()
    for raw_path in paths:
        if raw_path is None:
            continue
        path = raw_path.resolve()
        if not path.exists() or not path.is_dir() or path in seen:
            continue
        seen.add(path)
        result.append(path)
    return result


def _candidate_system_log_roots() -> list[Path]:
    return _unique_existing_dirs(
        [
            _INPUT_BASE_DIR,
            SCRIPT_DIR,
            Path.cwd(),
            REPO_ROOT,
            *_EXTRA_SYSTEM_LOG_DIRS,
        ]
    )


def _candidate_system_log_dirs() -> list[Path]:
    candidates: list[Path | None] = []
    for root in _candidate_system_log_roots():
        candidates.extend(
            [
                root,
                root / "logs",
                root / "logs" / "rollout" / "paper-burnin",
            ]
        )
    return _unique_existing_dirs(candidates)


def _iter_system_log_file_candidates(log_dir: Path, date_str: str) -> list[Path]:
    candidates: list[Path] = []
    seen: set[Path] = set()
    for pattern in (f"system.log.{date_str}*", "system.log"):
        for path in sorted(log_dir.glob(pattern)):
            if path in seen or not path.is_file():
                continue
            seen.add(path)
            candidates.append(path)
    return candidates


def _candidate_ghost_brain_config_paths() -> list[Path]:
    candidates: list[Path | None] = [
        _EXPLICIT_GHOST_BRAIN_CONFIG_PATH,
    ]
    for root in _candidate_system_log_roots():
        candidates.extend(
            [
                root / "ghost_brain_config.toml",
                root / "ghost-brain" / "ghost_brain_config.toml",
            ]
        )

    result: list[Path] = []
    seen: set[Path] = set()
    for raw_path in candidates:
        if raw_path is None:
            continue
        path = raw_path.resolve()
        if not path.exists() or not path.is_file() or path in seen:
            continue
        seen.add(path)
        result.append(path)
    return result


def _format_paths_for_log(paths: list[Path]) -> str:
    if not paths:
        return "[none]"
    return ", ".join(str(path) for path in paths)


def _collect_run_system_log_candidates(records: list[dict]) -> list[Path]:
    candidates: list[Path] = []
    seen: set[Path] = set()
    for record in records:
        try:
            window_spec = resolve_window_spec(record)
        except Exception:
            continue
        for path in _system_log_path_candidates(
            record,
            window_spec.start_ts_ms,
            window_spec.end_ts_ms,
        ):
            if path in seen:
                continue
            seen.add(path)
            candidates.append(path)
    return candidates


def _system_log_path_candidates(record: dict, start_ts_ms: int, end_ts_ms: int) -> list[Path]:
    candidate_dates: set[str] = set()

    for ts_ms in (start_ts_ms, end_ts_ms):
        if ts_ms > 0:
            candidate_dates.add(
                datetime.fromtimestamp(ts_ms / 1000.0, tz=timezone.utc).strftime("%Y-%m-%d")
            )

    record_ts_ms = _parse_record_timestamp_ms(record)
    if record_ts_ms > 0:
        candidate_dates.add(
            datetime.fromtimestamp(record_ts_ms / 1000.0, tz=timezone.utc).strftime("%Y-%m-%d")
        )

    candidates: list[Path] = []
    seen: set[Path] = set()

    for date_str in sorted(candidate_dates):
        for log_dir in _candidate_system_log_dirs():
            for path in _iter_system_log_file_candidates(log_dir, date_str):
                if path not in seen:
                    candidates.append(path)
                    seen.add(path)

            archive_dir = log_dir / "archive"
            if archive_dir.exists() and archive_dir.is_dir():
                for nested_archive_dir in sorted(path for path in archive_dir.iterdir() if path.is_dir()):
                    for path in _iter_system_log_file_candidates(nested_archive_dir, date_str):
                        if path not in seen:
                            candidates.append(path)
                            seen.add(path)

    return candidates


def _load_system_log_index(log_path: Path) -> dict[str, list[tuple[int, str]]]:
    cached = _SYSTEM_LOG_INDEX_CACHE.get(log_path)
    if cached is not None:
        return cached

    pool_events: dict[str, list[tuple[int, str]]] = {}
    if not log_path.exists():
        _SYSTEM_LOG_INDEX_CACHE[log_path] = pool_events
        return pool_events

    with log_path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            match = SYSTEM_LOG_EMIT_RE.match(line)
            if match is None:
                continue

            ts_ms = _parse_iso8601_ts_ms(match.group("ts"))
            if ts_ms is None:
                continue

            pool_id = match.group("pool")
            signature = match.group("sig")
            pool_events.setdefault(pool_id, []).append((ts_ms, signature))

    for events in pool_events.values():
        events.sort(key=lambda item: item[0])

    _SYSTEM_LOG_INDEX_CACHE[log_path] = pool_events
    return pool_events


def _load_gatekeeper_config_events(log_path: Path) -> list[tuple[int, float]]:
    cached = _SYSTEM_LOG_GATEKEEPER_CONFIG_CACHE.get(log_path)
    if cached is not None:
        return cached

    events: list[tuple[int, float]] = []
    if not log_path.exists():
        _SYSTEM_LOG_GATEKEEPER_CONFIG_CACHE[log_path] = events
        return events

    with log_path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            match = SYSTEM_LOG_GATEKEEPER_CONFIG_RE.match(line)
            if match is None:
                continue

            ts_ms = _parse_iso8601_ts_ms(match.group("ts"))
            if ts_ms is None:
                continue
            try:
                threshold = float(match.group("threshold"))
            except ValueError:
                continue
            events.append((ts_ms, threshold))

    events.sort(key=lambda item: item[0])
    _SYSTEM_LOG_GATEKEEPER_CONFIG_CACHE[log_path] = events
    return events


def _load_repo_config_min_sol_threshold() -> float:
    global _REPO_CONFIG_MIN_SOL_THRESHOLD
    if _REPO_CONFIG_MIN_SOL_THRESHOLD is not None:
        return _REPO_CONFIG_MIN_SOL_THRESHOLD

    for config_path in _candidate_ghost_brain_config_paths():
        try:
            text = config_path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        match = re.search(r"(?m)^min_sol_threshold\s*=\s*([0-9eE+.\-]+)\s*$", text)
        if match is not None:
            try:
                _REPO_CONFIG_MIN_SOL_THRESHOLD = float(match.group(1))
                return _REPO_CONFIG_MIN_SOL_THRESHOLD
            except ValueError:
                pass

    _REPO_CONFIG_MIN_SOL_THRESHOLD = 0.0
    return _REPO_CONFIG_MIN_SOL_THRESHOLD


def resolve_runtime_min_sol_threshold(
    record: dict,
    start_ts_ms: int,
    end_ts_ms: int,
) -> tuple[float, str]:
    explicit_threshold_f = record.get("min_sol_threshold")
    if isinstance(explicit_threshold_f, (float, int)) and not isinstance(explicit_threshold_f, bool):
        return float(explicit_threshold_f), "record.min_sol_threshold"
    if isinstance(explicit_threshold_f, str) and explicit_threshold_f.strip():
        try:
            return float(explicit_threshold_f.strip()), "record.min_sol_threshold"
        except ValueError:
            pass

    selected_threshold: float | None = None
    selected_source: str | None = None
    selected_ts_ms = -1
    for log_path in _system_log_path_candidates(record, start_ts_ms, end_ts_ms):
        for ts_ms, threshold in _load_gatekeeper_config_events(log_path):
            if ts_ms <= end_ts_ms and ts_ms > selected_ts_ms:
                selected_ts_ms = ts_ms
                selected_threshold = threshold
                selected_source = str(log_path)

    if selected_threshold is not None and selected_source is not None:
        return selected_threshold, f"system_log:{selected_source}"

    return _load_repo_config_min_sol_threshold(), "repo.ghost_brain_config.toml"


def lookup_exact_window_signatures(
    record: dict,
    pool_id: str,
    start_ts_ms: int,
    end_ts_ms: int,
) -> tuple[list[str] | None, int | None, str | None]:
    if not pool_id or start_ts_ms <= 0 or end_ts_ms < start_ts_ms:
        return None, None, None

    candidate_start_ts_ms = max(0, start_ts_ms - EXACT_WINDOW_LOG_START_SKEW_MS)
    candidate_end_ts_ms = end_ts_ms + EXACT_WINDOW_LOG_END_SKEW_MS

    raw_count = 0
    unique_signatures: list[str] = []
    seen_signatures: set[str] = set()
    source_paths: list[str] = []

    for log_path in _system_log_path_candidates(record, start_ts_ms, end_ts_ms):
        pool_events = _load_system_log_index(log_path).get(pool_id)
        if not pool_events:
            continue

        source_paths.append(str(log_path))
        timestamps = [ts for ts, _sig in pool_events]
        left = bisect_left(timestamps, candidate_start_ts_ms)
        right = bisect_right(timestamps, candidate_end_ts_ms)
        for ts_ms, signature in pool_events[left:right]:
            if candidate_start_ts_ms <= ts_ms <= candidate_end_ts_ms:
                raw_count += 1
                if signature not in seen_signatures:
                    seen_signatures.add(signature)
                    unique_signatures.append(signature)

    if not source_paths:
        return None, None, None

    return unique_signatures, raw_count, ",".join(source_paths)


def lookup_exact_window_emissions(
    record: dict,
    pool_id: str,
    start_ts_ms: int,
    end_ts_ms: int,
) -> tuple[int | None, int | None, str | None]:
    signatures, raw_count, source = lookup_exact_window_signatures(
        record,
        pool_id,
        start_ts_ms,
        end_ts_ms,
    )
    if signatures is None:
        return None, None, None
    return len(signatures), raw_count, source


# ══════════════════════════════════════════════════════════════════════════════
# TOKEN-BUCKET RATE LIMITER
# ══════════════════════════════════════════════════════════════════════════════

class RateLimiter:
    """
    Klasyczny token-bucket.  Utrzymuje ≤ rps żądań/sekundę globalnie,
    niezależnie od liczby równoległych coroutine.
    """

    __slots__ = ("rps", "capacity", "_tokens", "_last", "_blocked_until", "_lock")

    def __init__(self, rps: float, *, capacity: float | None = None, initial_tokens: float | None = None) -> None:
        self.rps     = rps
        self.capacity = max(1.0, float(capacity if capacity is not None else rps))
        self._tokens = min(self.capacity, float(self.capacity if initial_tokens is None else initial_tokens))
        self._last   = time.monotonic()
        self._blocked_until = 0.0
        self._lock   = asyncio.Lock()

    async def acquire(self) -> None:
        async with self._lock:
            now = time.monotonic()
            if now < self._blocked_until:
                await asyncio.sleep(self._blocked_until - now)
                now = time.monotonic()

            elapsed = now - self._last
            self._last   = now
            self._tokens = min(self.capacity, self._tokens + elapsed * self.rps)

            if self._tokens < 1.0:
                wait = (1.0 - self._tokens) / self.rps
                await asyncio.sleep(wait)
                self._tokens = 0.0
            else:
                self._tokens -= 1.0

    async def backoff(self, delay_s: float) -> None:
        delay_s = max(0.0, float(delay_s))
        if delay_s <= 0.0:
            return
        async with self._lock:
            blocked_until = time.monotonic() + delay_s
            if blocked_until > self._blocked_until:
                self._blocked_until = blocked_until
            self._last = max(self._last, self._blocked_until)
            self._tokens = min(self._tokens, 1.0)


class DiskTxCache:
    """Prosty cache gzip JSON na dysku keyed by transaction signature."""

    __slots__ = ("root",)

    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)

    def _path_for_signature(self, signature: str) -> Path:
        return self.root / f"{signature}.json.gz"

    def get(self, signature: str) -> dict | None:
        path = self._path_for_signature(signature)
        if not path.exists():
            return None
        try:
            with gzip.open(path, "rt", encoding="utf-8") as fh:
                payload = json.load(fh)
            if isinstance(payload, dict):
                return payload
        except (OSError, json.JSONDecodeError):
            log.warning("Ignoruję uszkodzony cache tx dla signature=%s", signature)
        return None

    def put(self, signature: str, tx: dict) -> None:
        path = self._path_for_signature(signature)
        tmp_path = path.with_suffix(path.suffix + ".tmp")
        with gzip.open(tmp_path, "wt", encoding="utf-8") as fh:
            json.dump(tx, fh, ensure_ascii=False, separators=(",", ":"))
        os.replace(tmp_path, path)


class PartialResultWriter:
    """Append-only journal wyników częściowych, bez blokowania finalnego write."""

    __slots__ = ("path", "_fh", "_lock", "_counter")

    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.path = path
        self._fh = path.open("a", encoding="utf-8")
        self._lock = asyncio.Lock()
        self._counter = 0

    async def append(self, record: dict) -> None:
        async with self._lock:
            self._fh.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
            self._counter += 1
            if self._counter % 10 == 0:
                self._fh.flush()
                os.fsync(self._fh.fileno())

    def close(self) -> None:
        self._fh.flush()
        os.fsync(self._fh.fileno())
        self._fh.close()


# ══════════════════════════════════════════════════════════════════════════════
# ASYNC SOLANA RPC CLIENT
# ══════════════════════════════════════════════════════════════════════════════

class SolanaRPC:
    """
    Minimalistyczny async JSON-RPC klient z:
      • rate-limitingiem (token-bucket)
      • exponential-backoff retry (429 / timeout / network error)
      • request-id counter (thread-safe dzięki asyncio single-threaded loop)
    """

    __slots__ = ("url", "_rl", "_tx_rl", "_tx_semaphore", "_session", "_req_id", "_tx_cache", "_tx_cache_lock", "_disk_tx_cache")

    def __init__(
        self,
        url: str,
        rate_limiter: RateLimiter,
        tx_rate_limiter: RateLimiter,
        tx_max_inflight: int,
        session: Any,
        disk_tx_cache: DiskTxCache | None = None,
    ) -> None:
        self.url     = url
        self._rl     = rate_limiter
        self._tx_rl  = tx_rate_limiter
        self._tx_semaphore = asyncio.Semaphore(max(1, tx_max_inflight))
        self._session = session
        self._req_id  = 0
        self._tx_cache: dict[str, dict] = {}
        self._tx_cache_lock = asyncio.Lock()
        self._disk_tx_cache = disk_tx_cache

    # ── core call ─────────────────────────────────────────────────────────────

    async def _call(
        self,
        method: str,
        params: list,
        retries: int = 4,
    ) -> object:
        """Wysyła jedno JSON-RPC żądanie z retry i rate-limitingiem."""
        self._req_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id":      self._req_id,
            "method":  method,
            "params":  params,
        }

        last_exc: Exception | None = None

        for attempt in range(retries):
            await self._rl.acquire()
            if method == "getTransaction":
                await self._tx_rl.acquire()
            try:
                if method == "getTransaction":
                    async with self._tx_semaphore:
                        async with self._session.post(
                            self.url,
                            json=payload,
                            timeout=aiohttp.ClientTimeout(total=20),
                        ) as resp:
                            response_payload = await self._handle_response(resp, method, attempt)
                else:
                    async with self._session.post(
                        self.url,
                        json=payload,
                        timeout=aiohttp.ClientTimeout(total=20),
                    ) as resp:
                        response_payload = await self._handle_response(resp, method, attempt)

                if response_payload is _RETRY_SENTINEL:
                    last_exc = RuntimeError(f"RPC {method} rate-limited after retry")
                    continue

                return response_payload

            except (aiohttp.ClientError, asyncio.TimeoutError, ValueError) as exc:
                last_exc = exc
                if attempt < retries - 1:
                    wait = 0.3 * (2 ** attempt)
                    log.debug("RPC %s error (attempt %d): %s — retry in %.1fs",
                              method, attempt + 1, exc, wait)
                    await asyncio.sleep(wait)

        raise RuntimeError(
            f"RPC {method} failed after {retries} attempts: {last_exc}"
        )

    async def _handle_response(self, resp: Any, method: str, attempt: int) -> object:
        if resp.status != 200:
            body = await resp.text()
            if resp.status == 429:
                retry_after = _parse_retry_delay_seconds(
                    resp.headers.get("Retry-After")
                )
                try:
                    body_json = json.loads(body)
                except json.JSONDecodeError:
                    body_json = None
                if isinstance(body_json, dict):
                    body_retry = _parse_retry_delay_seconds(
                        (((body_json.get("error") or {}).get("data") or {}).get("try_again_in"))
                    )
                    if body_retry is not None:
                        retry_after = body_retry
                if retry_after is None:
                    retry_after = min(2.0, 0.25 * (2 ** attempt))
                retry_after = max(0.05, retry_after)
                log.warning(
                    "HTTP 429 dla %s | retry_after=%.3fs | body=%s",
                    method,
                    retry_after,
                    body[:300],
                )
                if method == "getTransaction":
                    await self._tx_rl.backoff(retry_after)
                else:
                    await self._rl.backoff(retry_after)
                await asyncio.sleep(retry_after)
                return _RETRY_SENTINEL
            log.error(
                "HTTP %d dla %s | headers: %s | body: %s",
                resp.status, method,
                dict(resp.headers),
                body[:500],
            )
            raise RuntimeError(f"HTTP {resp.status} for {method}: {body[:200]}")

        data = await resp.json(content_type=None)

        if "error" in data:
            err = data["error"]
            if err.get("code") == -32005:
                retry_after = _parse_retry_delay_seconds(
                    ((err.get("data") or {}).get("try_again_in"))
                )
                if retry_after is None:
                    retry_after = min(2.0, 0.25 * (2 ** attempt))
                retry_after = max(0.05, retry_after)
                if method == "getTransaction":
                    await self._tx_rl.backoff(retry_after)
                else:
                    await self._rl.backoff(retry_after)
                await asyncio.sleep(retry_after)
                return _RETRY_SENTINEL
            raise ValueError(
                f"RPC error {err.get('code')}: {err.get('message')}"
            )

        return data.get("result")

    # ── public helpers ────────────────────────────────────────────────────────

    async def get_signatures_for_address(
        self,
        address: str,
        *,
        limit:  int           = SIG_PAGE_LIMIT,
        before: str  | None   = None,
        until:  str  | None   = None,
    ) -> list[dict]:
        config: dict = {"limit": limit, "commitment": "confirmed"}
        if before:
            config["before"] = before
        if until:
            config["until"]  = until
        result = await self._call("getSignaturesForAddress", [address, config])
        return result or []

    async def get_account_info(
        self,
        address: str,
        *,
        min_context_slot: int | None = None,
    ) -> dict | None:
        config: dict = {"encoding": "base64", "commitment": "confirmed"}
        if min_context_slot is not None:
            config["minContextSlot"] = min_context_slot
        result = await self._call("getAccountInfo", [address, config])
        return result  # {context: {slot}, value: {data, executable, lamports, ...} | null}

    async def get_transaction(self, signature: str) -> dict | None:
        async with self._tx_cache_lock:
            cached = self._tx_cache.get(signature)
        if cached is not None:
            return cached

        if self._disk_tx_cache is not None:
            cached = self._disk_tx_cache.get(signature)
            if cached is not None:
                async with self._tx_cache_lock:
                    self._tx_cache[signature] = cached
                return cached

        result = await self._call(
            "getTransaction",
            [
                signature,
                {
                    "encoding": "jsonParsed",
                    "commitment": "confirmed",
                    "maxSupportedTransactionVersion": 0,
                },
            ],
        )
        if isinstance(result, dict):
            async with self._tx_cache_lock:
                self._tx_cache[signature] = result
            if self._disk_tx_cache is not None:
                self._disk_tx_cache.put(signature, result)
            return result
        return None

    async def get_slot(self) -> int | None:
        return await self._call("getSlot", [{"commitment": "confirmed"}])

    async def get_block_time(self, slot: int) -> int | None:
        try:
            return await self._call("getBlockTime", [slot])
        except Exception:
            return None


# ══════════════════════════════════════════════════════════════════════════════
# POOL SNAPSHOT
# ══════════════════════════════════════════════════════════════════════════════

@dataclass
class PoolSnapshot:
    pool_id:            str
    target_ts_ms:       int             # window end
    target_unix_ts:     float           # target_ts_ms / 1000.0
    target_source:      str | None      = None
    window_start_ts_ms: int             = 0
    window_end_ts_ms:   int             = 0
    window_ms:          int             = 0
    window_source:      str | None      = None
    window_start_source:str | None      = None
    base_mint:          str | None      = None
    base_mint_source:   str | None      = None

    # ── tx counts ─────────────────────────────────────────────────────────────
    total_tx:           int             = 0    # trade-only tx w oknie
    confirmed_tx:       int             = 0    # trade-only tx bez błędu
    failed_tx:          int             = 0    # trade-only tx z err != null
    sig_pages_fetched:  int             = 0    # ile stron pobrano dla pool_id
    pool_signature_total_tx: int        = 0    # wszystkie sygnatury pool_id w oknie
    pool_signature_confirmed_tx: int    = 0
    pool_signature_failed_tx: int       = 0
    mint_signature_total_tx: int        = 0    # wszystkie sygnatury base_mint w oknie
    mint_signature_confirmed_tx: int    = 0
    mint_signature_failed_tx: int       = 0
    trade_signature_count: int          = 0
    trade_signature_confirmed_count: int = 0
    trade_signature_failed_count: int    = 0
    trade_event_total: int               = 0
    trade_event_confirmed: int           = 0
    trade_event_failed: int              = 0
    trade_event_bonus: int              = 0
    exact_window_emitted_unique_tx: int | None = None
    exact_window_emitted_raw_tx: int | None = None
    exact_window_source: str | None = None
    min_sol_threshold: float | None     = None
    min_sol_threshold_source: str | None = None
    dust_filtered_tx: int               = 0
    dust_filtered_confirmed_tx: int     = 0
    dust_filtered_failed_tx: int        = 0
    trade_volume_unresolved_tx: int     = 0
    non_trade_tx:       int             = 0
    off_pool_tx_excluded: int           = 0
    tx_fetch_failed:    int             = 0

    # ── account state (best-effort, może być aktualny jeśli brak archival RPC) ─
    account_lamports:   int | None      = None
    account_owner:      str | None      = None
    account_data_b64:   str | None      = None  # surowe dane curve/pool
    account_slot:       int | None      = None  # slot snapshot
    account_exec:       bool| None      = None

    # ── meta ──────────────────────────────────────────────────────────────────
    rpc_latency_ms:     float           = 0.0
    fetch_error:        str | None      = None


@dataclass(frozen=True)
class ObservedTxMetrics:
    total_tx_evaluated: int
    unique_tx_evaluated: int | None
    dust_filtered_count: int
    gatekeeper_seen_total: int
    confirmed_tx_evaluated: int | None
    failed_tx_evaluated: int | None
    split_available: bool
    split_consistent: bool
    split_source: str | None


def _first_present_record_int(
    record: dict,
    keys: tuple[str, ...],
) -> tuple[int | None, str | None]:
    for key in keys:
        value = _coerce_record_int(record, key)
        if value is not None:
            return int(value), key
    return None, None


def observed_tx_metrics_from_record(record: dict) -> ObservedTxMetrics:
    total_tx_evaluated = int(_coerce_record_int(record, "total_tx_evaluated") or 0)
    unique_tx_evaluated = _coerce_record_int(record, "unique_tx_evaluated")
    dust_filtered_count = int(_coerce_record_int(record, "dust_filtered_count") or 0)

    confirmed_tx_evaluated, confirmed_key = _first_present_record_int(
        record,
        (
            "confirmed_tx_evaluated",
            "successful_tx_evaluated",
            "success_tx_evaluated",
            "observed_confirmed_tx_evaluated",
        ),
    )
    failed_tx_evaluated, failed_key = _first_present_record_int(
        record,
        (
            "failed_tx_evaluated",
            "observed_failed_tx_evaluated",
            "failed_trade_tx_evaluated",
        ),
    )

    split_source: str | None = None
    if confirmed_tx_evaluated is None and failed_tx_evaluated is not None:
        confirmed_tx_evaluated = max(0, total_tx_evaluated - failed_tx_evaluated)
        split_source = f"derived.total_minus.{failed_key}"
    elif failed_tx_evaluated is None and confirmed_tx_evaluated is not None:
        failed_tx_evaluated = max(0, total_tx_evaluated - confirmed_tx_evaluated)
        split_source = f"derived.total_minus.{confirmed_key}"
    elif confirmed_tx_evaluated is not None and failed_tx_evaluated is not None:
        split_source = f"record.{confirmed_key}+record.{failed_key}"

    split_available = (
        confirmed_tx_evaluated is not None and failed_tx_evaluated is not None
    )
    split_consistent = (
        not split_available
        or confirmed_tx_evaluated + failed_tx_evaluated == total_tx_evaluated
    )
    if split_available and not split_consistent:
        confirmed_tx_evaluated = None
        failed_tx_evaluated = None
        split_available = False
        split_source = None

    return ObservedTxMetrics(
        total_tx_evaluated=total_tx_evaluated,
        unique_tx_evaluated=unique_tx_evaluated,
        dust_filtered_count=dust_filtered_count,
        gatekeeper_seen_total=total_tx_evaluated + dust_filtered_count,
        confirmed_tx_evaluated=confirmed_tx_evaluated,
        failed_tx_evaluated=failed_tx_evaluated,
        split_available=split_available,
        split_consistent=split_consistent,
        split_source=split_source,
    )


def observed_comparable_tx_target(record: dict) -> int:
    observed = observed_tx_metrics_from_record(record)
    return observed.total_tx_evaluated


def snapshot_has_numeric_coverage(snap: PoolSnapshot) -> bool:
    if snap.trade_signature_count > 0:
        return True
    return (
        snap.fetch_error is None
        and snap.tx_fetch_failed == 0
        and (
            snap.dust_filtered_tx > 0
            or snap.non_trade_tx > 0
            or snap.off_pool_tx_excluded > 0
            or snap.pool_signature_total_tx > 0
            or snap.mint_signature_total_tx > 0
        )
    )


def snapshot_requires_retry(record: dict, snap: PoolSnapshot) -> bool:
    if snapshot_has_numeric_coverage(snap):
        return False
    if snap.fetch_error is not None:
        return True
    if snap.tx_fetch_failed > 0:
        return True
    if snap.pool_signature_total_tx > 0:
        return True
    return observed_tx_metrics_from_record(record).total_tx_evaluated > 0


# ══════════════════════════════════════════════════════════════════════════════
# CORE FETCH LOGIC
# ══════════════════════════════════════════════════════════════════════════════

def _parse_record_timestamp_ms(record: dict) -> int:
    ts_str = record.get("timestamp", "")
    import re as _re

    ts_fixed = _re.sub(
        r'(\.\d{6})\d+',
        r'\1',
        ts_str,
    )
    dt = datetime.fromisoformat(ts_fixed)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return int(dt.timestamp() * 1000)


def _coerce_int(value: Any) -> int | None:
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    if isinstance(value, str):
        value = value.strip()
        if not value:
            return None
        try:
            return int(value)
        except ValueError:
            return None
    return None


def _coerce_record_int(record: dict, key: str) -> int | None:
    return _coerce_int(record.get(key))


def record_cohort_ts_ms(record: dict) -> int | None:
    envelope = record.get("envelope")
    if isinstance(envelope, dict):
        event_time_ms = _coerce_int(envelope.get("event_time_ms"))
        if event_time_ms is not None and event_time_ms > 0:
            return event_time_ms

    for key in ("observation_start_ts_ms", "first_seen_ts_ms", "ab_t0_event_ts_ms", "event_time_ms"):
        value = _coerce_record_int(record, key)
        if value is not None and value > 0:
            return value

    timestamp = record.get("timestamp")
    if isinstance(timestamp, str) and timestamp.strip():
        try:
            parsed = _parse_record_timestamp_ms(record)
        except ValueError:
            return None
        if parsed > 0:
            return parsed
    return None


def extract_record_run_id(record: dict) -> str | None:
    envelope = record.get("envelope")
    candidates = [record.get("run_id")]
    if isinstance(envelope, dict):
        candidates.append(envelope.get("run_id"))

    for candidate in candidates:
        if not isinstance(candidate, str):
            continue
        normalized = candidate.strip()
        if normalized:
            return normalized
    return None


def filter_input_records(
    records: list[dict],
    *,
    since_ms: int = 0,
    run_id: str | None = None,
) -> tuple[list[dict], dict[str, Any]]:
    selected: list[dict] = []
    cohort_ts_values: list[int] = []
    observed_run_ids: set[str] = set()
    skipped_before_since_ms = 0
    skipped_missing_cohort_ts = 0
    skipped_run_id_mismatch = 0

    for record in records:
        record_run_id = extract_record_run_id(record)
        if run_id is not None and record_run_id != run_id:
            skipped_run_id_mismatch += 1
            continue

        cohort_ts_ms = record_cohort_ts_ms(record)
        if since_ms > 0:
            if cohort_ts_ms is None:
                skipped_missing_cohort_ts += 1
                continue
            if cohort_ts_ms < since_ms:
                skipped_before_since_ms += 1
                continue

        selected.append(record)
        if cohort_ts_ms is not None:
            cohort_ts_values.append(cohort_ts_ms)
        if record_run_id is not None:
            observed_run_ids.add(record_run_id)

    return selected, {
        "source_loaded_record_count": len(records),
        "source_selected_record_count": len(selected),
        "source_since_ms": since_ms if since_ms > 0 else None,
        "source_requested_run_id": run_id,
        "source_skipped_before_since_ms": skipped_before_since_ms,
        "source_skipped_missing_cohort_ts": skipped_missing_cohort_ts,
        "source_skipped_run_id_mismatch": skipped_run_id_mismatch,
        "source_cohort_min_ts_ms": min(cohort_ts_values) if cohort_ts_values else None,
        "source_cohort_max_ts_ms": max(cohort_ts_values) if cohort_ts_values else None,
        "source_run_id_count": len(observed_run_ids),
        "source_run_ids_sample": sorted(observed_run_ids)[:8],
    }


@dataclass(frozen=True)
class WindowSpec:
    start_ts_ms: int
    end_ts_ms: int
    window_ms: int
    window_source: str
    start_source: str
    end_source: str


def resolve_window_spec(
    record: dict,
    *,
    target_ts_ms_override: int | None = None,
    window_ms_override: int | None = None,
) -> WindowSpec:
    log_ts_ms = _parse_record_timestamp_ms(record)

    start_ts_ms = 0
    start_source = ""
    explicit_end_ts_ms = 0
    for key in ("observation_start_ts_ms", "first_seen_ts_ms", "ab_t0_event_ts_ms"):
        value = _coerce_record_int(record, key)
        if value is not None and value > 0:
            start_ts_ms = value
            start_source = f"record.{key}"
            break

    explicit_end = _coerce_record_int(record, "observation_end_ts_ms")
    if explicit_end is not None and explicit_end > 0:
        explicit_end_ts_ms = explicit_end

    resolved_target_ts_ms = 0
    resolved_target_source = ""
    curve_t0_event_ts_ms = _coerce_record_int(record, "curve_t0_event_ts_ms")
    curve_wait_elapsed_ms = _coerce_record_int(record, "curve_wait_elapsed_ms")
    if explicit_end_ts_ms > 0:
        resolved_target_ts_ms = explicit_end_ts_ms
        resolved_target_source = "record.observation_end_ts_ms"
    elif (
        curve_t0_event_ts_ms is not None
        and curve_t0_event_ts_ms > 0
        and curve_wait_elapsed_ms is not None
        and curve_wait_elapsed_ms >= 0
    ):
        resolved_target_ts_ms = curve_t0_event_ts_ms + curve_wait_elapsed_ms
        resolved_target_source = "record.curve_t0_event_ts_ms_plus_curve_wait_elapsed_ms"
    elif start_ts_ms <= 0 and log_ts_ms > 0:
        resolved_target_ts_ms = log_ts_ms
        resolved_target_source = "record.timestamp"

    if window_ms_override is not None:
        window_ms = max(0, int(window_ms_override))
        window_source = "cli.window_ms"
    else:
        observation_window_ms = _coerce_record_int(record, "observation_window_ms")
        observation_duration_ms = _coerce_record_int(record, "observation_duration_ms")
        if explicit_end_ts_ms > 0 and start_ts_ms > 0:
            window_ms = max(0, explicit_end_ts_ms - start_ts_ms)
            window_source = "record.observation_end_ts_ms"
        elif observation_window_ms is not None and observation_window_ms > 0:
            window_ms = observation_window_ms
            window_source = "record.observation_window_ms"
        elif observation_duration_ms is not None and observation_duration_ms > 0:
            window_ms = observation_duration_ms
            window_source = "record.observation_duration_ms"
        else:
            window_ms = DEFAULT_WINDOW_MS
            window_source = "fallback.default_window_ms"

    if start_ts_ms <= 0:
        if target_ts_ms_override is not None:
            end_for_start = target_ts_ms_override
            start_source = "derived.target_minus_window"
        elif resolved_target_ts_ms > 0:
            end_for_start = resolved_target_ts_ms
            if resolved_target_source == "record.timestamp":
                start_source = "derived.log_minus_window"
            else:
                start_source = f"derived.{resolved_target_source}_minus_window"
        else:
            end_for_start = log_ts_ms
            start_source = "derived.log_minus_window"
        start_ts_ms = max(0, end_for_start - window_ms)

    if target_ts_ms_override is not None:
        end_ts_ms = max(start_ts_ms, int(target_ts_ms_override))
        end_source = "cli.target_ts_ms"
        if window_ms_override is None:
            window_ms = max(0, end_ts_ms - start_ts_ms)
            window_source = end_source
    elif resolved_target_ts_ms > 0:
        end_ts_ms = max(start_ts_ms, resolved_target_ts_ms)
        end_source = resolved_target_source
        if window_ms_override is None:
            window_ms = max(0, end_ts_ms - start_ts_ms)
            if not (
                resolved_target_source == "record.timestamp"
                and explicit_end_ts_ms <= 0
            ):
                window_source = end_source
    else:
        end_ts_ms = start_ts_ms + window_ms
        end_source = window_source

    return WindowSpec(
        start_ts_ms=start_ts_ms,
        end_ts_ms=end_ts_ms,
        window_ms=max(0, end_ts_ms - start_ts_ms),
        window_source=window_source,
        start_source=start_source,
        end_source=end_source,
    )


def infer_base_mint_from_tx(tx: dict, pool_id: str) -> tuple[str | None, str | None]:
    meta = tx.get("meta") or {}
    balances = list(meta.get("postTokenBalances") or []) + list(meta.get("preTokenBalances") or [])

    for balance in balances:
        mint = balance.get("mint")
        owner = balance.get("owner")
        if mint and mint not in BLACKLIST_MINTS and owner == pool_id:
            return str(mint), "tx_balance.owner_equals_pool"

    for balance in balances:
        mint = balance.get("mint")
        if mint and mint not in BLACKLIST_MINTS:
            return str(mint), "tx_balance.first_non_blacklisted"

    return None, None


def normalize_base_mint_hint(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    candidate = value.strip()
    if candidate.lower() in UNKNOWN_SENTINELS:
        return None
    return candidate or None


def _safe_list(value: object) -> list:
    if isinstance(value, list):
        return value
    return []


def _decode_base58(value: str) -> bytes | None:
    if not value:
        return b""

    number = 0
    try:
        for char in value:
            number = number * 58 + BASE58_INDEX[char]
    except KeyError:
        return None

    decoded = bytearray()
    while number > 0:
        number, remainder = divmod(number, 256)
        decoded.append(remainder)
    decoded.reverse()

    leading_zeroes = 0
    for char in value:
        if char == "1":
            leading_zeroes += 1
        else:
            break

    return (b"\x00" * leading_zeroes) + bytes(decoded)


def _account_keys_from_message(message: dict) -> list[str]:
    keys: list[str] = []
    for item in _safe_list(message.get("accountKeys")):
        if isinstance(item, str):
            keys.append(item)
        elif isinstance(item, dict):
            keys.append(str(item.get("pubkey", "")))
        else:
            keys.append("")
    return keys


def _loaded_addresses_from_meta(meta: dict) -> list[str]:
    loaded = meta.get("loadedAddresses") or {}
    keys: list[str] = []
    for bucket in ("writable", "readonly"):
        for item in _safe_list(loaded.get(bucket)):
            if isinstance(item, str):
                keys.append(item)
    return keys


def transaction_touches_pool(tx: dict, pool_id: str) -> bool:
    if not pool_id:
        return False

    transaction = tx.get("transaction") or {}
    message = transaction.get("message") or {}
    meta = tx.get("meta") or {}
    if pool_id in _account_keys_from_message(message):
        return True
    if pool_id in _loaded_addresses_from_meta(meta):
        return True

    for balances_key in ("preTokenBalances", "postTokenBalances"):
        for entry in _safe_list(meta.get(balances_key)):
            if not isinstance(entry, dict):
                continue
            if entry.get("owner") == pool_id:
                return True
    return False


def _resolve_program_id(ix: dict, account_keys: list[str]) -> str | None:
    program_id = ix.get("programId")
    if isinstance(program_id, str) and program_id:
        return program_id
    program_id_index = ix.get("programIdIndex")
    if isinstance(program_id_index, int) and 0 <= program_id_index < len(account_keys):
        candidate = account_keys[program_id_index]
        return candidate or None
    return None


def _instruction_data_bytes(ix: dict) -> bytes | None:
    data = ix.get("data")
    if isinstance(data, (bytes, bytearray)):
        return bytes(data)
    if not isinstance(data, str):
        return None
    return _decode_base58(data)


def _read_u64_le_at(data: bytes | None, offset: int) -> int | None:
    if data is None:
        return None
    end = offset + 8
    if end > len(data):
        return None
    return int.from_bytes(data[offset:end], "little", signed=False)


def _iter_program_ids(tx: dict) -> list[str]:
    transaction = tx.get("transaction") or {}
    message = transaction.get("message") or {}
    account_keys = _account_keys_from_message(message)
    program_ids: list[str] = []

    for ix in _safe_list(message.get("instructions")):
        if not isinstance(ix, dict):
            continue
        program_id = _resolve_program_id(ix, account_keys)
        if program_id:
            program_ids.append(program_id)

    meta = tx.get("meta") or {}
    for inner in _safe_list(meta.get("innerInstructions")):
        if not isinstance(inner, dict):
            continue
        for ix in _safe_list(inner.get("instructions")):
            if not isinstance(ix, dict):
                continue
            program_id = _resolve_program_id(ix, account_keys)
            if program_id:
                program_ids.append(program_id)

    return program_ids


def _iter_resolved_instructions(tx: dict) -> list[tuple[str, bytes | None, bool]]:
    transaction = tx.get("transaction") or {}
    message = transaction.get("message") or {}
    account_keys = _account_keys_from_message(message)
    instructions: list[tuple[str, bytes | None, bool]] = []

    for ix in _safe_list(message.get("instructions")):
        if not isinstance(ix, dict):
            continue
        program_id = _resolve_program_id(ix, account_keys)
        if program_id:
            instructions.append((program_id, _instruction_data_bytes(ix), False))

    meta = tx.get("meta") or {}
    for inner in _safe_list(meta.get("innerInstructions")):
        if not isinstance(inner, dict):
            continue
        for ix in _safe_list(inner.get("instructions")):
            if not isinstance(ix, dict):
                continue
            program_id = _resolve_program_id(ix, account_keys)
            if program_id:
                instructions.append((program_id, _instruction_data_bytes(ix), True))

    return instructions


def _iter_resolved_instruction_dicts(tx: dict) -> list[tuple[str, dict, bool, list[str]]]:
    transaction = tx.get("transaction") or {}
    message = transaction.get("message") or {}
    account_keys = _account_keys_from_message(message)
    instructions: list[tuple[str, dict, bool, list[str]]] = []

    for ix in _safe_list(message.get("instructions")):
        if not isinstance(ix, dict):
            continue
        program_id = _resolve_program_id(ix, account_keys)
        if program_id:
            instructions.append((program_id, ix, False, account_keys))

    meta = tx.get("meta") or {}
    for inner in _safe_list(meta.get("innerInstructions")):
        if not isinstance(inner, dict):
            continue
        for ix in _safe_list(inner.get("instructions")):
            if not isinstance(ix, dict):
                continue
            program_id = _resolve_program_id(ix, account_keys)
            if program_id:
                instructions.append((program_id, ix, True, account_keys))

    return instructions


def _resolved_instruction_accounts(ix: dict, account_keys: list[str]) -> list[str]:
    resolved: list[str] = []
    for entry in _safe_list(ix.get("accounts")):
        if isinstance(entry, str):
            resolved.append(entry)
        elif isinstance(entry, int) and 0 <= entry < len(account_keys):
            resolved.append(account_keys[entry])
    return resolved


def _pumpswap_pool_wsol_is_base(tx: dict, pool_id: str) -> bool:
    if not pool_id:
        return False
    for program_id, ix, _is_inner, account_keys in _iter_resolved_instruction_dicts(tx):
        if program_id != PUMP_SWAP_PROGRAM_ID:
            continue
        accounts = _resolved_instruction_accounts(ix, account_keys)
        if len(accounts) < 5 or accounts[0] != pool_id:
            continue
        return accounts[3] == WSOL_MINT
    return False


def tx_has_pump_trade_discriminator(tx: dict) -> bool:
    for program_id, data_bytes, _is_inner in _iter_resolved_instructions(tx):
        if program_id not in {PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID}:
            continue
        if not data_bytes or len(data_bytes) < 8:
            continue
        disc = data_bytes[:8]
        if disc in {
            DISC_BUY,
            DISC_SELL,
            DISC_SWAP_BUY_EXACT_QUOTE_IN,
            DISC_PUMP_BUY_ROUTED,
        }:
            return True
        if disc == DISC_SWAP_OUTER_WRAPPER and len(data_bytes) >= 16:
            inner_disc = data_bytes[8:16]
            if inner_disc in EVENT_TRADE_DISCS:
                return True
    return False


def _token_amount(entry: dict) -> int:
    ui = entry.get("uiTokenAmount") or {}
    amount = ui.get("amount", "0")
    try:
        return int(amount)
    except (TypeError, ValueError):
        return 0


def _token_balance_map(entries: list[dict], base_mint: str) -> dict[tuple[int, str], int]:
    result: dict[tuple[int, str], int] = {}
    synthetic_index = 1_000_000
    for entry in entries:
        if entry.get("mint") != base_mint:
            continue
        idx = entry.get("accountIndex")
        if not isinstance(idx, int):
            idx = synthetic_index
            synthetic_index += 1
        owner = str(entry.get("owner", ""))
        result[(idx, owner)] = _token_amount(entry)
    return result


def _has_base_mint_balance_change(tx: dict, base_mint: str) -> bool:
    meta = tx.get("meta") or {}
    pre_map = _token_balance_map(_safe_list(meta.get("preTokenBalances")), base_mint)
    post_map = _token_balance_map(_safe_list(meta.get("postTokenBalances")), base_mint)
    if not pre_map and not post_map:
        return False
    keys = set(pre_map) | set(post_map)
    return any(pre_map.get(key, 0) != post_map.get(key, 0) for key in keys)


def _token_owner_deltas(meta: dict, base_mint: str) -> dict[str, int]:
    pre_map = _token_balance_map(_safe_list(meta.get("preTokenBalances")), base_mint)
    post_map = _token_balance_map(_safe_list(meta.get("postTokenBalances")), base_mint)
    owner_deltas: dict[str, int] = {}
    for key in set(pre_map) | set(post_map):
        owner = key[1]
        if not owner:
            continue
        delta = post_map.get(key, 0) - pre_map.get(key, 0)
        if delta:
            owner_deltas[owner] = owner_deltas.get(owner, 0) + delta
    return owner_deltas


def _lamport_owner_deltas(tx: dict) -> dict[str, int]:
    transaction = tx.get("transaction") or {}
    message = transaction.get("message") or {}
    account_keys = _account_keys_from_message(message)
    meta = tx.get("meta") or {}
    pre = _safe_list(meta.get("preBalances"))
    post = _safe_list(meta.get("postBalances"))
    owner_deltas: dict[str, int] = {}
    n = min(len(account_keys), len(pre), len(post))
    for idx in range(n):
        owner = account_keys[idx]
        if not owner:
            continue
        try:
            delta = int(post[idx]) - int(pre[idx])
        except (TypeError, ValueError):
            continue
        owner_deltas[owner] = owner_deltas.get(owner, 0) + delta
    return owner_deltas


def _infer_trade_side(tx: dict, base_mint: str) -> str | None:
    meta = tx.get("meta") or {}
    token_deltas = _token_owner_deltas(meta, base_mint)
    if not token_deltas:
        return None
    lamport_deltas = _lamport_owner_deltas(tx)

    buyer_owner = None
    buyer_delta = 0
    seller_owner = None
    seller_delta = 0

    for owner, delta in token_deltas.items():
        if delta > buyer_delta:
            buyer_delta = delta
            buyer_owner = owner
        if delta < seller_delta:
            seller_delta = delta
            seller_owner = owner

    if buyer_owner and buyer_delta > 0 and lamport_deltas.get(buyer_owner, 0) < 0:
        return "BUY"
    if seller_owner and seller_delta < 0 and lamport_deltas.get(seller_owner, 0) > 0:
        return "SELL"
    return None


def _infer_trade_sol_lamports_from_payload(
    tx: dict,
    pool_id: str | None = None,
) -> int | None:
    pumpswap_wsol_is_base = _pumpswap_pool_wsol_is_base(tx, pool_id or "")
    top_level_fallback: int | None = None

    for program_id, data_bytes, is_inner in _iter_resolved_instructions(tx):
        if program_id not in {PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID}:
            continue
        if not data_bytes or len(data_bytes) < 8:
            continue

        disc = data_bytes[:8]
        if (
            not is_inner
            and disc in {
                DISC_BUY,
                DISC_SELL,
                DISC_SWAP_BUY_EXACT_QUOTE_IN,
                DISC_PUMP_BUY_ROUTED,
            }
        ):
            sol_amount = _read_u64_le_at(data_bytes, 16)
            if sol_amount is not None and sol_amount > 0:
                top_level_fallback = top_level_fallback or sol_amount

        if disc != DISC_SWAP_OUTER_WRAPPER or len(data_bytes) < 16:
            continue

        inner_disc = data_bytes[8:16]
        payload = data_bytes[16:]
        if inner_disc == DISC_EVENT_TRADE:
            sol_amount = _read_u64_le_at(payload, 32)
            if sol_amount is not None and sol_amount > 0:
                return sol_amount
        elif inner_disc == DISC_SWAP_EVENT_BUY:
            if pumpswap_wsol_is_base:
                sol_amount = _read_u64_le_at(payload, 8)
            else:
                sol_amount = _read_u64_le_at(payload, 56)
            if sol_amount is not None and sol_amount > 0:
                return sol_amount
        elif inner_disc == DISC_SWAP_EVENT_SELL:
            if pumpswap_wsol_is_base:
                sol_amount = _read_u64_le_at(payload, 8)
            else:
                sol_amount = _read_u64_le_at(payload, 56)
            if sol_amount is not None and sol_amount > 0:
                return sol_amount

    return top_level_fallback


def _tx_error_value(row: dict, tx: dict) -> object:
    row_err = row.get("err")
    if row_err is not None:
        return row_err
    meta = tx.get("meta") or {}
    return meta.get("err")


def _tx_block_time_seconds(row: dict, tx: dict) -> int | None:
    row_block_time = row.get("blockTime")
    if isinstance(row_block_time, int):
        return row_block_time

    tx_block_time = tx.get("blockTime")
    if isinstance(tx_block_time, int):
        return tx_block_time

    return None


def _tx_within_block_time_window(
    row: dict,
    tx: dict,
    start_ts_ms: int,
    end_ts_ms: int,
) -> bool:
    if start_ts_ms <= 0 or end_ts_ms < start_ts_ms:
        return True

    block_time_s = _tx_block_time_seconds(row, tx)
    if block_time_s is None:
        return True

    start_ts_s = start_ts_ms // 1000
    end_ts_s = end_ts_ms // 1000
    return start_ts_s <= block_time_s <= end_ts_s


def infer_trade_volume_sol_for_coverage(
    tx: dict,
    base_mint: str,
    pool_id: str | None = None,
) -> float | None:
    payload_sol_lamports = _infer_trade_sol_lamports_from_payload(tx, pool_id)
    if payload_sol_lamports is not None:
        return payload_sol_lamports / LAMPORTS_PER_SOL

    meta = tx.get("meta") or {}
    token_deltas = _token_owner_deltas(meta, base_mint)
    if not token_deltas:
        return None
    lamport_deltas = _lamport_owner_deltas(tx)

    buyer_owner = None
    buyer_delta = 0
    seller_owner = None
    seller_delta = 0

    for owner, delta in token_deltas.items():
        if delta > buyer_delta:
            buyer_delta = delta
            buyer_owner = owner
        if delta < seller_delta:
            seller_delta = delta
            seller_owner = owner

    if buyer_owner and buyer_delta > 0:
        lamports = -lamport_deltas.get(buyer_owner, 0)
        if lamports > 0:
            return lamports / LAMPORTS_PER_SOL

    if seller_owner and seller_delta < 0:
        lamports = lamport_deltas.get(seller_owner, 0)
        if lamports > 0:
            return lamports / LAMPORTS_PER_SOL

    return None


def classify_trade_for_coverage(tx: dict, base_mint: str) -> tuple[bool, str | None]:
    is_trade, side = classify_trade(tx, base_mint)
    if is_trade:
        return is_trade, side

    program_ids = set(_iter_program_ids(tx))
    if PUMP_SWAP_PROGRAM_ID not in program_ids and PUMP_FUN_PROGRAM_ID not in program_ids:
        return False, None
    if not _has_base_mint_balance_change(tx, base_mint):
        return False, None
    return True, _infer_trade_side(tx, base_mint)


def count_trade_events_for_coverage(tx: dict, base_mint: str) -> int:
    if not _has_base_mint_balance_change(tx, base_mint):
        return 0

    top_level_trade_count = 0
    event_trade_count = 0

    for program_id, data_bytes, is_inner in _iter_resolved_instructions(tx):
        if program_id not in {PUMP_FUN_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID}:
            continue
        if not data_bytes or len(data_bytes) < 8:
            continue

        disc = data_bytes[:8]
        if disc in TOP_LEVEL_TRADE_DISCS and not is_inner:
            top_level_trade_count += 1
            continue

        if disc in EVENT_TRADE_DISCS:
            event_trade_count += 1
            continue

        if disc == DISC_SWAP_OUTER_WRAPPER and len(data_bytes) >= 16:
            inner_disc = data_bytes[8:16]
            if inner_disc in EVENT_TRADE_DISCS:
                event_trade_count += 1

    if event_trade_count > 0:
        return event_trade_count
    if top_level_trade_count > 0:
        return top_level_trade_count
    return 1


def summarize_trade_transactions(
    pool_id: str,
    signature_rows: list[dict],
    transactions: list[tuple[dict, dict]],
    base_mint_hint: str | None = None,
    require_pool_touch: bool = False,
    min_sol_threshold: float = 0.0,
    trusted_exact_signatures: set[str] | None = None,
    window_start_ts_ms: int = 0,
    window_end_ts_ms: int = 0,
) -> dict:
    base_mint = normalize_base_mint_hint(base_mint_hint)
    base_mint_source = "record.base_mint" if base_mint else None
    trusted_exact_signatures = trusted_exact_signatures or set()
    in_window_transactions: list[tuple[dict, dict]] = []
    for row, tx in transactions:
        signature = row.get("signature")
        if isinstance(signature, str) and signature in trusted_exact_signatures:
            in_window_transactions.append((row, tx))
            continue
        if _tx_within_block_time_window(
            row,
            tx,
            window_start_ts_ms,
            window_end_ts_ms,
        ):
            in_window_transactions.append((row, tx))

    if base_mint is None:
        for _row, tx in in_window_transactions:
            base_mint, base_mint_source = infer_base_mint_from_tx(tx, pool_id)
            if base_mint:
                break

    trade_total = 0
    trade_confirmed = 0
    trade_failed = 0
    trade_signature_count = 0
    trade_signature_confirmed_count = 0
    trade_signature_failed_count = 0
    dust_filtered_tx = 0
    dust_filtered_confirmed_tx = 0
    dust_filtered_failed_tx = 0
    trade_volume_unresolved_tx = 0
    non_trade = 0
    off_pool_excluded = 0

    if base_mint is None:
        return {
            "base_mint": None,
            "base_mint_source": None,
            "trade_total": 0,
            "trade_confirmed": 0,
            "trade_failed": 0,
            "trade_signature_count": 0,
            "trade_signature_confirmed_count": 0,
            "trade_signature_failed_count": 0,
            "dust_filtered_tx": 0,
            "dust_filtered_confirmed_tx": 0,
            "dust_filtered_failed_tx": 0,
            "trade_volume_unresolved_tx": 0,
            "non_trade": len(in_window_transactions),
            "off_pool_excluded": 0,
            "classification_error": "base_mint_unresolved",
        }

    tx_by_sig = {
        row.get("signature"): tx
        for row, tx in in_window_transactions
        if isinstance(row.get("signature"), str)
    }

    for row in signature_rows:
        signature = row.get("signature")
        tx = tx_by_sig.get(signature)
        if tx is None:
            continue
        if require_pool_touch and not transaction_touches_pool(tx, pool_id):
            off_pool_excluded += 1
            continue
        is_trade, _side = classify_trade_for_coverage(tx, base_mint)
        if (
            not is_trade
            and isinstance(signature, str)
            and signature in trusted_exact_signatures
            and transaction_touches_pool(tx, pool_id)
            and tx_has_pump_trade_discriminator(tx)
        ):
            is_trade = True
        if is_trade:
            trade_volume_sol = infer_trade_volume_sol_for_coverage(tx, base_mint, pool_id)
            tx_error = _tx_error_value(row, tx)
            if trade_volume_sol is None:
                trade_volume_unresolved_tx += 1
            elif trade_volume_sol < min_sol_threshold:
                dust_filtered_tx += 1
                if tx_error is None:
                    dust_filtered_confirmed_tx += 1
                else:
                    dust_filtered_failed_tx += 1
                continue
            trade_signature_count += 1
            trade_event_count = count_trade_events_for_coverage(tx, base_mint)
            trade_total += trade_event_count
            if tx_error is None:
                trade_signature_confirmed_count += 1
                trade_confirmed += trade_event_count
            else:
                trade_signature_failed_count += 1
                trade_failed += trade_event_count
        else:
            non_trade += 1

    return {
        "base_mint": base_mint,
        "base_mint_source": base_mint_source,
        "trade_total": trade_total,
        "trade_confirmed": trade_confirmed,
        "trade_failed": trade_failed,
        "trade_signature_count": trade_signature_count,
        "trade_signature_confirmed_count": trade_signature_confirmed_count,
        "trade_signature_failed_count": trade_signature_failed_count,
        "dust_filtered_tx": dust_filtered_tx,
        "dust_filtered_confirmed_tx": dust_filtered_confirmed_tx,
        "dust_filtered_failed_tx": dust_filtered_failed_tx,
        "trade_volume_unresolved_tx": trade_volume_unresolved_tx,
        "trade_event_bonus": max(0, trade_total - trade_signature_count),
        "non_trade": non_trade,
        "off_pool_excluded": off_pool_excluded,
        "classification_error": None,
    }


def _merge_signature_rows(*groups: list[dict]) -> list[dict]:
    merged: dict[str, dict] = {}
    for rows in groups:
        for row in rows:
            signature = row.get("signature")
            if not isinstance(signature, str) or not signature:
                continue
            merged.setdefault(signature, row)
    return list(merged.values())


def _merge_fetched_transactions(
    existing: list[tuple[dict, dict]],
    extra: list[tuple[dict, dict]],
) -> list[tuple[dict, dict]]:
    merged: dict[str, tuple[dict, dict]] = {}
    for row, tx in existing:
        signature = row.get("signature")
        if isinstance(signature, str) and signature:
            merged[signature] = (row, tx)
    for row, tx in extra:
        signature = row.get("signature")
        if isinstance(signature, str) and signature:
            merged[signature] = (row, tx)
    return list(merged.values())

async def _collect_signatures(
    rpc:          SolanaRPC,
    address:      str,
    start_ts:     float,       # unix seconds
    end_ts:       float,       # unix seconds
    max_pages:    int,
) -> tuple[list[dict], int, int, int, int]:
    """
    Paginuje getSignaturesForAddress (newest-first) i zlicza tx
    w oknie [start_ts, end_ts].

    Zwraca: (signature_rows, total_tx, confirmed_tx, failed_tx, pages_fetched)
    """
    start_ts_floor = int(start_ts)
    end_ts_floor = int(end_ts)

    selected: list[dict] = []
    total     = 0
    confirmed = 0
    failed    = 0
    cursor: str | None = None          # 'before' pagination cursor

    for page_num in range(1, max_pages + 1):
        sigs = await rpc.get_signatures_for_address(
            address,
            limit=SIG_PAGE_LIMIT,
            before=cursor,
        )

        if not sigs:
            return selected, total, confirmed, failed, page_num

        for sig in sigs:
            bt: int | None = sig.get("blockTime")

            # brak blockTime bywa przejściowy na świeżych / przeciążonych odpowiedziach RPC.
            # Nie wolno tego gubić, bo kolejny rerun często zwraca już poprawny blockTime.
            # W coverage wolimy zachować taką sygnaturę prowizorycznie niż wyzerować całą pool.
            if bt is None:
                selected.append(sig)
                total += 1
                if sig.get("err") is None:
                    confirmed += 1
                else:
                    failed += 1
                continue

            # nowszy niż target → pomijamy (wrócimy do nich tylko w teorii —
            # getSignaturesForAddress zwraca newest-first, więc pierwsze sygnatury
            # mogą być po target jeśli pool nadal aktywna)
            if bt > end_ts_floor:
                continue

            # starszy niż początek okna → stop całkowicie
            if bt < start_ts_floor:
                return selected, total, confirmed, failed, page_num

            selected.append(sig)
            total += 1
            if sig.get("err") is None:
                confirmed += 1
            else:
                failed += 1

        # Jeśli ostatnia sygnatura na stronie jest przed oknem → koniec
        last_bt = sigs[-1].get("blockTime") or 0
        if last_bt < start_ts_floor:
            return selected, total, confirmed, failed, page_num

        # Mniej rekordów niż limit → to była ostatnia strona
        if len(sigs) < SIG_PAGE_LIMIT:
            return selected, total, confirmed, failed, page_num

        cursor = sigs[-1]["signature"]

    return selected, total, confirmed, failed, max_pages


async def _fetch_transactions_for_rows(
    rpc: SolanaRPC,
    signature_rows: list[dict],
    snap: PoolSnapshot,
    concurrency: int,
) -> tuple[list[tuple[dict, dict]], list[str]]:
    fetched_transactions: list[tuple[dict, dict]] = []
    tx_errors: list[str] = []

    semaphore = asyncio.Semaphore(max(1, concurrency))

    async def _fetch_one(sig_row: dict) -> tuple[dict, dict] | None:
        signature = sig_row.get("signature")
        if not isinstance(signature, str) or not signature:
            return None

        async with semaphore:
            try:
                tx = await rpc.get_transaction(signature)
            except Exception as exc:
                snap.tx_fetch_failed += 1
                tx_errors.append(f"{signature[:12]}:{exc}")
                return None

        if tx is None:
            snap.tx_fetch_failed += 1
            return None

        return sig_row, tx

    tasks = [
        asyncio.create_task(_fetch_one(sig_row))
        for sig_row in signature_rows
    ]

    if not tasks:
        return fetched_transactions, tx_errors

    for result in await asyncio.gather(*tasks):
        if result is not None:
            fetched_transactions.append(result)

    return fetched_transactions, tx_errors


async def fetch_pool_snapshot(
    rpc:       SolanaRPC,
    record:    dict,
    max_pages: int,
    target_ts_ms_override: int | None = None,
    window_ms_override: int | None = None,
    tx_fetch_concurrency: int = DEFAULT_TX_FETCH_CONCURRENCY,
    include_account_info: bool = DEFAULT_INCLUDE_ACCOUNT_INFO,
) -> PoolSnapshot:
    """
    Główna funkcja fetchu dla jednego rekordu z JSONL.
    """
    pool_id = record.get("pool_id", "")

    # ── 1. Oblicz docelowy timestamp ──────────────────────────────────────────
    try:
        window_spec = resolve_window_spec(
            record,
            target_ts_ms_override=target_ts_ms_override,
            window_ms_override=window_ms_override,
        )
        target_ts_ms = window_spec.end_ts_ms
        target_unix = target_ts_ms / 1000.0
    except Exception as exc:
        snap = PoolSnapshot(pool_id=pool_id, target_ts_ms=0, target_unix_ts=0.0)
        snap.fetch_error = f"timestamp_parse_error: {exc}"
        return snap

    snap = PoolSnapshot(
        pool_id      = pool_id,
        target_ts_ms = target_ts_ms,
        target_unix_ts = target_unix,
        target_source = window_spec.end_source,
        window_start_ts_ms = window_spec.start_ts_ms,
        window_end_ts_ms = window_spec.end_ts_ms,
        window_ms = window_spec.window_ms,
        window_source = window_spec.window_source,
        window_start_source = window_spec.start_source,
    )
    min_sol_threshold, min_sol_threshold_source = resolve_runtime_min_sol_threshold(
        record,
        window_spec.start_ts_ms,
        window_spec.end_ts_ms,
    )
    snap.min_sol_threshold = min_sol_threshold
    snap.min_sol_threshold_source = min_sol_threshold_source

    exact_signatures, exact_raw, exact_source = lookup_exact_window_signatures(
        record,
        pool_id,
        window_spec.start_ts_ms,
        window_spec.end_ts_ms,
    )
    snap.exact_window_emitted_unique_tx = (
        len(exact_signatures) if exact_signatures is not None else None
    )
    snap.exact_window_emitted_raw_tx = exact_raw
    snap.exact_window_source = exact_source
    exact_signature_rows = [
        {"signature": signature, "_source": "exact_window_emitted"}
        for signature in (exact_signatures or [])
    ]
    trusted_exact_signatures = {
        signature
        for signature in (exact_signatures or [])
        if isinstance(signature, str) and signature
    }

    t_start = time.monotonic()

    try:
        # ── 3. Zbierz surowe sygnatury pool_id w oknie ───────────────────────
        signature_rows, pool_total, pool_confirmed, pool_failed, pages = await _collect_signatures(
            rpc,
            pool_id,
            window_spec.start_ts_ms / 1000.0,
            window_spec.end_ts_ms / 1000.0,
            max_pages,
        )
        snap.pool_signature_total_tx = pool_total
        snap.pool_signature_confirmed_tx = pool_confirmed
        snap.pool_signature_failed_tx = pool_failed
        snap.sig_pages_fetched = pages

        # ── 4. Pobierz transakcje i odfiltruj non-trade ───────────────────────
        primary_signature_rows = _merge_signature_rows(
            signature_rows,
            exact_signature_rows,
        )
        fetched_transactions, tx_errors = await _fetch_transactions_for_rows(
            rpc,
            primary_signature_rows,
            snap,
            tx_fetch_concurrency,
        )

        trade_summary = summarize_trade_transactions(
            pool_id=pool_id,
            signature_rows=primary_signature_rows,
            transactions=fetched_transactions,
            base_mint_hint=record.get("base_mint"),
            min_sol_threshold=min_sol_threshold,
            trusted_exact_signatures=trusted_exact_signatures,
            window_start_ts_ms=window_spec.start_ts_ms,
            window_end_ts_ms=window_spec.end_ts_ms,
        )
        final_summary = trade_summary
        snap.base_mint = trade_summary["base_mint"]
        snap.base_mint_source = trade_summary["base_mint_source"]
        snap.total_tx = trade_summary["trade_signature_count"]
        snap.confirmed_tx = trade_summary["trade_signature_confirmed_count"]
        snap.failed_tx = trade_summary["trade_signature_failed_count"]
        snap.trade_signature_count = trade_summary["trade_signature_count"]
        snap.trade_signature_confirmed_count = trade_summary["trade_signature_confirmed_count"]
        snap.trade_signature_failed_count = trade_summary["trade_signature_failed_count"]
        snap.trade_event_total = trade_summary["trade_total"]
        snap.trade_event_confirmed = trade_summary["trade_confirmed"]
        snap.trade_event_failed = trade_summary["trade_failed"]
        snap.trade_event_bonus = trade_summary["trade_event_bonus"]
        snap.dust_filtered_tx = trade_summary["dust_filtered_tx"]
        snap.dust_filtered_confirmed_tx = trade_summary["dust_filtered_confirmed_tx"]
        snap.dust_filtered_failed_tx = trade_summary["dust_filtered_failed_tx"]
        snap.trade_volume_unresolved_tx = trade_summary["trade_volume_unresolved_tx"]
        snap.non_trade_tx = trade_summary["non_trade"]
        snap.off_pool_tx_excluded = trade_summary["off_pool_excluded"]

        base_mint_hint = normalize_base_mint_hint(snap.base_mint or record.get("base_mint"))
        observed_target_tx = observed_comparable_tx_target(record)
        should_fetch_mint_side = (
            base_mint_hint is not None
            and (
                final_summary["classification_error"] is not None
                or final_summary["trade_signature_count"] < observed_target_tx
                or (
                    exact_signatures is not None
                    and len(exact_signatures) < observed_target_tx
                )
            )
        )
        if should_fetch_mint_side:
            mint_signature_rows, mint_total, mint_confirmed, mint_failed, _mint_pages = await _collect_signatures(
                rpc,
                base_mint_hint,
                window_spec.start_ts_ms / 1000.0,
                window_spec.end_ts_ms / 1000.0,
                max_pages,
            )
            snap.mint_signature_total_tx = mint_total
            snap.mint_signature_confirmed_tx = mint_confirmed
            snap.mint_signature_failed_tx = mint_failed

            merged_signature_rows = _merge_signature_rows(
                primary_signature_rows,
                mint_signature_rows,
            )
            if merged_signature_rows:
                fetched_signatures = {
                    row.get("signature")
                    for row, _tx in fetched_transactions
                    if isinstance(row.get("signature"), str)
                }
                missing_signature_rows = [
                    row
                    for row in merged_signature_rows
                    if row.get("signature") not in fetched_signatures
                ]
                merged_transactions = fetched_transactions
                merged_tx_errors: list[str] = []
                if missing_signature_rows:
                    extra_transactions, merged_tx_errors = await _fetch_transactions_for_rows(
                        rpc,
                        missing_signature_rows,
                        snap,
                        tx_fetch_concurrency,
                    )
                    merged_transactions = _merge_fetched_transactions(
                        fetched_transactions,
                        extra_transactions,
                    )
                merged_summary = summarize_trade_transactions(
                    pool_id=pool_id,
                    signature_rows=merged_signature_rows,
                    transactions=merged_transactions,
                    base_mint_hint=base_mint_hint,
                    require_pool_touch=True,
                    min_sol_threshold=min_sol_threshold,
                    trusted_exact_signatures=trusted_exact_signatures,
                    window_start_ts_ms=window_spec.start_ts_ms,
                    window_end_ts_ms=window_spec.end_ts_ms,
                )
                tx_errors.extend(merged_tx_errors)
                if (
                    merged_summary["trade_signature_confirmed_count"] > snap.confirmed_tx
                    or merged_summary["trade_signature_count"] > snap.total_tx
                    or (
                        final_summary["classification_error"] is not None
                        and merged_summary["classification_error"] is None
                    )
                ):
                    final_summary = merged_summary
                    snap.base_mint = merged_summary["base_mint"]
                    snap.base_mint_source = merged_summary["base_mint_source"]
                    snap.total_tx = merged_summary["trade_signature_count"]
                    snap.confirmed_tx = merged_summary["trade_signature_confirmed_count"]
                    snap.failed_tx = merged_summary["trade_signature_failed_count"]
                    snap.trade_signature_count = merged_summary["trade_signature_count"]
                    snap.trade_signature_confirmed_count = merged_summary["trade_signature_confirmed_count"]
                    snap.trade_signature_failed_count = merged_summary["trade_signature_failed_count"]
                    snap.trade_event_total = merged_summary["trade_total"]
                    snap.trade_event_confirmed = merged_summary["trade_confirmed"]
                    snap.trade_event_failed = merged_summary["trade_failed"]
                    snap.trade_event_bonus = merged_summary["trade_event_bonus"]
                    snap.dust_filtered_tx = merged_summary["dust_filtered_tx"]
                    snap.dust_filtered_confirmed_tx = merged_summary["dust_filtered_confirmed_tx"]
                    snap.dust_filtered_failed_tx = merged_summary["dust_filtered_failed_tx"]
                    snap.trade_volume_unresolved_tx = merged_summary["trade_volume_unresolved_tx"]
                    snap.non_trade_tx = merged_summary["non_trade"]
                    snap.off_pool_tx_excluded = merged_summary["off_pool_excluded"]

        if final_summary["classification_error"]:
            snap.fetch_error = final_summary["classification_error"]
        elif snap.total_tx > 0:
            snap.fetch_error = None
        elif snap.tx_fetch_failed > 0 and not fetched_transactions:
            sample = tx_errors[0] if tx_errors else "tx_fetch_returned_none"
            snap.fetch_error = f"tx_fetch_failed_all:{sample}"
        elif snap.tx_fetch_failed > 0 and snap.total_tx == 0:
            sample = tx_errors[0] if tx_errors else "tx_fetch_partial"
            snap.fetch_error = f"tx_fetch_incomplete:{sample}"

        # ── 5. Account info (bonding-curve / pool state) ──────────────────────
        #
        # UWAGA: standardowy RPC zwraca AKTUALNY stan konta, nie historyczny.
        # Dane historyczne (market cap, reserves) wymagają archival/snapshot RPC
        # (np. Triton, Helius archive, własny validator z --no-snapshot-fetch).
        # Jeśli masz archival endpoint, ustaw go jako --rpc.
        #
        # Implementacja: pobieramy account info i zapisujemy surowe dane b64.
        # Parsowanie układu PumpFun BondingCurve / PumpSwap Pool Account
        # można dołączyć późnie, gdy będzie znany konkretny layout.

        if include_account_info:
            try:
                acc_result = await rpc.get_account_info(pool_id)
                if acc_result and acc_result.get("value"):
                    val = acc_result["value"]
                    snap.account_lamports = val.get("lamports")
                    snap.account_owner    = val.get("owner")
                    snap.account_exec     = val.get("executable")
                    raw_data = val.get("data")
                    if isinstance(raw_data, list):
                        snap.account_data_b64 = raw_data[0]   # base64 encoded
                    elif isinstance(raw_data, str):
                        snap.account_data_b64 = raw_data
                    ctx = acc_result.get("context")
                    if ctx:
                        snap.account_slot = ctx.get("slot")
            except Exception as acc_exc:
                log.debug("account_info error for %s: %s", pool_id[:12], acc_exc)
                # Non-fatal — kontynuujemy bez account data

    except Exception as exc:
        if snap.fetch_error is None:
            snap.fetch_error = str(exc)

    snap.rpc_latency_ms = round((time.monotonic() - t_start) * 1000, 1)
    return snap


async def fetch_pool_snapshot_resilient(
    rpc: SolanaRPC,
    record: dict,
    max_pages: int,
    target_ts_ms_override: int | None = None,
    window_ms_override: int | None = None,
    max_attempts: int = MAX_SNAPSHOT_RETRIES,
    tx_fetch_concurrency: int = DEFAULT_TX_FETCH_CONCURRENCY,
    include_account_info: bool = DEFAULT_INCLUDE_ACCOUNT_INFO,
) -> PoolSnapshot:
    last_snap: PoolSnapshot | None = None
    pool_id = str(record.get("pool_id", "?"))

    for attempt in range(1, max_attempts + 1):
        snap = await fetch_pool_snapshot(
            rpc,
            record,
            max_pages=max_pages,
            target_ts_ms_override=target_ts_ms_override,
            window_ms_override=window_ms_override,
            tx_fetch_concurrency=tx_fetch_concurrency,
            include_account_info=include_account_info,
        )
        last_snap = snap

        if snapshot_has_numeric_coverage(snap):
            if attempt > 1:
                log.info(
                    "Pool %s odzyskana w próbie %d/%d (trade=%d confirmed=%d)",
                    pool_id[:16],
                    attempt,
                    max_attempts,
                    snap.total_tx,
                    snap.confirmed_tx,
                )
            return snap

        if not snapshot_requires_retry(record, snap):
            return snap

        if attempt < max_attempts:
            wait_s = _retry_backoff_s(attempt)
            log.warning(
                "Pool %s bez coverage w próbie %d/%d; retry za %.2fs | raw=%d confirmed_raw=%d trade=%d tx_fetch_failed=%d err=%s",
                pool_id[:16],
                attempt,
                max_attempts,
                wait_s,
                snap.pool_signature_total_tx,
                snap.pool_signature_confirmed_tx,
                snap.total_tx,
                snap.tx_fetch_failed,
                snap.fetch_error or "-",
            )
            await asyncio.sleep(wait_s)

    assert last_snap is not None

    observed = observed_tx_metrics_from_record(record)
    observed_total = observed.total_tx_evaluated
    if (
        observed_total > 0
        and last_snap.pool_signature_total_tx == 0
        and last_snap.total_tx == 0
        and last_snap.fetch_error is None
    ):
        last_snap.fetch_error = "no_rpc_signatures_in_window"

    return last_snap


# ══════════════════════════════════════════════════════════════════════════════
# MERGE & COVERAGE RATIO
# ══════════════════════════════════════════════════════════════════════════════

def build_output_record(record: dict, snap: PoolSnapshot) -> dict:
    """
    Tworzy wyjściowy rekord: pola RPC na początku, potem oryginalne dane.

    coverage_ratio =
        observed_total_tx_evaluated (Gatekeeper total non-dust tx) /
        rpc_total_tx (on-chain total non-dust trade signatures)
    """
    observed = observed_tx_metrics_from_record(record)
    observed_tx_total: int = observed.total_tx_evaluated
    rpc_total_tx:       int = snap.total_tx
    rpc_confirmed_tx:   int = snap.confirmed_tx
    rpc_failed_tx:      int = snap.failed_tx
    observed_comparable_tx = observed_tx_total
    coverage_ratio_basis = "observed_total_non_dust_vs_rpc_total_non_dust"
    coverage_denominator = rpc_total_tx
    coverage_denominator_status = "rpc_total_tx"

    if coverage_denominator > 0:
        coverage_ratio_raw = round(observed_comparable_tx / coverage_denominator, 6)
        coverage_ratio_clamped = round(min(1.0, coverage_ratio_raw), 6)
        coverage_ratio = coverage_ratio_clamped
        if coverage_ratio_raw > 1.0:
            coverage_ratio_status = "overflow_observed_gt_onchain"
        else:
            coverage_ratio = coverage_ratio_raw
            coverage_ratio_status = "ok"
    else:
        if observed_comparable_tx == 0:
            coverage_ratio_raw = 1.0
            coverage_ratio = 1.0
            coverage_ratio_clamped = 1.0
            coverage_ratio_status = "ok"
        else:
            coverage_ratio_raw = None
            coverage_ratio = None
            coverage_ratio_clamped = None
            coverage_ratio_status = "missing_rpc_total_tx"

    coverage_ratio_all_tx_raw = coverage_ratio_raw
    coverage_ratio_all_tx = coverage_ratio
    coverage_ratio_all_tx_clamped = coverage_ratio_clamped
    coverage_ratio_all_tx_status = coverage_ratio_status

    coverage_attribution_blockers: list[str] = []
    if snap.min_sol_threshold_source != "record.min_sol_threshold":
        coverage_attribution_blockers.append("threshold_inferred")
    if snap.window_start_source != "record.observation_start_ts_ms":
        coverage_attribution_blockers.append("assessment_start_fallback")
    if snap.target_source != "record.observation_end_ts_ms":
        coverage_attribution_blockers.append("assessment_cutoff_fallback")
    if snap.exact_window_emitted_unique_tx is None:
        coverage_attribution_blockers.append("missing_exact_window_emissions")
    if snap.trade_volume_unresolved_tx > 0:
        coverage_attribution_blockers.append("unresolved_rpc_trade_volume")
    if snap.tx_fetch_failed > 0 or snap.fetch_error is not None:
        coverage_attribution_blockers.append("rpc_snapshot_incomplete")

    onchain_seen_total = snap.total_tx + snap.dust_filtered_tx
    gatekeeper_seen_total = observed.gatekeeper_seen_total
    coverage_attribution_tx_delta: int | None = None

    if coverage_attribution_blockers:
        coverage_attribution_status = "ambiguous_comparator_or_runtime"
    else:
        emitted_unique_tx = int(snap.exact_window_emitted_unique_tx or 0)
        if emitted_unique_tx < onchain_seen_total:
            coverage_attribution_status = "confirmed_runtime_emission_gap"
            coverage_attribution_tx_delta = onchain_seen_total - emitted_unique_tx
        elif emitted_unique_tx > onchain_seen_total:
            coverage_attribution_status = "confirmed_comparator_trade_classification_gap"
            coverage_attribution_tx_delta = emitted_unique_tx - onchain_seen_total
        elif gatekeeper_seen_total < emitted_unique_tx:
            coverage_attribution_status = "confirmed_gatekeeper_post_emission_gap"
            coverage_attribution_tx_delta = emitted_unique_tx - gatekeeper_seen_total
        elif gatekeeper_seen_total > emitted_unique_tx:
            coverage_attribution_status = "confirmed_observed_gt_emitted"
            coverage_attribution_tx_delta = gatekeeper_seen_total - emitted_unique_tx
        elif (
            observed.total_tx_evaluated == snap.total_tx
            and observed.dust_filtered_count == snap.dust_filtered_tx
        ):
            if coverage_ratio_status == "ok":
                coverage_attribution_status = "confirmed_full_match"
                coverage_attribution_tx_delta = 0
            else:
                coverage_attribution_status = "confirmed_observed_gt_onchain"
                coverage_attribution_tx_delta = observed.total_tx_evaluated - snap.total_tx
        else:
            coverage_attribution_status = "confirmed_volume_split_mismatch"
            coverage_attribution_tx_delta = observed.total_tx_evaluated - snap.total_tx

    if (
        observed.split_available
        and observed.confirmed_tx_evaluated is not None
    ):
        if rpc_confirmed_tx > 0:
            coverage_ratio_confirmed_only_raw = round(
                observed.confirmed_tx_evaluated / rpc_confirmed_tx,
                6,
            )
            coverage_ratio_confirmed_only_clamped = round(
                min(1.0, coverage_ratio_confirmed_only_raw),
                6,
            )
            coverage_ratio_confirmed_only = coverage_ratio_confirmed_only_clamped
            if coverage_ratio_confirmed_only_raw > 1.0:
                coverage_ratio_confirmed_only_status = "overflow_observed_confirmed_gt_onchain_confirmed"
            else:
                coverage_ratio_confirmed_only = coverage_ratio_confirmed_only_raw
                coverage_ratio_confirmed_only_status = "ok"
        elif observed.confirmed_tx_evaluated == 0:
            coverage_ratio_confirmed_only_raw = 1.0
            coverage_ratio_confirmed_only = 1.0
            coverage_ratio_confirmed_only_clamped = 1.0
            coverage_ratio_confirmed_only_status = "ok"
        else:
            coverage_ratio_confirmed_only_raw = None
            coverage_ratio_confirmed_only = None
            coverage_ratio_confirmed_only_clamped = None
            coverage_ratio_confirmed_only_status = "missing_rpc_confirmed_tx"
    else:
        coverage_ratio_confirmed_only_raw = None
        coverage_ratio_confirmed_only = None
        coverage_ratio_confirmed_only_clamped = None
        if not observed.split_available:
            coverage_ratio_confirmed_only_status = "missing_observed_success_failed_split"
        elif rpc_confirmed_tx <= 0:
            coverage_ratio_confirmed_only_status = "missing_rpc_confirmed_tx"
        else:
            coverage_ratio_confirmed_only_status = "missing_observed_confirmed_tx"

    if (
        observed.split_available
        and observed.failed_tx_evaluated is not None
    ):
        if rpc_failed_tx > 0:
            coverage_ratio_failed_only_raw = round(
                observed.failed_tx_evaluated / rpc_failed_tx,
                6,
            )
            coverage_ratio_failed_only_clamped = round(
                min(1.0, coverage_ratio_failed_only_raw),
                6,
            )
            coverage_ratio_failed_only = coverage_ratio_failed_only_clamped
            if coverage_ratio_failed_only_raw > 1.0:
                coverage_ratio_failed_only_status = "overflow_observed_failed_gt_onchain_failed"
            else:
                coverage_ratio_failed_only = coverage_ratio_failed_only_raw
                coverage_ratio_failed_only_status = "ok"
        elif observed.failed_tx_evaluated == 0:
            coverage_ratio_failed_only_raw = 1.0
            coverage_ratio_failed_only = 1.0
            coverage_ratio_failed_only_clamped = 1.0
            coverage_ratio_failed_only_status = "ok"
        else:
            coverage_ratio_failed_only_raw = None
            coverage_ratio_failed_only = None
            coverage_ratio_failed_only_clamped = None
            coverage_ratio_failed_only_status = "missing_rpc_failed_tx"
    else:
        coverage_ratio_failed_only_raw = None
        coverage_ratio_failed_only = None
        coverage_ratio_failed_only_clamped = None
        if not observed.split_available:
            coverage_ratio_failed_only_status = "missing_observed_success_failed_split"
        elif rpc_failed_tx <= 0:
            coverage_ratio_failed_only_status = "missing_rpc_failed_tx"
        else:
            coverage_ratio_failed_only_status = "missing_observed_failed_tx"

    rpc_prefix: dict = {
        # ── metryka główna ────────────────────────────────────────────────────
        "coverage_ratio":           coverage_ratio,
        "coverage_ratio_status":    coverage_ratio_status,
        "coverage_ratio_basis":     coverage_ratio_basis,
        "coverage_ratio_observed_comparable_tx": observed_comparable_tx,
        "coverage_ratio_denominator": coverage_denominator,
        "coverage_ratio_denominator_source": coverage_denominator_status,
        "coverage_ratio_all_tx":    coverage_ratio_all_tx,
        "coverage_ratio_all_tx_status": coverage_ratio_all_tx_status,
        "coverage_ratio_confirmed_only": coverage_ratio_confirmed_only,
        "coverage_ratio_confirmed_only_status": coverage_ratio_confirmed_only_status,
        "coverage_ratio_failed_only": coverage_ratio_failed_only,
        "coverage_ratio_failed_only_status": coverage_ratio_failed_only_status,
        "coverage_ratio_raw":       coverage_ratio_raw,
        "coverage_ratio_all_tx_raw": coverage_ratio_all_tx_raw,
        "coverage_ratio_confirmed_only_raw": coverage_ratio_confirmed_only_raw,
        "coverage_ratio_failed_only_raw": coverage_ratio_failed_only_raw,
        "coverage_ratio_clamped":   coverage_ratio_clamped,
        "coverage_ratio_all_tx_clamped": coverage_ratio_all_tx_clamped,
        "coverage_ratio_confirmed_only_clamped": coverage_ratio_confirmed_only_clamped,
        "coverage_ratio_failed_only_clamped": coverage_ratio_failed_only_clamped,
        "coverage_attribution_status": coverage_attribution_status,
        "coverage_attribution_tx_delta": coverage_attribution_tx_delta,
        "coverage_attribution_blockers": coverage_attribution_blockers,

        # ── observed-side semantics ──────────────────────────────────────────
        "observed_total_tx_evaluated": observed.total_tx_evaluated,
        "observed_dust_filtered_count": observed.dust_filtered_count,
        "observed_gatekeeper_seen_total_tx": observed.gatekeeper_seen_total,
        "observed_unique_tx_evaluated": observed.unique_tx_evaluated,
        "observed_confirmed_tx_evaluated": observed.confirmed_tx_evaluated,
        "observed_failed_tx_evaluated": observed.failed_tx_evaluated,
        "observed_tx_split_available": observed.split_available,
        "observed_tx_split_consistent": observed.split_consistent,
        "observed_tx_split_source": observed.split_source,

        # ── parametry pomiaru ─────────────────────────────────────────────────
        "rpc_target_ts_ms":         snap.target_ts_ms,
        "rpc_target_unix_ts":       snap.target_unix_ts,
        "rpc_target_source":        snap.target_source,
        "rpc_window_start_ts_ms":   snap.window_start_ts_ms,
        "rpc_window_end_ts_ms":     snap.window_end_ts_ms,
        "rpc_window_ms":            snap.window_ms,
        "rpc_window_source":        snap.window_source,
        "rpc_window_start_source":  snap.window_start_source,
        "rpc_base_mint":            snap.base_mint,
        "rpc_base_mint_source":     snap.base_mint_source,
        "rpc_min_sol_threshold":    snap.min_sol_threshold,
        "rpc_min_sol_threshold_source": snap.min_sol_threshold_source,

        # ── dane tx: trade-only (wyrównane do PoolTransaction Gatekeepera) ───
        "rpc_total_tx":             snap.total_tx,
        "rpc_confirmed_tx":         snap.confirmed_tx,
        "rpc_failed_tx":            snap.failed_tx,
        "rpc_trade_signature_count": snap.trade_signature_count,
        "rpc_trade_signature_confirmed_count": snap.trade_signature_confirmed_count,
        "rpc_trade_signature_failed_count": snap.trade_signature_failed_count,
        "rpc_trade_event_total":    snap.trade_event_total,
        "rpc_trade_event_confirmed": snap.trade_event_confirmed,
        "rpc_trade_event_failed":   snap.trade_event_failed,
        "rpc_trade_event_bonus":   snap.trade_event_bonus,
        "exact_window_emitted_unique_tx": snap.exact_window_emitted_unique_tx,
        "exact_window_emitted_raw_tx": snap.exact_window_emitted_raw_tx,
        "exact_window_source": snap.exact_window_source,
        "rpc_dust_filtered_tx":     snap.dust_filtered_tx,
        "rpc_dust_filtered_confirmed_tx": snap.dust_filtered_confirmed_tx,
        "rpc_dust_filtered_failed_tx": snap.dust_filtered_failed_tx,
        "rpc_trade_volume_unresolved_tx": snap.trade_volume_unresolved_tx,
        "rpc_non_trade_tx":         snap.non_trade_tx,
        "rpc_tx_fetch_failed":      snap.tx_fetch_failed,

        # ── dane tx: surowe sygnatury dla pool_id (diagnostyka) ──────────────
        "rpc_pool_signature_total_tx":        snap.pool_signature_total_tx,
        "rpc_pool_signature_confirmed_tx":    snap.pool_signature_confirmed_tx,
        "rpc_pool_signature_failed_tx":       snap.pool_signature_failed_tx,
        "rpc_mint_signature_total_tx":        snap.mint_signature_total_tx,
        "rpc_mint_signature_confirmed_tx":    snap.mint_signature_confirmed_tx,
        "rpc_mint_signature_failed_tx":       snap.mint_signature_failed_tx,
        "rpc_sig_pages_fetched":    snap.sig_pages_fetched,
        "rpc_off_pool_tx_excluded": snap.off_pool_tx_excluded,

        # ── account state ─────────────────────────────────────────────────────
        "rpc_account_slot":         snap.account_slot,
        "rpc_account_lamports":     snap.account_lamports,
        "rpc_account_owner":        snap.account_owner,
        "rpc_account_executable":   snap.account_exec,
        # rpc_account_data_b64 celowo pominięty — surowe bajty konta (base64)
        # nie wnoszą wartości analitycznej i zaśmiecają plik (ciągi AAAA...)

        # ── meta ──────────────────────────────────────────────────────────────
        "rpc_latency_ms":           snap.rpc_latency_ms,
        "rpc_fetch_error":          snap.fetch_error,
    }

    rpc_nullable_keys = {
        "coverage_ratio",
        "coverage_ratio_basis",
        "coverage_ratio_observed_comparable_tx",
        "coverage_ratio_denominator",
        "coverage_ratio_denominator_source",
        "coverage_ratio_all_tx",
        "coverage_ratio_all_tx_status",
        "coverage_ratio_confirmed_only",
        "coverage_ratio_status",
        "coverage_ratio_confirmed_only_status",
        "coverage_ratio_failed_only",
        "coverage_ratio_failed_only_status",
        "coverage_ratio_raw",
        "coverage_ratio_all_tx_raw",
        "coverage_ratio_confirmed_only_raw",
        "coverage_ratio_failed_only_raw",
        "coverage_ratio_clamped",
        "coverage_ratio_all_tx_clamped",
        "coverage_ratio_confirmed_only_clamped",
        "coverage_ratio_failed_only_clamped",
        "coverage_attribution_status",
        "coverage_attribution_tx_delta",
        "coverage_attribution_blockers",
        "observed_confirmed_tx_evaluated",
        "observed_failed_tx_evaluated",
        "observed_unique_tx_evaluated",
        "observed_tx_split_source",
        "rpc_min_sol_threshold",
        "rpc_min_sol_threshold_source",
        "rpc_target_source",
        "exact_window_emitted_unique_tx",
        "exact_window_emitted_raw_tx",
        "exact_window_source",
    }
    rpc_prefix = {
        k: v
        for k, v in rpc_prefix.items()
        if v is not None or k in rpc_nullable_keys
    }

    stale_prefixes = ("rpc_",)
    stale_explicit_keys = {
        "coverage_ratio",
        "coverage_ratio_basis",
        "coverage_ratio_observed_comparable_tx",
        "coverage_ratio_denominator",
        "coverage_ratio_denominator_source",
        "coverage_ratio_all_tx",
        "coverage_ratio_all_tx_status",
        "coverage_ratio_confirmed_only",
        "coverage_ratio_raw",
        "coverage_ratio_all_tx_raw",
        "coverage_ratio_confirmed_only_raw",
        "coverage_ratio_clamped",
        "coverage_ratio_all_tx_clamped",
        "coverage_ratio_confirmed_only_clamped",
        "coverage_attribution_status",
        "coverage_attribution_tx_delta",
        "coverage_attribution_blockers",
        "avg_coverage",
    }
    clean_record = {
        k: v
        for k, v in record.items()
        if not any(k.startswith(prefix) for prefix in stale_prefixes)
        and k not in stale_explicit_keys
    }

    # Zachowaj oryginalny rekord, ale pozwól świeżo wyliczonym polom RPC wygrać.
    return {**clean_record, **rpc_prefix}


def build_output_header(
    results: list[dict | None],
    *,
    unresolved_records: list[str],
    overflow_records: list[str],
    input_path: Path,
    selection_meta: dict[str, Any],
    generated_at: str | None = None,
) -> dict[str, Any]:
    coverage_values: list[float] = []
    coverage_status_counts = Counter(
        result.get("coverage_ratio_status", "unknown")
        for result in results
        if result is not None
    )
    coverage_attribution_status_counts = Counter(
        result.get("coverage_attribution_status", "unknown")
        for result in results
        if result is not None
    )
    warning_count = 0
    for result in results:
        if result is None:
            continue
        coverage_value = coverage_value_for_header(result)
        if coverage_value is not None:
            coverage_values.append(coverage_value)
        if (
            result.get("rpc_fetch_error") is None
            and coverage_status_is_warning(result.get("coverage_ratio_status"))
        ):
            warning_count += 1

    avg_coverage = sum(coverage_values) / len(coverage_values) if coverage_values else None
    header = {
        "avg_coverage": avg_coverage,
        "coverage_complete": len(unresolved_records) == 0,
        "unresolved_count": len(unresolved_records),
        "warning_count": warning_count,
        "coverage_status_counts": dict(coverage_status_counts),
        "coverage_attribution_status_counts": dict(coverage_attribution_status_counts),
        "generated_at": generated_at or datetime.now(timezone.utc).isoformat(),
        "source_input_path": str(input_path.resolve()),
    }
    header.update(selection_meta)
    return header


def coverage_status_is_resolved(status: object) -> bool:
    return status in {
        "ok",
        "overflow_observed_gt_onchain",
        "overflow_observed_gt_onchain_confirmed",
    }


def coverage_status_is_warning(status: object) -> bool:
    return status in {
        "overflow_observed_gt_onchain",
        "overflow_observed_gt_onchain_confirmed",
    }


def coverage_value_for_header(result: dict[str, Any]) -> float | None:
    coverage_ratio = result.get("coverage_ratio")
    if isinstance(coverage_ratio, (int, float)) and not isinstance(coverage_ratio, bool):
        return float(coverage_ratio)

    if not coverage_status_is_resolved(result.get("coverage_ratio_status")):
        return None

    coverage_ratio_clamped = result.get("coverage_ratio_clamped")
    if isinstance(coverage_ratio_clamped, (int, float)) and not isinstance(coverage_ratio_clamped, bool):
        return float(coverage_ratio_clamped)

    return None


def coverage_status_is_hard_error(status: object) -> bool:
    return status in {None, "missing_rpc_total_tx"}


def format_progress_success_status(merged: dict, snap: PoolSnapshot) -> str:
    exact_display = (
        f"{snap.exact_window_emitted_unique_tx:>3}"
        if snap.exact_window_emitted_unique_tx is not None
        else " na"
    )
    return (
        f"obs={merged.get('coverage_ratio_observed_comparable_tx', merged.get('observed_total_tx_evaluated', 0)):>3} rpc_trade={snap.total_tx:>3}"
        f" ok={snap.confirmed_tx:>3} fail={snap.failed_tx:>3}"
        f" exact={exact_display} raw={snap.pool_signature_total_tx:>3} non_trade={snap.non_trade_tx:>3}"
        f"  cov={merged.get('coverage_ratio', 'N/A')}"
        f" cov_all={merged.get('coverage_ratio_all_tx', 'N/A')}"
        f" [{merged.get('coverage_ratio_status', '-')}]"
        f"  {snap.rpc_latency_ms:.0f}ms"
    )


def format_progress_issue_status(merged: dict, snap: PoolSnapshot) -> str:
    coverage_status = merged.get("coverage_ratio_status")
    if snap.fetch_error is not None:
        return f"✗ {snap.fetch_error[:80]}"
    if coverage_status in {
        "overflow_observed_gt_onchain",
        "overflow_observed_gt_onchain_confirmed",
    }:
        return (
            f"! {format_progress_success_status(merged, snap)}"
            f" raw_cov={merged.get('coverage_ratio_raw', 'N/A')}"
        )
    return (
        f"✗ {coverage_status or 'coverage_missing'}"
        f" raw={snap.pool_signature_total_tx:>3} obs={merged.get('coverage_ratio_observed_comparable_tx', merged.get('observed_total_tx_evaluated', 0)):>3} rpc_trade={snap.total_tx:>3}"
        f" ok={snap.confirmed_tx:>3} fail={snap.failed_tx:>3}"
        f"  {snap.rpc_latency_ms:.0f}ms"
    )


def build_output_filename(now: datetime | None = None) -> str:
    current = now or datetime.now()
    return current.strftime("coverage_%Y%m%dT%H%M%S_%f.jsonl")


# ══════════════════════════════════════════════════════════════════════════════
# PROGRESS TRACKER
# ══════════════════════════════════════════════════════════════════════════════

class ProgressTracker:
    __slots__ = ("total", "_done", "_errors", "_warnings", "_t0", "_lock")

    def __init__(self, total: int) -> None:
        self.total   = total
        self._done   = 0
        self._errors = 0
        self._warnings = 0
        self._t0     = time.monotonic()
        self._lock   = asyncio.Lock()

    async def record(self, *, error: bool = False, warning: bool = False) -> tuple[int, int, int, float]:
        async with self._lock:
            self._done += 1
            if error:
                self._errors += 1
            if warning:
                self._warnings += 1
            elapsed = time.monotonic() - self._t0
            return self._done, self._errors, self._warnings, elapsed

    @property
    def done(self) -> int:
        return self._done

    @property
    def errors(self) -> int:
        return self._errors

    @property
    def warnings(self) -> int:
        return self._warnings


# ══════════════════════════════════════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════════════════════════════════════

async def run(args: argparse.Namespace) -> None:
    global _INPUT_BASE_DIR, _EXTRA_SYSTEM_LOG_DIRS, _EXPLICIT_GHOST_BRAIN_CONFIG_PATH

    if aiohttp is None:
        raise RuntimeError("Missing dependency: aiohttp. Install with `pip install aiohttp`.")

    if args.tx_rps > args.rps:
        raise RuntimeError("--tx-rps nie może być większe niż --rps")

    rpc_host = (urlparse(args.rpc).hostname or "").lower()
    if _is_nln_rpc_url(args.rpc) or rpc_host == ALCHEMY_RPC_HOST:
        if args.rps > SOLANA_MAINNET_PROVIDER_GUIDELINE_RPS:
            log.warning(
                "Ustawione --rps=%.1f przekracza projektowy throughput guideline dla Solana Mainnet (%.0f RPS). Spodziewaj się 429.",
                args.rps,
                SOLANA_MAINNET_PROVIDER_GUIDELINE_RPS,
            )
        if args.tx_rps > SOLANA_MAINNET_PROVIDER_GUIDELINE_RPS:
            log.warning(
                "Ustawione --tx-rps=%.1f przekracza projektowy throughput guideline dla Solana Mainnet (%.0f RPS).",
                args.tx_rps,
                SOLANA_MAINNET_PROVIDER_GUIDELINE_RPS,
            )

    # ── 1. Wczytaj input ──────────────────────────────────────────────────────
    input_path = Path(args.input)
    if not input_path.exists():
        log.error("Plik wejściowy nie istnieje: %s", input_path)
        sys.exit(1)

    _INPUT_BASE_DIR = input_path.resolve().parent
    _EXTRA_SYSTEM_LOG_DIRS = [
        Path(path).resolve()
        for path in (args.system_log_dir or [])
    ]
    _EXPLICIT_GHOST_BRAIN_CONFIG_PATH = (
        Path(args.ghost_brain_config).resolve()
        if args.ghost_brain_config
        else None
    )

    system_log_dirs = _candidate_system_log_dirs()
    ghost_brain_config_paths = _candidate_ghost_brain_config_paths()
    if system_log_dirs:
        log.info("System log search dirs: %s", _format_paths_for_log(system_log_dirs))
    else:
        log.warning(
            "Brak wykrytych katalogów z system.log.* — exact-window recovery będzie niedostępny. "
            "Jeśli logi są poza standardowym layoutem, podaj --system-log-dir PATH."
        )
    if ghost_brain_config_paths:
        log.info(
            "ghost_brain_config candidates: %s",
            _format_paths_for_log(ghost_brain_config_paths),
        )

    records: list[dict] = []
    with open(input_path, encoding="utf-8") as fh:
        for lineno, raw in enumerate(fh, 1):
            raw = raw.strip()
            if not raw:
                continue
            try:
                records.append(json.loads(raw))
            except json.JSONDecodeError as exc:
                log.warning("Pomijam linię %d (JSON error): %s", lineno, exc)

    records, selection_meta = filter_input_records(
        records,
        since_ms=max(0, int(args.since_ms)),
        run_id=args.run_id,
    )

    if not records:
        log.error(
            "Brak prawidłowych rekordów po filtracji inputu: since_ms=%s run_id=%s",
            selection_meta["source_since_ms"],
            selection_meta["source_requested_run_id"],
        )
        sys.exit(1)

    log.info(
        "Wczytano %d rekordów z %s",
        selection_meta["source_selected_record_count"], input_path,
    )
    run_system_log_candidates = _collect_run_system_log_candidates(records)
    if run_system_log_candidates:
        displayed_paths = run_system_log_candidates[:8]
        suffix = " ..." if len(run_system_log_candidates) > len(displayed_paths) else ""
        log.info(
            "System log candidate files for selection: %d (%s%s)",
            len(run_system_log_candidates),
            _format_paths_for_log(displayed_paths),
            suffix,
        )
    else:
        log.warning(
            "Nie znaleziono żadnych pasujących system.log dla zakresu rekordów — exact-window recovery będzie niedostępny, "
            "więc create-tx mogą zaniżać rpc_trade i sztucznie pompować overflow_observed_gt_onchain."
        )
    log.info(
        "Input selection: loaded=%d selected=%d since_ms=%s run_id=%s skipped_before_since_ms=%d skipped_missing_cohort_ts=%d skipped_run_id_mismatch=%d",
        selection_meta["source_loaded_record_count"],
        selection_meta["source_selected_record_count"],
        selection_meta["source_since_ms"],
        selection_meta["source_requested_run_id"],
        selection_meta["source_skipped_before_since_ms"],
        selection_meta["source_skipped_missing_cohort_ts"],
        selection_meta["source_skipped_run_id_mismatch"],
    )
    if selection_meta["source_cohort_min_ts_ms"] is not None:
        log.info(
            "Selected cohort ts range: %d..%d",
            selection_meta["source_cohort_min_ts_ms"],
            selection_meta["source_cohort_max_ts_ms"],
        )
    if selection_meta["source_run_id_count"] > 0:
        log.info(
            "Selected run_ids: %s%s",
            ",".join(selection_meta["source_run_ids_sample"]),
            "..."
            if selection_meta["source_run_id_count"] > len(selection_meta["source_run_ids_sample"])
            else "",
        )
    log.info(
        "RPC: %s  |  limit RPS: %.0f  |  tx_rps: %.0f  |  concurrency: %d  |  tx_fetch_concurrency: %d  |  tx_max_inflight: %d  |  account_info: %s  |  max_pages: %d",
        args.rpc, args.rps, args.tx_rps, args.concurrency, args.tx_fetch_concurrency, args.tx_max_inflight, args.include_account_info, args.max_pages,
    )
    if args.target_ts_ms is not None:
        log.info("Wymuszony target_ts_ms dla wszystkich rekordów: %d", args.target_ts_ms)
    if args.window_ms is not None:
        log.info("Wymuszone window_ms dla wszystkich rekordów: %d", args.window_ms)

    # ── 2. Przygotuj katalog i nazwę pliku wyjściowego ─────────────────────────
    out_dir  = Path(args.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    out_name = build_output_filename()
    out_path = out_dir / out_name
    partial_out_path = out_path.with_suffix(".partial.jsonl")
    log.info("Output: %s", out_path)
    log.info("Partial output journal: %s", partial_out_path)

    # ── 3. HTTP session + RPC + rate limiter ─────────────────────────────────
    connector = aiohttp.TCPConnector(
        limit                 = args.concurrency + 32,
        limit_per_host        = args.concurrency + 16,
        ttl_dns_cache         = 300,
        enable_cleanup_closed = True,
        force_close           = False,
    )
    session_timeout = aiohttp.ClientTimeout(total=30, connect=8)

    rate_limiter = RateLimiter(
        args.rps,
        capacity=min(max(1.0, args.rps), float(max(4, args.concurrency))),
        initial_tokens=1.0,
    )
    tx_rate_limiter = RateLimiter(
        args.tx_rps,
        capacity=min(max(1.0, args.tx_rps), float(max(1, args.tx_max_inflight))),
        initial_tokens=1.0,
    )
    sem          = asyncio.Semaphore(args.concurrency)
    progress     = ProgressTracker(len(records))
    disk_tx_cache = DiskTxCache(Path(args.tx_cache_dir))
    partial_writer = PartialResultWriter(partial_out_path)

    results: list[dict | None] = [None] * len(records)
    unresolved_records: list[str] = []
    overflow_records: list[str] = []

    # ── 4. Worker coroutine ───────────────────────────────────────────────────
    async def worker(idx: int, record: dict, rpc: SolanaRPC) -> None:
        async with sem:
            pool_id = record.get("pool_id", "???")

            snap = await fetch_pool_snapshot_resilient(
                rpc,
                record,
                max_pages=args.max_pages,
                target_ts_ms_override=args.target_ts_ms,
                window_ms_override=args.window_ms,
                max_attempts=args.snapshot_retries,
                tx_fetch_concurrency=args.tx_fetch_concurrency,
                include_account_info=args.include_account_info,
            )
            merged = build_output_record(record, snap)
            await partial_writer.append(merged)

            coverage_status = merged.get("coverage_ratio_status")
            coverage_resolved = coverage_status_is_resolved(coverage_status)
            hard_error = snap.fetch_error is not None or coverage_status_is_hard_error(coverage_status)
            warning = coverage_status_is_warning(coverage_status) and snap.fetch_error is None

            results[idx] = merged
            if hard_error:
                observed_total = observed_tx_metrics_from_record(record).total_tx_evaluated
                unresolved_records.append(
                    f"pool={pool_id} status={coverage_status or '-'} observed={observed_total} "
                    f"raw={snap.pool_signature_total_tx} confirmed_raw={snap.pool_signature_confirmed_tx} trade={snap.total_tx} "
                    f"tx_fetch_failed={snap.tx_fetch_failed} err={snap.fetch_error or '-'}"
                )
            elif warning:
                observed_total = observed_tx_metrics_from_record(record).total_tx_evaluated
                overflow_records.append(
                    f"pool={pool_id} observed={observed_total} raw_cov={merged.get('coverage_ratio_raw')} "
                    f"trade={snap.total_tx} raw={snap.pool_signature_total_tx}"
                )

            done, errs, warns, elapsed = await progress.record(error=hard_error, warning=warning)

            # ETA
            rate_done  = done / elapsed if elapsed > 0 else 1.0
            remaining  = (len(records) - done) / rate_done if rate_done > 0 else 0.0

            status = (
                format_progress_issue_status(merged, snap)
                if (hard_error or warning or not coverage_resolved)
                else format_progress_success_status(merged, snap)
            )

            log.info(
                "[%4d/%d | err=%d | warn=%d | ETA %ds]  %-44s  %s",
                done, len(records), errs, warns, int(remaining),
                pool_id[:44],
                status,
            )

    # ── 5. Uruchom wszystkie workery ──────────────────────────────────────────
    t0 = time.monotonic()

    # Headers celowo imitują przeglądarkę Chrome — Cloudflare stojący przed
    # dostawcami RPC potrafi blokować 403 żądania, których User-Agent / nagłówki
    # zdradzają automatyzację (bot/agent/rpc/solana itp.).  Neutralna nazwa + pełny zestaw
    # nagłówków browserowych omija WAF bez konieczności zmian po stronie whitelist.
    _CF_SAFE_HEADERS = {
        "Content-Type":    "application/json",
        "Accept":          "application/json, text/plain, */*",
        "User-Agent":      (
            "Mozilla/5.0 (X11; Linux x86_64) "
            "AppleWebKit/537.36 (KHTML, like Gecko) "
            "Chrome/122.0.0.0 Safari/537.36"
        ),
        "Accept-Language": "en-US,en;q=0.9",
        "Accept-Encoding": "gzip, deflate, br",
        "Connection":      "keep-alive",
        "Cache-Control":   "no-cache",
        "Pragma":          "no-cache",
        "Origin":          "https://explorer.solana.com",
        "Referer":         "https://explorer.solana.com/",
    }
    rpc_headers = dict(_CF_SAFE_HEADERS)
    rpc_headers.update(rpc_auth_headers(args.rpc))

    async with aiohttp.ClientSession(
        connector = connector,
        timeout   = session_timeout,
        headers   = rpc_headers,
    ) as session:
        rpc = SolanaRPC(
            args.rpc,
            rate_limiter,
            tx_rate_limiter,
            args.tx_max_inflight,
            session,
            disk_tx_cache,
        )
        await asyncio.gather(
            *[worker(i, r, rpc) for i, r in enumerate(records)]
        )

    partial_writer.close()

    total_elapsed = time.monotonic() - t0

    if unresolved_records:
        log.error("Pozostały rekordy z twardym błędem coverage: %d", len(unresolved_records))
        for item in unresolved_records[:25]:
            log.error("UNRESOLVED %s", item)
        log.warning(
            "Kontynuuję zapis częściowy: rekordy unresolved zostaną zapisane z rpc_fetch_error / null coverage zamiast abortować cały run."
        )

    if overflow_records:
        log.warning("Wykryto rekordy z overflow coverage (observed > on-chain): %d", len(overflow_records))
        for item in overflow_records[:25]:
            log.warning("OVERFLOW %s", item)

    # ── 6. Zapisz wyniki ──────────────────────────────────────────────────────
    header = build_output_header(
        results,
        unresolved_records=unresolved_records,
        overflow_records=overflow_records,
        input_path=input_path,
        selection_meta=selection_meta,
    )
    avg_coverage = header.get("avg_coverage")
    coverage_status_counts = Counter(header.get("coverage_status_counts", {}))
    coverage_attribution_status_counts = Counter(
        header.get("coverage_attribution_status_counts", {})
    )

    written = 0
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write(json.dumps(header, ensure_ascii=False, separators=(",", ":")) + "\n")
        for result in results:
            if result is not None:
                fh.write(
                    json.dumps(result, ensure_ascii=False, separators=(",", ":"))
                    + "\n"
                )
                written += 1

    # ── 7. Podsumowanie ───────────────────────────────────────────────────────
    ok_count  = written - progress.errors - progress.warnings
    err_count = progress.errors
    warn_count = progress.warnings

    log.info("━" * 60)
    log.info("Zakończono w %.1fs", total_elapsed)
    log.info("Zapisano:  %d rekordów → %s", written, out_path)
    log.info("OK: %d  |  Ostrzeżenia: %d  |  Błędy: %d", ok_count, warn_count, err_count)
    if avg_coverage is None:
        log.info("avg_coverage: null (brak rekordów z liczbowym coverage_ratio)")
    else:
        log.info("avg_coverage: %.6f", avg_coverage)
    for status, count in coverage_status_counts.most_common():
        log.info("coverage_status[%s]=%d", status, count)
    for status, count in coverage_attribution_status_counts.most_common():
        log.info("coverage_attribution[%s]=%d", status, count)
    log.info(
        "Wydajność: %.1f rekordów/s  (RPS limit: %.0f)",
        len(records) / total_elapsed if total_elapsed > 0 else 0,
        args.rps,
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description="PumpFun Pool Historical Coverage Scanner",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "input",
        nargs="?",
        default=DEFAULT_INPUT_PATH,
        help="Ścieżka do pliku JSONL z rekordami gatekeeper",
    )
    parser.add_argument(
        "--rpc",
        default=DEFAULT_RPC_URL,
        help="Solana JSON-RPC endpoint URL",
    )
    parser.add_argument(
        "--rps",
        type=float,
        default=DEFAULT_RPS,
        help="Maksymalna liczba wszystkich żądań RPC na sekundę",
    )
    parser.add_argument(
        "--concurrency",
        type=int,
        default=DEFAULT_CONCUR,
        help="Maksymalna liczba równoległych fetcherów",
    )
    parser.add_argument(
        "--tx-rps",
        type=float,
        default=DEFAULT_TX_RPS,
        help="Maksymalna liczba żądań getTransaction na sekundę (globalnie)",
    )
    parser.add_argument(
        "--tx-fetch-concurrency",
        type=int,
        default=DEFAULT_TX_FETCH_CONCURRENCY,
        help="Maksymalna liczba równoległych getTransaction w obrębie pojedynczego poola",
    )
    parser.add_argument(
        "--tx-max-inflight",
        type=int,
        default=DEFAULT_TX_INFLIGHT,
        help="Maksymalna liczba jednocześnie otwartych requestów getTransaction (globalnie)",
    )
    parser.add_argument(
        "--include-account-info",
        action="store_true",
        default=DEFAULT_INCLUDE_ACCOUNT_INFO,
        help="Pobieraj dodatkowo getAccountInfo dla poola (diagnostyka kosztem wydajności)",
    )
    parser.add_argument(
        "--max-pages",
        type=int,
        default=DEFAULT_MAXPAGES,
        help="Maksymalna liczba stron getSignaturesForAddress na pool",
    )
    parser.add_argument(
        "--snapshot-retries",
        type=int,
        default=MAX_SNAPSHOT_RETRIES,
        help="Liczba prób fetchu snapshotu dla jednego poola przed zapisem częściowym",
    )
    parser.add_argument(
        "--target-ts-ms",
        type=int,
        default=None,
        help=(
            "Jeśli podane, wymusza konkretny target timestamp w ms "
            "dla wszystkich rekordów zamiast końca okna wyliczanego z rekordu"
        ),
    )
    parser.add_argument(
        "--window-ms",
        type=int,
        default=None,
        help=(
            "Jeśli podane, liczy tx w oknie [t0, t0 + window_ms], gdzie t0 to "
            "first_seen_ts_ms / ab_t0_event_ts_ms / fallback z rekordu"
        ),
    )
    parser.add_argument(
        "--since-ms",
        type=int,
        default=0,
        help=(
            "Filtruje input do rekordów z kohorty >= since_ms, liczonej z "
            "observation_start_ts_ms / first_seen_ts_ms / ab_t0_event_ts_ms / event_time_ms / timestamp"
        ),
    )
    parser.add_argument(
        "--run-id",
        default=None,
        help="Opcjonalnie filtruje input do rekordów z konkretnym run_id, jeśli pole jest obecne w rekordzie.",
    )
    parser.add_argument(
        "--output-dir",
        default=str(OUTPUT_BASE),
        help="Katalog wyjściowy dla pliku coverage*.jsonl",
    )
    parser.add_argument(
        "--system-log-dir",
        action="append",
        default=[],
        help=(
            "Dodatkowy katalog do przeszukiwania po system.log.YYYY-MM-DD* "
            "(można podać wiele razy)."
        ),
    )
    parser.add_argument(
        "--ghost-brain-config",
        default=None,
        help=(
            "Opcjonalna ścieżka do ghost_brain_config.toml używana jako fallback "
            "dla min_sol_threshold."
        ),
    )
    parser.add_argument(
        "--tx-cache-dir",
        default=str(DEFAULT_TX_CACHE_DIR),
        help="Katalog dyskowego cache dla getTransaction między uruchomieniami",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        choices=["DEBUG", "INFO", "WARNING", "ERROR"],
        help="Poziom logowania",
    )

    args = parser.parse_args()
    logging.getLogger().setLevel(args.log_level)

    asyncio.run(run(args))


if __name__ == "__main__":
    main()
