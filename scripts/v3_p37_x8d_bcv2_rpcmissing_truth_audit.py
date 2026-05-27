#!/usr/bin/env python3
"""X8D-PR2A BCV2 RpcMissing / commitment / timing truth audit.

This is an audit-only tool. It performs current RPC visibility checks for the
unique working-builder `bonding_curve_v2` pubkeys produced by X8D-PR1. It does
not emit runtime evidence, does not change readiness policy, and does not imply
manifest readiness.
"""

from __future__ import annotations

import argparse
import base64
import csv
import json
import os
import re
import socket
import time
from collections import Counter
from pathlib import Path
from typing import Any
from urllib import error as urllib_error
from urllib import parse as urllib_parse
from urllib import request as urllib_request

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python <3.11 fallback
    tomllib = None  # type: ignore[assignment]


SCHEMA_VERSION = "x8d_pr2a_bcv2_rpcmissing_truth_audit_v1"
DEFAULT_COMMITMENTS = ("processed", "confirmed", "finalized")
DEFAULT_DELAYS_MS = (0, 250, 1000, 3000)
DEFAULT_CHUNK_SIZE = 100
DEFAULT_TIMEOUT_S = 20.0
BASE58_ALPHABET = set("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz")
CONFIG_VAR_RE = re.compile(r"^\$\{([^}]+)\}$")


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def load_toml(path: Path | None) -> dict[str, Any]:
    if path is None:
        return {}
    if tomllib is None:
        raise RuntimeError("tomllib unavailable; use Python 3.11+ or pass --rpc-url")
    with path.open("rb") as handle:
        return tomllib.load(handle)


def expand_config_value(value: Any) -> str | None:
    if not isinstance(value, str) or not value:
        return None
    match = CONFIG_VAR_RE.match(value)
    if match:
        expanded = os.environ.get(match.group(1))
        return expanded if expanded else None
    return value


def rpc_url_from_config(config: dict[str, Any]) -> str | None:
    candidates = [
        (((config.get("trigger") or {}).get("shadow_run") or {}).get("shadow_rpc_url")),
        ((config.get("trigger") or {}).get("rpc_url")),
        ((config.get("seer") or {}).get("rpc_endpoint")),
    ]
    for candidate in candidates:
        expanded = expand_config_value(candidate)
        if expanded:
            return expanded
    return None


def redact_url(url: str | None) -> str | None:
    if not url:
        return None
    parsed = urllib_parse.urlsplit(url)
    if not parsed.scheme or not parsed.netloc:
        return "***"
    return urllib_parse.urlunsplit((parsed.scheme, parsed.netloc, "/***", "", ""))


def parse_int_list(value: str, allowed: tuple[int, ...] | None = None) -> tuple[int, ...]:
    items = tuple(int(item.strip()) for item in value.split(",") if item.strip())
    if allowed is not None:
        unknown = [item for item in items if item not in allowed]
        if unknown:
            raise ValueError(f"unsupported values {unknown}; allowed={allowed}")
    return items


def parse_str_list(value: str, allowed: tuple[str, ...] | None = None) -> tuple[str, ...]:
    items = tuple(item.strip() for item in value.split(",") if item.strip())
    if allowed is not None:
        unknown = [item for item in items if item not in allowed]
        if unknown:
            raise ValueError(f"unsupported values {unknown}; allowed={allowed}")
    return items


def is_plausible_pubkey(pubkey: str) -> bool:
    return isinstance(pubkey, str) and 32 <= len(pubkey) <= 44 and all(ch in BASE58_ALPHABET for ch in pubkey)


def int_range_values(value: Any) -> tuple[int | None, int | None]:
    if not isinstance(value, list) or len(value) != 2:
        return None, None
    try:
        low = int(value[0]) if value[0] is not None else None
        high = int(value[1]) if value[1] is not None else None
    except (TypeError, ValueError):
        return None, None
    return low, high


