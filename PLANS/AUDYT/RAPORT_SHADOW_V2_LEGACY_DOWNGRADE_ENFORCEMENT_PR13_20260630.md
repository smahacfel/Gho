# RAPORT: Shadow V2 Legacy Downgrade Enforcement PR13

Data: 2026-06-30

Status:

```text
PR13_DOWNGRADE_ENFORCEMENT_READY_FOR_REVIEW
```

## 1. Cel

PR13 utrzymuje stare raporty i artefakty Shadow V1 jako dostępne, ale wymusza
ich poprawną interpretację. Shadow V1 nie może być cytowany jako live-equivalent
truth, executable fill proof ani unified lifecycle/replay truth.

Ten raport jest enforcement layer dla:

```text
reports/selector/shadow_v2_legacy_downgrade_matrix.csv
scripts/shadow_v2_legacy_downgrade_audit.py
```

## 2. Final Measurement Boundary

Obowiązujący verdict dla Shadow V1:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Konsekwencje:

- V1 never live-equivalent;
- V1 lifecycle is not canonical terminal truth;
- `shadow_exit_replay_v1` is mark/path evidence only;
- V1 entry price is not proven as live fill;
- V1 exit result is not executable sell fill;
- V1 cannot approve runtime, RCE, selector, active close or `shadow_close_only`;
- R51 remains ACTIVE_PARTIAL_DIAGNOSTIC_ONLY.

## 3. Wymagane Labelki

Macierz downgrade musi utrzymać co najmniej:

| Report family | Required label |
|---|---|
| ORG-A0 | `OFFLINE_PATH_LABEL_ONLY` |
| R48/R2 exit matrix | `MARK_PRICE_REPLAY_ONLY` |
| TSV2 A1/A2/A3 | `DIAGNOSTIC_ONLY` |
| EIX | `DATA_BLOCKED` |
| RTP-A0 | `DIAGNOSTIC_ONLY` |
| RUG-MARKUP-A0 | `COMPONENT_REPLAY_ONLY` |
| RCE-A0 | `BLOCKED_BY_MISSING_SURFACE` |
| R51 | `ACTIVE_PARTIAL_DIAGNOSTIC_ONLY` |
| Shadow V1 lifecycle | `LIFECYCLE_V1_NOT_CANONICAL` |
| `shadow_exit_replay_v1` | `MARK_PRICE_REPLAY_ONLY` |

## 4. Dozwolone Użycie

Stare raporty mogą być używane jako:

- offline mark/path label evidence;
- diagnostic-only evidence;
- component replay evidence with limitations;
- historyczne źródło hipotez do ponownej walidacji w Shadow V2.

## 5. Zablokowane Użycie

Previous reports must not be cited as proof of live PnL, executable fills, live
slippage behavior, real landing outcome, runtime approval, RCE approval,
selector proof, `shadow_close_only` approval or active close approval.

Zakazane jest też cytowanie pośrednie: jeżeli downstream raport bazuje na V1,
musi przenieść downgrade label i ograniczenia pomiarowe.

## 6. Upgrade Condition

Upgrade z downgrade state wymaga:

- Shadow V2 research-grade validation pass;
- entry/exit reconstruction coverage >= 99%;
- terminal reconciliation >= 99%;
- manifest completeness;
- density support for claimed horizons;
- PR14 live-confirmed calibration before any live-equivalence claim.

Bez PR14 maksymalny verdict pozostaje:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

## 7. Static Guard

Downgrade enforcement jest sprawdzany przez:

```text
python3 scripts/shadow_v2_legacy_downgrade_audit.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_legacy_downgrade_audit.py
```

Guard sprawdza:

- obecność wymaganych report families;
- wymagane labels;
- brak live-equivalent/live PnL/runtime approval wording w `allowed_use`;
- obecność dokumentacyjnych zakazów cytowania V1 jako live proof;
- brak czytania raw JSONL.

## 8. Decyzja

PR13 nie usuwa V1 i nie zmienia runtime. PR13 wymusza tylko poprawny downgrade
starych raportów i chroni przed nieuprawnionym cytowaniem V1 jako live truth.

Final PR13 status po merge:

```text
PR13_DOWNGRADE_ENFORCED_NO_V1_LIVE_EQUIVALENCE
```
