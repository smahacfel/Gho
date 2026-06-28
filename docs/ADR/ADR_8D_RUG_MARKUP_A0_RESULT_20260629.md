# ADR-8D: PR-RUG-MARKUP-A0 offline result

Status: RUG_MARKUP_REJECTED_FOR_RUNTIME / NO_RUNTIME
Typ: ADR-8D / offline research result
Data: 2026-06-29
Zakres: PR-RUG-MARKUP-A0
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Zaimplementowano offline proof `scripts/rug_markup_a0_offline_proof.py`.

Final verdict: `RUG_MARKUP_REJECTED_FOR_RUNTIME`

NO R51. CLOSE TRADING EDGE SEARCH.

## 2. Runtime boundary

Nie wykonano i nie zatwierdzono:

- runtime change,
- Gatekeeper/BUY/REJECT change,
- selector runtime change,
- `v25_confidence` change,
- V3 promotion change,
- TX builder/sender/Jito/live path change,
- active close,
- `shadow_close_only`,
- sidecar,
- `alpha_31100`,
- XGBoost,
- nowego runu.

## 3. Evidence

Inventory jest pierwszym outputem skryptu: `reports/selector/rug_markup_a0_evidence_inventory.csv`.

Requested scopes:

| scope | has_gatekeeper_v2_decisions | has_materialized_feature_snapshot | has_shadow_exit_replay_v1 | full_evidence_for_rug_markup_a0 | blocking_reason |
| --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | False | False | True | False | missing_gatekeeper_v2_decisions_jsonl;missing_materialized_or_pre_entry_fields |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | True | True | True | True |  |

## 4. Metoda

Przetestowano tylko predeclared classifier family R0-R4 oraz fixed exit grid:

- target_bps: `1000, 1500, 2000, 2500`
- stop_bps: `-300, -500, -700, -1000`
- max_hold_ms: `20000, 30000, 40000`
- cost: `100, 200`

Nie dodano nowych progow runtime, masek, broad grid search ani R50 tuning.

## 5. Wynik

Full evidence scope count: `1`

Passing fixed rules across two scopes: `0`

Single-scope signal rows: `0`

Best diagnostic rule: `R4_MARKUP_WITH_DUMP_RISK/2500/-300/40000`

## 6. Konsekwencje

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

`new_run_approval = false`

Surowe JSONL sa lokalnym dowodem i nie powinny byc commitowane.

## 7. Files

- `scripts/rug_markup_a0_offline_proof.py`
- `reports/selector/rug_markup_a0_evidence_inventory.csv`
- `reports/selector/rug_markup_a0_summary.csv`
- `reports/selector/rug_markup_a0_cost_sensitivity.csv`
- `reports/selector/rug_markup_a0_stability.csv`
- `reports/selector/rug_markup_a0_tail_audit.csv`
- `reports/selector/rug_markup_a0_threshold_manifest.csv`
- `PLANS/AUDYT/RAPORT_RUG_MARKUP_A0_OFFLINE_PROOF_20260629.md`