def load_pubkey_contexts(x8d_pr1_json: Path) -> list[dict[str, Any]]:
    payload = load_json(x8d_pr1_json)
    if payload.get("schema") != "x8d_pr1_unique_bcv2_pubkey_join_v1":
        raise ValueError(f"unexpected X8D-PR1 schema: {payload.get('schema')}")
    contexts: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in payload.get("rows", []):
        pubkey = row.get("bcv2_pubkey")
        if not isinstance(pubkey, str) or not pubkey or pubkey in seen:
            continue
        seen.add(pubkey)
        observed_min, observed_max = int_range_values(row.get("observed_slot_range"))
        precheck_min, precheck_max = int_range_values(row.get("precheck_context_slot_range"))
        contexts.append(
            {
                "pubkey": pubkey,
                "x8d_pr1_primary_bucket": row.get("x8d_pr1_primary_bucket"),
                "observed_slot_min": observed_min,
                "observed_slot_max": observed_max,
                "precheck_context_slot_min": precheck_min,
                "precheck_context_slot_max": precheck_max,
                "same_pubkey_account_update": bool(row.get("same_pubkey_account_update")),
                "included_in_subscribe_inferred": bool(row.get("included_in_subscribe_inferred")),
                "dropped_over_cap_inferred": bool(row.get("dropped_over_cap_inferred")),
                "hydration_missing_error_classes": row.get("hydration_missing_error_classes") or {},
            }
        )
    return sorted(contexts, key=lambda item: item["pubkey"])


def account_data_len(data_field: Any) -> int | None:
    if isinstance(data_field, list) and data_field:
        encoded = data_field[0]
    elif isinstance(data_field, str):
        encoded = data_field
    else:
        return None
    if not isinstance(encoded, str):
        return None
    try:
        return len(base64.b64decode(encoded, validate=False))
    except Exception:
        return len(encoded)


def rpc_get_multiple_accounts(
    rpc_url: str,
    pubkeys: list[str],
    commitment: str,
    timeout_s: float,
) -> tuple[int | None, dict[str, dict[str, Any]]]:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getMultipleAccounts",
        "params": [
            pubkeys,
            {
                "encoding": "base64",
                "commitment": commitment,
            },
        ],
    }
    req = urllib_request.Request(
        rpc_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "User-Agent": "ghost-p37-x8d-bcv2-rpcmissing-truth-audit",
        },
        method="POST",
    )
    with urllib_request.urlopen(req, timeout=timeout_s) as response:
        body = json.loads(response.read().decode("utf-8"))
    if "error" in body:
        error = body["error"]
        code = error.get("code") if isinstance(error, dict) else None
        raise RuntimeError(f"rpc_error:{code if code is not None else 'unknown'}")
    result = body.get("result") or {}
    context_slot = (result.get("context") or {}).get("slot")
    values = result.get("value") or []
    rows: dict[str, dict[str, Any]] = {}
    for pubkey, account in zip(pubkeys, values):
        if account is None:
            rows[pubkey] = {"ready": False, "missing": True, "error_class": "missing_on_rpc"}
        else:
            data_len = account_data_len(account.get("data"))
            rows[pubkey] = {
                "ready": bool(account.get("owner")) and data_len is not None,
                "missing": False,
                "error_class": None if account.get("owner") and data_len is not None else "decode_error",
                "owner": account.get("owner"),
                "data_len": data_len,
            }
    for pubkey in pubkeys[len(values) :]:
        rows[pubkey] = {"ready": False, "missing": False, "error_class": "decode_error"}
    return int(context_slot) if context_slot is not None else None, rows


def chunked(items: list[str], size: int) -> list[list[str]]:
    return [items[index : index + size] for index in range(0, len(items), size)]


def error_class_from_exception(exc: BaseException) -> str:
    if isinstance(exc, (socket.timeout, TimeoutError)):
        return "provider_timeout"
    if isinstance(exc, urllib_error.URLError) and isinstance(exc.reason, (socket.timeout, TimeoutError)):
        return "provider_timeout"
    message = str(exc)
    if message.startswith("rpc_error:"):
        return message
    if isinstance(exc, urllib_error.HTTPError):
        return f"rpc_error:http_{exc.code}"
    return f"rpc_error:{exc.__class__.__name__}"


