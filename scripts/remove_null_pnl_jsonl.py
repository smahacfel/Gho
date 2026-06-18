#!/usr/bin/env python3
import argparse
import os
import sys
from pathlib import Path

def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Usuń z pliku JSONL wszystkie linie zaczynające się od "
            'prefiksu: {"entry_price": null, "exit_price": null, "final_pnl_pct": null'
        )
    )
    parser.add_argument(
        "input",
        metavar="INPUT_JSONL",
        help="ścieżka do pliku JSONL do przefiltrowania",
    )
    parser.add_argument(
        "--output",
        metavar="OUTPUT_JSONL",
        help="ścieżka do pliku wyjściowego; jeśli nie podano, nadpisze plik wejściowy",
        default=None,
    )
    args = parser.parse_args()

    input_path = Path(args.input)
    if not input_path.exists():
        print(f"Błąd: plik wejściowy nie istnieje: {input_path}", file=sys.stderr)
        return 1

    prefix = '{"entry_price": null, "exit_price": null, "final_pnl_pct": null'
    output_path = Path(args.output) if args.output else input_path.with_suffix(input_path.suffix + ".filtered")
    temp_path = output_path.with_suffix(output_path.suffix + ".tmp")

    removed = 0
    total = 0

    with input_path.open("r", encoding="utf-8") as src, temp_path.open("w", encoding="utf-8") as dst:
        for line in src:
            total += 1
            if line.startswith(prefix):
                removed += 1
                continue
            dst.write(line)

    os.replace(temp_path, output_path)

    print(f"Wczytano {total} linii.")
    print(f"Usunięto {removed} linii zaczynających się od prefiksu.")
    print(f"Zapisano wynik do: {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
