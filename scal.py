#!/usr/bin/env python3
"""Łączy pliki JSONL w dwóch grupach: A i B."""

from pathlib import Path


def scal_grupe(katalog: Path, prefix: str, liczba_plikow: int = 5) -> None:
    """Łączy zbior_{prefix}1.jsonl ... zbior_{prefix}{N}.jsonl w plik zbior_{prefix}.jsonl."""
    wejscia = [katalog / f"zbior_{prefix}{i}.jsonl" for i in range(1, liczba_plikow + 1)]
    wyjscie = katalog / f"zbior_{prefix}.jsonl"

    zapisanych = 0
    pominietych = 0

    with wyjscie.open("w", encoding="utf-8", newline="") as out:
        for plik in wejscia:
            if not plik.exists():
                print(f"Pomijam brakujący plik: {plik.name}")
                pominietych += 1
                continue

            with plik.open("r", encoding="utf-8", newline="") as inp:
                for linia in inp:
                    out.write(linia if linia.endswith("\n") else f"{linia}\n")
                    zapisanych += 1

    if pominietych:
        print(f"Grupa {prefix}: zapisano {zapisanych} rekordów, pominięto {pominietych} brakujących plików.")
    else:
        print(f"Grupa {prefix}: zapisano {zapisanych} rekordów do {wyjscie.name}.")


def main() -> None:
    katalog = Path(__file__).resolve().parent
    for prefix in ("A", "B"):
        scal_grupe(katalog, prefix)


if __name__ == "__main__":
    main()
