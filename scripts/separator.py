#!/usr/bin/env python3
"""
separator.py

Użycie:
    python separator.py POS_THRESHOLD NEG_THRESHOLD [--input INPUT_FILE]

Przykład:
    python separator.py 30 -40

Skrypt zakłada, że plik źródłowy znajduje się w tym samym katalogu, z którego jest uruchamiany.
Domyślny plik wejściowy: gatekeeper_v2_buys_with_shadow_lifecycle_economics_flat.jsonl
Wynikowe pliki utworzone w katalogu roboczym:
    zbior_A.jsonl (wartości >= POS_THRESHOLD)
    zbior_B.jsonl (wartości <= NEG_THRESHOLD)
    zbior_N.jsonl (pozostałe lub brak/nieprawidłowe wartości)

Zachowuje oryginalne linie JSONL bez modyfikacji.
"""

import argparse
import json
import sys
from pathlib import Path


def parse_args():
    p = argparse.ArgumentParser(description="Podziel plik JSONL na trzy zbiory według final_pnl_pct")
    p.add_argument("pos", help="dodatni próg (np. 30)", type=float)
    p.add_argument("neg", help="ujemny próg (np. -40)", type=float)
    p.add_argument("--input", "-i", help="plik JSONL do przetworzenia",
                   default="gatekeeper_v2_buys_with_shadow_lifecycle_economics_flat.jsonl")
    p.add_argument("--skip-null", action="store_true",
                   help="pomiń linie zaczynające się od prefiksu null: '{\"entry_price\": null, \"exit_price\": null, \"final_pnl_pct\": null'")
    p.add_argument("--a", help="nazwa pliku dla zbior_A (wartości >= pos)", default="zbior_A.jsonl")
    p.add_argument("--b", help="nazwa pliku dla zbior_B (wartości <= neg)", default="zbior_B.jsonl")
    p.add_argument("--n", help="nazwa pliku dla zbior_N (pozostałe)", default="zbior_N.jsonl")
    return p.parse_args()


def main():
    args = parse_args()
    pos = args.pos
    neg = args.neg

    if pos <= 0:
        print("Błąd: wartość dodatnia 'pos' musi być > 0", file=sys.stderr)
        return 2
    if neg >= 0:
        print("Błąd: wartość ujemna 'neg' musi być < 0", file=sys.stderr)
        return 2

    input_path = Path(args.input)
    if not input_path.exists():
        print(f"Błąd: plik wejściowy nie istnieje: {input_path}", file=sys.stderr)
        return 3

    out_a = Path(args.a)
    out_b = Path(args.b)
    out_n = Path(args.n)

    counts = {"A": 0, "B": 0, "N": 0, "total": 0, "parse_errors": 0, "skipped_nulls": 0}
    null_prefix = '{"entry_price": null, "exit_price": null, "final_pnl_pct": null'
    skip_null = bool(getattr(args, "skip_null", False))

    with input_path.open("r", encoding="utf-8") as src, \
         out_a.open("w", encoding="utf-8") as fa, \
         out_b.open("w", encoding="utf-8") as fb, \
         out_n.open("w", encoding="utf-8") as fn:

        for raw in src:
            line = raw.rstrip("\n")
            if not line:
                continue
            counts["total"] += 1

            # optionally skip lines that start with the null-prefixed record
            if skip_null and line.lstrip().startswith(null_prefix):
                counts["skipped_nulls"] += 1
                continue

            try:
                obj = json.loads(line)
            except Exception:
                counts["parse_errors"] += 1
                fn.write(line + "\n")
                continue

            val = obj.get("final_pnl_pct") if isinstance(obj, dict) else None

            # treat non-numeric or None as N
            if val is None:
                fn.write(line + "\n")
                counts["N"] += 1
                continue

            try:
                fval = float(val)
            except Exception:
                fn.write(line + "\n")
                counts["N"] += 1
                continue

            if fval >= pos:
                fa.write(line + "\n")
                counts["A"] += 1
            elif fval <= neg:
                fb.write(line + "\n")
                counts["B"] += 1
            else:
                fn.write(line + "\n")
                counts["N"] += 1

    print(f"Przetworzono: {counts['total']} linii")
    print(f"zbior_A (>= {pos}): {counts['A']} linii -> {out_a}")
    print(f"zbior_B (<= {neg}): {counts['B']} linii -> {out_b}")
    print(f"zbior_N (pozostałe): {counts['N']} linii -> {out_n}")
    if counts["skipped_nulls"]:
        print(f"Pominięto {counts['skipped_nulls']} linii ze względu na prefiks null (skip-null=True)")
    if counts["parse_errors"]:
        print(f"Linie z błędami parsowania zapisano do zbior_N: {counts['parse_errors']}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
