# ADR-8D: Shadow Burnin V2 PR6/PR7 Exit Fill i Path Sampler

Data: 2026-06-30

Status:

```text
PR6_PR7_IMPLEMENTED_LOCAL_PENDING_REVIEW
```

## D1. Problem

Plan `PLAN_SHADOW_BURNIN_V2_REMEDIATION_20260629.md` wymaga rozdzielenia mark/path evidence od executable fill simulation. Po PR4/PR5 entry path ma już static fill model, ale exit path nadal potrzebował:

- osobnego static sell fill modelu,
- jawnych statusów `FILLED`, `NO_FILL`, `FAILED`, `BLOCKED_BY_DATA`,
- causal-boundary guard dla `pool_state_before`,
- jawnego rozdzielenia mark exit od executable sell fill,
- density/horizon contract dla 2s/3s, 120s oraz 300s/500s.

Bez tego target/stop/timeout mógłby zostać błędnie odczytany jako live sell fill albo jako wystarczające pokrycie długiego horyzontu.

## D2. Decision

Dodajemy PR6/PR7 jako inercyjne typy, helpery i fixture tests w:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs
```

PR6:

- wprowadza `ShadowExitFillModelConfig`,
- wprowadza static SELL model `shadow_v2_exit_fill_static_constant_product_v1`,
- rekonstruuje exit fill z `pool_state_sample_v2` i formuł PR4,
- zapisuje `fill_price_source`, normalized SOL/token amounts, `min_out`, fee, slippage i own sell impact,
- blokuje future/equal-boundary pool state oraz incomplete same-slot order,
- emituje typed `NO_FILL` / `FAILED` bez ceny fill,
- nie aktywuje active close ani `shadow_close_only`.

PR7:

- wprowadza `ShadowPathSamplingModeV2`,
- definiuje tryby `shadow_path_dense_3s`, `shadow_path_standard_120s`, `shadow_path_long_500s`,
- wprowadza `ShadowPathSamplingReasonV2`,
- dodaje horizon verdicts `EVALUABLE_EXACT`, `EVALUABLE_APPROX`, `SPARSE_APPROX_ONLY`, `NOT_EVALUABLE_NO_COVERAGE`, `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY`,
- pozwala oddzielić mark PnL od static executable quote PnL.

## D3. Evidence

Zmienione artefakty kontraktowe:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`
- `reports/selector/shadow_v2_remediation_workbreakdown.csv`
- `reports/selector/shadow_v2_risk_register.csv`
- `docs/ADR/ADR_8D_SHADOW_BURNIN_V2_PR6_PR7_EXIT_PATH_20260630.md`

Fixture evidence w Rust:

- `shadow_v2_exit_fill_static_model_reconstructs_sell_fill_from_pool_state`
- `shadow_v2_exit_fill_blocks_future_pool_state_and_same_slot_ambiguity`
- `shadow_v2_exit_fill_can_emit_explicit_no_fill_or_failure_without_price_claim`
- `shadow_v2_exit_attempt_requires_tie_break_for_same_slot_ambiguity`
- `shadow_v2_path_sample_reconstructs_mark_pnl_and_attaches_static_exit_quote`
- `shadow_v2_path_density_supports_dense_2s_3s_and_blocks_unsupported_long_horizons`
- `shadow_v2_path_density_marks_sparse_and_no_coverage_explicitly`
- `shadow_v2_path_sampler_modes_define_sampling_policy`

## D4. Root Cause

Shadow V1 mieszał offline path labels z lifecycle truth. Remediation plan wymaga, żeby V2 nie traktował target/stop hit jako sell fill. Dodatkowo poprzednie dane nie wspierały 300s/500s bez jawnego replay horizon i density evidence.

## D5. Corrective Action

W PR6/PR7:

- static exit fill używa `quote_constant_product(..., ShadowV2QuoteSide::Sell, ...)`,
- fill może być `FILLED` tylko przy causal-safe `pool_state_before`,
- brak danych lub niejednoznaczność same-slot blokuje exact fill reconstruction,
- modeled `NO_FILL` i `FAILED` nie dostają `fill_price`,
- mark path sample pozostaje `MARK_PRICE_REPLAY`,
- static executable quote zmienia sample na `FILL_MODEL_STATIC`, ale nadal nie jest live fill,
- każdy horizon dostaje jawny verdict i limitations,
- unsupported horizon nie jest inferowany.

## D6. Rejected Alternatives

Odrzucono:

- traktowanie target/stop/timeout jako executable sell fill,
- reuse entry-fill modelu bez osobnych exit blockers,
- akceptowanie future pool state względem exit fill boundary,
- silent same-slot target/stop resolution,
- traktowanie sparse path jako exact 2s/3s proof,
- inferowanie 300s/500s bez long-mode coverage,
- podpinanie PR6 do active close.

## D7. Consequences

PR6/PR7 poprawiają kontrakt simulatora, ale nie kończą Shadow V2 jako research-grade.

Nadal wymagane są:

- validation burnin PR12,
- evidence manifests,
- density reports per horizon,
- exit reconstruction coverage >= 99%,
- replay/lifecycle V2 derived reconciliation,
- PR14 live-confirmed calibration dataset dla live-equivalence.

Granice pozostają:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
SHADOW_V2_LIVE_EQUIVALENCE_GRADE=false
```

## D8. Verification

Lokalne testy przed finalnym commitem:

```text
cargo test -q -p ghost-brain shadow_v2_exit -- --nocapture
result: ok; 4 passed; 0 failed

cargo test -q -p ghost-brain shadow_v2_path -- --nocapture
result: ok; 4 passed; 0 failed
```

Do finalnej walidacji PR branch wymagane są jeszcze:

```text
cargo fmt --check
cargo test -q -p ghost-brain shadow_v2 -- --nocapture
cargo test -q -p ghost-core shadow_v2_price
git diff --check
git diff --cached --name-only
forbidden staged-file guard
```

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
NO_RUN_STARTED
NO_R51_TOUCH
```
