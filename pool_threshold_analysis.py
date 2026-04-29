#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WIELOPROGOWA ANALIZA ZYSKÓW I STRAT POOLI  (v1.0)

Dla każdej puli z danych JSONL buduje wektor labeli:
  - hit_10, hit_20, hit_30, ...  (max_return >= T)
  - loss_10, loss_20, loss_30, ... (min_return <= -T)

a następnie dla każdego progu T oblicza:
  1. hit_rate(T)        — P(max_return >= T)
  2. hit_rate_buy(T)    — P(max_return >= T | verdict = BUY)
  3. FN(T)              — false negatives: hit_T=1 ale verdict = REJECT
  4. risk_rate(T)       — P(min_return <= -T)
  5. risk_rate_buy(T)   — P(min_return <= -T | verdict = BUY)
  6. upside_no_drawdown(T)      — P(max_return >= T AND min_return > -T)
  7. upside_no_drawdown_buy(T)  — j.w. ale tylko dla BUY

Użycie:
  python3 pool_threshold_analysis.py <plik.jsonl> [--thresholds 10,20,30,40,50,60,70,80,90,100]
"""

import json
import sys
import os
from collections import defaultdict
from dataclasses import dataclass, field
from typing import List, Dict, Tuple, Optional

# ─── Kolory ANSI ─────────────────────────────────────────────────────────────
class C:
    RESET   = "\033[0m"
    BOLD    = "\033[1m"
    RED     = "\033[91m"
    GREEN   = "\033[92m"
    YELLOW  = "\033[93m"
    BLUE    = "\033[94m"
    MAGENTA = "\033[95m"
    CYAN    = "\033[96m"
    WHITE   = "\033[97m"
    GRAY    = "\033[90m"

# ─── Konfiguracja progów ─────────────────────────────────────────────────────
DEFAULT_THRESHOLDS = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]


@dataclass
class PoolRecord:
    """Pojedynczy rekord puli z danymi potrzebnymi do analizy."""
    pool_id: str
    max_return: float
    min_return: float
    verdict: str  # "BUY" lub "REJECT"

    # Wygenerowane labele
    hits: Dict[int, bool] = field(default_factory=dict)
    losses: Dict[int, bool] = field(default_factory=dict)


def parse_verdict(rec: dict) -> str:
    """Wyciąga verdict z rekordu — obsługuje różne nazwy pola."""
    for key in ("verdict_type", "verdict", "decision_verdict"):
        val = rec.get(key)
        if isinstance(val, str) and val.upper() in ("BUY", "REJECT"):
            return val.upper()
    # Fallback: sprawdź decision_verdict_buy (bool)
    dvb = rec.get("decision_verdict_buy")
    if isinstance(dvb, bool):
        return "BUY" if dvb else "REJECT"
    return "UNKNOWN"


def load_records(path: str) -> List[PoolRecord]:
    """Wczytuje plik JSONL i konwertuje na listę PoolRecord."""
    records = []
    with open(path, "r", encoding="utf-8") as fh:
        for line_no, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as e:
                print(f"  {C.YELLOW}⚠ Pomijam linię {line_no}: {e}{C.RESET}")
                continue

            max_ret = rec.get("threshold_window_max_return_pct")
            min_ret = rec.get("threshold_window_min_return_pct")
            verdict = parse_verdict(rec)

            if max_ret is None and min_ret is None:
                continue  # brak danych → pomiń

            pool_id = rec.get("pool_id", rec.get("ab_record_id", f"line_{line_no}"))

            records.append(PoolRecord(
                pool_id=str(pool_id),
                max_return=float(max_ret) if max_ret is not None else 0.0,
                min_return=float(min_ret) if min_ret is not None else 0.0,
                verdict=verdict,
            ))

    return records


def build_labels(records: List[PoolRecord], thresholds: List[int]) -> None:
    """Dla każdego rekordu generuje hit_T i loss_T dla wszystkich progów."""
    for rec in records:
        for t in thresholds:
            rec.hits[t] = rec.max_return >= t
            rec.losses[t] = rec.min_return <= -t


# ─── Funkcje analityczne ─────────────────────────────────────────────────────

def compute_hit_rate(records: List[PoolRecord], t: int) -> float:
    """P(max_return >= T)"""
    total = len(records)
    if total == 0:
        return 0.0
    hits = sum(1 for r in records if r.hits[t])
    return hits / total


def compute_hit_rate_buy(records: List[PoolRecord], t: int) -> float:
    """P(max_return >= T | verdict = BUY)"""
    buy_records = [r for r in records if r.verdict == "BUY"]
    if not buy_records:
        return 0.0
    hits = sum(1 for r in buy_records if r.hits[t])
    return hits / len(buy_records)


def compute_false_negatives(records: List[PoolRecord], t: int) -> Tuple[int, List[PoolRecord]]:
    """Pool ma hit_T=1, ale verdict = REJECT. Zwraca (count, lista_rekordów)."""
    fn = [r for r in records if r.hits[t] and r.verdict == "REJECT"]
    return len(fn), fn


def compute_risk_rate(records: List[PoolRecord], t: int) -> float:
    """P(min_return <= -T)"""
    total = len(records)
    if total == 0:
        return 0.0
    losses = sum(1 for r in records if r.losses[t])
    return losses / total


def compute_risk_rate_buy(records: List[PoolRecord], t: int) -> float:
    """P(min_return <= -T | verdict = BUY)"""
    buy_records = [r for r in records if r.verdict == "BUY"]
    if not buy_records:
        return 0.0
    losses = sum(1 for r in buy_records if r.losses[t])
    return losses / len(buy_records)


def compute_upside_no_drawdown(records: List[PoolRecord], t: int) -> float:
    """P(max_return >= T AND min_return > -T) — zysk bez dużego drawdownu."""
    total = len(records)
    if total == 0:
        return 0.0
    good = sum(1 for r in records if r.hits[t] and not r.losses[t])
    return good / total


def compute_upside_no_drawdown_buy(records: List[PoolRecord], t: int) -> float:
    """P(max_return >= T AND min_return > -T | verdict = BUY)"""
    buy_records = [r for r in records if r.verdict == "BUY"]
    if not buy_records:
        return 0.0
    good = sum(1 for r in buy_records if r.hits[t] and not r.losses[t])
    return good / len(buy_records)


# ─── Formatowanie wyjścia ────────────────────────────────────────────────────

def fmt_pct(val: float) -> str:
    return f"{val * 100:6.1f}%"


def print_separator(char: str = "─", width: int = 110):
    print(f"{C.GRAY}{char * width}{C.RESET}")


def print_header(title: str):
    print(f"\n{C.BOLD}{C.CYAN}{'=' * 110}{C.RESET}")
    print(f"{C.BOLD}{C.CYAN}  {title}{C.RESET}")
    print(f"{C.BOLD}{C.CYAN}{'=' * 110}{C.RESET}\n")


def print_summary(records: List[PoolRecord], thresholds: List[int]):
    """Drukuje tabelę podsumowującą dla wszystkich progów."""

    total = len(records)
    buy_count = sum(1 for r in records if r.verdict == "BUY")
    reject_count = sum(1 for r in records if r.verdict == "REJECT")
    unknown_count = sum(1 for r in records if r.verdict == "UNKNOWN")

    print(f"\n{C.BOLD}📊 DANE WEJŚCIOWE{C.RESET}")
    print(f"   Wszystkie poole: {total}")
    print(f"   BUY:    {C.GREEN}{buy_count}{C.RESET} ({buy_count/total*100:.1f}%)" if total else "")
    print(f"   REJECT: {C.RED}{reject_count}{C.RESET} ({reject_count/total*100:.1f}%)" if total else "")
    if unknown_count:
        print(f"   UNKNOWN: {C.YELLOW}{unknown_count}{C.RESET}")

    print(f"\n{C.BOLD}📈 PROGI ZYSKÓW (UPSIDE){C.RESET}")
    print_separator()
    header = (
        f"{'Próg':>6s} │ {'hit_rate':>8s} │ {'hit_rate':>8s} │ {'FN':>5s} │ "
        f"{'risk_rate':>9s} │ {'risk_rate':>9s} │ {'upside_no':>11s} │ {'upside_no':>11s}"
    )
    subheader = (
        f"{'T%':>6s} │ {'ogółem':>8s} │ {'BUY':>8s} │ {'count':>5s} │ "
        f"{'ogółem':>9s} │ {'BUY':>9s} │ {'DD ogółem':>11s} │ {'DD BUY':>11s}"
    )
    print(f"{C.GRAY}{header}{C.RESET}")
    print(f"{C.GRAY}{subheader}{C.RESET}")
    print_separator()

    for t in thresholds:
        hr = compute_hit_rate(records, t)
        hr_buy = compute_hit_rate_buy(records, t)
        fn_count, _ = compute_false_negatives(records, t)
        rr = compute_risk_rate(records, t)
        rr_buy = compute_risk_rate_buy(records, t)
        und = compute_upside_no_drawdown(records, t)
        und_buy = compute_upside_no_drawdown_buy(records, t)

        # Podświetlenie FN
        fn_str = f"{fn_count:>5d}"
        if fn_count > 0:
            fn_str = f"{C.RED}{fn_str}{C.RESET}"

        print(
            f"{t:>5d}% │ {fmt_pct(hr)} │ {fmt_pct(hr_buy)} │ {fn_str} │ "
            f"{fmt_pct(rr)} │ {fmt_pct(rr_buy)} │ {fmt_pct(und):>11s} │ {fmt_pct(und_buy):>11s}"
        )

    print_separator()
    print(f"  hit_rate      = P(max_return >= T)")
    print(f"  hit_rate BUY  = P(max_return >= T | verdict = BUY)")
    print(f"  FN            = pule z hit_T=1 ale REJECT (false negatives — stracone złoto)")
    print(f"  risk_rate     = P(min_return <= -T)")
    print(f"  upside_no_DD  = P(max_return >= T AND min_return > -T)  ★ zysk bez dużego drawdownu")


def print_false_negatives_detail(records: List[PoolRecord], thresholds: List[int]):
    """Drukuje szczegółową listę false negatives (straconych okazji)."""
    print_header("🔴 FALSE NEGATIVES — POOLE KTÓRE ZROBIŁY TARGET A ZOSTAŁY ODRZUCONE")

    for t in thresholds:
        fn_count, fn_records = compute_false_negatives(records, t)
        if fn_count == 0:
            continue

        print(f"\n{C.BOLD}  Próg +{t}% — {fn_count} false negatives:{C.RESET}")
        # Sortuj po max_return malejąco
        fn_records_sorted = sorted(fn_records, key=lambda r: r.max_return, reverse=True)
        for i, rec in enumerate(fn_records_sorted[:10], 1):
            loss_info = ""
            if rec.min_return <= -t:
                loss_info = f" {C.YELLOW}[także loss_{t}]{C.RESET}"
            print(
                f"    {i:>2d}. {C.RED}{rec.pool_id[:50]}{C.RESET}  "
                f"max_return={C.GREEN}{rec.max_return:+.1f}%{C.RESET}  "
                f"min_return={C.RED}{rec.min_return:+.1f}%{C.RESET}{loss_info}"
            )
        if fn_count > 10:
            print(f"    {C.GRAY}... i {fn_count - 10} więcej{C.RESET}")


def print_buy_quality_curve(records: List[PoolRecord], thresholds: List[int]):
    """Drukuje krzywą jakości BUY — hit_rate_buy vs próg."""
    print_header("📈 KRZYWA JAKOŚCI GATEKEEPERA: hit_rate_buy(T)")

    print(f"  {C.BOLD}{'Próg T':>8s}  {'hit_rate_buy(T)':>16s}  {'Wizualizacja'}{C.RESET}")
    print_separator("-", 90)

    max_hr = max(compute_hit_rate_buy(records, t) for t in thresholds) if thresholds else 1.0
    for t in thresholds:
        hr = compute_hit_rate_buy(records, t)
        bar_len = int(hr / max_hr * 50) if max_hr > 0 else 0
        bar = f"{C.GREEN}{'█' * bar_len}{C.RESET}{C.GRAY}{'░' * (50 - bar_len)}{C.RESET}"
        highlight = ""
        if hr >= 0.5:
            highlight = f" {C.GREEN}★ mocny sygnał{C.RESET}"
        elif hr >= 0.3:
            highlight = f" {C.YELLOW}● umiarkowany{C.RESET}"
        print(f"  {t:>6d}% │ {fmt_pct(hr):>8s} │ {bar}{highlight}")

    print_separator("-", 90)
    print(f"  {C.GRAY}Im wyższy hit_rate_buy dla danego T, tym lepiej Gatekeeper"
          f" trafia w poole z potencjałem ≥T%.{C.RESET}")


def print_distribution_summary(records: List[PoolRecord]):
    """Drukuje podstawowe statystyki rozkładów max_return i min_return."""
    print_header("📊 ROZKŁADY MAX_RETURN I MIN_RETURN")

    buy_recs = [r for r in records if r.verdict == "BUY"]
    reject_recs = [r for r in records if r.verdict == "REJECT"]

    def _stats(name: str, recs: List[PoolRecord], field: str):
        vals = [getattr(r, field) for r in recs if getattr(r, field) != 0.0 or True]
        if not vals:
            return
        vals_sorted = sorted(vals)
        n = len(vals_sorted)
        p10 = vals_sorted[int(n * 0.10)] if n > 1 else vals_sorted[0]
        p25 = vals_sorted[int(n * 0.25)] if n > 1 else vals_sorted[0]
        p50 = vals_sorted[int(n * 0.50)] if n > 1 else vals_sorted[0]
        p75 = vals_sorted[int(n * 0.75)] if n > 1 else vals_sorted[0]
        p90 = vals_sorted[int(n * 0.90)] if n > 1 else vals_sorted[0]
        avg = sum(vals) / n

        print(f"  {C.BOLD}{name} ({n} pooli) — {field}{C.RESET}")
        print(f"    min={min(vals):+.1f}%  p10={p10:+.1f}%  p25={p25:+.1f}%  "
              f"median={p50:+.1f}%  p75={p75:+.1f}%  p90={p90:+.1f}%  max={max(vals):+.1f}%  "
              f"avg={avg:+.1f}%")

    _stats("Wszystkie", records, "max_return")
    _stats("BUY      ", buy_recs, "max_return")
    _stats("REJECT   ", reject_recs, "max_return")
    print()
    _stats("Wszystkie", records, "min_return")
    _stats("BUY      ", buy_recs, "min_return")
    _stats("REJECT   ", reject_recs, "min_return")


def print_combined_upside_downside(records: List[PoolRecord], thresholds: List[int]):
    """Najważniejsza tabela: upside_no_drawdown w rozbiciu na BUY/REJECT."""
    print_header("🔥 UPSIDE BEZ DUŻEGO DRAWDOWNU (NAJWAŻNIEJSZA METRYKA)")

    print(f"  {C.BOLD}P(max_return >= +T  AND  min_return > -T){C.RESET}")
    print(f"  {C.GRAY}Czyli: ile pooli daje target bez wcześniejszego dużego spadku.{C.RESET}")
    print()
    print_separator()
    print(f"  {'T':>6s} │ {'ogółem':>10s} │ {'BUY':>10s} │ {'REJECT':>10s} │ {'BUY count':>10s} │ {'REJECT count':>12s}")
    print_separator()

    for t in thresholds:
        total_good = sum(1 for r in records if r.hits[t] and not r.losses[t])
        buy_good = sum(1 for r in records if r.verdict == "BUY" and r.hits[t] and not r.losses[t])
        reject_good = sum(1 for r in records if r.verdict == "REJECT" and r.hits[t] and not r.losses[t])

        total_n = len(records)
        buy_n = sum(1 for r in records if r.verdict == "BUY")
        reject_n = sum(1 for r in records if r.verdict == "REJECT")

        print(
            f"  {t:>5d}% │ {total_good/total_n*100:>9.1f}% │ "
            f"{buy_good/buy_n*100 if buy_n else 0:>9.1f}% │ "
            f"{reject_good/reject_n*100 if reject_n else 0:>9.1f}% │ "
            f"{buy_good:>10d} │ {reject_good:>12d}"
        )

    print_separator()
    print(f"  {C.GRAY}Kolumna REJECT pokazuje ile dobrych pooli zostało ODRZUCONYCH"
          f" — to Twój potencjalny zysk utracony.{C.RESET}")


# ─── MAIN ────────────────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 2:
        print(f"{C.BOLD}Użycie:{C.RESET} python3 pool_threshold_analysis.py <plik.jsonl> [--thresholds 10,20,30,...]")
        print(f"\n{C.GRAY}Plik JSONL musi zawierać pola:")
        print(f"  - threshold_window_max_return_pct  (float)")
        print(f"  - threshold_window_min_return_pct  (float)")
        print(f"  - verdict_type lub verdict         (string: BUY/REJECT)")
        print(f"  - pool_id                          (string, opcjonalne){C.RESET}")
        sys.exit(1)

    path = sys.argv[1]
    if not os.path.exists(path):
        print(f"{C.RED}✗ Plik nie istnieje: {path}{C.RESET}")
        sys.exit(1)

    # Parsuj progi z argumentów
    thresholds = DEFAULT_THRESHOLDS
    for arg in sys.argv[2:]:
        if arg.startswith("--thresholds="):
            try:
                thresholds = [int(x.strip()) for x in arg.split("=", 1)[1].split(",") if x.strip()]
            except ValueError:
                print(f"{C.YELLOW}⚠ Nieprawidłowy format --thresholds, używam domyślnych{C.RESET}")

    print(f"{C.BOLD}{C.BLUE}╔══════════════════════════════════════════════════════════════════╗{C.RESET}")
    print(f"{C.BOLD}{C.BLUE}║  WIELOPROGOWA ANALIZA ZYSKÓW I STRAT POOLI  (v1.0)              ║{C.RESET}")
    print(f"{C.BOLD}{C.BLUE}╚══════════════════════════════════════════════════════════════════╝{C.RESET}")
    print(f"\n  Plik:    {C.WHITE}{path}{C.RESET}")
    print(f"  Progi:   {C.WHITE}{thresholds}{C.RESET}")

    # ── Wczytaj dane ─────────────────────────────────────────────────────────
    records = load_records(path)
    if not records:
        print(f"\n{C.RED}✗ Brak rekordów z wymaganymi polami w pliku.{C.RESET}")
        sys.exit(1)

    print(f"  Rekordy: {C.WHITE}{len(records)}{C.RESET}")

    # ── Generuj labele ───────────────────────────────────────────────────────
    build_labels(records, thresholds)

    # ── Przykładowe rekordy ──────────────────────────────────────────────────
    print_header("📋 PRZYKŁADOWE REKORDY Z LABELAMI")
    print(f"  {'pool_id':>50s} │ {'verdict':>7s} │ max_ret │ min_ret │", end="")
    for t in thresholds:
        print(f" hit_{t}", end="")
    print(f" │", end="")
    for t in thresholds:
        print(f" loss_{t}", end="")
    print()
    print_separator()

    for rec in records[:8]:
        pid = rec.pool_id[:48]
        vcolor = C.GREEN if rec.verdict == "BUY" else C.RED if rec.verdict == "REJECT" else C.YELLOW
        print(
            f"  {pid:>50s} │ {vcolor}{rec.verdict:>7s}{C.RESET} │ "
            f"{rec.max_return:>+6.0f}% │ {rec.min_return:>+6.0f}% │",
            end=""
        )
        for t in thresholds:
            mark = f" {C.GREEN}1{C.RESET}" if rec.hits[t] else f" {C.GRAY}0{C.RESET}"
            print(mark, end="")
        print(f" │", end="")
        for t in thresholds:
            mark = f" {C.RED}1{C.RESET}" if rec.losses[t] else f" {C.GRAY}0{C.RESET}"
            print(mark, end="")
        print()

    if len(records) > 8:
        print(f"  {C.GRAY}... i {len(records) - 8} więcej rekordów{C.RESET}")

    # ── Rozkłady ────────────────────────────────────────────────────────────
    print_distribution_summary(records)

    # ── Główna tabela ───────────────────────────────────────────────────────
    print_header("📊 TABELA METRYK DLA WSZYSTKICH PROGÓW")
    print_summary(records, thresholds)

    # ── Krzywa jakości BUY ──────────────────────────────────────────────────
    print_buy_quality_curve(records, thresholds)

    # ── Upside bez drawdownu ─────────────────────────────────────────────────
    print_combined_upside_downside(records, thresholds)

    # ── False negatives ──────────────────────────────────────────────────────
    print_false_negatives_detail(records, thresholds)

    print(f"\n{C.BOLD}{C.GREEN}✅ Analiza zakończona.{C.RESET}\n")


if __name__ == "__main__":
    main()
