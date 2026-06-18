#!/usr/bin/env python3
"""
scripts/merge_lifecycles.py

Łączy rekordy lifecycle (shadow_lifecycle.jsonl + probe_shadow_lifecycle.jsonl)
z decyzjami z pliku gatekeeper_v2_decisions.jsonl. Filtruje tylko rekordy
z lifecycle, które zawierają pole `final_pnl_pct`, dopasowuje je po
wartości mint (np. `mint_id` / `base_mint`) i zapisuje spłaszczone,
scalone rekordy do pliku output JSONL (`lifecycles.jsonl` domyślnie).

Przykład użycia:
  python3 scripts/merge_lifecycles.py \
    -l /root/Gho/logs/shadow_run/.../shadow_lifecycle.jsonl \
    -l /root/Gho/logs/shadow_run/.../probe_shadow_lifecycle.jsonl \
    -d /root/Gho/logs/rollout/.../gatekeeper_v2_decisions.jsonl \
    -o /root/Gho/lifecycles.jsonl

Domyślne ścieżki są ustawione zgodnie z przykładową strukturą repozytorium.
"""
from __future__ import annotations

import argparse
import json
import logging
import os
from collections import defaultdict
from typing import Any, Dict, List, Optional, Set


def extract_mint(rec: Dict[str, Any]) -> Optional[str]:
    """Spróbuj wyciągnąć identyfikator mint z rekordu.

    Sprawdza kilka powszechnie występujących pól.
    """
    if not isinstance(rec, dict):
        return None
    for k in ("mint_id", "base_mint", "mint", "pool_mint", "token_mint"):
        v = rec.get(k)
        if isinstance(v, str) and v:
            return v
    return None


def load_lifecycle_files(paths: List[str], logger: logging.Logger) -> Dict[str, List[Dict[str, Any]]]:
    """Wczytaj pliki lifecycle i zwróć indeks: mint -> lista rekordów.

    Zwracamy tylko rekordy, które zawierają klucz `final_pnl_pct`.
    """
    idx: Dict[str, List[Dict[str, Any]]] = defaultdict(list)
    total = 0
    kept = 0
    for path in paths:
        if not path:
            continue
        if not os.path.exists(path):
            logger.warning("Lifecycle file not found: %s", path)
            continue
        logger.info("Reading lifecycle file: %s", path)
        with open(path, "r", encoding="utf-8") as f:
            for ln, line in enumerate(f, start=1):
                total += 1
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError:
                    logger.debug("Skipping invalid JSON in %s:%d", path, ln)
                    continue
                # filter: only records that have the field final_pnl_pct
                if "final_pnl_pct" not in rec:
                    continue
                mint = extract_mint(rec)
                if not mint:
                    logger.debug("Lifecycle record without mint in %s:%d", path, ln)
                    continue
                idx[mint].append(rec)
                kept += 1
    logger.info("Lifecycle records scanned=%d, kept_with_final_pnl_pct=%d", total, kept)
    return idx


