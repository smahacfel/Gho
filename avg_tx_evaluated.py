#!/usr/bin/env python3
import json

FILE = "/root/Gho/logs/decisions.jsonl/gatekeeper_v2_decisions.jsonl"

total = 0
count = 0

skipped = 0
with open(FILE) as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            val = json.loads(line).get("total_tx_evaluated")
        except json.JSONDecodeError:
            skipped += 1
            continue
        if val is not None and val != 0:
            total += val
            count += 1

if skipped:
    print(f"Pominięto (błąd JSON): {skipped}")
if count == 0:
    print("Brak rekordów z polem total_tx_evaluated.")
else:
    print(f"Rekordów: {count}")
    print(f"Suma:     {total}")
    print(f"Średnia:  {total / count:.4f}")