def context_base_row(context: dict[str, Any]) -> dict[str, Any]:
    return {
        "pubkey": context["pubkey"],
        "x8d_pr1_primary_bucket": context.get("x8d_pr1_primary_bucket"),
        "observed_slot_min": context.get("observed_slot_min"),
        "observed_slot_max": context.get("observed_slot_max"),
        "precheck_context_slot_min": context.get("precheck_context_slot_min"),
        "precheck_context_slot_max": context.get("precheck_context_slot_max"),
        "same_pubkey_account_update": context.get("same_pubkey_account_update", False),
        "included_in_subscribe_inferred": context.get("included_in_subscribe_inferred", False),
        "dropped_over_cap_inferred": context.get("dropped_over_cap_inferred", False),
    }


def attempt_row(
    context: dict[str, Any],
    provider_label: str,
    commitment: str,
    delay_ms: int,
    context_slot: int | None,
    latency_ms: int,
    age_ms: int,
    ready: bool,
    missing: bool,
    error_class: str | None,
    owner: str | None = None,
    data_len: int | None = None,
) -> dict[str, Any]:
    observed_max = context.get("observed_slot_max")
    age_slots = context_slot - observed_max if context_slot is not None and observed_max is not None else None
    row = context_base_row(context)
    row.update(
        {
            "provider_label": provider_label,
            "commitment": commitment,
            "delay_ms": delay_ms,
            "context_slot": context_slot,
            "age_slots": age_slots,
            "age_ms": age_ms,
            "latency_ms": latency_ms,
            "ready": ready,
            "missing": missing,
            "error_class": error_class,
            "owner": owner,
            "data_len": data_len,
        }
    )
    return row


def run_probe(
    contexts: list[dict[str, Any]],
    providers: list[dict[str, str | None]],
    commitments: tuple[str, ...],
    delays_ms: tuple[int, ...],
    chunk_size: int,
    timeout_s: float,
) -> list[dict[str, Any]]:
    attempts: list[dict[str, Any]] = []
    contexts_by_pubkey = {context["pubkey"]: context for context in contexts}
    valid_pubkeys = [context["pubkey"] for context in contexts if is_plausible_pubkey(context["pubkey"])]
    invalid_contexts = [context for context in contexts if not is_plausible_pubkey(context["pubkey"])]
    started_mono = time.monotonic()
    started_wall = time.time()

    for delay_ms in delays_ms:
        target = started_mono + delay_ms / 1000.0
        sleep_s = target - time.monotonic()
        if sleep_s > 0:
            time.sleep(sleep_s)
        for provider in providers:
            provider_label = str(provider.get("label") or "primary")
            rpc_url = provider.get("rpc_url")
            for commitment in commitments:
                age_ms = int((time.time() - started_wall) * 1000)
                for context in invalid_contexts:
                    attempts.append(
                        attempt_row(
                            context,
                            provider_label,
                            commitment,
                            delay_ms,
                            None,
                            0,
                            age_ms,
                            False,
                            False,
                            "invalid_pubkey",
                        )
                    )
                if not rpc_url:
                    for pubkey in valid_pubkeys:
                        attempts.append(
                            attempt_row(
                                contexts_by_pubkey[pubkey],
                                provider_label,
                                commitment,
                                delay_ms,
                                None,
                                0,
                                age_ms,
                                False,
                                False,
                                "rpc_url_unavailable",
                            )
                        )
                    continue

                for chunk in chunked(valid_pubkeys, chunk_size):
                    request_started = time.monotonic()
                    try:
                        context_slot, results = rpc_get_multiple_accounts(
                            str(rpc_url),
                            chunk,
                            commitment,
                            timeout_s,
                        )
                        latency_ms = int((time.monotonic() - request_started) * 1000)
                        age_ms = int((time.time() - started_wall) * 1000)
                        for pubkey in chunk:
                            result = results.get(pubkey) or {
                                "ready": False,
                                "missing": False,
                                "error_class": "decode_error",
                            }
                            attempts.append(
                                attempt_row(
                                    contexts_by_pubkey[pubkey],
                                    provider_label,
                                    commitment,
                                    delay_ms,
                                    context_slot,
                                    latency_ms,
                                    age_ms,
                                    bool(result.get("ready")),
                                    bool(result.get("missing")),
                                    result.get("error_class"),
                                    result.get("owner"),
                                    result.get("data_len"),
                                )
                            )
                    except Exception as exc:  # pragma: no cover - network dependent
                        latency_ms = int((time.monotonic() - request_started) * 1000)
                        age_ms = int((time.time() - started_wall) * 1000)
                        error_class = error_class_from_exception(exc)
                        for pubkey in chunk:
                            attempts.append(
                                attempt_row(
                                    contexts_by_pubkey[pubkey],
                                    provider_label,
                                    commitment,
                                    delay_ms,
                                    None,
                                    latency_ms,
                                    age_ms,
                                    False,
                                    False,
                                    error_class,
                                )
                            )
    return attempts


