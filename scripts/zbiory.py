#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class ScanStats:
    lifecycle_rows_scanned: int = 0
    lifecycle_json_errors: int = 0
    lifecycle_missing_pnl: int = 0
    lifecycle_missing_mint: int = 0
    lifecycle_duplicate_mints: int = 0
    lifecycle_unique_kept: int = 0
    decision_rows_scanned: int = 0
    decision_json_errors: int = 0
    decision_missing_base_mint: int = 0
    decision_non_target_mint: int = 0
    decision_matches: int = 0
    lifecycle_unmatched: int = 0
    output_a: int = 0
    output_b: int = 0
    output_n: int = 0


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Scal shadow_lifecycle.jsonl i probe_shadow_lifecycle.jsonl z "
            "gatekeeper_v2_decisions.jsonl, a potem podziel wynik na trzy zbiory "
            "wedlug final_pnl_pct."
        )
    )
    parser.add_argument("positive_threshold", type=float, help="Prog dodatni dla zbioru A, np. 30")
    parser.add_argument("negative_threshold", type=float, help="Prog ujemny dla zbioru B, np. -30")
    parser.add_argument(
        "--directory",
        "-d",
        type=Path,
        default=Path("."),
        help="Katalog z plikami JSONL. Domyslnie aktualny katalog roboczy.",
    )
    parser.add_argument(
        "--shadow-file",
        default="shadow_lifecycle.jsonl",
        help="Nazwa pliku shadow lifecycle w katalogu roboczym.",
    )
    parser.add_argument(
        "--probe-file",
        default="probe_shadow_lifecycle.jsonl",
        help="Nazwa pliku probe shadow lifecycle w katalogu roboczym.",
    )
    parser.add_argument(
        "--decisions-file",
        default="gatekeeper_v2_decisions.jsonl",
        help="Nazwa pliku decisions w katalogu roboczym.",
    )
    parser.add_argument("--output-a", default="zbior_A.jsonl", help="Plik wyjsciowy dla wartosci >= prog dodatni.")
    parser.add_argument("--output-b", default="zbior_B.jsonl", help="Plik wyjsciowy dla wartosci <= prog ujemny.")
    parser.add_argument("--output-n", default="zbior_N.jsonl", help="Plik wyjsciowy dla pozostalych rekordow.")
    return parser.parse_args(argv)


def ensure_file_exists(path: Path) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"Brak wymaganego pliku: {path}")


def iter_jsonl(path: Path) -> tuple[int, Any]:
    with path.open("r", encoding="utf-8") as handle:
        for line_no, raw_line in enumerate(handle, start=1):
            line = raw_line.strip()
            if not line:
                continue
            yield line_no, line


def to_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def load_unique_lifecycle_records(paths: list[Path], stats: ScanStats) -> dict[str, dict[str, Any]]:
    by_mint: dict[str, dict[str, Any]] = {}

    for path in paths:
        ensure_file_exists(path)
        for line_no, raw_line in iter_jsonl(path):
            stats.lifecycle_rows_scanned += 1
            try:
                record = json.loads(raw_line)
            except json.JSONDecodeError:
                stats.lifecycle_json_errors += 1
                continue
            if not isinstance(record, dict):
                stats.lifecycle_missing_mint += 1
                continue
            if "final_pnl_pct" not in record:
                stats.lifecycle_missing_pnl += 1
                continue

            mint_id = record.get("mint_id")
            if not isinstance(mint_id, str) or not mint_id.strip():
                stats.lifecycle_missing_mint += 1
                continue
            mint_id = mint_id.strip()

            if mint_id in by_mint:
                stats.lifecycle_duplicate_mints += 1
                continue

            merged_ready = dict(record)
            merged_ready["_lifecycle_source_file"] = path.name
            merged_ready["_lifecycle_source_line"] = line_no
            by_mint[mint_id] = merged_ready
            stats.lifecycle_unique_kept += 1

    return by_mint


def load_matching_decisions(
    path: Path,
    lifecycle_mints: set[str],
    stats: ScanStats,
) -> dict[str, list[dict[str, Any]]]:
    ensure_file_exists(path)
    by_mint: dict[str, list[dict[str, Any]]] = defaultdict(list)

    for line_no, raw_line in iter_jsonl(path):
        stats.decision_rows_scanned += 1
        try:
            record = json.loads(raw_line)
        except json.JSONDecodeError:
            stats.decision_json_errors += 1
            continue
        if not isinstance(record, dict):
            stats.decision_missing_base_mint += 1
            continue

        base_mint = record.get("base_mint")
        if not isinstance(base_mint, str) or not base_mint.strip():
            stats.decision_missing_base_mint += 1
            continue
        base_mint = base_mint.strip()

        if base_mint not in lifecycle_mints:
            stats.decision_non_target_mint += 1
            continue

        merged_ready = dict(record)
        merged_ready["_decision_source_file"] = path.name
        merged_ready["_decision_source_line"] = line_no
        by_mint[base_mint].append(merged_ready)

    return by_mint


