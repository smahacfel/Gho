#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
📊 ANALIZA PORÓWNAWCZA ZBIORÓW A vs B  (v5.0 — JSONL v7+ + Sybil Interference)

Skrypt wczytuje dwa pliki JSONL (zbior_A.jsonl, zbior_B.jsonl) i przeprowadza
wielopoziomową analizę z filtracją A/B i deduplikacją:

  SEKCJA 0  — Filtracja A/B: ab_window_complete, origin, dedup po ab_record_id
  SEKCJA 1  — Profil zbioru A: rozkłady, korelacje wewnętrzne, cechy wspólne
  SEKCJA 2  — Profil zbioru B: rozkłady, korelacje wewnętrzne, cechy wspólne
  SEKCJA 3  — Porównanie A vs B: zestawienie rozkładów, testy statystyczne,
              effect size (Cohen's d), różnice w korelacjach, analiza
              dyskryminacyjna (cechy najlepiej separujące zbiory)
  SEKCJA 4  — Deep dive: subtelne wzorce, interakcje, profil wielowymiarowy
  SEKCJA 5  — Podsumowanie: wnioski, rekomendacje, A/B integrity, fingerprint check
  SEKCJA 6  — Kształt Czasu (DTW na vectors_d_price / vectors_interval_ms)
  SEKCJA 7  — Odkrywanie Przyczynowości (Causal Discovery — Algorytm PC)
  SEKCJA 8  — Topologiczna Analiza Danych (TDA — Szukanie Dziur)
  SEKCJA 9  — Nieliniowa Wzajemna Informacja (MI + fingerprinty + vector features)
  SEKCJA 10 — Analiza Kruchości i Grubych Ogonów (Hill na wektorach v3)
  ── NOWE (v4.0) ────────────────────────────────────────────────────────────
  SEKCJA 11 — Optymalne Progi Separujące (Youden J + KS per cecha)
  SEKCJA 12 — Ranking AUC + Testy Istotności (Mann–Whitney p-value)
  SEKCJA 13 — Nakładanie Rozkładów (Bhattacharyya BC + Overlap OVL)
  SEKCJA 14 — Regresja Logistyczna L1 — feature importance (sklearn, opcjonalne)
  SEKCJA 15 — Kombinowana Reguła Scoringowa (top-K progów → score → ocena)
  SEKCJA 16 — Bootstrap CI dla Progów (stabilność n=250)
  SEKCJA 17 — Gotowa Reguła Decyzyjna — progi gotowe do implementacji w bocie
  ── NOWE (v5.0) — Sybil Interference Layer ─────────────────────────────────
  SEKCJA 18 — Analiza warstwy Sybil Interference:
              FTDI (fee_topology_diversity_index)
              DBIA (dev_buyer_infrastructure_affinity)
              SFD  (spend_fraction_divergence)
              DES  (demand_elasticity_score)
              CPV  (signer_cross_pool_velocity)
              FSC  (funding_source_concentration)
              + sybil_soft_points, sybil_soft_flags, sybil_lead_signal,
              + degradacja coverage, korelacje z innymi cechami,
              + fingerprint z-score A vs B, syntetyczna ocena warstwy

Uruchomienie:
  python3 analiza_porownawcza.py [ścieżka_do_A.jsonl] [ścieżka_do_B.jsonl]
  (domyślnie: /root/Ghost/logs/decisions.jsonl/zbior_A.jsonl i zbior_B.jsonl)

Parametry (env vars):
  AB_WINDOW_MS     — oczekiwany ab_window_ms (domyślnie: 10000)
  AB_MIN_TX        — minimalne ab_tx_count_window (domyślnie: 10)
  AB_MIN_VEC_LEN   — minimalna długość wektora dla DTW/Hill (domyślnie: 20)
"""

import json
import sys
import math
import os
import io
import re
import datetime
from bisect import bisect_left, bisect_right
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

try:
    import numpy as np
    _HAS_NUMPY = True
except ImportError:
    _HAS_NUMPY = False

# --- Opcjonalne zależności (sekcje 6–10) ---
try:
    from fastdtw import fastdtw
    from scipy.spatial.distance import euclidean as scipy_euclidean
    _HAS_DTW = True
except ImportError:
    _HAS_DTW = False

try:
    from causallearn.search.ConstraintBased.PC import pc as pc_algorithm
    import networkx as nx
    _HAS_CAUSAL = True
except ImportError:
    _HAS_CAUSAL = False

try:
    from ripser import ripser
    from persim import wasserstein as persim_wasserstein
    _HAS_TDA = True
except ImportError:
    _HAS_TDA = False

try:
    from sklearn.feature_selection import mutual_info_regression
    _HAS_MI = True
except ImportError:
    _HAS_MI = False


# ══════════════════════════════════════════════════════════════════════════════
#  CONFIG — A/B filtering & vector settings
#  Wartości ENV mają priorytet; jeśli nieustalone — autodetect z danych.
# ══════════════════════════════════════════════════════════════════════════════
_ENV_WINDOW_MS   = os.environ.get("AB_WINDOW_MS")    # None = autodetect
_ENV_MIN_TX      = os.environ.get("AB_MIN_TX")        # None = autodetect (→ 0)
_ENV_MIN_VEC_LEN = os.environ.get("AB_MIN_VEC_LEN")   # None = autodetect

@dataclass(frozen=True)
class FilterConfig:
        expected_window_ms: int = 0
        min_tx_in_window: int = 0
        min_vector_len: int = 0
        allow_any_window_ms: bool = False
        drop_fallback_origin: bool = True
        dedup_by_record_id: bool = True


def autodetect_filter_params(rec_a_raw: list, rec_b_raw: list) -> tuple[FilterConfig, list[str]]:
    """
        Analizuje surowe rekordy obu zbiorów i zwraca obiekt FilterConfig:
            expected_window_ms  — dominująca wartość ab_window_ms w danych
            min_tx_in_window    — wyłączone (0), bo ab_tx_count_window liczy tx w oknie A/B
                                                         które przy krótkich oknach może być 0 dla prawidłowych rekordów
            min_vector_len      — 25-ty percentyl długości vectors_d_price (min 3), albo 0 jeśli
                                                         wektory w ogóle nie występują
            allow_any_window_ms — True gdy dane mają więcej niż jedną dominującą wartość okna

    ENV vars AB_WINDOW_MS / AB_MIN_TX / AB_MIN_VEC_LEN mają zawsze priorytet.
    """
    all_raw = rec_a_raw + rec_b_raw
    notes: list[str] = []
    expected_window_ms = int(_ENV_WINDOW_MS) if _ENV_WINDOW_MS else 0
    min_tx_in_window = int(_ENV_MIN_TX) if _ENV_MIN_TX else 0
    min_vector_len = int(_ENV_MIN_VEC_LEN) if _ENV_MIN_VEC_LEN else 0
    allow_any_window_ms = False

    # ── 1. Autodetect ab_window_ms ──────────────────────────────────────────
    if not _ENV_WINDOW_MS:
        window_counts: dict = {}
        for r in all_raw:
            wms = r.get("ab_window_ms")
            if isinstance(wms, (int, float)) and wms > 0:
                key = int(wms)
                window_counts[key] = window_counts.get(key, 0) + 1

        if window_counts:
            dominant_wms = max(window_counts, key=window_counts.get)
            dominant_cnt = window_counts[dominant_wms]
            total_with_wms = sum(window_counts.values())
            dominant_pct = dominant_cnt / total_with_wms * 100

            if dominant_pct >= 80:
                # Wyraźna dominacja jednej wartości — filtruj po niej
                expected_window_ms = dominant_wms
                allow_any_window_ms = False
            else:
                # Kilka wartości okna — nie filtruj, bierz wszystkie
                expected_window_ms = dominant_wms
                allow_any_window_ms = True

            notes.append(
                f"ab_window_ms: dominujące={dominant_wms}ms ({dominant_pct:.0f}%)"
                + (f"  [rozkład: {dict(sorted(window_counts.items()))}]" if len(window_counts) > 1 else "")
                + ("  → ACCEPT_ALL (wiele okien)" if allow_any_window_ms else "")
            )
        else:
            # Pole ab_window_ms nie istnieje lub zawsze None — pomiń walidację
            expected_window_ms = 0
            allow_any_window_ms = True
            notes.append("ab_window_ms: brak w danych → walidacja okna wyłączona")

    # ── 2. Autodetect MIN_TX_IN_WINDOW ─────────────────────────────────────
    if not _ENV_MIN_TX:
        # ab_tx_count_window = liczba tx WEWNĄTRZ krótkiego okna A/B (może być 0
        # dla świeżych poolów z krótkim oknem). Filtr domyślnie wyłączamy.
        min_tx_in_window = 0
        notes.append(
            "ab_tx_count_window: filtr WYŁĄCZONY (wartość 0) — "
            "pole może być 0 dla prawidłowych świeżych poolów"
        )

    # ── 3. Autodetect MIN_VECTOR_LEN ────────────────────────────────────────
    if not _ENV_MIN_VEC_LEN:
        vec_lens = []
        for r in all_raw:
            v = r.get("vectors_d_price")
            if isinstance(v, list) and len(v) > 0:
                vec_lens.append(len(v))

        if vec_lens:
            vec_lens.sort()
            p25_len = vec_lens[max(0, int(len(vec_lens) * 0.25))]
            pct_have_vecs = len(vec_lens) / len(all_raw) * 100 if all_raw else 0
            # Domyślnie: filtr WYŁĄCZONY (0) — aby nie odrzucać rekordów bez wektorów.
            # DTW/Hill i tak pomijają rekordy bez wektorów wewnętrznie.
            # Aktywuj: AB_MIN_VEC_LEN=<liczba> (np. AB_MIN_VEC_LEN=5)
            min_vector_len = 0
            notes.append(
                f"vectors_d_price: {len(vec_lens)}/{len(all_raw)} rekordów ({pct_have_vecs:.0f}%) "
                f"ma wektory | min={min(vec_lens)} max={max(vec_lens)} p25={p25_len} "
                f"→ MIN_VECTOR_LEN=0 (WYŁĄCZONY — ustaw AB_MIN_VEC_LEN=N aby aktywować)"
            )
        else:
            min_vector_len = 0
            notes.append(
                "vectors_d_price: BRAK w danych → warunek długości wektora wyłączony"
            )
    return FilterConfig(
        expected_window_ms=expected_window_ms,
        min_tx_in_window=min_tx_in_window,
        min_vector_len=min_vector_len,
        allow_any_window_ms=allow_any_window_ms,
    ), notes


# ══════════════════════════════════════════════════════════════════════════════
#  TEE WRITER — przechwytuje stdout i zapisuje do bufora równolegle z wydrukiem
# ══════════════════════════════════════════════════════════════════════════════
class TeeWriter:
    """Przekierowuje sys.stdout: pisze jednocześnie do terminala i do bufora."""
    def __init__(self, original):
        self.original = original
        self.buf = io.StringIO()

    def write(self, text):
        self.original.write(text)
        self.buf.write(text)

    def flush(self):
        self.original.flush()

    def get_captured(self) -> str:
        return self.buf.getvalue()


# ══════════════════════════════════════════════════════════════════════════════
#  ANSI → HTML
# ══════════════════════════════════════════════════════════════════════════════
_ANSI_COLORS = {
    "0":  None,          # reset
    "1":  "font-weight:bold",
    "2":  "opacity:0.55",
    "91": "color:#ff5f5f",
    "92": "color:#5fff5f",
    "93": "color:#ffff5f",
    "94": "color:#5f87ff",
    "95": "color:#ff5fff",
    "96": "color:#5fffff",
    "97": "color:#ffffff",
    "41": "background:#ff5f5f;color:#000",
    "42": "background:#5fff5f;color:#000",
}


def ansi_to_html(text: str) -> str:
    """Konwertuje ANSI escape codes na HTML <span> ze stylami CSS."""
    parts = re.split(r"(\033\[[0-9;]*m)", text)
    out = []
    depth = 0
    for part in parts:
        m = re.fullmatch(r"\033\[([0-9;]*)m", part)
        if m:
            codes = m.group(1).split(";")
            for code in codes:
                css = _ANSI_COLORS.get(code)
                if css is None and code in ("0", ""):
                    # reset
                    out.append("</span>" * depth)
                    depth = 0
                elif css:
                    out.append(f'<span style="{css}">')
                    depth += 1
        else:
            safe = part.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            out.append(safe)
    out.append("</span>" * depth)
    return "".join(out)


def save_html_report(captured: str, path_a: str, path_b: str) -> str:
    """Zapisuje pełny raport jako plik HTML z motywem terminalowym."""
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    ts_file   = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    body = ansi_to_html(captured)

    html = f"""<!DOCTYPE html>
<html lang="pl">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Analiza A vs B &#8212; {timestamp}</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      background: #0e0e0e;
      color: #c8c8c8;
      font-family: 'Cascadia Code', 'Fira Code', 'Courier New', monospace;
      font-size: 13px;
      line-height: 1.45;
      padding: 24px 32px;
    }}
    pre {{
      white-space: pre-wrap;
      word-break: break-all;
    }}
    .meta {{
      color: #666;
      font-size: 11px;
      margin-bottom: 16px;
      border-bottom: 1px solid #222;
      padding-bottom: 8px;
    }}
  </style>
