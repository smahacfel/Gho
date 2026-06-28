# ADR-8D: ORG / TSV2 / EIX / RTP runtime rejection closure

Status: CLOSED / NO_RUNTIME / DIAGNOSTIC_ONLY / NEGATIVE_EVIDENCE
Typ: ADR-8D / offline research closure
Data: 2026-06-28
Zakres: ORG-A0, R48/R2 exit matrix, TSV2 A1/A2/A3, EIX, RTP-A0
Poziom ryzyka: LOW runtime risk / MEDIUM evidence risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Zamykamy linie ORG/TSV2/EIX/RTP jako brak podstaw do runtime.

Final decision:

- `R51_decision_from_this_line = NO_GO`
- `runtime_approval = false`
- `shadow_close_only_approval = false`
- `active_close_approval = false`

Nie wdrazac:

- runtime close,
- `shadow_close_only`,
- Gatekeeper BUY/REJECT change,
- selector runtime change,
- TX/Jito/live path change,
- `alpha_31100`,
- XGBoost,
- dalszego strojenia R48/R49/R50 w tej linii.

## 2. Evidence summary

| evidence block | status | runtime consequence |
| --- | --- | --- |
| ORG-A0 entry-only | Rejected for runtime | Brak stabilnego organic entry edge. |
| R48/R2 exit matrix | Negative after costs | Brak cost-positive global exit edge. |
| TSV2 A1/A2 | Damage-reduction signal, no runtime edge | Target-cut risk i ujemna absolutna ekonomia blokuja runtime. |
| TSV2 A3 | No fixed two-scope policy | `fixed_cell_passing_both_count = 0`. |
| EIX | Data-blocked | Brak R49 pre-entry rollout decision logs, wiec hipoteza nie jest numerycznie ocenialna. |
| RTP-A0 | Diagnostic-only | `passing_fixed_pair_count = 0`, `scope_pass_count = 0`. |

## 3. Root cause of closure

Ta linia nie upadla przez pojedynczy brak metryki. Zostala zamknieta przez kombinacje:

1. brak stabilnego entry edge na organic pools,
2. brak globalnego exit edge po kosztach,
3. brak stalej TSV2 fixed-cell polityki na dwoch scope,
4. nierozwiazany target-cut risk,
5. data-block EIX po utracie R49 rollout decision logs,
6. brak right-tail preservation fixed pair w RTP-A0.

## 4. Runtime boundary

Ten ADR nie zatwierdza zadnych zmian runtime.

Zakazane na podstawie tej linii:

- active close,
- `shadow_close_only`,
- BUY/REJECT change,
- Gatekeeper policy change,
- selector runtime change,
- V3 promotion change,
- `v25_confidence` change,
- TX builder/sender/Jito/live path change,
- production threshold copy.

## 5. Consequences

TSV2 pozostaje `diagnostic/logging-only`.

ORG-A0 pozostaje negative evidence.

EIX pozostaje `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`; nie wolno uznac go za pozytywny proof ani za runtime blocker oparty o wynik ekonomiczny.

RTP-A0 pozostaje `RTP_DIAGNOSTIC_ONLY / NO_RUNTIME`; brak podstaw do R51.

## 6. Next allowed research boundary

Dopuszczalna jest tylko oddzielna specyfikacja nowej hipotezy:

`PR-RUG-MARKUP-A0: offline proof for synthetic/rug/scambot markup pools`

To nie jest kontynuacja strojenia ORG/TSV2/RTP. To oddzielny offline-only research spec, bez runtime i bez startu nowego runu przed akceptacja specyfikacji.

## 7. Files

- `PLANS/AUDYT/RAPORT_CLOSURE_ORG_TSV2_RTP_NO_RUNTIME_20260628.md`
- `docs/ADR/ADR_8D_ORG_TSV2_RTP_RUNTIME_REJECTION_CLOSURE_20260628.md`
- `PLANS/AUDYT/PLAN_RUG_MARKUP_A0_OFFLINE_PROOF_20260628.md`
- `docs/ADR/ADR_8D_RUG_MARKUP_A0_OFFLINE_PROOF_20260628.md`