def write_partitioned_outputs(
    lifecycle_by_mint: dict[str, dict[str, Any]],
    decisions_by_mint: dict[str, list[dict[str, Any]]],
    positive_threshold: float,
    negative_threshold: float,
    output_a: Path,
    output_b: Path,
    output_n: Path,
    stats: ScanStats,
) -> None:
    with (
        output_a.open("w", encoding="utf-8") as handle_a,
        output_b.open("w", encoding="utf-8") as handle_b,
        output_n.open("w", encoding="utf-8") as handle_n,
    ):
        for mint_id, lifecycle_record in lifecycle_by_mint.items():
            decision_records = decisions_by_mint.get(mint_id)
            if not decision_records:
                stats.lifecycle_unmatched += 1
                continue

            for decision_record in decision_records:
                merged_record = {**decision_record, **lifecycle_record}
                merged_record["_merged_mint_id"] = mint_id
                pnl_value = to_float(merged_record.get("final_pnl_pct"))

                if pnl_value is not None and pnl_value >= positive_threshold:
                    handle_a.write(json.dumps(merged_record, ensure_ascii=False) + "\n")
                    stats.output_a += 1
                elif pnl_value is not None and pnl_value <= negative_threshold:
                    handle_b.write(json.dumps(merged_record, ensure_ascii=False) + "\n")
                    stats.output_b += 1
                else:
                    handle_n.write(json.dumps(merged_record, ensure_ascii=False) + "\n")
                    stats.output_n += 1

                stats.decision_matches += 1


def print_summary(
    working_directory: Path,
    output_a: Path,
    output_b: Path,
    output_n: Path,
    stats: ScanStats,
) -> None:
    print(f"Katalog roboczy: {working_directory}")
    print(f"Lifecycle przeskanowane: {stats.lifecycle_rows_scanned}")
    print(f"Lifecycle unikalne mint_id zachowane: {stats.lifecycle_unique_kept}")
    print(f"Lifecycle duplikaty mint_id pominiete: {stats.lifecycle_duplicate_mints}")
    print(f"Lifecycle bez final_pnl_pct pominiete: {stats.lifecycle_missing_pnl}")
    print(f"Lifecycle bez poprawnego mint_id pominiete: {stats.lifecycle_missing_mint}")
    print(f"Lifecycle bledy JSON pominiete: {stats.lifecycle_json_errors}")
    print(f"Decyzje przeskanowane: {stats.decision_rows_scanned}")
    print(f"Decyzje bez poprawnego base_mint pominiete: {stats.decision_missing_base_mint}")
    print(f"Decyzje spoza zbioru mint_id pominiete: {stats.decision_non_target_mint}")
    print(f"Liczba scalonych rekordow: {stats.decision_matches}")
    print(f"Lifecycle bez odpowiadajacej decyzji: {stats.lifecycle_unmatched}")
    print(f"{output_a.name}: {stats.output_a}")
    print(f"{output_b.name}: {stats.output_b}")
    print(f"{output_n.name}: {stats.output_n}")


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)

    if args.positive_threshold <= args.negative_threshold:
        print(
            "Blad: prog dodatni musi byc wiekszy od progu ujemnego.",
            file=sys.stderr,
        )
        return 2

    working_directory = args.directory.resolve()
    if not working_directory.is_dir():
        print(f"Blad: katalog nie istnieje: {working_directory}", file=sys.stderr)
        return 2

    shadow_path = working_directory / args.shadow_file
    probe_path = working_directory / args.probe_file
    decisions_path = working_directory / args.decisions_file
    output_a = working_directory / args.output_a
    output_b = working_directory / args.output_b
    output_n = working_directory / args.output_n

    stats = ScanStats()
    try:
        lifecycle_by_mint = load_unique_lifecycle_records([shadow_path, probe_path], stats)
        decisions_by_mint = load_matching_decisions(decisions_path, set(lifecycle_by_mint), stats)
    except FileNotFoundError as exc:
        print(str(exc), file=sys.stderr)
        return 2

    write_partitioned_outputs(
        lifecycle_by_mint=lifecycle_by_mint,
        decisions_by_mint=decisions_by_mint,
        positive_threshold=args.positive_threshold,
        negative_threshold=args.negative_threshold,
        output_a=output_a,
        output_b=output_b,
        output_n=output_n,
        stats=stats,
    )
    print_summary(working_directory, output_a, output_b, output_n, stats)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
