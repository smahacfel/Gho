#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
Kompleksowa analiza zbiorów A (sukces) vs B (wtopa) — XGBoost + SHAP.
Wyjście: 6 plików (0_cv_wyniki.csv, 1_statystyki_rozkladow.csv,
         2_pelna_analiza_cech.csv, 3_temporal_wyniki.csv,
         4_odrzucone_pola_leakage.csv, ghost_selector_xgb.json)

Kontrakt anty-leakage:
- model używa wyłącznie top-level numerycznych skalarów,
- dict/list/payload/snapshot/vector/string/bool są odrzucane,
- pola outcome/lifecycle/entry/exit/truth/decision/verdict/reason/config/hash/id/time
  są odrzucane,
- progi klasyfikacji w CV/temporal są dobierane tylko na zbiorze treningowym,
- dobór cech w foldach i temporal split bazuje tylko na oknie treningowym.
"""

import json
import warnings
from collections import Counter
from typing import Optional

import pandas as pd
import numpy as np
import shap
from scipy.stats import ks_2samp
from xgboost import XGBClassifier
from sklearn.model_selection import StratifiedKFold
from sklearn.metrics import (
    roc_auc_score, precision_recall_curve, auc, classification_report
)

warnings.filterwarnings('ignore')

# ---------------------------------------------------------------------------
# Konfiguracja
# ---------------------------------------------------------------------------

LEAKAGE_PATTERNS = [
    "pnl", "profit", "loss", "target", "stop", "timeout",
    "label", "outcome", "result", "exit", "entry", "trigger", "future",
    "lifecycle", "final", "decision", "verdict", "reason",
    "reason_code", "status", "snapshot", "payload", "source",
    "id", "join", "phase", "slot", "run", "record", "truth",
    "hash", "config", "confidence", "evidence", "gate", "policy",
    "shadow", "legacy", "dispatch", "quote", "position", "candidate",
    "sample", "timestamp", "clock", "wall", "eval", "terminal",
    "window_close", "close_reason", "rollout", "namespace", "profile",
]

# Pola znane z rekordów decision+lifecycle, które nigdy nie mogą trafić do modelu.
LEAKAGE_FIELDS = {
    "entry_price", "exit_price", "final_pnl_pct", "final_pnl",
    "target_hit", "stop_hit", "target_hit_ts_ms", "stop_hit_ts_ms",
    "business_label", "observation_start_ts_ms", "first_seen_ts_ms",
    "observation_end_ts_ms", "sample_age_ms", "position_epoch",
    "entry_value_sol", "exit_value_sol", "gross_pnl_sol", "net_pnl_sol",
    "estimated_costs_sol", "total_exits", "fraction_bps", "remaining_fraction_bps",
    "entry_slot", "entry_simulation_rpc_slot", "entry_market_anchor_slot",
    "entry_landed_slot", "exit_sample_slot", "exit_market_anchor_slot",
    "exit_reason_evaluation_ts_ms", "exit_landed_slot", "lane",
    "_decision_source_line", "sample_slot", "timestamp_ms",
    "sample_timestamp_ms", "_decision_source_file", "_lifecycle_source_file",
    "_lifecycle_source_line", "_merged_mint_id", "ab_record_id",
    "ab_t0_event_ts_ms", "ab_t_end_event_ts_ms", "ab_tx_count_window",
    "ab_unique_signers_window", "ab_window_complete", "ab_window_ms",
    "ab_window_origin", "ab_window_close_reason", "brain_config_hash",
    "brain_config_path", "candidate_id", "config_hash", "decision_eval_snapshots",
    "decision_plane", "decision_reason", "decision_time_series_dropped_oldest_count",
    "decision_time_series_retained_sample_count", "decision_time_series_retention_capacity",
    "decision_time_series_retention_policy", "decision_time_series_retention_status",
    "decision_time_series_total_tx_count", "decision_verdict_buy", "dispatch_source",
    "evidence_policy_context", "funding_source_diagnostics", "funding_source_v2",
    "gatekeeper_decision_payload", "gatekeeper_gate_trace", "gatekeeper_v2_config_payload",
    "gatekeeper_v2_phase_pass_vector", "gatekeeper_v2_replay_input_schema_version",
    "gatekeeper_v2_replay_ready_non_temporal", "gatekeeper_v2_replay_ready_temporal",
    "gatekeeper_v3_config_payload", "legacy_live_reason_chain", "legacy_live_verdict_buy",
    "legacy_live_verdict_type", "materialized_feature_snapshot", "join_key", "mint_id",
    "position_id", "pool_id", "quote_id", "reason_code", "reason_code_version",
    "record_type", "rollout_profile", "run_id", "sample_price_state", "session_id",
    "shadow_early_verdict", "shadow_extended_verdict", "shadow_fsc_v2_reason_if_enabled",
    "shadow_normal_verdict", "shadow_tas_reject_reason", "shadow_execution_outcome",
    "truth_source", "truth_status", "time_stop_v2_status", "time_stop_v2_candidate",
    "time_stop_v2_candidate_subreason", "time_stop_v2_subreason",
    "time_stop_v2_candidate_ts_ms", "time_stop_v2_failed_windows",
    "v25_shadow_decisions_payload", "v25_shadow_reason_chain", "v25_shadow_verdict_type",
    "v25_confidence_zeroed_by_pdd_hard_fail", "v25_confidence_zeroed_by_tas_hard_reject",
    "v25_confidence_unavailable_reason", "v3_evidence_status", "v3_feature_snapshot_hash",
    "v3_materialized_feature_snapshot", "v3_policy_config_hash", "v3_policy_config_payload",
    "v3_replay_payload_schema_version", "v3_shadow_confidence_after_risk",
    "v3_shadow_confidence_after_stage", "v3_shadow_confidence_cap",
    "v3_shadow_confidence_cap_reasons", "v3_shadow_confidence_final",
    "v3_shadow_confidence_raw", "v3_shadow_evidence_status", "v3_shadow_opportunity_status",
    "v3_shadow_reason_chain", "v3_shadow_reason_code", "v3_shadow_risk_status",
    "v3_shadow_secondary_reason_codes", "v3_shadow_verdict",
    "vectors_price_source_account_state_count", "vectors_price_source_carry_forward_count",
    "vectors_price_source_history_count", "vectors_price_source_market_cap_count",
    "vectors_price_source_missing_count", "vectors_price_source_quote_count",
    "vectors_price_source_reserve_count",
}

# Pola _ts_ms, które SĄ dopuszczone jako cechy. Pusta lista: zakaz czasu absolutnego.
TIMESTAMP_WHITELIST = set()

# Prefix/suffix blokujący leakage z progów, konfiguracji, diagnostyki i decyzji.
BLOCKED_PREFIXES = (
    "min_", "max_", "aps_", "v25_", "v3_", "legacy_", "shadow_",
    "gatekeeper_", "decision_", "selector_", "sybil_metric_",
    "time_stop_", "ab_", "vectors_", "curve_t0_", "end_10s_",
)
BLOCKED_SUFFIXES = (
    "_enabled", "_passed", "_pass", "_ready", "_known", "_present",
    "_eligible", "_available", "_source", "_status", "_reason",
    "_policy", "_mode", "_type", "_version", "_schema_version",
    "_hash", "_path", "_file", "_line", "_payload", "_snapshot",
    "_ts_ms", "_timestamp_ms", "_slot", "_id", "_verdict",
)

# Jawnie dopuszczone nazwy, gdy prefix max_ oznacza obserwowaną metrykę, a nie próg.
SAFE_NAME_ALLOWLIST = {
    "max_tx_per_signer_observed",
    "max_consecutive_buys_observed",
    "max_single_tx_price_impact_pct_observed",
    "max_single_sell_impact_pct_observed",
}

SHAP_SAMPLE_SIZE = 5_000
MIN_NON_NULL_FEATURE_VALUES = 50

XGB_PARAMS = dict(
    n_estimators=300,
    max_depth=4,
    learning_rate=0.05,
    subsample=0.8,
    colsample_bytree=0.8,
    tree_method='hist',
    eval_metric='logloss',
    random_state=42,
    n_jobs=-1,
)

# ---------------------------------------------------------------------------
# Utility
# ---------------------------------------------------------------------------

def load_jsonl(path: str, label: int) -> pd.DataFrame:
    rows = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            r['y'] = label
            rows.append(r)
    return pd.DataFrame(rows)


def _has_nested_or_non_scalar_value(series: pd.Series) -> bool:
    sample = series.dropna().head(200)
    return any(isinstance(v, (dict, list, tuple, set)) for v in sample)


def _is_bool_like(series: pd.Series) -> bool:
    sample = series.dropna().head(200)
    if sample.empty:
        return False
    return all(isinstance(v, (bool, np.bool_)) for v in sample)


def leakage_reason(col: str, series: Optional[pd.Series] = None) -> Optional[str]:
    """Zwraca powód odrzucenia kolumny albo None, jeśli może wejść do selekcji cech."""
    if col == 'y':
        return "label"

    c = col.lower()

    if col in SAFE_NAME_ALLOWLIST:
        return None
    if col in LEAKAGE_FIELDS:
        return "explicit_leakage_field"
    if c.endswith("_ts_ms") and col not in TIMESTAMP_WHITELIST:
        return "absolute_or_event_timestamp"
    if any(c.startswith(prefix) for prefix in BLOCKED_PREFIXES):
        return "blocked_prefix"
    if any(c.endswith(suffix) for suffix in BLOCKED_SUFFIXES):
        return "blocked_suffix"
    for p in LEAKAGE_PATTERNS:
        if p in c:
            return f"blocked_pattern:{p}"

    if series is not None:
        if _has_nested_or_non_scalar_value(series):
            return "nested_or_sequence_value"
        if _is_bool_like(series):
            return "bool_flag"
        numeric = pd.to_numeric(series, errors="coerce")
        if numeric.notna().sum() < MIN_NON_NULL_FEATURE_VALUES:
            return "too_few_numeric_values"
        if numeric.nunique(dropna=True) < 2:
            return "constant_or_single_value"

    return None


def is_leakage_col(col: str) -> bool:
    """Kompatybilny wrapper dla starszych wywołań."""
    return leakage_reason(col) is not None


def build_features(df: pd.DataFrame, candidate_cols: Optional[list] = None) -> list:
    feature_cols = []
    cols = candidate_cols if candidate_cols is not None else list(df.columns)
    for col in cols:
        if col not in df.columns:
            continue
        if leakage_reason(col, df[col]) is not None:
            continue
        feature_cols.append(col)
    return feature_cols


def write_leakage_report(df: pd.DataFrame, features: list) -> None:
    rows = []
    selected = set(features)
    for col in df.columns:
        if col == 'y':
            continue
        reason = leakage_reason(col, df[col])
        rows.append({
            'kolumna': col,
            'status': 'selected_feature' if col in selected else 'rejected',
            'powod': 'decision_time_numeric_scalar' if col in selected else reason,
            'non_null': int(df[col].notna().sum()),
            'dtype': str(df[col].dtype),
        })
    report = pd.DataFrame(rows).sort_values(['status', 'powod', 'kolumna'])
    report.to_csv("4_odrzucone_pola_leakage.csv", index=False)
    rejected = report[report['status'] == 'rejected']
    reasons = Counter(rejected['powod'].fillna('unknown'))
    print("[DANE] Raport leakage zapisany: 4_odrzucone_pola_leakage.csv")
    print("[DANE] Najczęstsze powody odrzucenia pól:")
    for reason, count in reasons.most_common(8):
        print(f"       - {reason}: {count}")


def to_feature_matrix(df: pd.DataFrame, features: list,
                       medians: pd.Series = None) -> tuple:
    """
    Buduje macierz X: float, Inf→NaN, NaN→mediana.

    Mediany muszą pochodzić ze zbioru treningowego dla danego foldu/splitu.
    """
    X = df[features].apply(pd.to_numeric, errors="coerce")
    X = X.replace([np.inf, -np.inf], np.nan)
    if medians is None:
        medians = X.median()
    return X.fillna(medians), medians


def _best_threshold_f1(y_true: np.ndarray, y_proba: np.ndarray) -> float:
    """Próg maksymalizujący F1 na danych treningowych/kalibracyjnych."""
    precision, recall, thresholds = precision_recall_curve(y_true, y_proba)
    if len(thresholds) == 0:
        return 0.5
    f1 = 2 * precision[:-1] * recall[:-1] / (precision[:-1] + recall[:-1] + 1e-9)
    return float(thresholds[np.argmax(f1)])


# ---------------------------------------------------------------------------
# Krok 1 — Testy statystyczne
# ---------------------------------------------------------------------------

def analyze_distributions(df: pd.DataFrame, features: list) -> pd.DataFrame:
    print("\n[1/5] Testy Kołmogorowa-Smirnowa na rozkładach cech...")
    results = []
    df_A, df_B = df[df['y'] == 1], df[df['y'] == 0]

    for f in features:
        A_vals = pd.to_numeric(df_A[f], errors='coerce').dropna()
        B_vals = pd.to_numeric(df_B[f], errors='coerce').dropna()
        if len(A_vals) < 10 or len(B_vals) < 10:
            continue
        stat, p_val = ks_2samp(A_vals, B_vals)
        results.append({
            'cecha':       f,
            'A_mediana':   A_vals.median(),
            'A_std':       A_vals.std(),
            'B_mediana':   B_vals.median(),
            'B_std':       B_vals.std(),
            'ks_stat':     stat,
            'p_value':     p_val,
            'istotna_p05': p_val < 0.05,
        })

    res_df = pd.DataFrame(results).sort_values('ks_stat', ascending=False)
    res_df.to_csv("1_statystyki_rozkladow.csv", index=False)
    n_sig = int(res_df['istotna_p05'].sum()) if len(res_df) else 0
    print(f"    -> {n_sig}/{len(res_df)} cech istotnych statystycznie (p<0.05)")
    print(f"    -> Zapisano: 1_statystyki_rozkladow.csv")
    return res_df


# ---------------------------------------------------------------------------
# Krok 2 — Cross Validation
# ---------------------------------------------------------------------------

def run_cross_validation(df: pd.DataFrame, candidate_features: list,
                          y: np.ndarray, pos: int, neg: int) -> pd.DataFrame:
    """
    Dobór cech, mediany imputacji i próg F1 są liczone wyłącznie na treningowej
    części foldu. Test fold nie zasila preprocessingu ani kalibracji progu.
    """
    print("\n[2/5] 5-Fold Stratified Cross Validation (Ranking Power)...")
    skf = StratifiedKFold(n_splits=5, shuffle=True, random_state=42)

    fold_results = []
    oof_proba    = np.zeros(len(y), dtype=np.float64)
    best_thrs    = []

    df_reset = df.reset_index(drop=True)

    for fold, (train_idx, test_idx) in enumerate(skf.split(df_reset[candidate_features], y), 1):
        train_df = df_reset.iloc[train_idx]
        test_df = df_reset.iloc[test_idx]
        fold_features = build_features(train_df, candidate_cols=candidate_features)
        if not fold_features:
            raise SystemExit(f"[ERROR] Fold {fold}: brak cech po treningowej filtracji leakage.")

        X_train, fold_medians = to_feature_matrix(train_df, fold_features)
        X_test,  _            = to_feature_matrix(test_df,  fold_features,
                                                  medians=fold_medians)
        y_train, y_test = y[train_idx], y[test_idx]

        fold_model = XGBClassifier(scale_pos_weight=(neg / pos), **XGB_PARAMS)
        fold_model.fit(X_train, y_train, verbose=False)

        train_preds = fold_model.predict_proba(X_train)[:, 1]
        best_t = _best_threshold_f1(y_train, train_preds)
        best_thrs.append(best_t)

        preds = fold_model.predict_proba(X_test)[:, 1]
        oof_proba[test_idx] = preds

        roc_auc_val  = roc_auc_score(y_test, preds)
        prec, rec, _ = precision_recall_curve(y_test, preds)
        pr_auc       = auc(rec, prec)

        print(f"    Fold {fold}: ROC-AUC={roc_auc_val:.4f}  PR-AUC={pr_auc:.4f}  train_thr={best_t:.3f}  features={len(fold_features)}")
        fold_results.append({
            'fold':              fold,
            'roc_auc':           roc_auc_val,
            'pr_auc':            pr_auc,
            'train_threshold_f1': best_t,
            'n_features':         len(fold_features),
            'n_train':           len(train_idx),
            'n_test':            len(test_idx),
        })

    cv_df = pd.DataFrame(fold_results)
    cv_df.to_csv("0_cv_wyniki.csv", index=False)

    print(f"    -> Śr. ROC-AUC: {cv_df['roc_auc'].mean():.4f} ± {cv_df['roc_auc'].std():.4f}")
    print(f"    -> Zapisano fold-by-fold: 0_cv_wyniki.csv")

    median_thr = float(np.median(best_thrs))
    oof_preds  = (oof_proba >= median_thr).astype(int)
    print(f"\n    OOF Classification Report (próg={median_thr:.3f}, mediana train max-F1):")
    print(classification_report(y, oof_preds, target_names=["Wtopa(B)", "Sukces(A)"], digits=4))

    return cv_df


# ---------------------------------------------------------------------------
# Krok 3 — Temporal Split (Out of Sample Time Test)
# ---------------------------------------------------------------------------

def run_temporal_validation(df: pd.DataFrame, candidate_features: list,
                             y: np.ndarray, pos: int, neg: int) -> float:
    """
    Dobór cech, mediana imputacji i próg F1 są liczone tylko z pierwszych 80%.
    Końcowe 20% jest czystym out-of-sample oknem czasowym.
    """
    print("\n[3/5] Temporal Validation (Out-Of-Sample chronologicznie)...")
    cutoff = int(len(df) * 0.8)

    df_train, df_test = df.iloc[:cutoff], df.iloc[cutoff:]
    y_train_t, y_test_t = y[:cutoff], y[cutoff:]

    temporal_features = build_features(df_train, candidate_cols=candidate_features)
    if not temporal_features:
        raise SystemExit("[ERROR] Temporal split: brak cech po treningowej filtracji leakage.")

    X_train_t, train_medians = to_feature_matrix(df_train, temporal_features)
    X_test_t,  _             = to_feature_matrix(df_test,  temporal_features,
                                                 medians=train_medians)

    temporal_model = XGBClassifier(scale_pos_weight=(neg / pos), **XGB_PARAMS)
    temporal_model.fit(X_train_t, y_train_t, verbose=False)
    train_preds_t = temporal_model.predict_proba(X_train_t)[:, 1]
    best_t        = _best_threshold_f1(y_train_t, train_preds_t)
    preds_t       = temporal_model.predict_proba(X_test_t)[:, 1]

    roc_auc_val  = roc_auc_score(y_test_t, preds_t)
    prec, rec, _ = precision_recall_curve(y_test_t, preds_t)
    pr_auc       = auc(rec, prec)

    print(f"    -> Temporal ROC-AUC: {roc_auc_val:.4f}")
    print(f"    -> Temporal PR-AUC:  {pr_auc:.4f}")
    print(f"    -> Próg F1 wyznaczony na treningowym oknie: {best_t:.3f}")

    temporal_preds = (preds_t >= best_t).astype(int)
    print(f"\n    Temporal OOS Classification Report (train próg={best_t:.3f}):")
    print(classification_report(y_test_t, temporal_preds,
                                 target_names=["Wtopa(B)", "Sukces(A)"], digits=4))

    pd.DataFrame([{
        'roc_auc':           roc_auc_val,
        'pr_auc':            pr_auc,
        'train_threshold_f1': best_t,
        'n_features':         len(temporal_features),
        'n_train':           cutoff,
        'n_test':            len(df) - cutoff,
    }]).to_csv("3_temporal_wyniki.csv", index=False)
    print(f"    -> Zapisano: 3_temporal_wyniki.csv")

    return roc_auc_val


# ---------------------------------------------------------------------------
# Krok 5 — SHAP
# ---------------------------------------------------------------------------

def run_shap_analysis(model: XGBClassifier, X: pd.DataFrame, features: list) -> pd.DataFrame:
    print("\n[5/5] Analiza SHAP (interpretowalność)...")

    if len(X) > SHAP_SAMPLE_SIZE:
        print(f"    [!] Duży dataset — SHAP obliczany na próbce {SHAP_SAMPLE_SIZE} wierszy")
        X_shap = X.sample(SHAP_SAMPLE_SIZE, random_state=42)
    else:
        X_shap = X

    explainer   = shap.TreeExplainer(model)
    shap_values = explainer.shap_values(X_shap)

    if isinstance(shap_values, list):
        shap_values = shap_values[1]

    mean_abs_shap = np.abs(shap_values).mean(axis=0)

    correlations = []
    for i, col in enumerate(features):
        fv = X_shap[col].values
        sv = shap_values[:, i]
        if np.std(fv) < 1e-9 or np.std(sv) < 1e-9:
            correlations.append(np.nan)
        else:
            correlations.append(float(np.corrcoef(fv, sv)[0, 1]))

    shap_df = pd.DataFrame({
        'cecha':                    features,
        'sredni_wplyw_absolutny':   mean_abs_shap,
        'korelacja_ze_zbiorem_A':   correlations,
    }).sort_values('sredni_wplyw_absolutny', ascending=False)

    return shap_df


def _get_logic(row: pd.Series) -> str:
    corr = row.get('korelacja_ze_zbiorem_A')
    if pd.isna(corr):
        return "Brak kierunku (zero-variance lub brak KS)"
    if corr > 0.1:
        return "WYŻSZA wartość → Sukces (A)"
    elif corr < -0.1:
        return "WYŻSZA wartość → Wtopa (B)"
    else:
        return "Nieliniowa (efekt anomalii / skrajności)"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    print("=" * 65)
    print("  ANALIZA: A (SUKCES) vs B (WTOPA)  |  XGBoost + SHAP")
    print("=" * 65)

    try:
        df_A = load_jsonl("zbior_A.jsonl", label=1)
        df_B = load_jsonl("zbior_B.jsonl", label=0)
    except Exception as e:
        raise SystemExit(f"[ERROR] {e}")

    df       = pd.concat([df_A, df_B], ignore_index=True)
    pos, neg = len(df_A), len(df_B)

    if pos == 0:
        raise SystemExit("[ERROR] Zbiór A jest pusty.")
    if neg == 0:
        raise SystemExit("[ERROR] Zbiór B jest pusty.")

    print(f"\n[DANE] Sukces (A): {pos} | Wtopa (B): {neg} | Stosunek 1:{neg/pos:.2f}")

    time_cols_to_check = ['curve_t0_event_ts_ms', 'observation_start_ts_ms',
                          'first_seen_ts_ms', 'timestamp_ms']
    sort_col = next((c for c in time_cols_to_check if c in df.columns), None)

    if sort_col:
        print(f"[DANE] Sortowanie chronologiczne po kolumnie: {sort_col}")
        df = df.sort_values(sort_col).reset_index(drop=True)
    else:
        print("[!] UWAGA: Nie znaleziono kolumny czasu. Zakładam, że pliki są już zrzucone chronologicznie.")

    candidate_features = build_features(df)
    print(f"[DANE] Cechy po konserwatywnej filtracji leakage: {len(candidate_features)}")
    write_leakage_report(df, candidate_features)

    if not candidate_features:
        raise SystemExit("[ERROR] Brak cech numerycznych po filtracji — sprawdź dane wejściowe.")

    y = df['y'].values

    dist_df = analyze_distributions(df, candidate_features)
    cv_df = run_cross_validation(df, candidate_features, y, pos, neg)
    temporal_auc = run_temporal_validation(df, candidate_features, y, pos, neg)

    print("\n[4/5] Trening finalnego modelu na pełnym zbiorze...")
    final_features = build_features(df, candidate_cols=candidate_features)
    X_final, _ = to_feature_matrix(df, final_features)
    final_model = XGBClassifier(scale_pos_weight=(neg / pos), **XGB_PARAMS)
    final_model.fit(X_final, y, verbose=False)
    final_model.save_model("ghost_selector_xgb.json")
    print("    -> Model zapisany: ghost_selector_xgb.json")

    shap_df = run_shap_analysis(final_model, X_final, final_features)

    final_report = pd.merge(shap_df, dist_df, on='cecha', how='left')
    final_report['jak_dziala_na_bota'] = final_report.apply(_get_logic, axis=1)
    final_report.to_csv("2_pelna_analiza_cech.csv", index=False)
    print("    -> Zapisano: 2_pelna_analiza_cech.csv")

    print("\n" + "=" * 65)
    print("  PODSUMOWANIE TEMPORALNE (STABILNOŚĆ)")
    print("=" * 65)
    cv_auc = cv_df['roc_auc'].mean()
    diff = cv_auc - temporal_auc

    print(f"Średni ROC-AUC z K-Fold (In-Sample Time): {cv_auc:.4f}")
    print(f"ROC-AUC z Temporal Split (Out-Of-Sample): {temporal_auc:.4f}")

    if diff > 0.05:
        print("\n[!!!] KRYTYCZNY ALARM [!!!]")
        print(f"Spadek skuteczności o {diff:.4f} na najnowszych danych. Meta Pump.fun uległa zmianie!")
        print("Model nie radzi sobie z aktualnym rynkiem. Oczekiwany poważny dryft.")
    elif diff < -0.02:
        print("\n[OK] ZIELONE ŚWIATŁO: Nowe dane wpadają w reguły jeszcze lepiej niż stare.")
    else:
        print("\n[OK] ZIELONE ŚWIATŁO: Model stabilny w czasie. Brak oznak drastycznego dryftu.")

    print("\n" + "=" * 65)
    print("  TOP 10 ZMIENNYCH (wg SHAP)")
    print("=" * 65)

    for rank, (_, row) in enumerate(final_report.head(10).iterrows(), 1):
        a_med  = f"{row['A_mediana']:.4f}" if pd.notna(row.get('A_mediana')) else "N/A"
        b_med  = f"{row['B_mediana']:.4f}" if pd.notna(row.get('B_mediana')) else "N/A"
        ks_str = (f"KS={row['ks_stat']:.4f} p={row['p_value']:.4f}"
                  if pd.notna(row.get('ks_stat')) else "KS=brak (za mało próbek)")
        print(f"\n  [{rank:2d}] {row['cecha'].upper()}")
        print(f"       Kierunek : {row['jak_dziala_na_bota']}")
        print(f"       Mediany  : A={a_med}  B={b_med}")
        print(f"       SHAP     : {row['sredni_wplyw_absolutny']:.4f}  |  {ks_str}")


if __name__ == "__main__":
    main()