</head>
<body>
<div class="meta">Wygenerowano: {timestamp} &nbsp;|&nbsp; A: {path_a} &nbsp;|&nbsp; B: {path_b}</div>
<pre>{body}</pre>
</body>
</html>
"""

    out_dir = Path(path_a).parent
    out_path = out_dir / f"analiza_{ts_file}.html"
    out_path.write_text(html, encoding="utf-8")
    return str(out_path)

# ══════════════════════════════════════════════════════════════════════════════
#  KOLORY TERMINALA
# ══════════════════════════════════════════════════════════════════════════════
class C:
    RESET   = "\033[0m";  BOLD    = "\033[1m";  DIM     = "\033[2m"
    RED     = "\033[91m"; GREEN   = "\033[92m";  YELLOW  = "\033[93m"
    BLUE    = "\033[94m"; MAGENTA = "\033[95m";  CYAN    = "\033[96m"
    WHITE   = "\033[97m"; BG_RED  = "\033[41m";  BG_GREEN= "\033[42m"

def hdr(title: str, color=C.CYAN):
    w = 80
    print(f"\n{color}{C.BOLD}{'═'*w}{C.RESET}")
    print(f"{color}{C.BOLD}  {title}{C.RESET}")
    print(f"{color}{C.BOLD}{'═'*w}{C.RESET}")

def sub(title: str, color=C.BLUE):
    print(f"\n{color}{C.BOLD}  ▶ {title}{C.RESET}")
    print(f"{color}  {'─'*70}{C.RESET}")

def row(label: str, value: str, color=C.WHITE, indent=4):
    pad = " " * indent
    print(f"{pad}{C.DIM}{label:<48}{C.RESET}{color}{value}{C.RESET}")

def warn(msg: str):  print(f"  {C.YELLOW}⚠  {msg}{C.RESET}")
def ok(msg: str):    print(f"  {C.GREEN}✓  {msg}{C.RESET}")
def bad(msg: str):   print(f"  {C.RED}✗  {msg}{C.RESET}")
def note(msg: str):  print(f"  {C.CYAN}ℹ  {msg}{C.RESET}")
def hint(msg: str):  print(f"  {C.DIM}// {msg}{C.RESET}")

# ══════════════════════════════════════════════════════════════════════════════
#  STATYSTYKI PODSTAWOWE
# ══════════════════════════════════════════════════════════════════════════════
def mean(xs):
    return math.fsum(xs) / len(xs) if xs else 0.0

def median_val(xs):
    if not xs: return 0.0
    s = sorted(xs); n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2

def std(xs):
    if len(xs) < 2: return 0.0
    m = mean(xs)
    return math.sqrt(math.fsum((x - m) ** 2 for x in xs) / (len(xs) - 1))

def percentile(xs, p):
    if not xs: return 0.0
    s = sorted(xs)
    idx = (len(s) - 1) * p / 100
    lo, hi = int(idx), min(int(idx) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (idx - lo)

def iqr(xs):
    return percentile(xs, 75) - percentile(xs, 25) if len(xs) >= 4 else 0.0

def mad(xs):
    if not xs: return 0.0
    med = median_val(xs)
    return median_val([abs(x - med) for x in xs])

def cohen_d(a, b):
    if len(a) < 2 or len(b) < 2: return float('nan')
    pooled = math.sqrt((std(a) ** 2 * (len(a) - 1) + std(b) ** 2 * (len(b) - 1))
                        / (len(a) + len(b) - 2))
    return (mean(a) - mean(b)) / pooled if pooled > 0 else float('nan')

def cohen_d_label(d):
    """Opis effect size."""
    a = abs(d)
    if math.isnan(a): return "N/A", C.DIM
    if a >= 1.2: return "OGROMNY",  C.RED if d < 0 else C.GREEN
    if a >= 0.8: return "DUŻY",     C.RED if d < 0 else C.GREEN
    if a >= 0.5: return "ŚREDNI",   C.YELLOW
    if a >= 0.2: return "MAŁY",     C.CYAN
    return "ZNIKOMY", C.DIM

def mann_whitney_u(xs, ys):
    """
    Aproksymacja Mann–Whitney U z normalizacją na [0,1].
    U/(n1*n2) → bliskie 0.5 = brak różnicy, 0 lub 1 = pełna separacja.
    """
    if not xs or not ys:
        return 0.5
    n1, n2 = len(xs), len(ys)
    tagged = [(float(x), 0) for x in xs] + [(float(y), 1) for y in ys]
    tagged.sort(key=lambda item: item[0])

    rank_sum_x = 0.0
    i = 0
    total = len(tagged)
    while i < total:
        j = i + 1
        value = tagged[i][0]
        while j < total and tagged[j][0] == value:
            j += 1
        avg_rank = (i + 1 + j) / 2.0
        x_count = sum(1 for _, group in tagged[i:j] if group == 0)
        rank_sum_x += avg_rank * x_count
        i = j

    u1 = rank_sum_x - n1 * (n1 + 1) / 2.0
    return u1 / (n1 * n2)

def rank_biserial(u_norm):
    """Rank-biserial correlation z U-normed: r = 2U/(n1*n2) - 1."""
    return 2 * u_norm - 1

# ══════════════════════════════════════════════════════════════════════════════
#  SPEARMAN + KORELACJE
# ══════════════════════════════════════════════════════════════════════════════
def rank_with_ties(lst):
    n = len(lst)
    sorted_idx = sorted(range(n), key=lambda i: lst[i])
    ranks = [0.0] * n
    i = 0
    while i < n:
        j = i
        while j < n and lst[sorted_idx[j]] == lst[sorted_idx[i]]:
            j += 1
        avg_rank = (i + j - 1) / 2.0 + 1
        for k in range(i, j):
            ranks[sorted_idx[k]] = avg_rank
        i = j
    return ranks

def pearson_raw(xs, ys):
    if len(xs) < 3: return 0.0
    if _HAS_NUMPY:
        arr_x = np.asarray(xs, dtype=float)
        arr_y = np.asarray(ys, dtype=float)
        if arr_x.size < 3 or arr_y.size < 3:
            return 0.0
        return float(np.corrcoef(arr_x, arr_y)[0, 1]) if np.std(arr_x) > 0 and np.std(arr_y) > 0 else 0.0
    mx, my = mean(xs), mean(ys)
    centered_x = [x - mx for x in xs]
    centered_y = [y - my for y in ys]
    num = math.fsum(x * y for x, y in zip(centered_x, centered_y))
    dx = math.sqrt(math.fsum(x * x for x in centered_x))
    dy = math.sqrt(math.fsum(y * y for y in centered_y))
    return num / (dx * dy) if dx > 0 and dy > 0 else 0.0


def collect_pairs(records, f1, f2):
    xs = []
    ys = []
    for r in records:
        x = get_val(r, f1)
        y = get_val(r, f2)
        if x is not None and y is not None:
            xs.append(x)
            ys.append(y)
    return xs, ys


def spearman_from_values(xs, ys):
    if len(xs) < 3:
        return 0.0, len(xs)
    return pearson_raw(rank_with_ties(xs), rank_with_ties(ys)), len(xs)

def spearman(records, f1, f2):
    xs, ys = collect_pairs(records, f1, f2)
    return spearman_from_values(xs, ys)

def corr_label(r):
    a = abs(r)
    sign = "+" if r >= 0 else "-"
    if a >= 0.85: return f"{sign}SILNA",    C.GREEN if r > 0 else C.RED
    if a >= 0.60: return f"{sign}UMIARKOW", C.YELLOW
    if a >= 0.35: return f"{sign}SŁABA",    C.CYAN
    return "BRAK", C.DIM

# ══════════════════════════════════════════════════════════════════════════════
#  EKSTRAKTORY
# ══════════════════════════════════════════════════════════════════════════════
def get_val(r, field):
    v = r.get(field)
    if isinstance(v, bool): return 1.0 if v else 0.0
    if isinstance(v, (int, float)):
        fv = float(v)
        if math.isfinite(fv): return fv
    return None

def iter_pairs(records, f1, f2):
    for r in records:
        x, y = get_val(r, f1), get_val(r, f2)
        if x is not None and y is not None:
            yield x, y

def extract(records, field):
    return [v for r in records if (v := get_val(r, field)) is not None]


def build_field_cache(records, fields):
    unique_fields = dict.fromkeys(fields)
    return {field: extract(records, field) for field in unique_fields}

def get_bool(r, field):
    """Return bool value from record, or None if missing."""
    v = r.get(field)
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return bool(v)
    if isinstance(v, str):
        return v.lower() in ("true", "1")
    return None

def get_str(r, field):
    """Return string value from record, or None if missing."""
    v = r.get(field)
    if v is None:
        return None
    return str(v)

def get_vector(r, field):
    """Return list/vector from record, filtering NaN values. Returns empty list if missing."""
    v = r.get(field)
    if not isinstance(v, list):
        return []
    return [x for x in v if isinstance(x, (int, float)) and math.isfinite(x)]

def vector_features(r):
    """Extract scalar features from vector fields for MI analysis."""
    if not _HAS_NUMPY:
        return {}
    feats = {}
    dp = get_vector(r, "vectors_d_price")
    iv = get_vector(r, "vectors_interval_ms")
    if len(dp) >= 5:
        arr = np.array(dp, dtype=float)
        abs_arr = np.abs(arr)
        feats["vf_d_price_std"] = float(np.std(arr, ddof=1)) if len(arr) > 1 else 0.0
        feats["vf_abs_d_price_p95"] = float(np.percentile(abs_arr, 95))
        mean_sign = float(np.mean(np.sign(arr)))
        feats["vf_d_price_skew_proxy"] = mean_sign * float(np.std(arr, ddof=1)) if len(arr) > 1 else 0.0
    if len(iv) >= 5:
        arr_iv = np.array(iv, dtype=float)
        m_iv = float(np.mean(arr_iv))
        s_iv = float(np.std(arr_iv, ddof=1)) if len(arr_iv) > 1 else 0.0
        feats["vf_interval_p95"] = float(np.percentile(arr_iv, 95))
        feats["vf_interval_cv"] = s_iv / m_iv if m_iv > 1e-9 else 0.0
    return feats

def filter_ab_records(records, name, config: FilterConfig):
    """
    Filter records for A/B comparability. Returns (kept, dropped_stats_dict).
    Parametry filtracji przekazywane są jawnie przez FilterConfig.
    """
    stats = {
        "input": len(records),
        "dropped_incomplete_window": 0,
        "dropped_fallback_origin": 0,
        "dropped_wrong_window_ms": 0,
        "dropped_low_tx_in_window": 0,
        "dropped_missing_record_id": 0,
        "dropped_duplicates": 0,
        "dropped_missing_vectors": 0,
    }
    kept = []
    seen_ids = set()
    for r in records:
        # 1. ab_window_complete must be True (jeśli pole istnieje)
        wc = get_bool(r, "ab_window_complete")
        if wc is False:                        # None = pole brak → przepuść
            stats["dropped_incomplete_window"] += 1
            continue

        # 2. ab_window_origin == "NewPoolDetected" (jeśli DROP_FALLBACK_ORIGIN)
        if config.drop_fallback_origin:
            origin = get_str(r, "ab_window_origin")
            if origin is not None and origin != "NewPoolDetected":
                stats["dropped_fallback_origin"] += 1
                continue

        # 3. ab_window_ms — walidacja tylko gdy ALLOW_ANY_WINDOW_MS=False
        #    i EXPECTED_WINDOW_MS > 0 (autodetect zdecydował o konkretnej wartości)
        if not config.allow_any_window_ms and config.expected_window_ms > 0:
            wms = r.get("ab_window_ms")
            if wms is not None:
                if isinstance(wms, (int, float)) and int(wms) != config.expected_window_ms:
                    stats["dropped_wrong_window_ms"] += 1
                    continue

        # 4. ab_tx_count_window >= MIN_TX_IN_WINDOW
        #    Domyślnie MIN_TX_IN_WINDOW=0, więc warunek jest de facto wyłączony.
        #    Aktywuje się tylko gdy użytkownik ustawi AB_MIN_TX > 0.
        if config.min_tx_in_window > 0:
            txc = r.get("ab_tx_count_window")
            if txc is not None:
                if isinstance(txc, (int, float)) and int(txc) < config.min_tx_in_window:
                    stats["dropped_low_tx_in_window"] += 1
                    continue

        # 5. Dedup by ab_record_id
        if config.dedup_by_record_id:
            rid = get_str(r, "ab_record_id")
            if not rid:
                stats["dropped_missing_record_id"] += 1
                continue
            if rid in seen_ids:
                stats["dropped_duplicates"] += 1
                continue
            seen_ids.add(rid)

        # 6. Vector length check — wyłączone gdy MIN_VECTOR_LEN == 0
        #    (autodetect ustawia 0 gdy wektory nie istnieją w danych)
        if config.min_vector_len > 0:
            dp = get_vector(r, "vectors_d_price")
            if len(dp) < config.min_vector_len:
                stats["dropped_missing_vectors"] += 1
                continue

        kept.append(r)
    stats["kept"] = len(kept)
    return kept, stats

def _print_filter_report(name, stats, color, config: FilterConfig):
    """Print filter report for a dataset."""
    sub(f"Filtr A/B — Zbiór {name}", color)
    row("Wejście (raw)", f"{stats['input']} rekordów", color)
    row("Zachowane (kept)", f"{stats['kept']} rekordów", C.GREEN)
    for key in ["dropped_incomplete_window", "dropped_fallback_origin",
                "dropped_wrong_window_ms", "dropped_low_tx_in_window",
                "dropped_missing_record_id", "dropped_duplicates",
                "dropped_missing_vectors"]:
        if stats[key] > 0:
            row(f"  {key}", f"{stats[key]}", C.YELLOW)
    # Informacja o wyłączonych filtrach
    if config.allow_any_window_ms:
        hint("    dropped_wrong_window_ms: WYŁĄCZONY (wiele okien w danych / autodetect)")
    if config.min_tx_in_window == 0:
        hint("    dropped_low_tx_in_window: WYŁĄCZONY (MIN_TX=0 — ustaw AB_MIN_TX=N aby aktywować)")
    if config.min_vector_len == 0:
        hint("    dropped_missing_vectors: WYŁĄCZONY (brak wektorów w danych)")
    total_dropped = stats["input"] - stats["kept"]
    if total_dropped > 0:
        pct = total_dropped / stats["input"] * 100 if stats["input"] > 0 else 0
        row("Łącznie odrzuconych", f"{total_dropped} ({pct:.1f}%)", C.RED)
    elif stats["kept"] == stats["input"]:
        ok(f"Filtr przepuścił wszystkie {stats['kept']} rekordów")

# ══════════════════════════════════════════════════════════════════════════════
#  DEFINICJE PÓL
# ══════════════════════════════════════════════════════════════════════════════
NUMERIC_FIELDS = [
    "total_tx", "phases_passed", "observation_duration_ms", "eval_count",
    "dust_filtered_count", "total_tx_evaluated",
    "unique_signers_evaluated", "buy_count",
    "interval_cv", "burst_ratio", "avg_interval_ms", "timing_entropy",
    "unique_ratio", "hhi", "max_tx_per_signer_observed",
    "volume_gini", "top3_volume_pct", "same_ms_tx_ratio",
    "buy_ratio", "avg_tx_sol", "volume_cv", "total_volume_sol",
    "sol_buy_ratio", "max_consecutive_buys_observed",
    "dev_buy_total_sol", "dev_tx_ratio", "dev_volume_ratio",
    "price_change_ratio", "max_single_tx_price_impact_pct_observed",
    "max_single_sell_impact_pct_observed",
    "bonding_progress_pct", "min_bonding_progress_pct", "max_bonding_progress_pct",
    "current_market_cap_sol",
    "block0_sniped_supply_pct",
    "flip_ratio_10s",
    "cu_price_p90_1s",
    "cu_price_p90_10s",
    "priority_fee_surge_slope",
    "buyer_pre_balance_cv",
    "avg_inner_ix_count_50tx",
    "avg_cpi_depth_50tx",
    "sell_buy_ratio",
    "compute_unit_cluster_dominance",
    "static_fee_profile_ratio",
    "fixed_size_buy_ratio",
    "fixed_size_buy_ratio_1e4",
    "flipper_presence_ratio",
    "jito_tip_intensity",
    "early_slot_volume_dominance_buy",
    "whale_reversal_ratio_top3",
    "whale_reversal_ratio_top1",
    "dev_paperhand_latency_ms",
    "failed_tx_ratio",
    "ab_tx_count_window",
    "ab_unique_signers_window",
    "ab_fail_count_window",
    # ── Sybil Interference metryki (v5.0) ──────────────────────────────────
    "fee_topology_diversity_index",
    "dev_buyer_infrastructure_affinity",
    "spend_fraction_divergence",
    "demand_elasticity_score",
    "signer_cross_pool_velocity",
    "funding_source_concentration",
    "sybil_soft_points",
]
BOOL_FIELDS = [
    "phase2_passed", "phase3_passed", "phase4_passed",
    "phase5_passed", "phase6_passed", "dev_wallet_known", "dev_has_sold",
    "dev_sold_within_3s", "dev_sold_within_5s",
    "sybil_interference_layer_enabled", "sybil_combo_veto_enabled",
]

# Nowe metryki rankingowe — jawnie wydzielone, aby zawsze trafiały do
# wszystkich rankingów opartych o KEY_METRICS.
RANKING_EXTENSION_KEY_METRICS = [
    ("block0_sniped_supply_pct",        "",      6),
    ("flip_ratio_10s",                  "",      4),
    ("cu_price_p90_1s",                 "μL/CU", 0),
    ("cu_price_p90_10s",                "μL/CU", 0),
    ("priority_fee_surge_slope",        "",      4),
    ("avg_inner_ix_count_50tx",         "",      2),
    ("avg_cpi_depth_50tx",              "",      2),
    ("sell_buy_ratio",                  "",      4),
    ("compute_unit_cluster_dominance",  "",      4),
    ("fixed_size_buy_ratio",            "",      4),
    ("fixed_size_buy_ratio_1e4",        "",      4),
    ("flipper_presence_ratio",          "",      4),
    ("jito_tip_intensity",              "",      4),
    ("early_slot_volume_dominance_buy", "",      4),
    ("whale_reversal_ratio_top3",       "",      4),
    ("whale_reversal_ratio_top1",       "",      4),
    ("dev_paperhand_latency_ms",        "ms",    0),
    # ── Sybil Interference (v5.0) ──────────────────────────────────────────
    ("fee_topology_diversity_index",       "",   4),
    ("dev_buyer_infrastructure_affinity",  "",   4),
    ("spend_fraction_divergence",          "",   4),
    ("demand_elasticity_score",            "",   4),
    ("signer_cross_pool_velocity",         "",   4),
    ("funding_source_concentration",       "",   4),
    ("sybil_soft_points",                  "",   0),
]

# Kluczowe metryki obserwowane (nie-config)
KEY_METRICS = [
    ("total_tx",                                 "tx",  0),
    ("total_volume_sol",                         "SOL", 2),
    ("current_market_cap_sol",                   "SOL", 1),
    ("bonding_progress_pct",                     "%",   1),
    ("buy_ratio",                                "",    3),
    ("sol_buy_ratio",                            "",    3),
    ("avg_tx_sol",                               "SOL", 3),
    ("observation_duration_ms",                  "ms",  0),
    ("interval_cv",                              "",    3),
    ("timing_entropy",                           "",    3),
    ("volume_gini",                              "",    3),
    ("hhi",                                      "",    4),
    ("price_change_ratio",                       "",    3),
    ("max_single_tx_price_impact_pct_observed",  "%",   1),
    ("max_single_sell_impact_pct_observed",       "%",   1),
    ("top3_volume_pct",                          "",    3),
    ("unique_ratio",                             "",    3),
    ("volume_cv",                                "",    3),
    ("burst_ratio",                              "",    3),
    ("avg_interval_ms",                          "ms",  1),
    ("same_ms_tx_ratio",                         "",    3),
    ("max_consecutive_buys_observed",            "",    0),
    ("dev_buy_total_sol",                        "SOL", 2),
    ("dev_volume_ratio",                         "",    3),
    ("dev_tx_ratio",                             "",    3),
    ("total_tx_evaluated",                       "",    0),
    ("unique_signers_evaluated",                 "",    0),
    ("buy_count",                                "",    0),
    ("eval_count",                               "",    0),
    ("dust_filtered_count",                      "",    0),
    ("phases_passed",                            "",    0),
    ("buyer_pre_balance_cv",                     "",    4),
    ("static_fee_profile_ratio",                 "",    4),
    *RANKING_EXTENSION_KEY_METRICS,
    ("failed_tx_ratio",                          "",    4),
    ("ab_tx_count_window",                       "",    0),
    ("ab_unique_signers_window",                 "",    0),
    ("ab_fail_count_window",                     "",    0),
]

# ══════════════════════════════════════════════════════════════════════════════
#  IO
# ══════════════════════════════════════════════════════════════════════════════
def load(path: str) -> list:
    records = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("//"):
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return records

# ══════════════════════════════════════════════════════════════════════════════
#  NARZĘDZIA WIZUALIZACJI
# ══════════════════════════════════════════════════════════════════════════════
def bar(val, max_val, width=20):
    n = int(val / max_val * width) if max_val > 0 else 0
    n = max(0, min(width, n))
    return "█" * n + "░" * (width - n)

def dual_bar(val_a, val_b, max_val, width=20):
    """Dwa paski obok siebie do porównania."""
    ba = bar(val_a, max_val, width)
    bb = bar(val_b, max_val, width)
    return f"{C.CYAN}{ba}{C.RESET} A | B {C.MAGENTA}{bb}{C.RESET}"

def delta_arrow(delta_pct):
    if delta_pct > 15:   return f"{C.GREEN}▲ +{delta_pct:.1f}%{C.RESET}"
    if delta_pct < -15:  return f"{C.RED}▼ {delta_pct:.1f}%{C.RESET}"
    return f"{C.DIM}≈ {delta_pct:+.1f}%{C.RESET}"

# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 1 & 2 — PROFIL WEWNĘTRZNY ZBIORU
# ══════════════════════════════════════════════════════════════════════════════
def section_profile(records, name, color):
    hdr(f"📊 PROFIL ZBIORU {name} (n={len(records)})", color)
    n = len(records)
    metric_fields = [field for field, _, _ in KEY_METRICS]
    field_cache = build_field_cache(records, BOOL_FIELDS[:5] + metric_fields + ["phases_passed"])

    # --- Pass rate faz ---
    hint("Pasek = % elementów które przeszły daną fazę.")
    sub(f"Fazy — pass rate [{name}]", color)
    for ph in BOOL_FIELDS[:5]:
        vals = field_cache[ph]
        if not vals: continue
        pct = mean(vals) * 100
        col = C.GREEN if pct > 70 else C.YELLOW if pct > 40 else C.RED
        row(ph, f"[{bar(pct, 100)}] {pct:5.1f}%  ({sum(v > 0.5 for v in vals)}/{n})", col)

    # --- Rozkłady ---
    hint("μ±σ = średnia ± odch.std | med = mediana | IQR = Q1–Q3 | [min–max]")
    sub(f"Rozkłady metryk [{name}]", color)
    print(f"  {C.DIM}{'Metryka':<42} {'μ±σ':>22} {'med':>8} {'IQR':>16} {'[min–max]':>20}{C.RESET}")
    print(f"  {'─'*108}")
    for field, unit, dec in KEY_METRICS:
        vals = field_cache[field]
        if not vals: continue
        q1, q3 = percentile(vals, 25), percentile(vals, 75)
        print(f"  {C.DIM}{field:<42}{C.RESET}"
              f"{mean(vals):>{12}.{dec}f}±{std(vals):<{8}.{dec}f} "
              f"{median_val(vals):>{8}.{dec}f} "
              f"[{q1:.{dec}f}–{q3:.{dec}f}]{unit:>4} "
              f"[{min(vals):.{dec}f}–{max(vals):.{dec}f}]")

    # --- Korelacje wewnętrzne ---
    hint("Spearman row-level. Pokazane pary z |r|≥0.40 (istotne zależności wewnątrz zbioru).")
    sub(f"TOP korelacje wewnętrzne [{name}] (|r|≥0.40)", color)
    all_fields = NUMERIC_FIELDS + BOOL_FIELDS
    corrs = []
    for i, f1 in enumerate(all_fields):
        for f2 in all_fields[i + 1:]:
            r, np = spearman(records, f1, f2)
            if np >= 3 and abs(r) >= 0.40:
                corrs.append((abs(r), r, f1, f2, np))
    corrs.sort(reverse=True)

    print(f"  {'Para':<60} {'r':>7}  {'n':>4}  Siła")
    print(f"  {'─'*78}")
    for _, r, f1, f2, np in corrs[:20]:
        label, col = corr_label(r)
        proxy = f" {C.DIM}[bool]{C.RESET}" if f1 in BOOL_FIELDS or f2 in BOOL_FIELDS else ""
        print(f"  {C.DIM}{f1[:27]:<27} ↔ {f2[:27]:<27}{C.RESET}"
              f" {col}{r:>+7.3f}{C.RESET}  {np:>4}  {col}{label}{C.RESET}{proxy}")

    # --- Phase pass rate summary ---
    phases_vals = field_cache["phases_passed"]
    if phases_vals:
        sub(f"Fazy przechodzenia [{name}]", color)
        phase_counter = Counter(int(v) for v in phases_vals)
        for nph in range(0, 8):
            cnt = phase_counter.get(nph, 0)
            if cnt > 0:
                pct = cnt / n * 100
                row(f"phases_passed = {nph}", f"{cnt:>3}x  ({pct:5.1f}%)  [{bar(pct, 100, 15)}]")

    # --- Cechy wspólne (niska wariancja) ---
    hint("Cechy o niskim CV (coeff. of variation) = wartości stabilne/wspólne w zbiorze.")
    sub(f"Cechy wspólne — niska zmienność [{name}] (CV < 0.3)", color)
    low_cv = []
    for field, _, dec in KEY_METRICS:
        vals = field_cache[field]
        if len(vals) < 3: continue
        m = mean(vals)
        s = std(vals)
        cv = s / abs(m) if abs(m) > 1e-9 else float('inf')
        if cv < 0.3:
            low_cv.append((cv, field, m, s, median_val(vals), dec))
    low_cv.sort()
    if low_cv:
        for cv, field, m, s, med, dec in low_cv:
            row(f"{field}", f"CV={cv:.3f}  μ={m:.{dec}f}  med={med:.{dec}f}", C.GREEN)
    else:
        note("Brak pól z CV < 0.3 — duża zmienność w zbiorze")

    # --- Cechy o wysokiej zmienności ---
    hint("Cechy o wysokim CV (>1.0) = duży rozrzut, heterogeniczna populacja.")
    sub(f"Cechy heterogeniczne [{name}] (CV > 1.0)", color)
    high_cv = []
    for field, _, dec in KEY_METRICS:
        vals = field_cache[field]
        if len(vals) < 3: continue
        m = mean(vals)
        s = std(vals)
        cv = s / abs(m) if abs(m) > 1e-9 else float('inf')
        if cv > 1.0 and math.isfinite(cv):
            high_cv.append((cv, field, m, s, dec))
    high_cv.sort(reverse=True)
    for cv, field, m, s, dec in high_cv[:10]:
        row(f"{field}", f"CV={cv:.2f}  μ={m:.{dec}f}±{s:.{dec}f}", C.YELLOW)

    return corrs  # zwracamy korelacje do późniejszego porównania


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 3 — PORÓWNANIE A vs B
# ══════════════════════════════════════════════════════════════════════════════
def section_compare(rec_a, rec_b, corrs_a, corrs_b):
    hdr("⚖️  SEKCJA 3: PORÓWNANIE A vs B — ROZKŁADY, TESTY, SEPARACJA", C.MAGENTA)
    na, nb = len(rec_a), len(rec_b)
    metric_fields = [field for field, _, _ in KEY_METRICS]
    values_a = build_field_cache(rec_a, metric_fields + BOOL_FIELDS[:5])
    values_b = build_field_cache(rec_b, metric_fields + BOOL_FIELDS[:5])

    # --- 3.1 Porównanie rozkładów ---
    hint("Δμ% = różnica średnich (A vs B). Cohen d: effect size (>0.8 = duży).")
    hint("U-norm: Mann–Whitney U/(n1·n2). 0.5 = brak różnicy, <0.35 lub >0.65 = separacja.")
    hint("r_rb = rank-biserial correlation: [-1,+1], >0 = A wyższe, <0 = B wyższe.")
    sub("Zestawienie rozkładów A vs B")
    print(f"  {C.DIM}{'Metryka':<36} {'μ_A':>8} {'μ_B':>8} {'Δμ%':>8}"
          f" {'med_A':>8} {'med_B':>8}"
          f" {'d':>7} {'effect':>8}"
          f" {'U-norm':>7} {'r_rb':>6}{C.RESET}")
    print(f"  {'─'*116}")

    # Zbieramy wyniki do rankingu dyskryminacyjnego
    disc_scores = []

    for field, unit, dec in KEY_METRICS:
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 2 or len(vb) < 2:
            continue

        ma, mb = mean(va), mean(vb)
        delta = (ma - mb) / abs(mb) * 100 if abs(mb) > 1e-9 else 0
        d = cohen_d(va, vb)
        d_lbl, d_col = cohen_d_label(d)
        u = mann_whitney_u(va, vb)
        r_rb = rank_biserial(u)

        # Score dyskryminacyjny = |Cohen d| + |r_rb| (combo)
        disc_combo = (abs(d) if math.isfinite(d) else 0) + abs(r_rb)
        disc_scores.append((disc_combo, abs(d) if math.isfinite(d) else 0,
                            abs(r_rb), field, d, r_rb, delta, ma, mb,
                            median_val(va), median_val(vb)))

        u_col = C.RED if abs(u - 0.5) > 0.15 else C.DIM
        d_str = f"{d:>+.2f}" if math.isfinite(d) else "  N/A"

        print(f"  {C.DIM}{field[:35]:<36}{C.RESET}"
              f"{ma:>8.{dec}f} {mb:>8.{dec}f} "
              f"{delta_arrow(delta):>18} "
              f"{median_val(va):>8.{dec}f} {median_val(vb):>8.{dec}f} "
              f"{d_col}{d_str:>7}{C.RESET} {d_col}{d_lbl:>8}{C.RESET} "
              f"{u_col}{u:>7.3f}{C.RESET} {u_col}{r_rb:>+6.3f}{C.RESET}")

    # --- 3.2 Ranking dyskryminacyjny ---
    hint("Ranking cech które NAJLEPIEJ ODRÓŻNIAJĄ zbiór A od B.")
    hint("disc_score = |Cohen d| + |r_rb|. Wyższy = lepsza separacja.")
    sub("🎯 TOP cechy dyskryminujące A od B")
    disc_scores.sort(reverse=True)
    print(f"  {'#':>3} {'Metryka':<36} {'disc':>5} {'|d|':>5} {'|r_rb|':>6}"
          f" {'A wyższe?':>10} {'Δμ%':>8}")
    print(f"  {'─'*82}")
    for rank, (combo, ad, arb, field, d, r_rb, delta, ma, mb, meda, medb) in enumerate(disc_scores[:15], 1):
        direction = "A > B" if d > 0 else "B > A" if d < 0 else "≈"
        dir_col = C.CYAN if d > 0 else C.MAGENTA if d < 0 else C.DIM
        score_col = C.GREEN if combo >= 1.0 else C.YELLOW if combo >= 0.5 else C.DIM
        print(f"  {rank:>3}. {C.DIM}{field:<35}{C.RESET}"
              f" {score_col}{combo:>5.2f}{C.RESET}"
              f" {ad:>5.2f} {arb:>6.3f}"
              f" {dir_col}{direction:>10}{C.RESET}"
              f" {delta:>+8.1f}%")

    # --- 3.3 Porównanie pass rate faz ---
    sub("Porównanie pass rate faz A vs B")
    for ph in BOOL_FIELDS[:5]:
        va = values_a[ph]
        vb = values_b[ph]
        if not va or not vb: continue
        pa = mean(va) * 100
        pb = mean(vb) * 100
        diff = pa - pb
        diff_col = C.GREEN if diff > 5 else C.RED if diff < -5 else C.DIM
        row(ph, f"A={pa:5.1f}%  B={pb:5.1f}%  Δ={diff_col}{diff:+5.1f}pp{C.RESET}")

    # --- 3.4 Porównanie korelacji wewnętrznych ---
    hint("Korelacje obecne w jednym zbiorze ale nie w drugim (|Δr| > 0.3).")
    hint("To ujawnia STRUKTURALNE różnice — np. w A timing napędza mcap, w B nie.")
    sub("Różnice w korelacjach wewnętrznych (|Δr| > 0.3)")

    # Budujemy mapę korelacji
    def corr_map(records, all_fields):
        m = {}
        for i, f1 in enumerate(all_fields):
            for f2 in all_fields[i + 1:]:
                r, np = spearman(records, f1, f2)
                if np >= 3:
                    key = (f1, f2) if f1 < f2 else (f2, f1)
                    m[key] = r
        return m

    all_f = NUMERIC_FIELDS + BOOL_FIELDS
    cm_a = corr_map(rec_a, all_f)
    cm_b = corr_map(rec_b, all_f)

    all_keys = set(cm_a.keys()) | set(cm_b.keys())
    diffs = []
    for key in all_keys:
        ra = cm_a.get(key, 0.0)
        rb = cm_b.get(key, 0.0)
        dr = abs(ra - rb)
        if dr > 0.3:
            diffs.append((dr, ra, rb, key[0], key[1]))
    diffs.sort(reverse=True)

    print(f"  {'Para':<56} {'r_A':>7} {'r_B':>7} {'Δr':>7} Interpretacja")
    print(f"  {'─'*92}")
    for dr, ra, rb, f1, f2 in diffs[:20]:
        # Interpretacja
        if abs(ra) >= 0.4 and abs(rb) < 0.2:
            interp = f"w A: korelacja, w B: brak"
            icolor = C.CYAN
        elif abs(rb) >= 0.4 and abs(ra) < 0.2:
            interp = f"w B: korelacja, w A: brak"
            icolor = C.MAGENTA
        elif ra * rb < 0:
            interp = f"ODWRÓCONY KIERUNEK!"
            icolor = C.RED
        else:
            interp = f"różna siła"
            icolor = C.YELLOW

        print(f"  {C.DIM}{f1[:26]:<26} ↔ {f2[:26]:<26}{C.RESET}"
              f" {C.CYAN}{ra:>+7.3f}{C.RESET}"
              f" {C.MAGENTA}{rb:>+7.3f}{C.RESET}"
              f" {C.YELLOW}{dr:>7.3f}{C.RESET}"
              f" {icolor}{interp}{C.RESET}")

    return disc_scores


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 4 — DEEP DIVE: SUBTELNE WZORCE
# ══════════════════════════════════════════════════════════════════════════════
def section_deep_dive(rec_a, rec_b, disc_scores):
    hdr("🔬 SEKCJA 4: DEEP DIVE — SUBTELNE WZORCE I INTERAKCJE", C.YELLOW)
    metric_fields = [field for field, _, _ in KEY_METRICS]
    bool_fields = BOOL_FIELDS + ["dev_wallet_known", "dev_has_sold"]
    values_a = build_field_cache(rec_a, metric_fields + bool_fields)
    values_b = build_field_cache(rec_b, metric_fields + bool_fields)
    combined = rec_a + rec_b
    combined_cache = build_field_cache(combined, metric_fields)

    # --- 4.1 Rozkład ogonów (tails) ---
    hint("Porównanie ogonów rozkładu: p10, p25, p75, p90. Różnice w ogonach")
    hint("ujawniają subtelności niewidoczne w porównaniu średnich.")
    sub("Porównanie percentyli (A vs B) — analiza ogonów")
    print(f"  {C.DIM}{'Metryka':<32} {'p10_A':>7} {'p10_B':>7}"
          f" {'p25_A':>7} {'p25_B':>7}"
          f" {'p75_A':>7} {'p75_B':>7}"
          f" {'p90_A':>7} {'p90_B':>7}{C.RESET}")
    print(f"  {'─'*100}")

    tail_diffs = []
    for field, unit, dec in KEY_METRICS:
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 4 or len(vb) < 4:
            continue
        pa = [percentile(va, p) for p in [10, 25, 75, 90]]
        pb = [percentile(vb, p) for p in [10, 25, 75, 90]]

        # Mierz asymetrię ogonów
        tail_diff_score = 0
        for a, b in zip(pa, pb):
            denom = max(abs(a), abs(b), 1e-6)
            tail_diff_score += abs(a - b) / denom
        tail_diffs.append((tail_diff_score, field, pa, pb, dec))

    tail_diffs.sort(reverse=True)
    for _, field, pa, pb, dec in tail_diffs[:15]:
        print(f"  {C.DIM}{field[:31]:<32}{C.RESET}", end="")
        for a, b in zip(pa, pb):
            col = C.YELLOW if abs(a - b) / max(abs(a), abs(b), 1e-6) > 0.2 else C.DIM
            print(f" {C.CYAN}{a:>7.{dec}f}{C.RESET} {C.MAGENTA}{b:>7.{dec}f}{C.RESET}", end="")
        print()

    # --- 4.2 Proporcje booli / categorical ---
    hint("Porównanie proporcji cech binarnych.")
    sub("Proporcje boolowskie A vs B")
    all_bools = BOOL_FIELDS + ["dev_wallet_known", "dev_has_sold"]
    seen = set()
    for bf in all_bools:
        if bf in seen: continue
        seen.add(bf)
        va = values_a[bf]
        vb = values_b[bf]
        if not va or not vb: continue
        pa = mean(va) * 100
        pb = mean(vb) * 100
        diff = pa - pb
        col = C.GREEN if diff > 10 else C.RED if diff < -10 else C.DIM
        row(bf, f"A={pa:5.1f}%  B={pb:5.1f}%  Δ={col}{diff:>+6.1f}pp{C.RESET}"
            f"  [{bar(pa, 100, 10)}] vs [{bar(pb, 100, 10)}]")

    # --- 4.3 Analiza wielowymiarowa: profil tabelaryczny ---
    hint("'Fingerprint' — znormalizowane mediany cech dla każdego zbioru.")
    hint("Wartości = (median - global_median) / global_MAD. Duże |z| = cecha odbiega od normy.")
    sub("Fingerprint wielowymiarowy A vs B (znormalizowane mediany)")

    print(f"  {C.DIM}{'Metryka':<36} {'z_A':>8} {'z_B':>8} {'Δz':>7}  Wizualizacja (A=cyan, B=magenta){C.RESET}")
    print(f"  {'─'*90}")

    fingerprint_diffs = []
    for field, _, dec in KEY_METRICS:
        vc = combined_cache[field]
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 2 or len(vb) < 2 or len(vc) < 4:
            continue
        gmed = median_val(vc)
        gmad = mad(vc)
        if gmad < 1e-9:
            gmad = std(vc)
        if gmad < 1e-9:
            continue

        za = (median_val(va) - gmed) / gmad
        zb = (median_val(vb) - gmed) / gmad
        dz = za - zb

        fingerprint_diffs.append((abs(dz), field, za, zb, dz))

        # Wizualizacja: pasek -3..+3
        def z_bar(z, ch, col, width=15):
            center = width // 2
            pos = int(max(-3, min(3, z)) / 3 * center) + center
            line = list("·" * width)
            line[center] = "|"
            pos = max(0, min(width - 1, pos))
            line[pos] = ch
            return f"{col}{''.join(line)}{C.RESET}"

        viz_a = z_bar(za, "A", C.CYAN)
        viz_b = z_bar(zb, "B", C.MAGENTA)

        dz_col = C.YELLOW if abs(dz) > 1.0 else C.DIM
        print(f"  {C.DIM}{field[:35]:<36}{C.RESET}"
              f" {za:>+8.2f} {zb:>+8.2f} {dz_col}{dz:>+7.2f}{C.RESET}"
              f"  {viz_a} {viz_b}")

    # --- 4.4 Interakcje: top cechy dyskryminujące × inne ---
    if disc_scores:
        top_disc_fields = [s[3] for s in disc_scores[:5]]
        hint("Interakcje: jak top cechy dyskryminujące korelują z innymi — osobno w A i B.")
        sub("Interakcje top dyskryminatorów")
        for tfield in top_disc_fields[:3]:
            print(f"\n    {C.BOLD}{tfield}{C.RESET}:")
            for other, _, _ in KEY_METRICS:
                if other == tfield: continue
                ra, na = spearman(rec_a, tfield, other)
                rb, nb = spearman(rec_b, tfield, other)
                if na < 3 or nb < 3: continue
                dr = abs(ra - rb)
                if dr > 0.25 or abs(ra) > 0.5 or abs(rb) > 0.5:
                    la, ca = corr_label(ra)
                    lb, cb = corr_label(rb)
                    dr_col = C.YELLOW if dr > 0.3 else C.DIM
                    print(f"      {C.DIM}{other[:30]:<30}{C.RESET}"
                          f" A:{ca}{ra:>+.3f}{C.RESET}"
                          f" B:{cb}{rb:>+.3f}{C.RESET}"
                          f" {dr_col}Δ={dr:.3f}{C.RESET}")

    # --- 4.5 Buckety porównawcze ---
    hint("Buckety: podział cech na przedziały → porównanie składu A vs B w każdym.")
    sub("Rozkład bucketowy wybranych cech")

    bucket_defs = [
        ("current_market_cap_sol", [
            ("<30", 0, 30), ("30–50", 30, 50), ("50–70", 50, 70),
            ("70–100", 70, 100), (">100", 100, 1e9)]),
        ("total_volume_sol", [
            ("<5", 0, 5), ("5–10", 5, 10), ("10–20", 10, 20),
            ("20–40", 20, 40), (">40", 40, 1e9)]),
        ("bonding_progress_pct", [
            ("<15%", 0, 15), ("15–25%", 15, 25), ("25–35%", 25, 35),
            ("35–45%", 35, 45), (">45%", 45, 100)]),
        ("timing_entropy", [
            ("<1.0", 0, 1.0), ("1.0–1.5", 1.0, 1.5), ("1.5–2.0", 1.5, 2.0),
            (">2.0", 2.0, 100)]),
        ("buy_ratio", [
            ("<0.7", 0, 0.7), ("0.7–0.85", 0.7, 0.85), ("0.85–0.95", 0.85, 0.95),
            (">0.95", 0.95, 2.0)]),
    ]

    for field, bins in bucket_defs:
        print(f"\n    {C.BOLD}{field}{C.RESET}:")
        print(f"      {'Bucket':<16} {'A':<22} {'B':<22} {'A%':>6} {'B%':>6} {'Δpp':>7}")
        print(f"      {'─'*70}")
        for label, lo, hi in bins:
            ca = sum(1 for r in rec_a if lo <= (r.get(field) or 0) < hi)
            cb = sum(1 for r in rec_b if lo <= (r.get(field) or 0) < hi)
            pa = ca / len(rec_a) * 100 if rec_a else 0
            pb = cb / len(rec_b) * 100 if rec_b else 0
            diff = pa - pb
            d_col = C.CYAN if diff > 10 else C.MAGENTA if diff < -10 else C.DIM
            bar_a = bar(pa, 100, 10)
            bar_b = bar(pb, 100, 10)
            print(f"      {label:<16} {C.CYAN}{ca:>3}x [{bar_a}]{C.RESET}"
                  f" {C.MAGENTA}{cb:>3}x [{bar_b}]{C.RESET}"
                  f" {pa:>5.1f}% {pb:>5.1f}%"
                  f" {d_col}{diff:>+6.1f}{C.RESET}")


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 5 — PODSUMOWANIE
# ══════════════════════════════════════════════════════════════════════════════
def section_summary(rec_a, rec_b, disc_scores):
    hdr("📋 SEKCJA 5: PODSUMOWANIE I WNIOSKI", C.GREEN)
    na, nb = len(rec_a), len(rec_b)
    metric_fields = [field for field, _, _ in KEY_METRICS]
    values_a = build_field_cache(rec_a, metric_fields + ["phase6_passed", "ab_tx_count_window", "ab_unique_signers_window"])
    values_b = build_field_cache(rec_b, metric_fields + ["phase6_passed", "ab_tx_count_window", "ab_unique_signers_window"])

    sub("Rozmiar zbiorów")
    row("Zbiór A", f"{na} elementów", C.CYAN)
    row("Zbiór B", f"{nb} elementów", C.MAGENTA)
    ratio = na / nb if nb > 0 else 0
    if ratio < 0.3 or ratio > 3:
        warn(f"Nierównomierny rozmiar zbiorów (ratio {ratio:.2f}:1) — wyniki mogą być mniej stabilne dla mniejszego zbioru.")

    # ── A/B integrity ──
    sub("A/B integrity — integralność danych po filtracji")
    # Distribution of ab_tx_count_window
    for name_s, values, col in [("A", values_a, C.CYAN), ("B", values_b, C.MAGENTA)]:
        txw = values["ab_tx_count_window"]
        usw = values["ab_unique_signers_window"]
        if txw:
            row(f"ab_tx_count_window [{name_s}]",
                f"μ={mean(txw):.1f}  med={median_val(txw):.0f}  [min={min(txw):.0f}–max={max(txw):.0f}]", col)
        if usw:
            row(f"ab_unique_signers_window [{name_s}]",
                f"μ={mean(usw):.1f}  med={median_val(usw):.0f}  [min={min(usw):.0f}–max={max(usw):.0f}]", col)

    # Vector presence
    for name_s, recs, col in [("A", rec_a, C.CYAN), ("B", rec_b, C.MAGENTA)]:
        n_recs = len(recs)
        if n_recs == 0:
            continue
        has_dp = sum(1 for r in recs if isinstance(r.get("vectors_d_price"), list) and len(r["vectors_d_price"]) > 0)
        has_pr = sum(1 for r in recs if isinstance(r.get("vectors_prices"), list) and len(r["vectors_prices"]) > 0)
        nan_pr = sum(1 for r in recs if isinstance(r.get("vectors_prices"), list)
                     and any(isinstance(x, (int, float)) and not math.isfinite(x) for x in r["vectors_prices"]))
        row(f"Wektory niepuste [{name_s}]",
            f"d_price: {has_dp}/{n_recs} ({has_dp/n_recs*100:.0f}%)  "
            f"prices: {has_pr}/{n_recs} ({has_pr/n_recs*100:.0f}%)", col)
        if has_pr > 0:
            row(f"  prices z NaN [{name_s}]", f"{nan_pr}/{has_pr}", C.YELLOW if nan_pr > 0 else C.DIM)

    # Fingerprint presence check
    sub("Fingerprint presence check")
    fp_fields = ["block0_sniped_supply_pct", "flip_ratio_10s", "cu_price_p90_1s",
                 "cu_price_p90_10s", "priority_fee_surge_slope", "buyer_pre_balance_cv",
                 "avg_inner_ix_count_50tx", "avg_cpi_depth_50tx",
                 "sell_buy_ratio", "compute_unit_cluster_dominance",
                 "static_fee_profile_ratio", "fixed_size_buy_ratio",
                 "fixed_size_buy_ratio_1e4", "flipper_presence_ratio",
                 "jito_tip_intensity", "early_slot_volume_dominance_buy",
                 "whale_reversal_ratio_top3", "whale_reversal_ratio_top1",
                 "dev_paperhand_latency_ms", "dev_sold_within_3s", "dev_sold_within_5s",
                 # Sybil Interference (v5.0)
                 "fee_topology_diversity_index", "dev_buyer_infrastructure_affinity",
                 "spend_fraction_divergence", "demand_elasticity_score",
                 "signer_cross_pool_velocity", "funding_source_concentration",
                 "sybil_soft_points", "sybil_interference_layer_enabled"]
    all_recs = rec_a + rec_b
    n_all = len(all_recs)
    missing_most = False
    for fp in fp_fields:
        present = sum(1 for r in all_recs if r.get(fp) is not None)
        pct = present / n_all * 100 if n_all > 0 else 0
        col = C.GREEN if pct > 80 else C.YELLOW if pct > 20 else C.RED
        row(fp, f"{present}/{n_all} ({pct:.0f}%)", col)
        if pct < 20:
            missing_most = True
    if missing_most:
        warn("Większość rekordów nie ma fingerprintów — dataset za wczesny lub fingerprinty nie podłączone.")

    # --- Cechy STABILNE (wspólne) ---
    sub("Cechy WSPÓLNE (niska różnica A vs B)")
    similar = []
    for field, unit, dec in KEY_METRICS:
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 2 or len(vb) < 2: continue
        d = cohen_d(va, vb)
        if math.isfinite(d) and abs(d) < 0.2:
            similar.append((abs(d), field, mean(va), mean(vb), dec))
    similar.sort()
    if similar:
        for _, field, ma, mb, dec in similar[:10]:
            row(f"{field}", f"μ_A={ma:.{dec}f}  μ_B={mb:.{dec}f}  (praktycznie identyczne)", C.GREEN)
    else:
        note("Brak cech z znikomym effect size — zbiory różnią się w większości wymiarów.")

    # --- TOP różnice ---
    sub("TOP różnice separujące A od B")
    if disc_scores:
        for i, (combo, ad, arb, field, d, r_rb, delta, ma, mb, meda, medb) in enumerate(disc_scores[:10], 1):
            direction = "A > B" if d > 0 else "B > A"
            d_lbl, d_col = cohen_d_label(d)
            interpretation = ""
            if field == "total_tx":
                interpretation = " ← A obserwuje więcej transakcji?" if d > 0 else " ← B obserwuje więcej transakcji?"
            elif field == "total_volume_sol":
                interpretation = " ← A ma wyższy wolumen" if d > 0 else " ← B ma wyższy wolumen"
            elif field == "bonding_progress_pct":
                interpretation = " ← A dalej na bonding curve" if d > 0 else " ← B dalej na bonding curve"
            elif field == "current_market_cap_sol":
                interpretation = " ← A ma wyższy mcap" if d > 0 else " ← B ma wyższy mcap"
            elif field in ("timing_entropy", "interval_cv", "burst_ratio"):
                interpretation = " ← różnica w mikrostrukturze timingu"
            elif field.startswith("dev_"):
                interpretation = " ← różnica w profilu dev-wallet"
            elif field in ("unique_ratio", "hhi", "volume_gini"):
                interpretation = " ← różnica w koncentracji/dystrybucji"

            print(f"  {C.BOLD}{i:>2}.{C.RESET} {d_col}{field:<36}{C.RESET}"
                  f" d={d:>+5.2f} ({d_lbl})  {direction}"
                  f" {C.DIM}{interpretation}{C.RESET}")

    # --- Syntetyczne wnioski ---
    sub("Syntetyczne wnioski")

    # Policzy ile cech to A>B vs B>A wśród top cech
    if disc_scores:
        top10 = disc_scores[:10]
        a_wins = sum(1 for s in top10 if s[4] > 0.2)
        b_wins = sum(1 for s in top10 if s[4] < -0.2)
        neutral = 10 - a_wins - b_wins

        if a_wins > b_wins + 2:
            ok(f"Zbiór A ma WYŻSZE wartości w {a_wins}/10 top cech dyskryminujących")
            note("A = profil 'silniejszy' / bardziej aktywny")
        elif b_wins > a_wins + 2:
            ok(f"Zbiór B ma WYŻSZE wartości w {b_wins}/10 top cech dyskryminujących")
            note("B = profil 'silniejszy' / bardziej aktywny")
        else:
            note(f"Mieszany obraz: A wyższe w {a_wins}, B wyższe w {b_wins}, neutralne {neutral}")

        # Podsumowanie siły separacji
        mean_combo = mean([s[0] for s in top10])
        if mean_combo > 1.0:
            bad(f"Zbiory są DOBRZE SEPAROWALNE (śr. disc_score top10 = {mean_combo:.2f})")
        elif mean_combo > 0.5:
            warn(f"Zbiory mają UMIARKOWANE różnice (śr. disc_score top10 = {mean_combo:.2f})")
        else:
            ok(f"Zbiory są DUŻo podobne (śr. disc_score top10 = {mean_combo:.2f})")

    # Phase 6 comparison
    p6a = mean(values_a["phase6_passed"]) * 100
    p6b = mean(values_b["phase6_passed"]) * 100
    if abs(p6a - p6b) > 10:
        warn(f"Różnica w Phase 6 pass rate: A={p6a:.0f}% vs B={p6b:.0f}% (Δ={p6a - p6b:+.0f}pp)")
    else:
        note(f"Phase 6 pass rate zbliżony: A={p6a:.0f}% vs B={p6b:.0f}%")


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 6 — KSZTAŁT CZASU (DYNAMIC TIME WARPING)
# ══════════════════════════════════════════════════════════════════════════════
def _zscore_normalize(series):
    """Z-Score normalizacja szeregu (średnia 0, odchylenie 1).

    Musi być odporna na serie puste / 1‑elementowe, bo w danych v3
    (np. vectors_d_price) mogą pojawić się wektory o długości 0.
    """
    arr = np.asarray(series, dtype=float).ravel()
    if arr.size == 0:
        return arr

    # Usuń NaN/Inf — DTW z niefinitywnymi wartościami jest bezsensowne.
    arr = arr[np.isfinite(arr)]
    if arr.size == 0:
        return arr

    m = float(arr.mean())
    if arr.size < 2:
        return arr - m

    # ddof=1 ma sens dopiero dla n>=2; tutaj n>=2 jest gwarantowane.
    s = float(arr.std(ddof=1))
    if (not math.isfinite(s)) or s < 1e-12:
        return arr - m
    return (arr - m) / s


def _extract_interval_series(records, field="avg_interval_ms", min_len=5):
    """Wyciąga ciągi interwałów z rekordów, grupowane per aktor."""
    series_list = []
    current = []
    for r in records:
        v = get_val(r, field)
        if v is not None:
            current.append(v)
        else:
            if len(current) >= min_len:
                series_list.append(current)
            current = []
    if len(current) >= min_len:
        series_list.append(current)
    # Jeśli brak naturalnych grup — podziel na okna
    if not series_list:
        all_vals = extract(records, field)
        win = max(min_len, len(all_vals) // 4)
        for i in range(0, len(all_vals) - win + 1, win):
            series_list.append(all_vals[i:i + win])
    return series_list


def _extract_series_list(records, vector_field, min_len=0):
    """Extract vector series from records for DTW analysis.

    Filtruje puste / niepoprawne wektory, bo prowadzą do warningów NumPy
    i dzielenia przez 0 w DTW.
    """
    series_list = []
    req_len = max(1, int(min_len or 0))
    for r in records:
        vec = get_vector(r, vector_field)
        if not vec:
            continue

        cleaned = []
        for x in vec:
            if x is None:
                continue
            try:
                fx = float(x)
            except (TypeError, ValueError):
                continue
            if math.isfinite(fx):
                cleaned.append(fx)

        if len(cleaned) >= req_len:
            series_list.append(cleaned)
    return series_list


def _dtw_distance(s1, s2):
    """Oblicza DTW między dwoma znormalizowanymi szeregami."""
    n1 = _zscore_normalize(s1)
    n2 = _zscore_normalize(s2)

    # Puste serie (np. vectors_d_price przy 1 punkcie) → DTW niezdefiniowane.
    if len(n1) == 0 or len(n2) == 0:
        return float('nan')

    dist, _ = fastdtw(n1.reshape(-1, 1), n2.reshape(-1, 1), dist=scipy_euclidean)
    denom = max(len(n1), len(n2))
    return float(dist / denom) if denom else float('nan')


def _mean_dtw(series_list):
    """Średni dystans DTW wewnątrz listy szeregów."""
    if len(series_list) < 2:
        return float('nan')
    dists = []
    for i in range(len(series_list)):
        for j in range(i + 1, len(series_list)):
            d = _dtw_distance(series_list[i], series_list[j])
            if math.isfinite(d):
                dists.append(d)
    return float(np.mean(dists)) if dists else float('nan')


def _cross_dtw(series_a, series_b, max_pairs=50):
    """Średni dystans DTW między dwoma zbiorami szeregów."""
    dists = []
    count = 0
    for sa in series_a:
        for sb in series_b:
            d = _dtw_distance(sa, sb)
            if math.isfinite(d):
                dists.append(d)
            count += 1
            if count >= max_pairs:
                break
        if count >= max_pairs:
            break
    return float(np.mean(dists)) if dists else float('nan')


def section_dtw(rec_a, rec_b, config: FilterConfig):
    """SEKCJA 6: Kształt Czasu — Dynamic Time Warping (vectors v3)."""
    hdr("⏱️  SEKCJA 6: KSZTAŁT CZASU (DYNAMIC TIME WARPING)", C.CYAN)

    if not _HAS_DTW:
        warn("Brak bibliotek fastdtw/scipy — pomiń SEKCJA 6.")
        hint("Zainstaluj: pip install fastdtw scipy")
        return

    hint("DTW porównuje kształt sekwencji w oknie A/B (wektory v3).")
    hint("Niski dystans A↔B = ten sam wzorzec algorytmu.")

    dtw_vector_fields = ["vectors_d_price", "vectors_interval_ms"]
    has_any_vectors = False

    for vfield in dtw_vector_fields:
        series_a = _extract_series_list(rec_a, vfield, config.min_vector_len)
        series_b = _extract_series_list(rec_b, vfield, config.min_vector_len)

        if not series_a and not series_b:
            continue
        has_any_vectors = True

        if not series_a or not series_b:
            note(f"Brak wystarczających danych wektorowych '{vfield}' w jednym ze zbiorów — pomijam DTW.")
            continue

        sub(f"DTW — {vfield} (A: {len(series_a)} serii, B: {len(series_b)} serii)")

        dtw_aa = _mean_dtw(series_a)
        dtw_bb = _mean_dtw(series_b)
        dtw_ab = _cross_dtw(series_a, series_b)

        row("DTW wewnątrz A", f"{dtw_aa:.4f}", C.CYAN)
        row("DTW wewnątrz B", f"{dtw_bb:.4f}", C.MAGENTA)
        row("DTW między A↔B", f"{dtw_ab:.4f}", C.YELLOW)

        intra_vals = [v for v in (dtw_aa, dtw_bb) if math.isfinite(v)]
        intra_avg = float(np.mean(intra_vals)) if intra_vals else float('nan')
        if math.isfinite(dtw_ab) and math.isfinite(intra_avg):
            if dtw_ab < intra_avg * 0.8:
                print()
                bad(f"OSTRZEŻENIE: A i B wykazują identyczną sygnaturę ({vfield}).")
                bad("To prawdopodobnie ten sam algorytm.")
            elif dtw_ab < intra_avg * 1.2:
                note(f"Sygnatury A i B zbliżone (DTW A↔B ≈ wewnętrzny).")
            else:
                ok(f"Różne sygnatury (DTW A↔B znacząco wyższy niż wewnętrzny).")

    if not has_any_vectors:
        warn("Brak wektorów v3 (vectors_d_price / vectors_interval_ms) — DTW na wektorach niedostępne.")
        hint("Fallback: DTW na agregatach (legacy).")
        # Legacy DTW on aggregates
        dtw_fields = ["avg_interval_ms", "interval_cv", "burst_ratio"]
        for field in dtw_fields:
            series_a = _extract_interval_series(rec_a, field)
            series_b = _extract_interval_series(rec_b, field)
            if not series_a or not series_b:
                continue
            sub(f"DTW legacy — {field} (A: {len(series_a)} seg., B: {len(series_b)} seg.)")
            dtw_aa = _mean_dtw(series_a)
            dtw_bb = _mean_dtw(series_b)
            dtw_ab = _cross_dtw(series_a, series_b)
            row("DTW wewnątrz A", f"{dtw_aa:.4f}", C.CYAN)
            row("DTW wewnątrz B", f"{dtw_bb:.4f}", C.MAGENTA)
            row("DTW między A↔B", f"{dtw_ab:.4f}", C.YELLOW)


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 7 — ODKRYWANIE PRZYCZYNOWOŚCI (CAUSAL DISCOVERY — ALGORYTM PC)
# ══════════════════════════════════════════════════════════════════════════════
CAUSAL_METRICS = [
    "dev_tx_ratio", "buy_ratio", "bonding_progress_pct",
    "price_change_ratio", "volume_cv", "interval_cv", "timing_entropy",
    "buyer_pre_balance_cv", "priority_fee_surge_slope", "flip_ratio_10s",
    "sell_buy_ratio", "compute_unit_cluster_dominance", "static_fee_profile_ratio",
    "fixed_size_buy_ratio", "flipper_presence_ratio", "jito_tip_intensity",
    "early_slot_volume_dominance_buy", "whale_reversal_ratio_top3",
    "dev_paperhand_latency_ms",
    # ── Sybil Interference (v5.0) ───────────────────────────────────────────
    "fee_topology_diversity_index",
    "dev_buyer_infrastructure_affinity",
    "signer_cross_pool_velocity",
]


def _build_causal_matrix(records, metrics):
    """Buduje macierz danych z rekordów dla wybranych metryk (z imputacją medianą)."""
    # Oblicz medianę dla każdej metryki — zastąpi brakujące wartości None
    medians = {}
    for m in metrics:
        vals = [v for r in records if (v := get_val(r, m)) is not None]
        medians[m] = median_val(vals) if vals else 0.0
    rows = []
    for r in records:
        row_vals = [v if (v := get_val(r, m)) is not None else medians[m] for m in metrics]
        rows.append(row_vals)
    return np.array(rows, dtype=float) if rows else None


def _extract_edges(cg, metrics):
    """Wyciąga skierowane krawędzie z wyniku algorytmu PC."""
    graph = cg.G.graph  # macierz sąsiedztwa
    n = len(metrics)
    edges = []
    for i in range(n):
        for j in range(n):
            if i == j:
                continue
            # W causal-learn: graph[i,j] = -1 i graph[j,i] = 1 oznacza i -> j
            if graph[i, j] == -1 and graph[j, i] == 1:
                edges.append((metrics[i], metrics[j]))
    return edges


def _find_roots(edges, metrics):
    """Znajdź korzenie DAG — zmienne z których wychodzą strzałki, ale żadne nie wchodzą."""
    sources = set()
    targets = set()
    for src, tgt in edges:
        sources.add(src)
        targets.add(tgt)
    return sources - targets


def section_causal(rec_a, rec_b):
    """SEKCJA 7: Odkrywanie Przyczynowości — Algorytm PC."""
    hdr("🔗 SEKCJA 7: ODKRYWANIE PRZYCZYNOWOŚCI (ALGORYTM PC)", C.BLUE)

    if not _HAS_CAUSAL:
        warn("Brak bibliotek causal-learn/networkx — pomiń SEKCJA 7.")
        hint("Zainstaluj: pip install causal-learn networkx")
        return

    hint("Algorytm PC buduje graf przyczynowy (DAG) osobno dla A i B.")
    hint("Porównanie ujawnia odwrócone wektory — inny tryb operacyjny.")

    # Filtruj metryki dostępne w danych
    values_a = build_field_cache(rec_a, CAUSAL_METRICS)
    values_b = build_field_cache(rec_b, CAUSAL_METRICS)
    avail_metrics = [m for m in CAUSAL_METRICS
                     if len(values_a[m]) >= 10 and len(values_b[m]) >= 10]

    if len(avail_metrics) < 3:
        warn("Za mało wspólnych metryk (min 3) — pomijam analizę przyczynowości.")
        return

    mat_a = _build_causal_matrix(rec_a, avail_metrics)
    mat_b = _build_causal_matrix(rec_b, avail_metrics)

    if mat_a is None or mat_b is None or len(mat_a) < 10 or len(mat_b) < 10:
        warn("Za mało kompletnych rekordów — pomijam analizę przyczynowości.")
        return

    sub(f"Budowa DAG (metryki: {', '.join(avail_metrics)})")
    note(f"Zbiór A: {len(mat_a)} rekordów | Zbiór B: {len(mat_b)} rekordów")

    try:
        cg_a = pc_algorithm(mat_a, alpha=0.05, indep_test='fisherz', show_progress=False)
        cg_b = pc_algorithm(mat_b, alpha=0.05, indep_test='fisherz', show_progress=False)
    except Exception as exc:
        warn(f"Błąd algorytmu PC: {exc}")
        return

    edges_a = _extract_edges(cg_a, avail_metrics)
    edges_b = _extract_edges(cg_b, avail_metrics)

    # Wyświetl krawędzie
    sub("Krawędzie przyczynowe — Zbiór A")
    if edges_a:
        for src, tgt in edges_a:
            row(f"{src}", f"→ {tgt}", C.CYAN)
    else:
        note("Brak wykrytych krawędzi skierowanych w zbiorze A.")

    roots_a = _find_roots(edges_a, avail_metrics)
    if roots_a:
        row("Korzenie DAG (A)", f"{', '.join(roots_a)}", C.GREEN)

    sub("Krawędzie przyczynowe — Zbiór B")
    if edges_b:
        for src, tgt in edges_b:
            row(f"{src}", f"→ {tgt}", C.MAGENTA)
    else:
        note("Brak wykrytych krawędzi skierowanych w zbiorze B.")

    roots_b = _find_roots(edges_b, avail_metrics)
    if roots_b:
        row("Korzenie DAG (B)", f"{', '.join(roots_b)}", C.GREEN)

    # Porównanie: szukaj odwróconych wektorów
    sub("Odwrócone wektory przyczynowe (A vs B)")
    set_a = set(edges_a)
    set_b = set(edges_b)
    reversed_found = False
    for src, tgt in set_a:
        if (tgt, src) in set_b:
            reversed_found = True
            bad(f"ODWRÓCONY: W zbiorze A [{src}] → [{tgt}], "
                f"ale w zbiorze B [{tgt}] → [{src}]")
    if not reversed_found:
        note("Brak odwróconych wektorów przyczynowych między zbiorami.")

    # Krawędzie obecne tylko w jednym zbiorze
    only_a = set_a - set_b - {(t, s) for s, t in set_b}
    only_b = set_b - set_a - {(t, s) for s, t in set_a}
    if only_a:
        sub("Krawędzie TYLKO w A")
        for src, tgt in only_a:
            row(f"{src}", f"→ {tgt}", C.CYAN)
    if only_b:
        sub("Krawędzie TYLKO w B")
        for src, tgt in only_b:
            row(f"{src}", f"→ {tgt}", C.MAGENTA)


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 8 — TOPOLOGICZNA ANALIZA DANYCH (TDA — SZUKANIE DZIUR)
# ══════════════════════════════════════════════════════════════════════════════
TDA_DIMS = [
    "volume_cv",
    "interval_cv",
    "price_change_ratio",
    "buyer_pre_balance_cv",
    "static_fee_profile_ratio",
    "fixed_size_buy_ratio",
    "compute_unit_cluster_dominance",
    "whale_reversal_ratio_top3",
]


def _build_point_cloud(records, dims):
    """Buduje chmurę punktów z rekordów dla wybranych wymiarów (z imputacją medianą)."""
    # Oblicz medianę dla każdego wymiaru — zastąpi brakujące wartości None
    medians = {}
    for d in dims:
        vals = [v for r in records if (v := get_val(r, d)) is not None]
        medians[d] = median_val(vals) if vals else 0.0
    points = []
    for r in records:
        row_vals = [v if (v := get_val(r, d)) is not None else medians[d] for d in dims]
        points.append(row_vals)
    return np.array(points, dtype=float) if points else None


def section_tda(rec_a, rec_b):
    """SEKCJA 8: Topologiczna Analiza Danych (TDA) — Szukanie Dziur."""
    hdr("🕳️  SEKCJA 8: TOPOLOGICZNA ANALIZA DANYCH (TDA)", C.MAGENTA)

    if not _HAS_TDA:
        warn("Brak bibliotek ripser/persim — pomiń SEKCJA 8.")
        hint("Zainstaluj: pip install ripser persim")
        return

    hint("TDA szuka 'dziur topologicznych' — pustych stref w danych.")
    hint("Dziury = nienaturalne bariery algorytmiczne (twarde limity kodu).")

    values_a = build_field_cache(rec_a, TDA_DIMS)
    values_b = build_field_cache(rec_b, TDA_DIMS)
    avail_dims = [d for d in TDA_DIMS
                  if len(values_a[d]) >= 5 and len(values_b[d]) >= 5]

    if len(avail_dims) < 2:
        warn("Za mało dostępnych wymiarów — pomijam TDA.")
        return

    cloud_a = _build_point_cloud(rec_a, avail_dims)
    cloud_b = _build_point_cloud(rec_b, avail_dims)

    if cloud_a is None or cloud_b is None or len(cloud_a) < 5 or len(cloud_b) < 5:
        warn("Za mało kompletnych punktów — pomijam TDA.")
        return

    sub(f"Chmura punktów (wymiary: {', '.join(avail_dims)})")
    note(f"Zbiór A: {len(cloud_a)} punktów | Zbiór B: {len(cloud_b)} punktów")

    try:
        result_a = ripser(cloud_a, maxdim=1)
        result_b = ripser(cloud_b, maxdim=1)
    except Exception as exc:
        warn(f"Błąd ripser: {exc}")
        return

    dgms_a = result_a['dgms']
    dgms_b = result_b['dgms']

    # H0 (klastry) i H1 (pętle/dziury)
    for dim, dim_name in [(0, "H0 (klastry)"), (1, "H1 (pętle/dziury)")]:
        sub(f"Diagramy Persystencji — {dim_name}")

        dgm_a = dgms_a[dim]
        dgm_b = dgms_b[dim]

        # Filtruj nieskończone
        finite_a = dgm_a[np.isfinite(dgm_a).all(axis=1)]
        finite_b = dgm_b[np.isfinite(dgm_b).all(axis=1)]

        row(f"Cechy {dim_name} w A", f"{len(finite_a)} elementów", C.CYAN)
        row(f"Cechy {dim_name} w B", f"{len(finite_b)} elementów", C.MAGENTA)

        if len(finite_a) > 0:
            persist_a = finite_a[:, 1] - finite_a[:, 0]
            row("Max persystencja A", f"{np.max(persist_a):.4f}", C.CYAN)
        if len(finite_b) > 0:
            persist_b = finite_b[:, 1] - finite_b[:, 0]
            row("Max persystencja B", f"{np.max(persist_b):.4f}", C.MAGENTA)

        # Dystans Wasserstein
        try:
            w_dist = persim_wasserstein(dgm_a, dgm_b)
            row(f"Dystans Wasserstein ({dim_name})", f"{w_dist:.4f}", C.YELLOW)
        except Exception:
            note(f"Nie udało się obliczyć dystansu Wasserstein dla {dim_name}.")

        # Wykrywanie dziur (H1)
        if dim == 1 and len(finite_a) > 0:
            persist_a = finite_a[:, 1] - finite_a[:, 0]
            top_idx = np.argsort(persist_a)[::-1]
            for k in range(min(3, len(top_idx))):
                idx = top_idx[k]
                birth, death = finite_a[idx]
                p = persist_a[idx]
                if p > 0.1:  # Próg istotności
                    bad(f"Wykryto pustą strefę w zbiorze A "
                        f"(brak transakcji w przedziale {birth:.3f}–{death:.3f} "
                        f"mimo płynności wokół). Sygnatura sztucznego algorytmu.")

        if dim == 1 and len(finite_b) > 0:
            persist_b = finite_b[:, 1] - finite_b[:, 0]
            top_idx = np.argsort(persist_b)[::-1]
            for k in range(min(3, len(top_idx))):
                idx = top_idx[k]
                birth, death = finite_b[idx]
                p = persist_b[idx]
                if p > 0.1:
                    bad(f"Wykryto pustą strefę w zbiorze B "
                        f"(brak transakcji w przedziale {birth:.3f}–{death:.3f} "
                        f"mimo płynności wokół). Sygnatura sztucznego algorytmu.")


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 9 — NIELINIOWA WZAJEMNA INFORMACJA (MUTUAL INFORMATION)
# ══════════════════════════════════════════════════════════════════════════════
SPEARMAN_DEAD_ZONE_THRESHOLD = 0.15   # |Spearman| poniżej tego = brak korelacji liniowej
MI_SIGNIFICANCE_THRESHOLD = 0.6       # MI powyżej tego = silny związek nieliniowy
def _entropy(x):
    """Przybliżona entropia (metoda binów) do normalizacji MI."""
    arr = np.array(x, dtype=float)
    n_bins = max(5, int(np.sqrt(len(arr))))
    counts, _ = np.histogram(arr, bins=n_bins)
    probs = counts / counts.sum()
    probs = probs[probs > 0]
    return -np.sum(probs * np.log(probs))


def section_mutual_info(rec_a, rec_b):
    """SEKCJA 9: Nieliniowa Wzajemna Informacja (Mutual Information)."""
    hdr("🧬 SEKCJA 9: NIELINIOWA WZAJEMNA INFORMACJA", C.GREEN)

    if not _HAS_MI:
        warn("Brak biblioteki scikit-learn — pomiń SEKCJA 9.")
        hint("Zainstaluj: pip install scikit-learn")
        return

    hint("MI (Mutual Information) wykrywa zależności nieliniowe niewidoczne dla Spearmana.")
    hint("Szukamy 'martwych stref Spearmana': |Spearman| < 0.15 ORAZ MI > 0.6.")

    # Enrich records with vector-derived scalar features
    vf_fields = ["vf_d_price_std", "vf_abs_d_price_p95", "vf_d_price_skew_proxy",
                 "vf_interval_p95", "vf_interval_cv"]
    for r in rec_a + rec_b:
        feats = vector_features(r)
        for k, v in feats.items():
            r[k] = v
    all_fields = NUMERIC_FIELDS + BOOL_FIELDS + vf_fields
    combined = rec_a + rec_b

    hidden_links = []

    sub("Analiza MI dla połączonych zbiorów A+B")

    for i, f1 in enumerate(all_fields):
        x_all = extract(combined, f1)
        if len(x_all) < 10:
            continue
        for f2 in all_fields[i + 1:]:
            xs_list, ys_list = collect_pairs(combined, f1, f2)
            if len(xs_list) < 10:
                continue

            xs = np.asarray(xs_list, dtype=float).reshape(-1, 1)
            ys = np.asarray(ys_list, dtype=float)

            # Spearman
            r_sp, _ = spearman_from_values(xs_list, ys_list)

            # Mutual Information
            try:
                mi_raw = mutual_info_regression(xs, ys, random_state=42, n_neighbors=5)[0]
            except Exception:
                continue

            # Normalizacja MI przez entropię
            ent_y = _entropy(ys)
            mi_norm = mi_raw / ent_y if ent_y > 1e-9 else 0.0
            mi_norm = min(mi_norm, 1.0)

            # Szukanie martwych stref Spearmana
            if abs(r_sp) < SPEARMAN_DEAD_ZONE_THRESHOLD and mi_norm > MI_SIGNIFICANCE_THRESHOLD:
                hidden_links.append((mi_norm, r_sp, f1, f2))

    sub("UKRYTE ZWIĄZKI NIELINIOWE")
    hint("Zmienne bez korelacji liniowej, ale zespawane z perspektywy teorii informacji.")

    if hidden_links:
        hidden_links.sort(reverse=True)
        print(f"  {C.DIM}{'Para':<56} {'Spearman':>9} {'MI_norm':>8} Status{C.RESET}")
        print(f"  {'─'*90}")
        for mi, rsp, f1, f2 in hidden_links[:20]:
            bad(f"{f1[:26]:<26} ↔ {f2[:26]:<26}"
                f" {rsp:>+9.3f} {mi:>8.3f}"
                f" ← UKRYTY ZWIĄZEK NIELINIOWY")
    else:
        ok("Brak wykrytych ukrytych związków nieliniowych (brak martwych stref Spearmana).")

    # Dodatkowy raport: top MI ogólnie (z podziałem na zbiory)
    sub("TOP pary wg Mutual Information (A osobno, B osobno)")
    for name, records, color in [("A", rec_a, C.CYAN), ("B", rec_b, C.MAGENTA)]:
        top_mi = []
        for i, f1 in enumerate(all_fields):
            for f2 in all_fields[i + 1:]:
                xs_list, ys_list = collect_pairs(records, f1, f2)
                if len(xs_list) < 10:
                    continue
                xs = np.asarray(xs_list, dtype=float).reshape(-1, 1)
                ys = np.asarray(ys_list, dtype=float)
                try:
                    mi_raw = mutual_info_regression(xs, ys, random_state=42, n_neighbors=5)[0]
                except Exception:
                    continue
                ent_y = _entropy(ys)
                mi_norm = mi_raw / ent_y if ent_y > 1e-9 else 0.0
                mi_norm = min(mi_norm, 1.0)
                if mi_norm > 0.3:
                    top_mi.append((mi_norm, f1, f2))
        top_mi.sort(reverse=True)
        print(f"\n    {C.BOLD}Zbiór {name} — TOP MI:{C.RESET}")
        for mi, f1, f2 in top_mi[:10]:
            row(f"{f1[:26]} ↔ {f2[:26]}", f"MI={mi:.3f}", color)


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 10 — ANALIZA KRUCHOŚCI I GRUBYCH OGONÓW (HILL ESTIMATOR)
# ══════════════════════════════════════════════════════════════════════════════
HILL_METRICS = [
    "max_single_sell_impact_pct_observed",
    "max_tx_per_signer_observed",
    "max_single_tx_price_impact_pct_observed",
    "price_change_ratio",
    "volume_cv",
    "burst_ratio",
    "max_consecutive_buys_observed",
]


def _hill_estimator(values, tail_pct=0.10):
    """
    Estymacja Indeksu Hilla (alpha) na górnym ogonie rozkładu.

    Indeks Hilla jest estymatorem indeksu ogona (tail index) rozkładu
    o regularnej wariacji. Obliczany jako odwrotność średniej logarytmów
    z wartości w ogonie podzielonych przez próg odcięcia.

    Parametry:
        values:   lista wartości do analizy
        tail_pct: frakcja górnych wartości tworzących ogon (domyślnie 10%,
                  standardowy wybór balansujący bias vs wariancja estymatora)

    Interpretacja alpha:
        Alpha < 1.5 — ekstremalnie kruchy (nawet średnia nie istnieje)
        Alpha < 2.0 — bardzo kruchy (wariancja → ∞, potężne grube ogony)
        Alpha < 3.0 — umiarkowane ryzyko
        Alpha > 3.0 — stabilny, cienkie ogony (świat Gaussowski)

    Zwraca: (alpha, n_tail, threshold)
    """
    arr = np.array(values, dtype=float)
    arr = arr[arr > 0]  # Tylko dodatnie
    if len(arr) < 10:
        return float('nan'), 0, 0.0

    arr_sorted = np.sort(arr)[::-1]  # Malejąco
    n_tail = max(2, int(len(arr) * tail_pct))
    threshold = arr_sorted[n_tail - 1]

    if threshold <= 0:
        return float('nan'), n_tail, 0.0

    tail = arr_sorted[:n_tail]
    log_ratios = np.log(tail / threshold)
    mean_log = np.mean(log_ratios)

    if mean_log <= 0:
        return float('nan'), n_tail, threshold

    alpha = 1.0 / mean_log
    return alpha, n_tail, threshold


def _hill_interpret(alpha):
    """Interpretacja Alphy Hilla w ludzkim języku."""
    if not math.isfinite(alpha):
        return "N/A", C.DIM
    if alpha < 1.5:
        return "EKSTREMALNIE KRUCHY", C.RED
    if alpha < 2.0:
        return "BARDZO KRUCHY (grube ogony)", C.RED
    if alpha < 3.0:
        return "UMIARKOWANE RYZYKO", C.YELLOW
    return "STABILNY (cienkie ogony)", C.GREEN


def section_hill(rec_a, rec_b):
    """SEKCJA 10: Analiza Kruchości i Grubych Ogonów — Hill Estimator."""
    hdr("📉 SEKCJA 10: ANALIZA KRUCHOŚCI I GRUBYCH OGONÓW (HILL)", C.RED)

    if not _HAS_NUMPY:
        warn("Brak biblioteki numpy — pomiń SEKCJA 10.")
        return

    hint("Indeks Hilla (alpha) mierzy 'grubość ogonów' rozkładu.")
    hint("Alpha < 2 = wariancja → ∞ (potężne 'grube ogony', Czarne Łabędzie).")
    hint("Alpha > 3 = stabilny, normalny świat.")

    sub("Zestawienie Indeksu Kruchości Hilla: A vs B")
    print(f"  {C.DIM}{'Metryka':<44} {'α_A':>7} {'α_B':>7}"
          f" {'n_tail_A':>8} {'n_tail_B':>8}"
          f" Interpretacja{C.RESET}")
    print(f"  {'─'*110}")

    alerts = []
    values_a = build_field_cache(rec_a, HILL_METRICS)
    values_b = build_field_cache(rec_b, HILL_METRICS)

    for field in HILL_METRICS:
        va = values_a[field]
        vb = values_b[field]

        if len(va) < 10 or len(vb) < 10:
            continue

        alpha_a, n_a, thr_a = _hill_estimator(va)
        alpha_b, n_b, thr_b = _hill_estimator(vb)

        lbl_a, col_a = _hill_interpret(alpha_a)
        lbl_b, col_b = _hill_interpret(alpha_b)

        alpha_a_str = f"{alpha_a:.2f}" if math.isfinite(alpha_a) else "N/A"
        alpha_b_str = f"{alpha_b:.2f}" if math.isfinite(alpha_b) else "N/A"

        print(f"  {C.DIM}{field[:43]:<44}{C.RESET}"
              f" {col_a}{alpha_a_str:>7}{C.RESET}"
              f" {col_b}{alpha_b_str:>7}{C.RESET}"
              f" {n_a:>8} {n_b:>8}"
              f"  A={col_a}{lbl_a}{C.RESET} | B={col_b}{lbl_b}{C.RESET}")

        # Zestawienie ryzyka
        if math.isfinite(alpha_a) and math.isfinite(alpha_b):
            if alpha_a < 2.0 and alpha_b > 3.0:
                alerts.append((field, alpha_a, alpha_b, "A"))
            elif alpha_b < 2.0 and alpha_a > 3.0:
                alerts.append((field, alpha_a, alpha_b, "B"))

    if alerts:
        sub("⚠️ ALERTY KRUCHOŚCI")
        for field, aa, ab, risky_set in alerts:
            bad(f"Indeks Kruchości Hilla: Zbiór A (Alpha={aa:.1f}) vs Zbiór B (Alpha={ab:.1f})")
            bad(f"UWAGA: Zbiór {risky_set} jest ekstremalnie podatny na nagłe i niszczycielskie "
                f"anomalie wielkoskalowe ({field}), mimo że standardowe miary centralne "
                f"mogą wyglądać stabilnie.")
    else:
        ok("Brak ekstremalnych rozbieżności kruchości między zbiorami.")

    # ── Vector-based Hill analysis (v3) ──
    sub("Hill Estimator na wektorach v3")
    for vfield, vlabel in [("vectors_d_price", "abs(d_price)"),
                           ("vectors_sol_amounts", "sol_amounts")]:
        all_a = []
        all_b = []
        for r in rec_a:
            vec = get_vector(r, vfield)
            if vfield == "vectors_d_price":
                all_a.extend([abs(x) for x in vec if x != 0])
            else:
                all_a.extend([x for x in vec if x > 0])
        for r in rec_b:
            vec = get_vector(r, vfield)
            if vfield == "vectors_d_price":
                all_b.extend([abs(x) for x in vec if x != 0])
            else:
                all_b.extend([x for x in vec if x > 0])

        if len(all_a) < 10 and len(all_b) < 10:
            if not any(isinstance(r.get(vfield), list) for r in (rec_a[:5] + rec_b[:5])):
                note(f"Brak wektorów v3 ({vfield}) — Hill na wektorach niedostępny.")
            continue

        alpha_a, n_a, _ = _hill_estimator(all_a)
        alpha_b, n_b, _ = _hill_estimator(all_b)
        lbl_a, col_a = _hill_interpret(alpha_a)
        lbl_b, col_b = _hill_interpret(alpha_b)
        alpha_a_str = f"{alpha_a:.2f}" if math.isfinite(alpha_a) else "N/A"
        alpha_b_str = f"{alpha_b:.2f}" if math.isfinite(alpha_b) else "N/A"

        row(f"{vlabel} (A: {len(all_a)} vals)",
            f"α={col_a}{alpha_a_str}{C.RESET}  {col_a}{lbl_a}{C.RESET}", C.CYAN)
        row(f"{vlabel} (B: {len(all_b)} vals)",
            f"α={col_b}{alpha_b_str}{C.RESET}  {col_b}{lbl_b}{C.RESET}", C.MAGENTA)


# ── main() przeniesiony na koniec pliku — patrz poniżej sekcji 11–17 ──────


# ══════════════════════════════════════════════════════════════════════════════
#  HELPERS SEKCJE 11–17
# ══════════════════════════════════════════════════════════════════════════════

def _norm_sf(z: float) -> float:
    """Survival function P(Z > z) standardowego N(0,1) via math.erfc."""
    return 0.5 * math.erfc(z / math.sqrt(2.0))


def _mw_pvalue_approx(n1: int, n2: int, u_norm: float) -> float:
    """
    Two-tailed p-value Mann–Whitney U (aproksymacja normalna).
    u_norm = U / (n1*n2) — normalizacja stosowana w mannwhitney_u().
    """
    if n1 < 1 or n2 < 1:
        return 1.0
    U = u_norm * n1 * n2
    mean_U = n1 * n2 / 2.0
    std_U = math.sqrt(n1 * n2 * (n1 + n2 + 1) / 12.0)
    if std_U < 1e-9:
        return 1.0
    z = abs(U - mean_U) / std_U
    return min(1.0, 2.0 * _norm_sf(z))


def _ks_optimal(va: list, vb: list, max_candidates: int = 500):
    """
    Kolmogorov–Smirnov: max|CDF_A(t) − CDF_B(t)| + próg, gdzie max jest osiągany.
    Zwraca (ks_stat, ks_threshold, direction).
    direction: 'A_above' jeśli A ma tendencję do bycia wyżej, 'B_above' w p.p.
    """
    if not va or not vb:
        return 0.0, None, "A_above"

    sa = sorted(va)
    sb = sorted(vb)
    n_a, n_b = len(sa), len(sb)

    combined = sorted(set(sa + sb))
    if len(combined) > max_candidates:
        step = max(1, len(combined) // max_candidates)
        combined = combined[::step]

    best_ks = -1.0
    best_t = combined[0]
    best_direction = "A_above"

    for t in combined:
        cdf_a = bisect_right(sa, t) / n_a
        cdf_b = bisect_right(sb, t) / n_b
        diff = abs(cdf_a - cdf_b)
        if diff > best_ks:
            best_ks = diff
            best_t = t
            # CDF_A < CDF_B at t  →  A stochastycznie wyżej (mniej wartości ≤ t)
            best_direction = "A_above" if cdf_a < cdf_b else "B_above"

    return best_ks, best_t, best_direction


def _youden_optimal(va: list, vb: list, direction: str = "A_above",
                    max_candidates: int = 500):
    """
    Próg Youden: maksymalizuje J = Sensitivity + Specificity − 1.
    direction='A_above': TP = A ≥ thr, FP = B ≥ thr.
    Zwraca (best_j, best_thr, sensitivity, specificity, accuracy).
    """
    if not va or not vb:
        return 0.0, None, 0.0, 0.0, 0.0

    n_a, n_b = len(va), len(vb)
    combined_vals = sorted(set(va + vb))

    if len(combined_vals) > max_candidates:
        step = max(1, len(combined_vals) // max_candidates)
        combined_vals = combined_vals[::step]

    # Kandydaci: punkty środkowe między kolejnymi wartościami + skrajne
    candidates = []
    for i in range(len(combined_vals) - 1):
        candidates.append((combined_vals[i] + combined_vals[i + 1]) / 2.0)
    if not candidates:
        candidates = combined_vals[:]

    best_j = -1.0
    best_thr = candidates[0]
    best_sens = best_spec = best_acc = 0.0
    va_sorted = va if all(va[i] <= va[i + 1] for i in range(len(va) - 1)) else sorted(va)
    vb_sorted = vb if all(vb[i] <= vb[i + 1] for i in range(len(vb) - 1)) else sorted(vb)

    for thr in candidates:
        if direction == "A_above":
            tp = n_a - bisect_left(va_sorted, thr)
            fp = n_b - bisect_left(vb_sorted, thr)
            sens = tp / n_a
            spec = 1.0 - fp / n_b
            acc = (tp + (n_b - fp)) / (n_a + n_b)
        else:
            tp = n_b - bisect_left(vb_sorted, thr)
            fp = n_a - bisect_left(va_sorted, thr)
            sens = tp / n_b
            spec = 1.0 - fp / n_a
            acc = (tp + (n_a - fp)) / (n_a + n_b)

        j = sens + spec - 1.0
        if j > best_j:
            best_j = j
            best_thr = thr
            best_sens = sens
            best_spec = spec
            best_acc = acc

    return best_j, best_thr, best_sens, best_spec, best_acc


def _get_dec(field: str) -> int:
    """Pobiera liczbę miejsc po przecinku dla pola z KEY_METRICS."""
    for f, _, d in KEY_METRICS:
        if f == field:
            return d
    return 3


def _bhattacharyya(va: list, vb: list, n_bins: int = 40):
    """
    Współczynnik Bhattacharyya BC = Σ sqrt(p_i · q_i).
    BC ∈ [0,1]: 0 = brak nakładania, 1 = identyczne dystrybucje.
    BD = −ln(BC) — dystans.
    """
    if not va or not vb:
        return 0.0, float("inf")

    all_vals = va + vb
    lo, hi = min(all_vals), max(all_vals)
    if hi == lo:
        return 1.0, 0.0

    bin_size = (hi - lo) / n_bins
    hist_a = [0] * n_bins
    hist_b = [0] * n_bins

    for v in va:
        idx = min(int((v - lo) / bin_size), n_bins - 1)
        hist_a[idx] += 1
    for v in vb:
        idx = min(int((v - lo) / bin_size), n_bins - 1)
        hist_b[idx] += 1

    na, nb = float(len(va)), float(len(vb))
    p = [h / na for h in hist_a]
    q = [h / nb for h in hist_b]

    bc = min(1.0, sum(math.sqrt(pi * qi) for pi, qi in zip(p, q)))
    bd = -math.log(bc + 1e-12)
    return bc, bd


def _overlap_coefficient(va: list, vb: list, n_bins: int = 40) -> float:
    """
    Współczynnik nakładania (Weitzman OVL) = Σ min(p_i, q_i). ∈ [0,1].
    """
    if not va or not vb:
        return 0.0
    all_vals = va + vb
    lo, hi = min(all_vals), max(all_vals)
    if hi == lo:
        return 1.0

    bin_size = (hi - lo) / n_bins
    hist_a = [0] * n_bins
    hist_b = [0] * n_bins

    for v in va:
        idx = min(int((v - lo) / bin_size), n_bins - 1)
        hist_a[idx] += 1
    for v in vb:
        idx = min(int((v - lo) / bin_size), n_bins - 1)
        hist_b[idx] += 1

    na, nb = float(len(va)), float(len(vb))
    p = [h / na for h in hist_a]
    q = [h / nb for h in hist_b]
    return min(1.0, sum(min(pi, qi) for pi, qi in zip(p, q)))


def _bootstrap_threshold(va: list, vb: list, direction: str,
                          n_boot: int = 250, seed: int = 42):
    """
    Bootstrap CI95 dla progu Youden.
    Zwraca (median_thr, ci_lo, ci_hi, ci_j_lo, ci_j_hi) albo (None, …) przy błędzie.
    """
    import random
    rng = random.Random(seed)

    thresholds: list = []
    j_vals: list = []

    for _ in range(n_boot):
        boot_a = [rng.choice(va) for _ in va]
        boot_b = [rng.choice(vb) for _ in vb]
        j, thr, _, _, _ = _youden_optimal(boot_a, boot_b, direction,
                                           max_candidates=200)
        if thr is not None and math.isfinite(j):
            thresholds.append(thr)
            j_vals.append(j)

    if len(thresholds) < 20:
        return None, None, None, None, None

    thresholds.sort()
    j_vals.sort()
    n = len(thresholds)

    return (thresholds[n // 2],
            thresholds[max(0, int(n * 0.025))],
            thresholds[min(n - 1, int(n * 0.975))],
            j_vals[max(0, int(n * 0.025))],
            j_vals[min(n - 1, int(n * 0.975))])


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 11 — OPTYMALNE PROGI SEPARUJĄCE (Youden J + KS)
# ══════════════════════════════════════════════════════════════════════════════

def section_optimal_thresholds(rec_a, rec_b, disc_scores):
    """SEKCJA 11: Dla każdej cechy wyznacza próg max Youden J + próg KS."""
    hdr("🎯 SEKCJA 11: OPTYMALNE PROGI SEPARUJĄCE (Youden J + KS)", C.GREEN)

    hint("Youden J = Sensitivity + Specificity − 1.  1.0 = idealna separacja, 0 = brak.")
    hint("KS stat = max|CDF_A − CDF_B|.  KS_thr = miejsce największej rozbieżności CDF.")
    hint("Acc = accuracy binarnego klasyfikatora z tym progiem.")
    hint("DIR: A_above → warunek A-like: cecha ≥ próg | B_above → cecha < próg.")

    sub("Progi optymalne — pełna lista")
    print(f"  {C.DIM}{'#':>3} {'Metryka':<32} {'DIR':>8}"
          f" {'PRÓG_Youden':>13} {'J':>6} {'Sens':>5} {'Spec':>5} {'Acc':>5}"
          f" {'PRÓG_KS':>13} {'KS':>6}{C.RESET}")
    print(f"  {'─'*115}")

    # Kolejność: najpierw top dyskryminatory, reszta KEY_METRICS
    ordered_fields = [s[3] for s in disc_scores[:20]] if disc_scores else []
    seen = set(ordered_fields)
    for f, _, _ in KEY_METRICS:
        if f not in seen:
            ordered_fields.append(f)
            seen.add(f)
    values_a = build_field_cache(rec_a, ordered_fields)
    values_b = build_field_cache(rec_b, ordered_fields)
    disc_map = {s[3]: s[4] for s in disc_scores}

    results = []
    rank = 0

    for field in ordered_fields:
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 5 or len(vb) < 5:
            continue

        va_sorted = sorted(va)
        vb_sorted = sorted(vb)
        ks_stat, ks_thr, direction = _ks_optimal(va_sorted, vb_sorted)
        j, y_thr, sens, spec, acc = _youden_optimal(va_sorted, vb_sorted, direction)

        if y_thr is None or ks_thr is None:
            continue

        d_val = disc_map.get(field, 0.0)
        dec = _get_dec(field)

        j_col  = C.GREEN  if j       >= 0.5 else C.YELLOW if j       >= 0.25 else C.DIM
        ks_col = C.GREEN  if ks_stat >= 0.5 else C.YELLOW if ks_stat >= 0.25 else C.DIM
        d_col  = C.CYAN   if direction == "A_above" else C.MAGENTA

        rank += 1
        print(f"  {rank:>3}. {C.DIM}{field[:31]:<32}{C.RESET}"
              f" {d_col}{direction[:8]:>8}{C.RESET}"
              f" {y_thr:>13.{dec}f}"
              f" {j_col}{j:>6.3f}{C.RESET}"
              f" {sens:>5.3f} {spec:>5.3f} {acc:>5.3f}"
              f" {ks_thr:>13.{dec}f}"
              f" {ks_col}{ks_stat:>6.3f}{C.RESET}")

        results.append(dict(field=field, direction=direction,
                            youden_thr=y_thr, youden_j=j,
                            sensitivity=sens, specificity=spec,
                            accuracy=acc, ks_stat=ks_stat,
                            ks_thr=ks_thr, d_val=d_val))

        if rank >= 30:
            break

    # TOP 10 gotowe reguły
    sub("TOP 10 gotowych reguł (sortowane wg Youden J)")
    sorted_res = sorted(results, key=lambda x: x["youden_j"], reverse=True)
    for i, tr in enumerate(sorted_res[:10], 1):
        dec = _get_dec(tr["field"])
        op_a  = ">=" if tr["direction"] == "A_above" else "<"
        op_b  = "<"  if tr["direction"] == "A_above" else ">="
        j_col = C.GREEN if tr["youden_j"] >= 0.5 else C.YELLOW if tr["youden_j"] >= 0.25 else C.RED
        print(f"\n  {C.BOLD}{i:>2}.{C.RESET} {C.WHITE}{tr['field']}{C.RESET}")
        print(f"      {C.CYAN}A-like:{C.RESET}  {tr['field']} {op_a} {tr['youden_thr']:.{dec}f}"
              f"  {C.MAGENTA}B-like:{C.RESET}  {tr['field']} {op_b} {tr['youden_thr']:.{dec}f}")
        print(f"      J={j_col}{tr['youden_j']:.3f}{C.RESET}"
              f"  Sens={tr['sensitivity']:.3f}  Spec={tr['specificity']:.3f}"
              f"  Acc={tr['accuracy']:.3f}  KS={tr['ks_stat']:.3f}")

    return results


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 12 — RANKING AUC + TESTY ISTOTNOŚCI (MW p-value)
# ══════════════════════════════════════════════════════════════════════════════

def section_auc_ranking(rec_a, rec_b):
    """SEKCJA 12: AUC per cecha = U-stat + aproksymacja p-value MW."""
    hdr("📈 SEKCJA 12: RANKING AUC + TESTY ISTOTNOŚCI (Mann–Whitney)", C.CYAN)

    hint("AUC = Mann–Whitney U / (n1·n2). AUC = 0.5 → brak separacji.")
    hint("|AUC − 0.5| ≥ 0.20 → DOBRA separacja. ≥ 0.35 → SILNA.")
    hint("p-value (aproksymacja normalna, two-tailed): * <0.05  ** <0.01  *** <0.001")

    sub("AUC ranking — wszystkie cechy w KEY_METRICS")
    print(f"  {C.DIM}{'Metryka':<36} {'AUC':>6} {'DIR':>7} {'p-val':>10} {'sig':>5}"
          f" {'|AUC−.5|':>9} {'n_A':>5} {'n_B':>5}{C.RESET}")
    print(f"  {'─'*93}")

    auc_results = []
    metric_fields = [field for field, _, _ in KEY_METRICS]
    values_a = build_field_cache(rec_a, metric_fields)
    values_b = build_field_cache(rec_b, metric_fields)
    for field, unit, dec in KEY_METRICS:
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 5 or len(vb) < 5:
            continue

        u_norm = mann_whitney_u(va, vb)
        sep    = abs(u_norm - 0.5)
        p_val  = _mw_pvalue_approx(len(va), len(vb), u_norm)

        auc_results.append((sep, field, u_norm, p_val, len(va), len(vb), dec))

    auc_results.sort(reverse=True)

    for sep, field, auc, p_val, n1, n2, dec in auc_results:
        dir_str = "A>B" if auc > 0.5 else "B>A"
        dir_col = C.CYAN if auc > 0.5 else C.MAGENTA
        auc_col = C.GREEN if sep >= 0.2 else C.YELLOW if sep >= 0.1 else C.DIM

        if p_val < 0.001:
            sig_str, sig_col = "***", C.GREEN
        elif p_val < 0.01:
            sig_str, sig_col = "**",  C.GREEN
        elif p_val < 0.05:
            sig_str, sig_col = "*",   C.YELLOW
        else:
            sig_str, sig_col = "n.s.", C.DIM

        p_str = f"{p_val:.4f}" if p_val >= 0.0001 else "<.0001"

        print(f"  {C.DIM}{field[:35]:<36}{C.RESET}"
              f" {auc_col}{auc:>6.4f}{C.RESET}"
              f" {dir_col}{dir_str:>7}{C.RESET}"
              f" {p_str:>10}"
              f" {sig_col}{sig_str:>5}{C.RESET}"
              f" {auc_col}{sep:>9.4f}{C.RESET}"
              f" {n1:>5} {n2:>5}")

    n_sig    = sum(1 for _, _, _, p, _, _, _ in auc_results if p < 0.05)
    n_strong = sum(1 for s, *_ in auc_results if s >= 0.2)
    print()
    ok(f"Cechy istotne statystycznie (p < 0.05): {n_sig}/{len(auc_results)}")
    ok(f"Cechy z silną separacją (|AUC−0.5| ≥ 0.20): {n_strong}/{len(auc_results)}")
    return auc_results


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 13 — NAKŁADANIE ROZKŁADÓW (Bhattacharyya + OVL)
# ══════════════════════════════════════════════════════════════════════════════

def section_distribution_overlap(rec_a, rec_b, disc_scores):
    """SEKCJA 13: BC (Bhattacharyya) + OVL (Overlap coefficient) per cecha."""
    hdr("🔀 SEKCJA 13: NAKŁADANIE ROZKŁADÓW (Bhattacharyya + OVL)", C.YELLOW)

    hint("BC (Bhattacharyya coefficient): 0 = brak nakładania, 1 = identyczne.")
    hint("BD = −ln(BC): 0 = identyczne, duże = dobra separacja.")
    hint("OVL = integral min(f_A, f_B). Oba wskaźniki: mniejszy → łatwiej wyznaczyć próg.")

    sub("Nakładanie rozkładów — wszystkie cechy KEY_METRICS")
    print(f"  {C.DIM}{'Metryka':<36} {'BC':>6} {'BD':>7} {'OVL':>6} {'n_A':>5} {'n_B':>5}  Separowalność{C.RESET}")
    print(f"  {'─'*92}")

    overlap_results = []
    metric_fields = [field for field, _, _ in KEY_METRICS]
    values_a = build_field_cache(rec_a, metric_fields)
    values_b = build_field_cache(rec_b, metric_fields)
    for field, _, dec in KEY_METRICS:
        va = values_a[field]
        vb = values_b[field]
        if len(va) < 5 or len(vb) < 5:
            continue

        bc, bd = _bhattacharyya(va, vb)
        ovl     = _overlap_coefficient(va, vb)

        if bc < 0.2:
            sep_str, sep_col = "DOSKONAŁA",   C.GREEN
        elif bc < 0.4:
            sep_str, sep_col = "DOBRA",        C.GREEN
        elif bc < 0.6:
            sep_str, sep_col = "UMIARKOWANA",  C.YELLOW
        elif bc < 0.8:
            sep_str, sep_col = "SŁABA",        C.RED
        else:
            sep_str, sep_col = "BRAK",         C.DIM

        bd_str = f"{bd:.3f}" if math.isfinite(bd) else "  ∞"
        print(f"  {C.DIM}{field[:35]:<36}{C.RESET}"
              f" {sep_col}{bc:>6.3f}{C.RESET}"
              f" {bd_str:>7}"
              f" {ovl:>6.3f}"
              f" {len(va):>5} {len(vb):>5}"
              f"  {sep_col}{sep_str}{C.RESET}")

        overlap_results.append((bc, field, bd, ovl, len(va), len(vb)))

    overlap_results.sort()

    sub("TOP 8 cech z najniższym nakładaniem (prime kandydaci na próg separacyjny)")
    for bc, field, bd, ovl, na, nb in overlap_results[:8]:
        bd_str = f"{bd:.3f}" if math.isfinite(bd) else "∞"
        ok(f"  {field:<36}  BC={bc:.3f}  BD={bd_str}  OVL={ovl:.3f}")

    return overlap_results


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 14 — REGRESJA LOGISTYCZNA (feature importance, sklearn optional)
# ══════════════════════════════════════════════════════════════════════════════

def section_logistic_regression(rec_a, rec_b):
    """SEKCJA 14: LR L1 — standaryzowane wagi cech jako miara ważności."""
    hdr("🧮 SEKCJA 14: REGRESJA LOGISTYCZNA — FEATURE IMPORTANCE (L1)", C.MAGENTA)

    try:
        from sklearn.linear_model import LogisticRegression
        from sklearn.preprocessing import StandardScaler
        from sklearn.metrics import roc_auc_score, accuracy_score
        _has_lr = True
    except ImportError:
        _has_lr = False

    if not _has_lr or not _HAS_NUMPY:
        warn("Brak sklearn/numpy — pomiń SEKCJA 14.")
        hint("Zainstaluj: pip install scikit-learn numpy")
        return []

    # Wybierz cechy obecne w obu zbiorach (≥ 10 wartości)
    metric_fields = [field for field, _, _ in KEY_METRICS]
    values_a = build_field_cache(rec_a, metric_fields)
    values_b = build_field_cache(rec_b, metric_fields)
    usable = [f for f, _, _ in KEY_METRICS
              if len(values_a[f]) >= 10 and len(values_b[f]) >= 10]

    if len(usable) < 3:
        warn("Za mało wspólnych pól — pomijam LR.")
        return []

    X_rows, y_rows = [], []
    for r in rec_a:
        row_f = [get_val(r, f) for f in usable]
        if all(v is not None for v in row_f):
            X_rows.append(row_f); y_rows.append(1)
    for r in rec_b:
        row_f = [get_val(r, f) for f in usable]
        if all(v is not None for v in row_f):
            X_rows.append(row_f); y_rows.append(0)

    if len(X_rows) < 20:
        warn("Za mało kompletnych rekordów — pomijam LR.")
        return []

    X = np.array(X_rows, dtype=float)
    y = np.array(y_rows, dtype=int)

    scaler  = StandardScaler()
    Xs      = scaler.fit_transform(X)

    lr = LogisticRegression(penalty="l1", solver="liblinear", C=1.0,
                             max_iter=2000, random_state=42)
    lr.fit(Xs, y)

    acc = accuracy_score(y, lr.predict(Xs))
    try:
        auc_lr = roc_auc_score(y, lr.predict_proba(Xs)[:, 1])
        auc_str = f"{auc_lr:.4f}"
    except Exception:
        auc_str = "N/A"

    n_a_used = int(np.sum(y == 1))
    n_b_used = int(np.sum(y == 0))
    n_zero   = int(np.sum(lr.coef_[0] == 0))

    ok(f"LR (L1, C=1.0) train — Acc={acc:.3f}  AUC={auc_str}  "
       f"rekordy: {n_a_used} A + {n_b_used} B  cechy: {len(usable)}  "
       f"zerowych (nieistotnych): {n_zero}")
    warn("UWAGA: accuracy/AUC mierzone na danych treningowych — może być zawyżone.")

    sub("Wagi LR (standaryzowane) — sortowane wg |coef|")
    hint("coef > 0 → wyższa wartość = bardziej A-like. coef < 0 → bardziej B-like.")
    hint("coef = 0 (L1 Lasso) → cecha usunięta przez regularyzację — nieistotna.")

    feat_coefs = sorted(zip(usable, lr.coef_[0]), key=lambda x: abs(x[1]), reverse=True)

    print(f"  {C.DIM}{'Metryka':<36} {'Coef':>9} {'|Coef|':>7}  Kierunek{C.RESET}")
    print(f"  {'─'*65}")

    returned = []
    for field, coef in feat_coefs:
        if abs(coef) < 1e-7:
            continue
        col     = C.CYAN   if coef > 0 else C.MAGENTA
        dir_str = "→ A-like ↑" if coef > 0 else "→ B-like ↑"
        print(f"  {C.DIM}{field[:35]:<36}{C.RESET}"
              f" {col}{coef:>+9.4f}{C.RESET}"
              f" {abs(coef):>7.4f}"
              f"  {col}{dir_str}{C.RESET}")
        returned.append((field, coef))

    if n_zero == len(usable):
        warn("L1 wyzerował wszystkie cechy — spróbuj zwiększyć C (mniej regularyzacji).")

    return returned


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 15 — KOMBINOWANA REGUŁA SCORINGOWA
# ══════════════════════════════════════════════════════════════════════════════

def section_scoring_rule(rec_a, rec_b, threshold_results):
    """SEKCJA 15: Łączy TOP-K progów w regułę punktową i ocenia separację."""
    hdr("🏆 SEKCJA 15: KOMBINOWANA REGUŁA SCORINGOWA", C.GREEN)

    if not threshold_results:
        warn("Brak wyników z Sekcji 11 — pomijam Sekcję 15.")
        return

    hint("Score = liczba spełnionych warunków progowych z TOP-K cech (Youden J).")
    hint("A-like: wysoki score. B-like: niski score.")
    hint("Szukamy optymalnego progu score T: records z score ≥ T klasyfikowane jako A.")

    TOP_K   = min(12, len(threshold_results))
    best_th = sorted(threshold_results, key=lambda x: x["youden_j"], reverse=True)[:TOP_K]

    sub(f"Użyte reguły (top {TOP_K} wg Youden J)")
    for i, tr in enumerate(best_th, 1):
        dec   = _get_dec(tr["field"])
        op    = ">=" if tr["direction"] == "A_above" else "<"
        j_col = C.GREEN if tr["youden_j"] >= 0.5 else C.YELLOW
        print(f"  {i:>2}. {C.DIM}{tr['field'][:35]:<35}{C.RESET}"
              f"  {op} {tr['youden_thr']:.{dec}f}"
              f"  J={j_col}{tr['youden_j']:.3f}{C.RESET}")

    def _score(r):
        s = 0
        for tr in best_th:
            v = get_val(r, tr["field"])
            if v is None:
                continue
            if (tr["direction"] == "A_above" and v >= tr["youden_thr"]) or \
               (tr["direction"] == "B_above" and v <  tr["youden_thr"]):
                s += 1
        return s

    scores_a = [_score(r) for r in rec_a]
    scores_b = [_score(r) for r in rec_b]
    na, nb   = len(scores_a), len(scores_b)
    counter_a = Counter(scores_a)
    counter_b = Counter(scores_b)

    sub("Rozkład score w zbiorach A i B")
    print(f"  {C.DIM}{'Score':>6} {'#A':>6} {'%A':>7} {'#B':>6} {'%B':>7}"
          f"  A=cyan   B=magenta{C.RESET}")
    print(f"  {'─'*70}")

    for sc in range(0, TOP_K + 1):
        cnt_a = counter_a.get(sc, 0)
        cnt_b = counter_b.get(sc, 0)
        pct_a = cnt_a / na * 100 if na else 0
        pct_b = cnt_b / nb * 100 if nb else 0
        viz   = f"{C.CYAN}{bar(pct_a, 100, 12)}{C.RESET} | {C.MAGENTA}{bar(pct_b, 100, 12)}{C.RESET}"
        print(f"  {sc:>6} {cnt_a:>6} {pct_a:>6.1f}% {cnt_b:>6} {pct_b:>6.1f}%  {viz}")

    sub("Optymalne cięcie score (Youden J na poziomie scorów)")
    print(f"  {C.DIM}{'T':>5} {'Acc':>6} {'J':>7} {'Sens':>6} {'Spec':>6}"
          f" {'TP':>5} {'FP':>5} {'TN':>5} {'FN':>5}{C.RESET}")
    print(f"  {'─'*62}")

    best_j_sc  = -1.0
    best_T     = 0
    best_acc_T = 0.0
    suffix_a = [0] * (TOP_K + 2)
    suffix_b = [0] * (TOP_K + 2)
    for sc in range(TOP_K, -1, -1):
        suffix_a[sc] = suffix_a[sc + 1] + counter_a.get(sc, 0)
        suffix_b[sc] = suffix_b[sc + 1] + counter_b.get(sc, 0)

    for T in range(0, TOP_K + 1):
        tp = suffix_a[T]
        fp = suffix_b[T]
        tn = nb - fp
        fn = na - tp
        if na > 0 and nb > 0:
            sens = tp / na
            spec = tn / nb
            acc  = (tp + tn) / (na + nb)
            j    = sens + spec - 1.0
        else:
            sens = spec = acc = j = 0.0

        j_col = C.GREEN if j >= 0.5 else C.YELLOW if j >= 0.3 else C.DIM
        print(f"  {T:>5} {acc:>6.3f} {j_col}{j:>7.3f}{C.RESET} {sens:>6.3f} {spec:>6.3f}"
              f" {tp:>5} {fp:>5} {tn:>5} {fn:>5}")

        if j > best_j_sc:
            best_j_sc  = j
            best_T     = T
            best_acc_T = acc

    print()
    j_col = C.GREEN if best_j_sc >= 0.5 else C.YELLOW
    ok(f"Optymalny próg: score ≥ {best_T}"
       f"  (J={j_col}{best_j_sc:.3f}{C.RESET}, Acc={best_acc_T:.3f})")
    note(f"Śr. score: A = {mean(scores_a):.2f}  |  B = {mean(scores_b):.2f}")


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 16 — BOOTSTRAP CI DLA PROGÓW (stabilność progów)
# ══════════════════════════════════════════════════════════════════════════════

def section_bootstrap_thresholds(rec_a, rec_b, threshold_results):
    """SEKCJA 16: Bootstrap CI95 dla top-7 progów Youden."""
    hdr("🔄 SEKCJA 16: BOOTSTRAP CI DLA PROGÓW (stabilność n=250)", C.CYAN)

    if not threshold_results:
        warn("Brak wyników z Sekcji 11 — pomijam Sekcję 16.")
        return

    hint("Bootstrap CI95 wąskie → próg stabilny i godny zaufania.")
    hint("CI95 szerokie → próg wrażliwy na próbkę — zbierz więcej danych lub zmień cechę.")
    hint("Rel.width = szerokość CI / |median_thr|. < 10% = stabilny. > 30% = niestabilny.")

    top7 = sorted(threshold_results, key=lambda x: x["youden_j"], reverse=True)[:7]
    fields_top7 = [tr["field"] for tr in top7]
    values_a = build_field_cache(rec_a, fields_top7)
    values_b = build_field_cache(rec_b, fields_top7)

    sub("Bootstrap CI95 — top 7 cech wg Youden J")
    print(f"  {C.DIM}{'Metryka':<32} {'Med_thr':>11} {'CI95_lo':>11} {'CI95_hi':>11}"
          f" {'Width':>9} {'Rel%':>6} {'Stabilność':<15} J CI95{C.RESET}")
    print(f"  {'─'*115}")

    for tr in top7:
        field     = tr["field"]
        direction = tr["direction"]
        va = values_a[field]
        vb = values_b[field]

        if len(va) < 10 or len(vb) < 10:
            warn(f"  Za mało danych dla {field} — pominięto bootstrap.")
            continue

        med_t, ci_lo, ci_hi, ci_j_lo, ci_j_hi = _bootstrap_threshold(
            sorted(va), sorted(vb), direction, n_boot=250)

        if med_t is None:
            warn(f"  Bootstrap nieudany: {field}")
            continue

        dec       = _get_dec(field)
        width     = ci_hi - ci_lo
        rel_width = width / abs(med_t) * 100 if abs(med_t) > 1e-9 else float("inf")

        if rel_width < 10:
            stab_str, stab_col = "✓ STABILNY",    C.GREEN
        elif rel_width < 30:
            stab_str, stab_col = "~ UMIARKOWANY", C.YELLOW
        else:
            stab_str, stab_col = "✗ NIESTABILNY", C.RED

        print(f"  {C.DIM}{field[:31]:<32}{C.RESET}"
              f" {med_t:>11.{dec}f}"
              f" {ci_lo:>11.{dec}f}"
              f" {ci_hi:>11.{dec}f}"
              f" {width:>9.{dec}f}"
              f" {rel_width:>5.1f}%"
              f" {stab_col}{stab_str:<15}{C.RESET}"
              f" [{ci_j_lo:.3f}–{ci_j_hi:.3f}]")


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 17 — GOTOWA REGUŁA DECYZYJNA (copy-paste do bota)
# ══════════════════════════════════════════════════════════════════════════════

def section_decision_rules_summary(rec_a, rec_b, threshold_results,
                                   auc_results, disc_scores):
    """SEKCJA 17: Syntetyczna reguła decyzyjna — progi gotowe do implementacji."""
    hdr("📋 SEKCJA 17: GOTOWA REGUŁA DECYZYJNA — PROGI DO IMPLEMENTACJI", C.GREEN)

    hint("Confidence = 0.4·J + 0.4·|AUC−0.5| + 0.2·min(|d|,2)/2")
    hint("Każda linia = jednoznaczna reguła akceptacji/odrzucenia poolu.")

    if not threshold_results:
        warn("Brak danych — pomijam Sekcję 17.")
        return

    # Zbieramy dowody z różnych sekcji
    ev_map: dict = {}
    for tr in threshold_results:
        ev_map[tr["field"]] = dict(
            youden_j   = tr["youden_j"],
            youden_thr = tr["youden_thr"],
            ks_stat    = tr["ks_stat"],
            ks_thr     = tr["ks_thr"],
            direction  = tr["direction"],
            sensitivity= tr["sensitivity"],
            specificity= tr["specificity"],
            accuracy   = tr["accuracy"],
            d_val      = tr.get("d_val", 0.0),
            auc        = 0.5,
            auc_sep    = 0.0,
            p_val      = 1.0,
        )

    for sep, field, auc, p_val, n1, n2, dec in auc_results:
        if field in ev_map:
            ev_map[field]["auc"]     = auc
            ev_map[field]["auc_sep"] = sep
            ev_map[field]["p_val"]   = p_val

    for field, ev in ev_map.items():
        d_abs = min(abs(ev.get("d_val", 0.0)), 2.0) / 2.0
        ev["confidence"] = 0.4 * ev["youden_j"] + 0.4 * ev["auc_sep"] + 0.2 * d_abs

    sorted_ev = sorted(ev_map.items(), key=lambda x: x[1]["confidence"], reverse=True)

    sub("Ranking reguł wg łącznego confidence")
    print(f"  {C.DIM}{'#':>3} {'Metryka':<32} {'PRÓG':>12} {'OP':>4}"
          f" {'Conf':>6} {'J':>6} {'AUC':>6} {'p<.05':>6} {'d':>6}"
          f" {'Acc':>5}{C.RESET}")
    print(f"  {'─'*102}")

    for rank, (field, ev) in enumerate(sorted_ev[:20], 1):
        dec = _get_dec(field)
        op  = ">=" if ev["direction"] == "A_above" else "<"
        c   = ev["confidence"]
        c_col = C.GREEN if c >= 0.4 else C.YELLOW if c >= 0.2 else C.DIM
        p_sig = "YES" if ev["p_val"] < 0.05 else "no"
        p_col = C.GREEN if p_sig == "YES" else C.DIM
        d_str = f"{ev['d_val']:+.2f}" if math.isfinite(ev.get("d_val", float("nan"))) else " N/A"

        print(f"  {rank:>3}. {C.DIM}{field[:31]:<32}{C.RESET}"
              f" {ev['youden_thr']:>12.{dec}f}"
              f" {op:>4}"
              f" {c_col}{c:>6.3f}{C.RESET}"
              f" {ev['youden_j']:>6.3f}"
              f" {ev['auc']:>6.4f}"
              f" {p_col}{p_sig:>6}{C.RESET}"
              f" {d_str:>6}"
              f" {ev['accuracy']:>5.3f}")

    # Blok konfiguracyjny — gotowy do wklejenia
    sub("Blok konfiguracyjny — KOPIUJ DO BOTA")

    a_rules = [(f, ev) for f, ev in sorted_ev
               if ev["direction"] == "A_above"
               and ev["youden_j"] >= 0.20
               and ev["p_val"] < 0.05][:10]

    b_rules = [(f, ev) for f, ev in sorted_ev
               if ev["direction"] == "B_above"
               and ev["youden_j"] >= 0.20
               and ev["p_val"] < 0.05][:5]

    print(f"\n  {C.BOLD}{C.GREEN}// ═══ REGUŁY A-LIKE (akceptuj pool) ═══{C.RESET}")
    if a_rules:
        for field, ev in a_rules:
            dec = _get_dec(field)
            print(f"  {C.CYAN}{field}: >= {ev['youden_thr']:.{dec}f}"
                  f"  // J={ev['youden_j']:.3f}  Acc={ev['accuracy']:.3f}"
                  f"  AUC={ev['auc']:.4f}{C.RESET}")
    else:
        note("Brak reguł A-above z J ≥ 0.20 i p < 0.05 — za mało danych lub słaba separacja.")

    if b_rules:
        print(f"\n  {C.BOLD}{C.MAGENTA}// ═══ REGUŁY B-LIKE (odrzuć pool) ═══{C.RESET}")
        for field, ev in b_rules:
            dec = _get_dec(field)
            print(f"  {C.MAGENTA}{field}: < {ev['youden_thr']:.{dec}f}"
                  f"  // J={ev['youden_j']:.3f}  Acc={ev['accuracy']:.3f}"
                  f"  AUC={ev['auc']:.4f}{C.RESET}")

    # Łączna ocena jakości reguły
    print()
    good_rules = [(f, ev) for f, ev in sorted_ev
                  if ev["youden_j"] >= 0.45 and ev["p_val"] < 0.05]
    ok_rules   = [(f, ev) for f, ev in sorted_ev
                  if 0.25 <= ev["youden_j"] < 0.5 and ev["p_val"] < 0.05]

    if len(good_rules) >= 3:
        ok(f"Zidentyfikowano {len(good_rules)} SILNYCH reguł separacyjnych (J ≥ 0.5, p < 0.05) — "
           f"wystarczające do skutecznego filtra!")
    elif len(good_rules) + len(ok_rules) >= 3:
        warn(f"Dostępnych {len(good_rules)} silnych + {len(ok_rules)} umiarkowanych reguł — "
             f"filtr możliwy, ale mniej pewny.")
    else:
        bad(f"Mało wiarygodnych reguł (J ≥ 0.25, p < 0.05): {len(good_rules) + len(ok_rules)} — "
            f"rozważ zebranie większej próbki.")


# ══════════════════════════════════════════════════════════════════════════════
#  SEKCJA 18 — ANALIZA WARSTWY SYBIL INTERFERENCE
#  (FTDI, DBIA, SFD, DES, CPV, FSC + sybil_soft_points + degradacja)
# ══════════════════════════════════════════════════════════════════════════════

# Progi i kierunki z ghost_brain_config.toml (Stage D FSC bake)
_SYBIL_METRICS_CONFIG = {
    "fee_topology_diversity_index": {
        "abbr": "FTDI", "direction": "min",  "threshold": 0.25,
        "penalty": 1, "flag": "low_ftdi",
        "hint": "niska różnorodność topologii fee → potencjalny cabal",
    },
    "dev_buyer_infrastructure_affinity": {
        "abbr": "DBIA", "direction": "max",  "threshold": 0.60,
        "penalty": 1, "flag": "high_dbia",
        "hint": "wysoka podobność infrastruktury dev↔buyer → podejrzany matching",
    },
    "spend_fraction_divergence": {
        "abbr": "SFD",  "direction": "min",  "threshold": 0.08,
        "penalty": 2, "flag": "low_sfd",
        "hint": "niski MAD wydatkowanej frakcji portfela → homogeniczny cabal",
    },
    "demand_elasticity_score": {
        "abbr": "DES",  "direction": "min",  "threshold": 0.15,
        "penalty": 3, "flag": "inelastic_demand",
        "hint": "niska elastyczność popytu → sztywny / skryptowany wzorzec kupna",
    },
    "signer_cross_pool_velocity": {
        "abbr": "CPV",  "direction": "max",  "threshold": 0.50,
        "penalty": 1, "flag": "high_cpv",
        "hint": "wysoka prędkość cross-pool sygnerów → recykling portfeli",
    },
    "funding_source_concentration": {
        "abbr": "FSC",  "direction": "max",  "threshold": 0.60,
        "penalty": 0, "flag": "high_fsc",
        "hint": "wysoka koncentracja źródła finansowania → wspólny funder (telemetry-only)",
    },
}

_SYBIL_COMBOS = [
    ("high_dbia_low_ftdi_combo",  "DBIA↑ + FTDI↓",   2),
    ("low_des_low_sfd_combo",     "DES↓ + SFD↓",     2),
    ("high_cpv_low_des_combo",    "CPV↑ + DES↓",     0),
    ("high_fsc_high_cpv_combo",   "FSC↑ + CPV↑",     0),
]

_SYBIL_DEGRADED_KNOWN = [
    "FTDI_INSUFFICIENT_BUYS",         "FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE",
    "DBIA_NO_DEV_BUY",                "DBIA_INSUFFICIENT_BUYERS",
    "DBIA_RAW_FINGERPRINT_UNAVAILABLE",
    "SFD_INSUFFICIENT_BUYS",          "SFD_ZERO_PREBALANCE_SKIPPED",
    "SFD_POSTBALANCE_UNAVAILABLE",
    "DES_CURVE_DATA_UNAVAILABLE",     "DES_INSUFFICIENT_BUYS",
    "CPV_INDEX_NOT_READY",
    "FSC_INSUFFICIENT_KNOWN_SOURCES", "FSC_INDEX_NOT_READY",
]


def section_sybil_interference(rec_a, rec_b):
    """SEKCJA 18: Analiza warstwy Sybil Interference — 6 metryk + sybil_soft_points."""
    hdr("🕵️  SEKCJA 18: ANALIZA WARSTWY SYBIL INTERFERENCE (FTDI/DBIA/SFD/DES/CPV/FSC)", C.MAGENTA)

    na, nb = len(rec_a), len(rec_b)
    all_recs = rec_a + rec_b

    hint("Progi z ghost_brain_config.toml (Stage D FSC bake — aktywny stan produkcyjny).")
    hint("FTDI/DBIA/CPV obecne; SFD/DES/FSC mogą być None gdy zdegradowane — pokazane osobno.")
    hint("Naruszenie progu = metryka po złej stronie progu konfiguracyjnego.")

    # ─── 18.1 Podstawowe statystyki każdej metryki per zbiór ────────────────
    sub("18.1 Statystyki sybil-metryk A vs B")
    print(f"  {C.DIM}{'Metryka (abbr)':<34} {'μ_A':>9} {'μ_B':>9} {'med_A':>8} {'med_B':>8}"
          f" {'n_A':>5} {'n_B':>5}  {'Próg':>7}  {'Dir':>4}{C.RESET}")
    print(f"  {'─'*100}")

    sybil_vals_a: dict = {}
    sybil_vals_b: dict = {}

    for field, cfg in _SYBIL_METRICS_CONFIG.items():
        va = extract(rec_a, field)
        vb = extract(rec_b, field)
        sybil_vals_a[field] = va
        sybil_vals_b[field] = vb

        na_f, nb_f = len(va), len(vb)
        if na_f == 0 and nb_f == 0:
            row(f"{cfg['abbr']} ({field[:22]})",
                "BRAK DANYCH w obu zbiorach (metryka zdegradowana wszędzie)", C.DIM)
            continue

        ma  = mean(va)   if va else float("nan")
        mb  = mean(vb)   if vb else float("nan")
        mda = median_val(va) if va else float("nan")
        mdb = median_val(vb) if vb else float("nan")
        thr = cfg["threshold"]
        dir_sym = "↑max" if cfg["direction"] == "max" else "↓min"
        dir_col = C.RED if cfg["direction"] == "max" else C.CYAN

        # Podświetl gdy mediana dostępnych narusza próg
        def _thr_color(m, thr, direction):
            if not math.isfinite(m): return C.DIM
            if direction == "min":   return C.RED if m < thr else C.GREEN
            return C.RED if m > thr else C.GREEN

        col_a = _thr_color(mda, thr, cfg["direction"])
        col_b = _thr_color(mdb, thr, cfg["direction"])

        ma_str  = f"{ma:.4f}"  if math.isfinite(ma)  else " N/A"
        mb_str  = f"{mb:.4f}"  if math.isfinite(mb)  else " N/A"
        mda_str = f"{mda:.4f}" if math.isfinite(mda) else " N/A"
        mdb_str = f"{mdb:.4f}" if math.isfinite(mdb) else " N/A"

        print(f"  {C.DIM}{cfg['abbr']:<6} {field[:27]:<27}{C.RESET}"
              f" {ma_str:>9} {mb_str:>9}"
              f" {col_a}{mda_str:>8}{C.RESET} {col_b}{mdb_str:>8}{C.RESET}"
              f" {na_f:>5} {nb_f:>5}"
              f"  {thr:>7.4f}  {dir_col}{dir_sym}{C.RESET}")
        hint(f"    └ {cfg['hint']}")

    # ─── 18.2 Wskaźniki naruszenia progu (% rekordów poniżej/powyżej progu) ─
    sub("18.2 Wskaźniki naruszenia progu konfiguracyjnego")
    hint("Naruszenie = metryka jest po 'złej' stronie progu (FTDI<0.25, DBIA>0.60 itp.).")
    hint("Podane tylko dla rekordów które mają wartość metryki (bez zdegradowanych).")
    print(f"  {C.DIM}{'Metryka':<8} {'Naruszeń A':>13} {'Naruszeń B':>13}"
          f"  {'Δpp':>7}  Interpretacja{C.RESET}")
    print(f"  {'─'*80}")

    for field, cfg in _SYBIL_METRICS_CONFIG.items():
        va = sybil_vals_a[field]
        vb = sybil_vals_b[field]
        thr = cfg["threshold"]
        if not va and not vb:
            continue

        def _viol_pct(vals, thr, direction):
            if not vals: return float("nan")
            n_viol = sum(1 for v in vals if (direction == "min" and v < thr) or
                                             (direction == "max" and v > thr))
            return n_viol / len(vals) * 100

        pct_a = _viol_pct(va, thr, cfg["direction"])
        pct_b = _viol_pct(vb, thr, cfg["direction"])
        diff  = pct_a - pct_b if math.isfinite(pct_a) and math.isfinite(pct_b) else float("nan")

        pa_str = f"{pct_a:.1f}%  ({int(pct_a/100*len(va)+.5)}/{len(va)})" if va else "N/A"
        pb_str = f"{pct_b:.1f}%  ({int(pct_b/100*len(vb)+.5)}/{len(vb)})" if vb else "N/A"

        if math.isfinite(diff):
            diff_col = C.RED if abs(diff) > 15 else C.YELLOW if abs(diff) > 5 else C.DIM
            diff_str = f"{diff_col}{diff:>+.1f}pp{C.RESET}"
        else:
            diff_str = C.DIM + " N/A" + C.RESET

        # Interpretacja: jeśli pct_a >> pct_b → A ma więcej naruszeń → A = gorszy profil
        if math.isfinite(pct_a) and math.isfinite(pct_b):
            if pct_a > pct_b + 15:
                interp = "A gorzej (więcej naruszeń)"
                i_col  = C.CYAN
            elif pct_b > pct_a + 15:
                interp = "B gorzej (więcej naruszeń)"
                i_col  = C.MAGENTA
            else:
                interp = "porównywalnie"
                i_col  = C.DIM
        else:
            interp, i_col = "brak danych", C.DIM

        print(f"  {cfg['abbr']:<8}"
              f" {pa_str:>25} {pb_str:>25}"
              f"  {diff_str:>10}  {i_col}{interp}{C.RESET}")

    # ─── 18.3 Pokrycie danych — coverage każdej metryki (% rekordów z wartością) ──
    sub("18.3 Coverage sybil-metryk (% rekordów z wartością)")
    hint("Niska coverage = metryka często zdegradowana → sygnał słaby / niestabilny.")
    print(f"  {C.DIM}{'Metryka':<8} {'Cover_A':>12} {'Cover_B':>12}"
          f"  {'Cover_ALL':>12}  Stan{C.RESET}")
    print(f"  {'─'*70}")

    for field, cfg in _SYBIL_METRICS_CONFIG.items():
        n_a_with = sum(1 for r in rec_a if r.get(field) is not None)
        n_b_with = sum(1 for r in rec_b if r.get(field) is not None)
        n_all_with = n_a_with + n_b_with
        pct_cov_a = n_a_with / na * 100 if na > 0 else 0.0
        pct_cov_b = n_b_with / nb * 100 if nb > 0 else 0.0
        pct_cov   = n_all_with / (na + nb) * 100 if (na + nb) > 0 else 0.0

        cov_col = C.GREEN if pct_cov >= 70 else C.YELLOW if pct_cov >= 30 else C.RED
        state   = "DOBRA" if pct_cov >= 70 else ("CZĘŚCIOWA" if pct_cov >= 30 else "SŁABA")

        print(f"  {cfg['abbr']:<8}"
              f" {pct_cov_a:>9.1f}%   {pct_cov_b:>9.1f}%"
              f"   {cov_col}{pct_cov:>9.1f}%{C.RESET}  {cov_col}{state}{C.RESET}")

    # ─── 18.4 Analiza zdegradowanych powodów ───────────────────────────────
    sub("18.4 Analiza degradacji sybil_metric_degraded_reasons")
    hint("Zlicza wystąpienia każdego reason code w obu zbiorach.")

    def _count_degraded(records):
        counter: dict = defaultdict(int)
        for r in records:
            reasons = r.get("sybil_metric_degraded_reasons")
            if isinstance(reasons, list):
                for reason in reasons:
                    if isinstance(reason, str):
                        counter[reason] += 1
        return counter

    deg_a = _count_degraded(rec_a)
    deg_b = _count_degraded(rec_b)
    all_reasons = sorted(set(list(deg_a.keys()) + list(deg_b.keys()) + _SYBIL_DEGRADED_KNOWN))

    n_with_any_deg_a = sum(1 for r in rec_a if isinstance(r.get("sybil_metric_degraded_reasons"), list)
                           and len(r["sybil_metric_degraded_reasons"]) > 0)
    n_with_any_deg_b = sum(1 for r in rec_b if isinstance(r.get("sybil_metric_degraded_reasons"), list)
                           and len(r["sybil_metric_degraded_reasons"]) > 0)

    row("Rekordy z ≥1 degraded reason [A]",
        f"{n_with_any_deg_a}/{na}  ({n_with_any_deg_a/na*100:.1f}%)" if na > 0 else "N/A", C.CYAN)
    row("Rekordy z ≥1 degraded reason [B]",
        f"{n_with_any_deg_b}/{nb}  ({n_with_any_deg_b/nb*100:.1f}%)" if nb > 0 else "N/A", C.MAGENTA)

    if all_reasons:
        print(f"\n  {C.DIM}{'Reason code':<45} {'#A':>5} {'#B':>5}  {'%_A':>6}  {'%_B':>6}{C.RESET}")
        print(f"  {'─'*72}")
        for reason in all_reasons:
            ca = deg_a.get(reason, 0)
            cb = deg_b.get(reason, 0)
            pa = ca / na * 100 if na > 0 else 0
            pb = cb / nb * 100 if nb > 0 else 0
            col = C.RED if (pa > 50 or pb > 50) else C.YELLOW if (pa > 20 or pb > 20) else C.DIM
            if ca + cb > 0:
                print(f"  {col}{reason:<45}{C.RESET}"
                      f" {ca:>5} {cb:>5}  {pa:>5.1f}%  {pb:>5.1f}%")
    else:
        ok("Brak rekordów z degraded reasons w danych.")

    # ─── 18.5 Rozkład sybil_soft_points ────────────────────────────────────
    sub("18.5 Rozkład sybil_soft_points A vs B")
    hint("sybil_soft_points = suma kar z warstwy sybil. Max=6. Wyższy → bardziej podejrzany.")

    ssp_a = extract(rec_a, "sybil_soft_points")
    ssp_b = extract(rec_b, "sybil_soft_points")
    max_ssp = 6

    if ssp_a or ssp_b:
        print(f"  {C.DIM}{'pts':>5} {'#A':>6} {'%A':>7} {'#B':>6} {'%B':>7}  Wizualizacja{C.RESET}")
        print(f"  {'─'*70}")
        counter_a_sp = Counter(int(v) for v in ssp_a)
        counter_b_sp = Counter(int(v) for v in ssp_b)
        for sc in range(0, max_ssp + 1):
            cnt_a = counter_a_sp.get(sc, 0)
            cnt_b = counter_b_sp.get(sc, 0)
            pct_a = cnt_a / len(ssp_a) * 100 if ssp_a else 0
            pct_b = cnt_b / len(ssp_b) * 100 if ssp_b else 0
            risk_col = C.RED if sc >= 4 else C.YELLOW if sc >= 2 else C.GREEN if sc == 0 else C.DIM
            viz = f"{C.CYAN}{bar(pct_a, 100, 12)}{C.RESET} | {C.MAGENTA}{bar(pct_b, 100, 12)}{C.RESET}"
            print(f"  {risk_col}{sc:>5}{C.RESET}"
                  f" {cnt_a:>6} {pct_a:>6.1f}%"
                  f" {cnt_b:>6} {pct_b:>6.1f}%  {viz}")

        row("śr sybil_soft_points [A]",
            f"{mean(ssp_a):.3f}  (med={median_val(ssp_a):.1f})", C.CYAN)
        row("śr sybil_soft_points [B]",
            f"{mean(ssp_b):.3f}  (med={median_val(ssp_b):.1f})", C.MAGENTA)
        d_sp = cohen_d(ssp_a, ssp_b)
        if math.isfinite(d_sp):
            lbl, col = cohen_d_label(d_sp)
            row("Cohen d (sybil_soft_points A vs B)", f"{d_sp:+.3f}  ({lbl})", col)
        # Ile rekordów dostało ≥ sybil_soft_threshold → sybil penalty wpłynął na decyzję
        n_risky_a = sum(1 for v in ssp_a if v >= 4)
        n_risky_b = sum(1 for v in ssp_b if v >= 4)
        if ssp_a:
            warn(f"Rekordy z sybil_soft_points ≥ 4 [A]: {n_risky_a}/{len(ssp_a)} "
                 f"({n_risky_a/len(ssp_a)*100:.1f}%) — potencjalny blok/penalty")
        if ssp_b:
            warn(f"Rekordy z sybil_soft_points ≥ 4 [B]: {n_risky_b}/{len(ssp_b)} "
                 f"({n_risky_b/len(ssp_b)*100:.1f}%) — potencjalny blok/penalty")
    else:
        note("Brak pola sybil_soft_points w danych — warstwa sybil nieaktywna lub stary schemat logu.")

    # ─── 18.6 Analiza sybil_soft_flags i sybil_lead_signal ─────────────────
    sub("18.6 Analiza sybil_soft_flags i sybil_lead_signal")

    def _count_str_field(records, field):
        c: dict = defaultdict(int)
        for r in records:
            v = r.get(field)
            if isinstance(v, str) and v:
                for token in v.replace(",", " ").split():
                    token = token.strip()
                    if token:
                        c[token] += 1
        return c

    flags_a = _count_str_field(rec_a, "sybil_soft_flags")
    flags_b = _count_str_field(rec_b, "sybil_soft_flags")
    all_flags = sorted(set(list(flags_a.keys()) + list(flags_b.keys())))

    leads_a: dict = defaultdict(int)
    leads_b: dict = defaultdict(int)
    for r in rec_a:
        v = r.get("sybil_lead_signal")
        if v and isinstance(v, str): leads_a[v] += 1
    for r in rec_b:
        v = r.get("sybil_lead_signal")
        if v and isinstance(v, str): leads_b[v] += 1

    if all_flags:
        print(f"\n  {C.DIM}{'sybil_soft_flag':<35} {'#A':>5} {'%A':>7} {'#B':>5} {'%B':>7}{C.RESET}")
        print(f"  {'─'*60}")
        for flag in all_flags:
            ca, cb = flags_a.get(flag, 0), flags_b.get(flag, 0)
            pa = ca / na * 100 if na > 0 else 0
            pb = cb / nb * 100 if nb > 0 else 0
            col = C.RED if (pa > 30 or pb > 30) else C.YELLOW if (pa > 10 or pb > 10) else C.DIM
            print(f"  {col}{flag:<35}{C.RESET} {ca:>5} {pa:>6.1f}% {cb:>5} {pb:>6.1f}%")
    else:
        note("Brak sybil_soft_flags w danych.")

    all_leads = sorted(set(list(leads_a.keys()) + list(leads_b.keys())))
    if all_leads:
        print(f"\n  {C.DIM}{'sybil_lead_signal':<30} {'#A':>5} {'%A':>7} {'#B':>5} {'%B':>7}{C.RESET}")
        print(f"  {'─'*58}")
        for lead in all_leads:
            ca, cb = leads_a.get(lead, 0), leads_b.get(lead, 0)
            pa = ca / na * 100 if na > 0 else 0
            pb = cb / nb * 100 if nb > 0 else 0
            col = C.RED if (pa > 20 or pb > 20) else C.YELLOW if (pa > 5 or pb > 5) else C.DIM
            print(f"  {col}{lead:<30}{C.RESET} {ca:>5} {pa:>6.1f}% {cb:>5} {pb:>6.1f}%")
    else:
        note("Brak sybil_lead_signal w danych.")

    # ─── 18.7 Korelacje sybil-metryk z innymi cechami ───────────────────────
    sub("18.7 Korelacje Spearmana sybil-metryk z kluczowymi cechami (|r|≥0.30)")
    hint("Szukamy metryk które silnie współzmierzają z sybil-sygnałami.")
    ref_fields = [
        "buy_ratio", "hhi", "timing_entropy", "volume_gini",
        "flip_ratio_10s", "compute_unit_cluster_dominance", "static_fee_profile_ratio",
        "fixed_size_buy_ratio", "buyer_pre_balance_cv", "early_slot_volume_dominance_buy",
        "dev_volume_ratio", "block0_sniped_supply_pct",
    ]
    combined = rec_a + rec_b

    for s_field, cfg in _SYBIL_METRICS_CONFIG.items():
        corrs_found = []
        for ref in ref_fields:
            r_sp, n_sp = spearman(combined, s_field, ref)
            if n_sp >= 5 and abs(r_sp) >= 0.30:
                corrs_found.append((abs(r_sp), r_sp, ref, n_sp))
        if corrs_found:
            corrs_found.sort(reverse=True)
            print(f"\n    {C.BOLD}{cfg['abbr']}{C.RESET} ({s_field}):")
            for _, r_val, ref, n_sp in corrs_found[:5]:
                lbl, col = corr_label(r_val)
                print(f"      {C.DIM}{ref:<40}{C.RESET}"
                      f" {col}{r_val:>+.3f}{C.RESET}  n={n_sp}  {col}{lbl}{C.RESET}")

    # ─── 18.8 Syntetyczny profil sybil A vs B ───────────────────────────────
    sub("18.8 Syntetyczny profil sybil — fingerprint A vs B")
    hint("Znormalizowane mediany (z-score globalny). Duże |z| = metryka odbiega od normy.")

    all_sybil_fields = list(_SYBIL_METRICS_CONFIG.keys())
    combined_cache = build_field_cache(combined, all_sybil_fields)
    va_cache       = build_field_cache(rec_a,    all_sybil_fields)
    vb_cache       = build_field_cache(rec_b,    all_sybil_fields)

    print(f"  {C.DIM}{'Metryka':<34} {'z_A':>8} {'z_B':>8} {'Δz':>7}  {'Próg_z':>7}  Ocena{C.RESET}")
    print(f"  {'─'*85}")

    for field, cfg in _SYBIL_METRICS_CONFIG.items():
        vc = combined_cache[field]
        va = va_cache[field]
        vb = vb_cache[field]
        if len(vc) < 4 or not va or not vb:
            note(f"  {cfg['abbr']}: za mało danych do fingerprinta")
            continue
        gmed = median_val(vc)
        gmad = mad(vc)
        if gmad < 1e-9: gmad = std(vc)
        if gmad < 1e-9:
            continue
        za = (median_val(va) - gmed) / gmad
        zb = (median_val(vb) - gmed) / gmad
        dz = za - zb
        # z-score progu konfiguracyjnego
        thr_z = (cfg["threshold"] - gmed) / gmad
        dz_col = C.YELLOW if abs(dz) > 1.0 else C.DIM
        # Ocena A
        if cfg["direction"] == "min":
            eval_a = "OK" if za >= thr_z else "NARUSZENIE"
            eval_b = "OK" if zb >= thr_z else "NARUSZENIE"
        else:
            eval_a = "OK" if za <= thr_z else "NARUSZENIE"
            eval_b = "OK" if zb <= thr_z else "NARUSZENIE"
        eval_col_a = C.GREEN if eval_a == "OK" else C.RED
        eval_col_b = C.GREEN if eval_b == "OK" else C.RED
        print(f"  {C.DIM}{cfg['abbr']:<6} {field[:27]:<27}{C.RESET}"
              f" {za:>+8.2f} {zb:>+8.2f} {dz_col}{dz:>+7.2f}{C.RESET}"
              f"  {thr_z:>+7.2f}"
              f"  A:{eval_col_a}{eval_a}{C.RESET} B:{eval_col_b}{eval_b}{C.RESET}")

    # ─── 18.9 Wyniki syntetyczne ─────────────────────────────────────────────
    sub("18.9 Podsumowanie warstwy sybil")

    sybil_active_a = sum(1 for r in rec_a if r.get("sybil_interference_layer_enabled") is True or
                          r.get("sybil_interference_layer_enabled") == 1)
    sybil_active_b = sum(1 for r in rec_b if r.get("sybil_interference_layer_enabled") is True or
                          r.get("sybil_interference_layer_enabled") == 1)
    combo_a = sum(1 for r in rec_a if r.get("sybil_combo_veto_enabled") is True or
                   r.get("sybil_combo_veto_enabled") == 1)
    combo_b = sum(1 for r in rec_b if r.get("sybil_combo_veto_enabled") is True or
                   r.get("sybil_combo_veto_enabled") == 1)

    row("sybil_interference_layer_enabled [A]",
        f"{sybil_active_a}/{na} ({sybil_active_a/na*100:.0f}%)" if na else "N/A", C.CYAN)
    row("sybil_interference_layer_enabled [B]",
        f"{sybil_active_b}/{nb} ({sybil_active_b/nb*100:.0f}%)" if nb else "N/A", C.MAGENTA)
    row("sybil_combo_veto_enabled [A]",
        f"{combo_a}/{na} ({combo_a/na*100:.0f}%)" if na else "N/A", C.CYAN)
    row("sybil_combo_veto_enabled [B]",
        f"{combo_b}/{nb} ({combo_b/nb*100:.0f}%)" if nb else "N/A", C.MAGENTA)

    # Rekomendacja
    print()
    all_covered = sum(
        1 for f in ["fee_topology_diversity_index", "dev_buyer_infrastructure_affinity",
                    "signer_cross_pool_velocity"]
        if len(combined_cache.get(f, [])) > 0
    )
    degraded_dominant = sum(
        1 for f in ["spend_fraction_divergence", "demand_elasticity_score",
                    "funding_source_concentration"]
        if len(combined_cache.get(f, [])) == 0
    )

    if all_covered >= 3 and degraded_dominant >= 2:
        warn("Metryki FTDI/DBIA/CPV dostępne ✓ | SFD/DES/FSC przeważnie zdegradowane — "
             "rozważ domknięcie transportu (signer_post_balance, curve_data, FSC index warmup).")
    elif all_covered >= 3 and degraded_dominant == 0:
        ok("Wszystkie 6 sybil-metryk dostępne w obu zbiorach — pełna analiza możliwa.")
    elif all_covered == 0:
        bad("ŻADNA z sybil-metryk nie jest obecna w danych — "
            "sprawdź log_schema_version i sybil_interference_layer_enabled.")
    else:
        note(f"Coverage: {all_covered}/3 pełnych metryk | {degraded_dominant}/3 zdegradowanych.")


# ══════════════════════════════════════════════════════════════════════════════
#  MAIN  (na końcu pliku — wszystkie sekcje 1–18 już zdefiniowane powyżej)
# ══════════════════════════════════════════════════════════════════════════════
DEFAULT_DIR = Path("/root/Ghost/logs/decisions.jsonl")


def main():
    if len(sys.argv) >= 3:
        path_a = sys.argv[1]
        path_b = sys.argv[2]
    elif len(sys.argv) == 2:
        d = Path(sys.argv[1])
        path_a = str(d / "zbior_A.jsonl")
        path_b = str(d / "zbior_B.jsonl")
    else:
        path_a = str(DEFAULT_DIR / "zbior_A.jsonl")
        path_b = str(DEFAULT_DIR / "zbior_B.jsonl")

    for p, lname in [(path_a, "A"), (path_b, "B")]:
        if not Path(p).exists():
            print(f"{C.RED}Plik nie istnieje: {p}{C.RESET}")
            sys.exit(1)

    rec_a_raw = load(path_a)
    rec_b_raw = load(path_b)

    if not rec_a_raw:
        print(f"{C.RED}Brak danych w zbiorze A: {path_a}{C.RESET}")
        sys.exit(1)
    if not rec_b_raw:
        print(f"{C.RED}Brak danych w zbiorze B: {path_b}{C.RESET}")
        sys.exit(1)

    # ── Przechwytywanie stdout (tee → HTML) ─────────────────────────────────
    _tee = TeeWriter(sys.stdout)
    sys.stdout = _tee

    print(f"\n{C.BOLD}{C.MAGENTA}")
    print("  ╔═══════════════════════════════════════════════════════════════════════════╗")
    print("  ║       📊 ANALIZA PORÓWNAWCZA ZBIORÓW A vs B  v5.0                        ║")
    print("  ║  Spearman · Cohen d · MW-U · DTW · Causal PC · TDA · Hill · MI           ║")
    print("  ║  NEW: Youden J · KS · AUC · Bhattacharyya · LR L1 · Bootstrap CI        ║")
    print("  ║  NEW: Scoring Rule · Decision Rules · Ready-to-implement thresholds      ║")
    print("  ║  v5.0: SYBIL INTERFERENCE — FTDI/DBIA/SFD/DES/CPV/FSC analysis         ║")
    print("  ╚═══════════════════════════════════════════════════════════════════════════╝")
    print(C.RESET)
    note(f"Zbiór A (raw): {path_a}  ({len(rec_a_raw)} rekordów)")
    note(f"Zbiór B (raw): {path_b}  ({len(rec_b_raw)} rekordów)")

    # ── AUTODETECT parametrów filtracji (przed filtrem!) ────────────────────
    config, autodetect_notes = autodetect_filter_params(rec_a_raw, rec_b_raw)
    note(f"CONFIG (po autodetect): WINDOW_MS={config.expected_window_ms}"
         f"{'(ANY)' if config.allow_any_window_ms else ''}"
         f"  MIN_TX={config.min_tx_in_window}  MIN_VEC_LEN={config.min_vector_len}")
    for n in autodetect_notes:
        hint(f"  autodetect → {n}")

    # ── Sekcja 0: Filtracja A/B ─────────────────────────────────────────────
    hdr("🔍 SEKCJA 0: FILTRACJA A/B + DEDUP", C.YELLOW)
    rec_a, stats_a = filter_ab_records(rec_a_raw, "A", config)
    rec_b, stats_b = filter_ab_records(rec_b_raw, "B", config)
    _print_filter_report("A", stats_a, C.CYAN, config)
    _print_filter_report("B", stats_b, C.MAGENTA, config)

    if len(rec_a) < 10:
        bad(f"Zbiór A po filtrze ma tylko {len(rec_a)} rekordów — za mało do analizy (min 10). PRZERWANIE.")
        sys.stdout = _tee.original
        sys.exit(1)
    if len(rec_b) < 10:
        bad(f"Zbiór B po filtrze ma tylko {len(rec_b)} rekordów — za mało do analizy (min 10). PRZERWANIE.")
        sys.stdout = _tee.original
        sys.exit(1)

    # Vector integrity check (3 sample records)
    sub("Vector integrity check (sample)")
    for name_s, recs, col in [("A", rec_a, C.CYAN), ("B", rec_b, C.MAGENTA)]:
        for idx, r in enumerate(recs[:3]):
            ts = r.get("vectors_ts_offsets_ms")
            pr = r.get("vectors_prices")
            sa = r.get("vectors_sol_amounts")
            iv = r.get("vectors_interval_ms")
            dp = r.get("vectors_d_price")
            if isinstance(ts, list) and isinstance(pr, list) and isinstance(sa, list):
                ok_len = len(ts) == len(pr) == len(sa)
                ok_d   = True
                if isinstance(iv, list): ok_d = ok_d and (len(iv) == len(ts) - 1)
                if isinstance(dp, list): ok_d = ok_d and (len(dp) == len(ts) - 1)
                if ok_len and ok_d:
                    ok(f"Zbiór {name_s} rec[{idx}]: ts={len(ts)} prices={len(pr)} "
                       f"d_price={len(dp) if isinstance(dp, list) else 'N/A'} — OK")
                else:
                    warn(f"Zbiór {name_s} rec[{idx}]: niespójne długości wektorów!")
            else:
                note(f"Zbiór {name_s} rec[{idx}]: brak wektorów v3")

    ok(f"Zbiór A po filtrze: {len(rec_a)} rekordów")
    ok(f"Zbiór B po filtrze: {len(rec_b)} rekordów")

    # SEKCJA 1 & 2: Profile
    corrs_a = section_profile(rec_a, "A", C.CYAN)
    corrs_b = section_profile(rec_b, "B", C.MAGENTA)

    # SEKCJA 3: Porównanie A vs B
    disc_scores = section_compare(rec_a, rec_b, corrs_a, corrs_b)

    # SEKCJA 4: Deep dive
    section_deep_dive(rec_a, rec_b, disc_scores)

    # SEKCJA 6–10: Zaawansowane (opcjonalne biblioteki)
    section_dtw(rec_a, rec_b, config)
    section_causal(rec_a, rec_b)
    section_tda(rec_a, rec_b)
    section_mutual_info(rec_a, rec_b)
    section_hill(rec_a, rec_b)

    # SEKCJA 11–17: Threshold engine (pure Python, zawsze działa)
    threshold_results = section_optimal_thresholds(rec_a, rec_b, disc_scores)
    auc_results       = section_auc_ranking(rec_a, rec_b)
    section_distribution_overlap(rec_a, rec_b, disc_scores)
    section_logistic_regression(rec_a, rec_b)
    section_scoring_rule(rec_a, rec_b, threshold_results)
    section_bootstrap_thresholds(rec_a, rec_b, threshold_results)
    section_decision_rules_summary(rec_a, rec_b, threshold_results,
                                   auc_results, disc_scores)

    # SEKCJA 18: Sybil Interference (zawsze — czyste Python, bez dep zewnętrznych)
    section_sybil_interference(rec_a, rec_b)

    # SEKCJA 5: Podsumowanie (zawsze na końcu)
    section_summary(rec_a, rec_b, disc_scores)

    hdr("✅ ANALIZA ZAKOŃCZONA", C.GREEN)
    ok(f"Zbiór A: {len(rec_a)} rekordów (z {len(rec_a_raw)} raw)"
       f"  |  Zbiór B: {len(rec_b)} rekordów (z {len(rec_b_raw)} raw)")
    print()

    # ── Zapisz raport HTML ──────────────────────────────────────────────────
    sys.stdout = _tee.original
    try:
        html_path = save_html_report(_tee.get_captured(), path_a, path_b)
        print(f"\n  {C.GREEN}{C.BOLD}📄 Raport HTML zapisany:{C.RESET} {C.CYAN}{html_path}{C.RESET}\n")
    except Exception as exc:
        print(f"\n  {C.YELLOW}⚠  Nie udało się zapisać raportu HTML: {exc}{C.RESET}\n")


if __name__ == "__main__":
    main()