# PLAN P3.7 Truth-Source Acquisition

Data: 2026-05-18

Status: **BLOCKER REMEDIATION PLAN / NO P2 / NO LIVE**

## 1. Context

P3.7 Phase A wykazal, ze R10/R11/R13 maja poprawny full replay,
label-v2 artefakty i execution-feasibility join, ale nie maja wystarczajacej
prawdy outcome do `good_clean` / `good_executable`.

Aktualne blokery:

- `no_good_clean_rows`,
- `no_good_executable_rows`,
- `no_label_v2_mfe_mae_rows`,
- `no_post_decision_price_path_rows`.

Raport zrodlowy:

- `PLANS/AUDYT/RAPORT_P3_7_EVIDENCE_AVAILABILITY_R10_R11_R13_20260518.md`

## 2. Decision

Nie przechodzimy do P3.7 Phase B feature prototype jako candidate work.

Nastepny krok to pozyskanie albo odtworzenie post-decision price/lifecycle path
dla R10/R11/R13 w formacie, ktory Outcome Label v2 moze przeliczyc na:

- MFE/MAE 10s/30s/60s,
- time-to-MFE/time-to-MAE,
- drawdown-before-plus40,
- clean/dirty market outcome,
- execution-feasibility-aware final decision quality.

## 3. Non-goals

Nie wolno:

- promowac V3 do P2,
- wlaczac live,
- tuningowac thresholdow,
- traktowac decision-time vectors jako outcome truth,
- traktowac `threshold_window_max_return_pct` jako pelnej sciezki MFE/MAE,
- patchowac historycznych R10/R11/R13 artefaktow in place,
- patchowac log-local `fetch_pool_price_at_30s.py` jako kanonicznego narzedzia repo.

## 4. Evidence classification

### 4.1 Dostepne, ale tylko decision-time

Decision logs zawieraja:

- `vectors_prices`,
- `vectors_ts_offsets_ms`,
- `v3_materialized_feature_snapshot.checkpoint_features.price_trajectory`.

To jest decision-time evidence i moze sluzyc pozniej do feature prototyping, ale
nie moze byc uzyte jako post-decision outcome path.

### 4.2 Dostepne, ale zbyt grube

Threshold hit artefakty zawieraja:

- `threshold_window_max_return_pct`,
- `threshold_window_min_return_pct`,
- `threshold_hit_after_entry_s`,
- `threshold_verdict`.

To wystarcza do v1 `+40 before stop`, ale nie wystarcza do Label v2
`good_clean`, bo brakuje pelnej sciezki i rozkladu drawdown/MAE.

### 4.3 Niedostepne w obecnym formacie

Brakuje:

- `price_path_samples`,
- `lifecycle_price_samples`,
- per-sample `ts_ms`,
- per-sample `price_sol`,
- per-sample `return_pct` wzgledem entry,
- enough post-entry samples for 10s/30s/60s.

## 5. Required tracked tool

Utworzyc nowy tracked skrypt:

- `scripts/v3_p37_price_path_fetcher.py`

Nie przenosic bezrefleksyjnie log-local fetchera. Mozna reuse'owac sprawdzone
idee, ale kod repo musi miec:

- normalne testy,
- resumable checkpoint,
- rate/concurrency control,
- jawne statusy fail-closed,
- output additive JSONL.

Test:

- `scripts/test_v3_p37_price_path_fetcher.py`

## 6. Input

Minimalny input per run:

- decision log,
- threshold hits v1,
- optional system log base for DIAG account updates,
- RPC URL,
- output path.

Rekomendowany CLI:

```bash
python3 scripts/v3_p37_price_path_fetcher.py \
  --decisions <gatekeeper_v2_decisions.jsonl> \
  --threshold-hits <p3_5_or_p3_6_threshold_hits.jsonl> \
  --output <p3_7_price_path_samples.jsonl> \
  --rpc "$SOLANA_RPC_URL" \
  --workers 8 \
  --max-rps 40 \
  --window-s 60 \
  --checkpoint <p3_7_price_path_samples.checkpoint.jsonl>
```

Concurrency defaults must be conservative. The goal is smooth completion, not
max throughput.