def pubkey_classification(pubkey: str, rows: list[dict[str, Any]], context: dict[str, Any]) -> dict[str, Any]:
    ready_rows = [row for row in rows if row.get("ready") is True]
    error_classes = Counter(row.get("error_class") or "none" for row in rows)
    ready_commitments = sorted({row.get("commitment") for row in ready_rows if row.get("commitment")})
    ready_delays = sorted({int(row.get("delay_ms") or 0) for row in ready_rows})
    ready_providers = sorted({row.get("provider_label") for row in ready_rows if row.get("provider_label")})
    all_error_classes = {row.get("error_class") for row in rows}
    no_ready = not ready_rows
    valid_rows = [row for row in rows if row.get("error_class") != "invalid_pubkey"]
    missing_all = no_ready and bool(valid_rows) and all(row.get("missing") is True for row in valid_rows)
    provider_timeout = no_ready and any(row.get("error_class") == "provider_timeout" for row in rows)
    invalid_pubkey = any(row.get("error_class") == "invalid_pubkey" for row in rows)
    conflicting_account_update = bool(context.get("same_pubkey_account_update")) and no_ready
    inconclusive_rpc_error = no_ready and not missing_all and not provider_timeout and not invalid_pubkey

    nonexclusive: list[str] = []
    if any(row.get("ready") and row.get("commitment") == "processed" for row in rows):
        nonexclusive.append("ready_on_processed")
    if any(row.get("ready") and row.get("commitment") == "confirmed" for row in rows):
        nonexclusive.append("ready_on_confirmed")
    if any(row.get("ready") and row.get("commitment") == "finalized" for row in rows):
        nonexclusive.append("ready_on_finalized")
    if any(row.get("ready") and int(row.get("delay_ms") or 0) > 0 for row in rows):
        nonexclusive.append("ready_after_delay")
    if missing_all:
        nonexclusive.append("missing_all_commitments_all_delays")
    if provider_timeout:
        nonexclusive.append("provider_timeout")
    if invalid_pubkey:
        nonexclusive.append("invalid_pubkey")
    if conflicting_account_update:
        nonexclusive.append("conflicting_account_update")
    if inconclusive_rpc_error:
        nonexclusive.append("inconclusive_rpc_error")

    if invalid_pubkey:
        primary = "invalid_pubkey"
    elif "ready_on_processed" in nonexclusive:
        primary = "ready_on_processed"
    elif "ready_on_confirmed" in nonexclusive:
        primary = "ready_on_confirmed"
    elif "ready_on_finalized" in nonexclusive:
        primary = "ready_on_finalized"
    elif provider_timeout:
        primary = "provider_timeout"
    elif conflicting_account_update:
        primary = "conflicting_account_update"
    elif missing_all:
        primary = "missing_all_commitments_all_delays"
    else:
        primary = "inconclusive_rpc_error"

    mixed_ready = bool(ready_rows) and (
        len(ready_rows) != len(rows)
        or len(ready_commitments) != len({row.get("commitment") for row in rows if row.get("commitment")})
        or len(ready_delays) != len({int(row.get("delay_ms") or 0) for row in rows})
    )

    return {
        "pubkey": pubkey,
        "primary_bucket": primary,
        "audit_buckets": sorted(set(nonexclusive)),
        "ready": bool(ready_rows),
        "ready_rows": len(ready_rows),
        "ready_commitments": ready_commitments,
        "ready_delays_ms": ready_delays,
        "ready_providers": ready_providers,
        "mixed_ready": mixed_ready,
        "error_class_counts": dict(sorted(error_classes.items())),
        "owner_values": sorted({row.get("owner") for row in ready_rows if row.get("owner")}),
        "data_len_values": sorted({row.get("data_len") for row in ready_rows if row.get("data_len") is not None}),
        "x8d_pr1_primary_bucket": context.get("x8d_pr1_primary_bucket"),
        "same_pubkey_account_update": bool(context.get("same_pubkey_account_update")),
        "included_in_subscribe_inferred": bool(context.get("included_in_subscribe_inferred")),
        "dropped_over_cap_inferred": bool(context.get("dropped_over_cap_inferred")),
        "observed_slot_min": context.get("observed_slot_min"),
        "observed_slot_max": context.get("observed_slot_max"),
        "precheck_context_slot_min": context.get("precheck_context_slot_min"),
        "precheck_context_slot_max": context.get("precheck_context_slot_max"),
        "error_classes_seen": sorted(str(item) for item in all_error_classes if item),
    }


