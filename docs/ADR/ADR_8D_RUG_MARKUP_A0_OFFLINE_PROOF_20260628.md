# ADR-8D: PR-RUG-MARKUP-A0 offline proof specification

Status: SPEC_ONLY / OFFLINE_ONLY / NO_RUNTIME
Typ: ADR-8D / research plan
Data: 2026-06-28
Zakres: PR-RUG-MARKUP-A0
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Decyzja

Przygotowano specyfikacje offline-only dla nowej hipotezy:

`PR-RUG-MARKUP-A0: predictable synthetic/rug/scambot markup pools may be more exploitable than normal organic pools.`

Nie wykonano implementacji runtime, nie uruchomiono runu i nie zmieniono zadnego aktywnego path.

## 2. Non-goals

Ten ADR nie zatwierdza:

- runtime change,
- BUY/REJECT change,
- Gatekeeper policy change,
- selector runtime change,
- live close,
- active close,
- `shadow_close_only`,
- TX builder/sender/Jito/live path change,
- `alpha_31100`,
- XGBoost,
- nowego runu bez osobnej akceptacji specyfikacji.

## 3. Hipoteza

Normalne organic pools nie pokazaly stabilnego edge w ORG-A0. Nowa hipoteza odwraca kierunek:

Czy syntetyczne/rug/scambot markup pools maja bardziej przewidywalna faze markup, ktora da sie zidentyfikowac za pomoca decision-time-safe concentration/burst/sybil features i wykorzystac tylko offline jako proof?

## 4. Required data

Wymagane decision-time-safe input fields:

- pre-entry/dev/signer concentration,
- early buy burst features,
- holder/signature concentration,
- dev/sybil funding/behavior if available,
- unique ratio / hhi / top3 concentration,
- same-ms transaction burst,
- buy_count / total_tx / volume / buy ratios,
- signer cross-pool velocity / cpv other pool activity if available.

Evaluation-only fields:

- `shadow_exit_replay_v1.path_bps`,
- MFE / MAE,
- time-to-dump,
- target/stop/timeout,
- final PnL.

Evaluation-only fields must not become input features.

## 5. Fixed grid

- max hold: `20000`, `30000`, `40000` ms,
- target grid: `1000`, `1500`, `2000`, `2500` bps,
- stop grid: `-300`, `-500`, `-700`, `-1000` bps,
- cost: `100`, `200` bps.

No broad grid search. No holdout tuning.

## 6. Acceptance

`PROMISING_OFFLINE_ONLY` requires the same fixed classifier/grid rule to pass on at least two independent scopes:

- precision `>= 65%`,
- cost100 positive,
- cost200 positive,
- median PnL nonnegative,
- no result dependent on top 5% only,
- stability across chronological segments,
- sufficient sample size,
- clean decision-time feature surface,
- no use of future outcome as feature.

## 7. Kill criteria

Reject or mark inconclusive if:

- no clean classifier for rug-markup phase,
- positive avg only from right tail,
- median negative after cost,
- cannot distinguish from organic/noise,
- no fresh evidence logging path,
- sample too small,
- one-scope only,
- requires future labels as features,
- requires broad retuning.

## 8. Evidence retention

Any future implementation/run must include retention guard before execution:

- no active log symlinks to archive volume,
- archive volume read-only by default,
- active logs written only to explicit non-archive active path,
- immutable manifest with sha256/size/rows before cleanup,
- no cleanup without explicit scope allowlist,
- raw JSONL not committed.

## 9. Consequences

RUG-MARKUP-A0 is a separate hypothesis. It does not reopen ORG-A0, TSV2, EIX or RTP runtime paths.

Current decision:

- `runtime_approval = false`
- `shadow_close_only_approval = false`
- `active_close_approval = false`
- `new_run_approval = false`

Next action, if approved later, is implementation of an offline proof script and reports only.

## 10. Files

- `PLANS/AUDYT/PLAN_RUG_MARKUP_A0_OFFLINE_PROOF_20260628.md`
- `docs/ADR/ADR_8D_RUG_MARKUP_A0_OFFLINE_PROOF_20260628.md`
