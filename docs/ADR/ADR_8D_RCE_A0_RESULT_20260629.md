# ADR-8D: PR-RCE-A0 offline result

Status: RCE_BLOCKED_BY_DATA / NO_RUNTIME
Typ: ADR-8D / offline research result
Data: 2026-06-29
Zakres: PR-RCE-A0
Poziom ryzyka: LOW runtime risk / MEDIUM evidence risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Final verdict: `RCE_BLOCKED_BY_DATA`

GO_R51_LOGGING_ONLY wymaga osobnej zgody; bez niej NO_GO_CLOSE_PROJECT.

## 2. Runtime boundary

Nie zatwierdzono:

- runtime change,
- BUY/REJECT change,
- Gatekeeper policy change,
- selector runtime change,
- `shadow_close_only`,
- active close,
- TX/Jito/live path change,
- `alpha_31100`,
- XGBoost.

## 3. Evidence

| scope | has_decision_log | has_exit_replay | has_pre_entry_path_summary_v1 | has_session_regime_snapshot_v1 | full_rce_surface |
| --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | False | True | False | False | False |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | True | True | False | False | False |
| shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1 | False | False | False | False | False |

## 4. Result

Full RCE surface scope count: `0`

Passing fixed rules across two scopes: `0`

Single-scope passing rules: `0`

Best rule: ``

## 5. Files

- `scripts/rce_a0_offline_proof.py`
- `reports/selector/rce_a0_summary.csv`
- `reports/selector/rce_a0_cost_sensitivity.csv`
- `reports/selector/rce_a0_stability.csv`
- `reports/selector/rce_a0_tail_audit.csv`
- `reports/selector/rce_a0_threshold_manifest.csv`
- `PLANS/AUDYT/RAPORT_RCE_A0_OFFLINE_PROOF_20260629.md`