def summarize_attempts(contexts: list[dict[str, Any]], attempts: list[dict[str, Any]]) -> dict[str, Any]:
    rows_by_pubkey: dict[str, list[dict[str, Any]]] = {context["pubkey"]: [] for context in contexts}
    contexts_by_pubkey = {context["pubkey"]: context for context in contexts}
    for row in attempts:
        rows_by_pubkey.setdefault(row["pubkey"], []).append(row)

    pubkey_rows: list[dict[str, Any]] = []
    primary_counts: Counter[str] = Counter()
    audit_counts: Counter[str] = Counter()
    for pubkey in sorted(rows_by_pubkey):
        classified = pubkey_classification(pubkey, rows_by_pubkey[pubkey], contexts_by_pubkey.get(pubkey, {"pubkey": pubkey}))
        pubkey_rows.append(classified)
        primary_counts[classified["primary_bucket"]] += 1
        for bucket in classified["audit_buckets"]:
            audit_counts[bucket] += 1

    ready_pubkeys = sum(1 for row in pubkey_rows if row["ready"])
    mixed_ready_pubkeys = sum(1 for row in pubkey_rows if row["mixed_ready"])
    error_attempts = sum(1 for row in attempts if row.get("error_class") not in (None, "missing_on_rpc"))
    if ready_pubkeys == 0:
        if primary_counts.get("provider_timeout", 0) or primary_counts.get("inconclusive_rpc_error", 0):
            verdict = "PR2A-INCONCLUSIVE_RPC_ERRORS"
        else:
            verdict = "PR2A-B_ZERO_READY_CURRENT_MISSING"
    elif mixed_ready_pubkeys > 0:
        verdict = "PR2A-C_PROVIDER_TIMING_DEPENDENT"
    else:
        verdict = "PR2A-A_READY_SUBSET_FOUND"

    return {
        "verdict": verdict,
        "unique_bcv2_pubkeys": len(pubkey_rows),
        "attempt_rows": len(attempts),
        "ready_unique_pubkeys": ready_pubkeys,
        "mixed_ready_unique_pubkeys": mixed_ready_pubkeys,
        "error_attempt_rows": error_attempts,
        "primary_bucket_unique_pubkeys": dict(sorted(primary_counts.items())),
        "audit_bucket_unique_pubkeys": dict(sorted(audit_counts.items())),
        "attempt_error_class_counts": dict(
            sorted(Counter(row.get("error_class") or "none" for row in attempts).items())
        ),
        "attempt_commitment_counts": dict(sorted(Counter(row.get("commitment") for row in attempts).items())),
        "attempt_delay_counts": dict(sorted((str(k), v) for k, v in Counter(row.get("delay_ms") for row in attempts).items())),
        "pubkey_rows": pubkey_rows,
    }


