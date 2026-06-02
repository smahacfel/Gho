#!/usr/bin/env python3
"""Produce archive-RPC audit transfer events for FSC v2 PR8 provider checks.

The sampled_block_audit mode scans Solana blocks independently through Alchemy
archive RPC and extracts native SOL system transfers.  It is intentionally
separate from NLN observed-event fidelity so the provider benchmark can measure
coverage rather than echoing events NLN already saw.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Iterable

import selector_pipeline_common as common


SYSTEM_PROGRAM_ID = "11111111111111111111111111111111"
AUDIT_PROVIDER = "Alchemy"
AUDIT_SOURCE_KIND = "archive_rpc"
SAMPLED_BLOCK_AUDIT = "sampled_block_audit"
OBSERVED_EVENT_FIDELITY = "observed_event_fidelity"


def parse_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, str):
        value = value.strip()
        if not value:
            return None
        try:
            return int(value, 10)
        except ValueError:
            return None
    return None


def first_value(row: dict[str, Any], names: Iterable[str]) -> Any:
    payload = row.get("payload_json")
    for name in names:
        value = row.get(name)
        if value not in (None, ""):
            return value
        if isinstance(payload, dict):
            value = payload.get(name)
            if value not in (None, ""):
                return value
    return None


def str_value(row: dict[str, Any], *names: str) -> str | None:
    value = first_value(row, names)
    if isinstance(value, str) and value:
        return value
    if isinstance(value, int):
        return str(value)
    return None


def int_value(row: dict[str, Any], *names: str) -> int | None:
    return parse_int(first_value(row, names))


def event_key(row: dict[str, Any]) -> str | None:
    signature = str_value(row, "signature", "tx_signature")
    tx_index = int_value(row, "tx_index", "txIndex")
    instruction_index = int_value(row, "instruction_index", "instructionIndex")
    from_wallet = str_value(row, "from_wallet", "fromWallet", "source_wallet", "from")
    to_wallet = str_value(row, "to_wallet", "toWallet", "recipient_wallet", "to")
    amount = int_value(row, "amount", "amount_lamports", "lamports")
    if None in (signature, tx_index, instruction_index, from_wallet, to_wallet, amount):
        return None
    return f"{signature}:{tx_index}:{instruction_index}:{from_wallet}:{to_wallet}:{amount}"


def load_existing(path: Path) -> tuple[set[str], set[int]]:
    keys: set[str] = set()
    slots: set[int] = set()
    if not path.exists():
        return keys, slots
    for row in common.iter_json_objects(path):
        key = event_key(row)
        if key:
            keys.add(key)
        slot = int_value(row, "slot")
        if slot is not None:
            slots.add(slot)
    return keys, slots


class RpcClient:
    def __init__(self, url: str, timeout_s: float) -> None:
        self.url = url
        self.timeout_s = timeout_s
        self.request_id = 1

    def call(self, method: str, params: list[Any]) -> Any:
        payload = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params,
        }
        self.request_id += 1
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            self.url,
            data=data,
            headers={"content-type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=self.timeout_s) as response:
            body = json.loads(response.read().decode("utf-8"))
        if body.get("error"):
            raise RuntimeError(body["error"])
        return body.get("result")


def iter_nln_slots(path: Path) -> tuple[int | None, int | None, set[str]]:
    slots: list[int] = []
    signatures: set[str] = set()
    if not path.exists():
        return None, None, signatures
    for row in common.iter_json_objects(path):
        slot = int_value(row, "slot")
        if slot is not None:
            slots.append(slot)
        signature = str_value(row, "signature", "tx_signature")
        if signature:
            signatures.add(signature)
    if not slots:
        return None, None, signatures
    return min(slots), max(slots), signatures


def sampled_slots(start_slot: int, end_slot: int, min_slots: int, stride: int) -> list[int]:
    if start_slot > end_slot:
        return []
    if stride > 1:
        return list(range(start_slot, end_slot + 1, stride))
    available = end_slot - start_slot + 1
    if available <= min_slots:
        return list(range(start_slot, end_slot + 1))
    step = max(1, available // max(1, min_slots))
    slots = list(range(start_slot, end_slot + 1, step))
    return slots[:min_slots]


def parsed_transfer_row(
    *,
    slot: int,
    block_time: int | None,
    tx_index: int,
    signature: str,
    instruction_index: int,
    parsed: dict[str, Any],
    inner_instruction_index: int | None = None,
) -> dict[str, Any] | None:
    info = parsed.get("info")
    if not isinstance(info, dict):
        return None
    transfer_type = parsed.get("type")
    if transfer_type not in {"transfer", "transferWithSeed"}:
        return None
    from_wallet = info.get("source")
    to_wallet = info.get("destination")
    amount = parse_int(info.get("lamports"))
    if not isinstance(from_wallet, str) or not isinstance(to_wallet, str) or amount is None:
        return None
    event_ts_ms = block_time * 1000 if isinstance(block_time, int) and block_time > 0 else None
    return {
        "provider": AUDIT_PROVIDER,
        "source_kind": AUDIT_SOURCE_KIND,
        "audit_mode": SAMPLED_BLOCK_AUDIT,
        "signature": signature,
        "slot": slot,
        "tx_index": tx_index,
        "instruction_index": instruction_index,
        "inner_instruction_index": inner_instruction_index,
        "from_wallet": from_wallet,
        "to_wallet": to_wallet,
        "amount_lamports": amount,
        "token_address": "solana",
        "event_ts_ms": event_ts_ms,
        "event_order_key": [slot, tx_index, instruction_index, signature],
    }


def instruction_program_id(instruction: dict[str, Any]) -> str | None:
    value = instruction.get("programId")
    if isinstance(value, str):
        return value
    value = instruction.get("program")
    if value == "system":
        return SYSTEM_PROGRAM_ID
    return None


def extract_transfers_from_block(block: dict[str, Any], slot: int) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    block_time = block.get("blockTime")
    transactions = block.get("transactions") or []
    for tx_index, tx_entry in enumerate(transactions):
        transaction = tx_entry.get("transaction") if isinstance(tx_entry, dict) else None
        meta = tx_entry.get("meta") if isinstance(tx_entry, dict) else None
        if not isinstance(transaction, dict):
            continue
        signatures = transaction.get("signatures") or []
        signature = signatures[0] if signatures else None
        message = transaction.get("message")
        if not isinstance(signature, str) or not isinstance(message, dict):
            continue
        for instruction_index, instruction in enumerate(message.get("instructions") or []):
            if not isinstance(instruction, dict):
                continue
            if instruction_program_id(instruction) != SYSTEM_PROGRAM_ID:
                continue
            parsed = instruction.get("parsed")
            if isinstance(parsed, dict):
                row = parsed_transfer_row(
                    slot=slot,
                    block_time=block_time,
                    tx_index=tx_index,
                    signature=signature,
                    instruction_index=instruction_index,
                    parsed=parsed,
                )
                if row:
                    rows.append(row)
        inner_groups = meta.get("innerInstructions") if isinstance(meta, dict) else []
        for group in inner_groups or []:
            if not isinstance(group, dict):
                continue
            parent_index = parse_int(group.get("index"))
            if parent_index is None:
                continue
            for inner_index, instruction in enumerate(group.get("instructions") or []):
                if not isinstance(instruction, dict):
                    continue
                if instruction_program_id(instruction) != SYSTEM_PROGRAM_ID:
                    continue
                parsed = instruction.get("parsed")
                if not isinstance(parsed, dict):
                    continue
                row = parsed_transfer_row(
                    slot=slot,
                    block_time=block_time,
                    tx_index=tx_index,
                    signature=signature,
                    instruction_index=parent_index * 1000 + inner_index,
                    parsed=parsed,
                    inner_instruction_index=inner_index,
                )
                if row:
                    rows.append(row)
    return rows


def append_rows(path: Path, rows: list[dict[str, Any]], seen_keys: set[str]) -> int:
    if not rows:
        return 0
    path.parent.mkdir(parents=True, exist_ok=True)
    written = 0
    with path.open("a", encoding="utf-8") as fh:
        for row in rows:
            key = event_key(row)
            if not key or key in seen_keys:
                continue
            seen_keys.add(key)
            fh.write(json.dumps(row, sort_keys=True) + "\n")
            written += 1
    return written


def get_block(client: RpcClient, slot: int) -> dict[str, Any] | None:
    return client.call(
        "getBlock",
        [
            slot,
            {
                "encoding": "jsonParsed",
                "transactionDetails": "full",
                "rewards": False,
                "maxSupportedTransactionVersion": 0,
            },
        ],
    )


def get_transaction(client: RpcClient, signature: str) -> dict[str, Any] | None:
    return client.call(
        "getTransaction",
        [
            signature,
            {
                "encoding": "jsonParsed",
                "maxSupportedTransactionVersion": 0,
            },
        ],
    )


def maybe_request_delay(args: argparse.Namespace) -> None:
    delay_s = getattr(args, "request_delay_s", 0.0) or 0.0
    if delay_s > 0:
        time.sleep(delay_s)


def run_sampled_block_audit(args: argparse.Namespace, client: RpcClient) -> dict[str, Any]:
    seen_keys, scanned_slots = load_existing(args.out)
    started = time.time()
    errors = 0
    while True:
        start_slot = args.start_slot
        end_slot = args.end_slot
        if start_slot is None or end_slot is None:
            nln_start, nln_end, _ = iter_nln_slots(args.nln_transfer)
            start_slot = start_slot if start_slot is not None else nln_start
            end_slot = end_slot if end_slot is not None else nln_end
        if start_slot is None or end_slot is None:
            if args.once:
                break
            time.sleep(args.poll_seconds)
            continue
        slots = sampled_slots(start_slot, end_slot, args.min_slots, args.slot_stride)
        progress = False
        for slot in slots:
            if slot in scanned_slots:
                continue
            try:
                block = get_block(client, slot)
            except (urllib.error.URLError, TimeoutError, RuntimeError) as err:
                errors += 1
                print(f"alchemy_audit_getBlock_error slot={slot} error={err}", file=sys.stderr)
                maybe_request_delay(args)
                if errors >= args.max_errors:
                    raise
                continue
            maybe_request_delay(args)
            scanned_slots.add(slot)
            progress = True
            if not isinstance(block, dict):
                continue
            rows = extract_transfers_from_block(block, slot)
            for row in rows:
                row["audit_slots_sampled_total"] = len(scanned_slots)
            append_rows(args.out, rows, seen_keys)
            if len(scanned_slots) >= args.min_slots or len(seen_keys) >= args.min_audit_transfer_events:
                return {
                    "status": "PASS",
                    "audit_mode": SAMPLED_BLOCK_AUDIT,
                    "audit_slots_sampled": len(scanned_slots),
                    "audit_transfer_event_keys": len(seen_keys),
                    "elapsed_seconds": round(time.time() - started, 3),
                }
        if args.once or (len(scanned_slots) >= args.min_slots or len(seen_keys) >= args.min_audit_transfer_events):
            break
        if not progress:
            time.sleep(args.poll_seconds)
    status = (
        "PASS"
        if len(scanned_slots) >= args.min_slots or len(seen_keys) >= args.min_audit_transfer_events
        else "PENDING"
    )
    return {
        "status": status,
        "audit_mode": SAMPLED_BLOCK_AUDIT,
        "audit_slots_sampled": len(scanned_slots),
        "audit_transfer_event_keys": len(seen_keys),
        "elapsed_seconds": round(time.time() - started, 3),
    }


def run_observed_event_fidelity(args: argparse.Namespace, client: RpcClient) -> dict[str, Any]:
    seen_keys, _ = load_existing(args.out)
    _, _, signatures = iter_nln_slots(args.nln_transfer)
    checked = 0
    for signature in sorted(signatures):
        try:
            tx = get_transaction(client, signature)
        except (urllib.error.URLError, TimeoutError, RuntimeError) as err:
            print(f"alchemy_audit_getTransaction_error signature={signature} error={err}", file=sys.stderr)
            maybe_request_delay(args)
            continue
        maybe_request_delay(args)
        checked += 1
        if not isinstance(tx, dict):
            continue
        slot = parse_int(tx.get("slot"))
        if slot is None:
            continue
        block = {
            "blockTime": tx.get("blockTime"),
            "transactions": [{"transaction": tx.get("transaction"), "meta": tx.get("meta")}],
        }
        rows = extract_transfers_from_block(block, slot)
        for row in rows:
            row["audit_mode"] = OBSERVED_EVENT_FIDELITY
        append_rows(args.out, rows, seen_keys)
    return {
        "status": "PASS" if checked > 0 else "NO-GO",
        "audit_mode": OBSERVED_EVENT_FIDELITY,
        "signatures_checked": checked,
        "audit_transfer_event_keys": len(seen_keys),
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--root", type=Path, default=Path("/root/Gho"))
    parser.add_argument(
        "--mode",
        choices=[SAMPLED_BLOCK_AUDIT, OBSERVED_EVENT_FIDELITY],
        default=SAMPLED_BLOCK_AUDIT,
    )
    parser.add_argument("--rpc-url", default=os.environ.get("ALCHEMY_SOLANA_RPC_URL"))
    parser.add_argument("--nln-transfer", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--start-slot", type=int)
    parser.add_argument("--end-slot", type=int)
    parser.add_argument("--slot-stride", type=int, default=1)
    parser.add_argument("--min-slots", type=int, default=1000)
    parser.add_argument("--min-audit-transfer-events", type=int, default=10_000)
    parser.add_argument("--poll-seconds", type=float, default=30.0)
    parser.add_argument("--request-timeout-s", type=float, default=20.0)
    parser.add_argument("--request-delay-s", type=float, default=0.0)
    parser.add_argument("--max-errors", type=int, default=25)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if not args.rpc_url:
        print("ALCHEMY_SOLANA_RPC_URL or --rpc-url is required", file=sys.stderr)
        return 2
    client = RpcClient(args.rpc_url, args.request_timeout_s)
    if args.mode == SAMPLED_BLOCK_AUDIT:
        result = run_sampled_block_audit(args, client)
    else:
        result = run_observed_event_fidelity(args, client)
    result.update({"scope": args.scope, "out": str(args.out)})
    if args.json:
        print(json.dumps(result, sort_keys=True))
    else:
        print(
            " ".join(f"{key}={value}" for key, value in sorted(result.items())),
            flush=True,
        )
    return 0 if result["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