def merge_and_write(
    decisions_path: str,
    lifecycle_idx: Dict[str, List[Dict[str, Any]]],
    out_path: str,
    prefer: str,
    include_unmatched_lifecycles: bool,
    logger: logging.Logger,
) -> None:
    """Wczytaj decyzje i zapisz scalenia do `out_path`.

    - jeśli jest dopasowanie (mint) -> wypisz dla każdej pary (decision, lifecycle)
    - jeśli `include_unmatched_lifecycles` = True, dopisz lifecycley bez par
    """
    if not os.path.exists(decisions_path):
        logger.error("Decisions file not found: %s", decisions_path)
        raise SystemExit(2)

    matched_mints: Set[str] = set()
    total_decisions = 0
    merged_count = 0
    skipped_decisions = 0

    with open(decisions_path, "r", encoding="utf-8") as f_dec, open(out_path, "w", encoding="utf-8") as f_out:
        logger.info("Reading decisions file: %s", decisions_path)
        for ln, line in enumerate(f_dec, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                dec = json.loads(line)
            except json.JSONDecodeError:
                logger.debug("Skipping invalid JSON in decisions %s:%d", decisions_path, ln)
                continue
            total_decisions += 1
            mint = extract_mint(dec)
            if not mint:
                skipped_decisions += 1
                logger.debug("Decision record without mint in %s:%d", decisions_path, ln)
                continue

            lrecs = lifecycle_idx.get(mint)
            if not lrecs:
                skipped_decisions += 1
                continue

            matched_mints.add(mint)
            for lrec in lrecs:
                # merge flat: order decides which side overrides on key collision
                if prefer == "lifecycle":
                    merged = {**dec, **lrec}
                else:
                    merged = {**lrec, **dec}
                # minimal provenance metadata
                merged["_merged_from_mint"] = mint
                merged["_merged_decision_file"] = os.path.basename(decisions_path)
                f_out.write(json.dumps(merged, ensure_ascii=False) + "\n")
                merged_count += 1

    if include_unmatched_lifecycles:
        # dopisz lifecycle-only rekordy, które nie miały dopasowania
        appended = 0
        with open(out_path, "a", encoding="utf-8") as f_out:
            for mint, lrecs in lifecycle_idx.items():
                if mint in matched_mints:
                    continue
                for lrec in lrecs:
                    rec = dict(lrec)
                    rec["_merged_from_mint"] = mint
                    rec["_merged_decision_file"] = None
                    rec["_merged_only_lifecycle"] = True
                    f_out.write(json.dumps(rec, ensure_ascii=False) + "\n")
                    appended += 1
        logger.info("Appended unmatched lifecycle-only records=%d", appended)

    logger.info(
        "Decisions scanned=%d, merged_pairs_written=%d, skipped_decisions_without_match=%d",
        total_decisions,
        merged_count,
        skipped_decisions,
    )


def find_existing(paths: List[str]) -> Optional[str]:
    for p in paths:
        if p and os.path.exists(p):
            return p
    return None


def main(argv: List[str] | None = None) -> None:
    p = argparse.ArgumentParser(description="Merge lifecycle records with gatekeeper v2 decisions by mint id.")
    p.add_argument("-l", "--lifecycle", action="append", help="Path to lifecycle JSONL (can repeat)")
    p.add_argument("-d", "--decisions", help="Path to gatekeeper_v2_decisions.jsonl")
    p.add_argument("-o", "--output", default="lifecycles.jsonl", help="Output JSONL path (default: lifecycles.jsonl)")
    p.add_argument(
        "--prefer",
        choices=("lifecycle", "decision"),
        default="lifecycle",
        help="Which record wins on key conflicts (default: lifecycle)",
    )
    p.add_argument(
        "--include-unmatched-lifecycles",
        action="store_true",
        help="Include lifecycle records without matching decision records (appended to output)",
    )
    args = p.parse_args(argv)

    logging.basicConfig(format="%(levelname)s: %(message)s", level=logging.INFO)
    logger = logging.getLogger("merge_lifecycles")

    # sensowne domyślne ścieżki (przykłady z repo)
    default_lifecycle_candidates = [
        "/root/Gho/logs/shadow_run/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/shadow_lifecycle.jsonl",
        "/root/Gho/logs/shadow_run/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/probe_shadow_lifecycle.jsonl",
    ]
    default_decision_candidates = [
        "/root/Gho/logs/rollout/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/decisions/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/v2.2/legacy_live/3e082d2a10120b33ee5c6050c0d3592588613abf7c872c504710314404e7637e/gatekeeper_v2_decisions.jsonl",
        "logs/rollout/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/decisions/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/v2.2/legacy_live/3e082d2a10120b33ee5c6050c0d3592588613abf7c872c504710314404e7637e/gatekeeper_v2_decisions.jsonl",
    ]

    lifecycle_paths = args.lifecycle or default_lifecycle_candidates
    # if user supplied a single lifecycle path via -l it will be list. If not, keep defaults

    decisions_path = args.decisions or find_existing(default_decision_candidates)
    if not decisions_path:
        logger.error("Nie znaleziono pliku decisions (podaj przez --decisions).")
        raise SystemExit(2)

    logger.info("Lifecycle paths: %s", lifecycle_paths)
    logger.info("Decisions path: %s", decisions_path)
    logger.info("Output: %s", args.output)

    lifecycle_idx = load_lifecycle_files(lifecycle_paths, logger)
    if not lifecycle_idx:
        logger.warning("Brak rekordów lifecycle z polem final_pnl_pct. Nic do zrobienia.")
        # optionally create empty output
        open(args.output, "w", encoding="utf-8").close()
        return

    merge_and_write(decisions_path, lifecycle_idx, args.output, args.prefer, args.include_unmatched_lifecycles, logger)
    logger.info("Zapisano wynik do %s", args.output)


if __name__ == "__main__":
    main()