def flatten_pubkey_rows(pubkey_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    fields = [
        "pubkey",
        "primary_bucket",
        "audit_buckets",
        "ready",
        "ready_rows",
        "ready_commitments",
        "ready_delays_ms",
        "ready_providers",
        "mixed_ready",
        "error_class_counts",
        "owner_values",
        "data_len_values",
        "x8d_pr1_primary_bucket",
        "same_pubkey_account_update",
        "included_in_subscribe_inferred",
        "dropped_over_cap_inferred",
        "observed_slot_min",
        "observed_slot_max",
        "precheck_context_slot_min",
        "precheck_context_slot_max",
        "error_classes_seen",
    ]
    flattened: list[dict[str, Any]] = []
    for row in pubkey_rows:
        item = {field: row.get(field) for field in fields}
        for key, value in list(item.items()):
            if isinstance(value, (dict, list)):
                item[key] = json.dumps(value, ensure_ascii=False, sort_keys=True)
        flattened.append(item)
    return flattened


def render_markdown(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    lines = [
        "# P3.7-X8D-PR2A - BCV2 RpcMissing / Commitment / Provider Truth Audit",
        "",
        f"Generated at UTC: {payload['generated_at_utc']}",
        "",
        "## Status",
        "",
        "```text",
        f"verdict = {summary['verdict']}",
        f"unique_bcv2_pubkeys = {summary['unique_bcv2_pubkeys']}",
        f"attempt_rows = {summary['attempt_rows']}",
        f"ready_unique_pubkeys = {summary['ready_unique_pubkeys']}",
        f"mixed_ready_unique_pubkeys = {summary['mixed_ready_unique_pubkeys']}",
        "readiness_policy_changed = false",
        "R18 = NO-GO",
        "Sender/live = NO-GO",
        "Gatekeeper/scoring/fallback = NO-GO",
        "```",
        "",
        "## Inputs",
        "",
        "```text",
        f"x8d_pr1_json = {payload['inputs']['x8d_pr1_json']}",
        f"config_path = {payload['inputs'].get('config_path')}",
        f"providers = {payload['inputs']['providers']}",
        f"commitments = {payload['inputs']['commitments']}",
        f"delays_ms = {payload['inputs']['delays_ms']}",
        "```",
        "",
        "## Buckets",
        "",
        "```json",
        json.dumps(
            {
                "primary_bucket_unique_pubkeys": summary["primary_bucket_unique_pubkeys"],
                "audit_bucket_unique_pubkeys": summary["audit_bucket_unique_pubkeys"],
                "attempt_error_class_counts": summary["attempt_error_class_counts"],
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        ),
        "```",
        "",
        "## Interpretation",
        "",
    ]
    verdict = summary["verdict"]
    if verdict == "PR2A-B_ZERO_READY_CURRENT_MISSING":
        lines.extend(
            [
                "PR2A did not find any current RPC-loadable BCV2 account across all configured",
                "commitments and delays. This is not evidence that `RpcReady` is the wrong",
                "contract. It is evidence that these historical BCV2 pubkeys are not normally",
                "durable/loadable after the fact.",
                "",
                "Operational consequence: D2 `AccountUpdateReceived` execution-ready proof is",
                "cancelled for this path, D3 final burnin variant A is cancelled, and the only",
                "valid next step is live diagnostic smoke for timing / ephemeral-account truth",
                "or formal route exclusion.",
            ]
        )
    elif verdict == "PR2A-A_READY_SUBSET_FOUND":
        lines.extend(
            [
                "PR2A found a current ready subset. The next step is to inspect only that subset",
                "and decide whether a controlled proof can validate `RpcReady`/`PrecheckReady`",
                "without changing readiness policy prematurely.",
            ]
        )
    elif verdict == "PR2A-C_PROVIDER_TIMING_DEPENDENT":
        lines.extend(
            [
                "PR2A found ready evidence that varies by provider, commitment, or delay.",
                "The next step is provider/timing repair or a controlled proof focused on the",
                "dependent subset. Do not change builder, Gatekeeper, or readiness policy.",
            ]
        )
    else:
        lines.extend(
            [
                "PR2A is inconclusive because RPC errors/timeouts dominate at least part of the",
                "sample. Fix provider availability before drawing account-truth conclusions.",
            ]
        )
    lines.extend(["", "## Sample Pubkeys", ""])
    for row in summary["pubkey_rows"][:20]:
        lines.extend(
            [
                "```text",
                f"pubkey = {row['pubkey']}",
                f"primary_bucket = {row['primary_bucket']}",
                f"audit_buckets = {row['audit_buckets']}",
                f"x8d_pr1_primary_bucket = {row.get('x8d_pr1_primary_bucket')}",
                f"same_pubkey_account_update = {row.get('same_pubkey_account_update')}",
                f"ready_commitments = {row.get('ready_commitments')}",
                f"ready_delays_ms = {row.get('ready_delays_ms')}",
                f"error_class_counts = {row.get('error_class_counts')}",
                "```",
                "",
            ]
        )
    return "\n".join(lines).rstrip() + "\n"


def build_payload(
    x8d_pr1_json: Path,
    config_path: Path | None,
    providers: list[dict[str, str | None]],
    commitments: tuple[str, ...],
    delays_ms: tuple[int, ...],
    chunk_size: int,
    timeout_s: float,
) -> dict[str, Any]:
    contexts = load_pubkey_contexts(x8d_pr1_json)
    attempts = run_probe(contexts, providers, commitments, delays_ms, chunk_size, timeout_s)
    summary = summarize_attempts(contexts, attempts)
    return {
        "schema": SCHEMA_VERSION,
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "inputs": {
            "x8d_pr1_json": str(x8d_pr1_json),
            "config_path": str(config_path) if config_path else None,
            "providers": [
                {
                    "label": provider.get("label"),
                    "rpc_url_redacted": redact_url(provider.get("rpc_url")),
                    "available": bool(provider.get("rpc_url")),
                }
                for provider in providers
            ],
            "commitments": list(commitments),
            "delays_ms": list(delays_ms),
            "chunk_size": chunk_size,
            "timeout_s": timeout_s,
        },
        "summary": summary,
        "attempt_rows": attempts,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--x8d-pr1-json", type=Path, required=True)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--rpc-url")
    parser.add_argument("--provider-label", default="primary")
    parser.add_argument("--secondary-rpc-url")
    parser.add_argument("--secondary-provider-label", default="secondary")
    parser.add_argument("--commitments", default=",".join(DEFAULT_COMMITMENTS))
    parser.add_argument("--delays-ms", default=",".join(str(value) for value in DEFAULT_DELAYS_MS))
    parser.add_argument("--chunk-size", type=int, default=DEFAULT_CHUNK_SIZE)
    parser.add_argument("--timeout-s", type=float, default=DEFAULT_TIMEOUT_S)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--output-rows-csv", type=Path)
    parser.add_argument("--output-pubkeys-csv", type=Path)
    parser.add_argument("--output-md", type=Path)
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    config = load_toml(args.config)
    primary_rpc_url = args.rpc_url or rpc_url_from_config(config)
    providers: list[dict[str, str | None]] = [
        {
            "label": args.provider_label,
            "rpc_url": primary_rpc_url,
        }
    ]
    if args.secondary_rpc_url:
        providers.append(
            {
                "label": args.secondary_provider_label,
                "rpc_url": args.secondary_rpc_url,
            }
        )
    commitments = parse_str_list(args.commitments, DEFAULT_COMMITMENTS)
    delays_ms = parse_int_list(args.delays_ms, DEFAULT_DELAYS_MS)
    payload = build_payload(
        args.x8d_pr1_json,
        args.config,
        providers,
        commitments,
        delays_ms,
        args.chunk_size,
        args.timeout_s,
    )
    if args.output_json:
        write_json(args.output_json, payload)
    if args.output_rows_csv:
        write_csv(args.output_rows_csv, payload["attempt_rows"], list(payload["attempt_rows"][0].keys()))
    if args.output_pubkeys_csv:
        write_csv(
            args.output_pubkeys_csv,
            flatten_pubkey_rows(payload["summary"]["pubkey_rows"]),
            list(flatten_pubkey_rows(payload["summary"]["pubkey_rows"])[0].keys()),
        )
    if args.output_md:
        args.output_md.parent.mkdir(parents=True, exist_ok=True)
        args.output_md.write_text(render_markdown(payload), encoding="utf-8")
    if args.json or not any((args.output_json, args.output_rows_csv, args.output_pubkeys_csv, args.output_md)):
        print(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