## 7. Output schema

One output row per joined decision/threshold row:

```json
{
  "price_path_schema_version": 1,
  "ab_record_id": "...",
  "join_key": "...",
  "pool_id": "...",
  "base_mint": "...",
  "entry_ts_ms": 0,
  "entry_price": 0.0,
  "entry_price_source": "threshold_hypothetical_entry",
  "entry_match_confidence": "usable_causal_match",
  "path_source": "rpc_pool_signatures|diag_account_update|mixed|unavailable",
  "path_status": "ok|partial|unavailable|rpc_error|entry_invalid|schema_error",
  "samples": [
    {
      "ts_ms": 0,
      "offset_ms": 0,
      "price_sol": 0.0,
      "return_pct": 0.0,
      "source": "rpc_tx|diag_update",
      "signature": null,
      "slot": null
    }
  ],
  "mfe_pct_10s": null,
  "mae_pct_10s": null,
  "mfe_pct_30s": null,
  "mae_pct_30s": null,
  "mfe_pct_60s": null,
  "mae_pct_60s": null,
  "time_to_mfe_ms": null,
  "time_to_mae_ms": null,
  "drawdown_before_plus40": null,
  "unknown_reason": null
}
```

## 8. Integration with Outcome Label v2

Nie przepisywac v1 labels.

Rozszerzyc `scripts/v3_p37_outcome_label_v2.py` tak, aby przyjmowal:

```bash
--price-path-samples <p3_7_price_path_samples.jsonl>
```

Regula:

- jesli price path row ma `path_status=ok` albo dopuszczalny `partial`, labeler
  moze wyliczyc clean/dirty,
- jesli path row jest unavailable/rpc_error/schema_error, row pozostaje
  `good_dirty`, `bad_dirty` albo `unknown`,
- brak path row nie moze byc wypelniony zerami.

## 9. Acceptance

P3.7 Truth-Source Acquisition jest zaliczone dopiero, gdy:

1. Dla R11 i R13 istnieja `p3_7_price_path_samples.jsonl`.
2. Artefakty maja row count zgodny z label-v2 baseline albo jawne missing rows.
3. `p3_7_label_v2` po regeneracji ma niezerowe `price_path_source != none`.
4. Temporal split ma niezerowe MFE/MAE availability w R11 i R13.
5. `good_clean` nadal wymaga usable path i nie powstaje z samego v1 threshold.
6. `good_executable` nadal wymaga execution feasibility, nie tylko market move.
7. `R10/R11/R13` historyczne artefakty pozostaja immutable; nowe artefakty sa
   addytywne.
8. Raport P3.7 evidence availability zmienia status tylko na podstawie nowych
   artefaktow path/lifecycle, nie na podstawie decision-time vectors.

## 10. Expected next implementation commits

### Commit A - tracked price path fetcher skeleton

- `scripts/v3_p37_price_path_fetcher.py`
- `scripts/test_v3_p37_price_path_fetcher.py`

Zakres:

- parsing inputow,
- join z threshold hits,
- path row schema,
- checkpoint/resume,
- unit tests bez RPC.

### Commit B - RPC/diag path collection

Zakres:

- bounded RPC scanner,
- diag account update fallback jesli system log dostepny,
- conservative rate limiter,
- per-row fail-closed statuses.

### Commit C - label-v2 integration

Zakres:

- `--price-path-samples`,
- clean/dirty classification from path,
- tests for no-zero-fill and bad path statuses.

### Commit D - R10/R11/R13 regeneration and reports

Zakres:

- wygenerowac additive path artifacts,
- zregenerowac label-v2 artefakty pod nowymi nazwami,
- zregenerowac execution feasibility join,
- zregenerowac temporal split,
- zregenerowac evidence availability.

## 11. Governance checkpoint

Po Commit D:

- jezeli nadal `good_clean=0` lub `good_executable=0`, Phase B pozostaje
  zablokowane,
- jezeli R11/R13 maja clean/executable target, dopiero wtedy mozna wykonac
  P3.7 Phase B feature prototype,
- nadal nie ma P2/live/threshold tuning.
