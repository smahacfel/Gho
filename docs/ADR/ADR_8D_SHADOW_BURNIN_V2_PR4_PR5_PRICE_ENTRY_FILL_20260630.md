# ADR-8D: Shadow Burnin V2 Price Reconstruction i Entry Fill Static Model

Data: 2026-06-30

Status:

```text
IMPLEMENTED_LOCAL_PENDING_REVIEW
```

## D1. Problem

P0 Shadow Burnin Fidelity Audit zwrócił:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Po PR2/PR3 Shadow V2 ma już kanoniczny event stream oraz `pool_state_sample_v2`
z provenance, ale nadal brakowało dwóch elementów wymaganych przez plan
remediacji:

- niezależnej, deterministycznej biblioteki rekonstrukcji ceny;
- modelu wejścia, który oddziela `decision_mark_price`, `entry_quote_price`
  i `entry_fill_price`.

Bez tego entry price może być dalej mylona z sampled mark price albo
nieudowodnionym live fill.

## D2. Decision

Wprowadzono PR4 jako inercyjny moduł:

```text
ghost-core/src/shadow_v2_price.rs
```

Moduł definiuje:

- `ShadowV2Reserves`,
- `ShadowV2PoolPhase`,
- `ShadowV2QuoteSide`,
- `ShadowV2Quote`,
- `ShadowV2PriceError`,
- `mark_price_sol_per_token`,
- `quote_constant_product`,
- `apply_slippage_bps_floor`.

Formuła:

```text
shadow_v2_constant_product_price_v1
```

PR4 liczy jawnie:

- mark price z rezerw, token decimals i lamports-per-SOL;
- BUY quote;
- SELL quote;
- fee bps i fee lamports;
- configured slippage tolerance i `min_out`;
- own impact bps oddzielony od fee;
- deterministyczne post-trade reserves.

Review-fix dla `review_id=4597701109` domyka rounding contract:

- BUY output jest liczony jako
  `floor(token_reserves_raw * effective_sol_in / (sol_reserves_lamports + effective_sol_in))`;
- SELL output jest liczony jako
  `floor(sol_reserves_lamports * token_in_raw / (token_reserves_raw + token_in_raw))`;
- wariant `reserve_before - floor(k / post_reserve)` jest zakazany dla output,
  bo może zawyżyć wynik o jeden raw unit;
- fixtures sprawdzają adversarial off-by-one dla BUY i SELL niezależnie od
  implementacji.

Wprowadzono PR5 jako inercyjny static entry fill model w:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs
```

Model:

```text
shadow_v2_entry_fill_static_constant_product_v1
```

PR5 dodaje:

- `ShadowEntryFillModelConfig`,
- `ShadowEntryFillV2::from_static_buy_model`,
- `ShadowEntryAttemptV2::attach_static_entry_quote`,
- addytywne pola `ShadowEntryAttemptV2`:
  - `decision_mark_price`,
  - `entry_quote_price`,
  - `entry_quote_tokens_out`,
  - `entry_quote_min_out`.

Static entry fill może zwrócić `FILLED` tylko wtedy, gdy `pool_state_sample_v2`
jest research-ready, temporal class jest dozwolona dla entry causal boundary,
`pool_state_sample_v2.event_order_key` jest ściśle przed fill boundary, reserve
provenance istnieje, normalizacja decimals/lamports jest jawna, a quote
rekonstrukcja przechodzi. Future pool state, równy process sequence, unknown fill
slot, brak observed wall-clock dla fill eventu albo niepełny same-slot order
blokują `FILLED` i dają `BLOCKED_BY_DATA`.

W przeciwnym razie zwraca:

```text
fill_status = BLOCKED_BY_DATA
reconstruction_status = ENTRY_FILL_BLOCKED_BY_DATA
```

z jawnymi blockerami w `limitations`.

## D3. Evidence

Pliki implementacyjne:

- `ghost-core/src/shadow_v2_price.rs`
- `ghost-core/src/lib.rs`
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Artefakty kontraktowe:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `reports/selector/shadow_v2_remediation_workbreakdown.csv`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`
- `reports/selector/shadow_v2_risk_register.csv`

Testy PR4:

- `shadow_v2_price_mark_price_normalizes_reserves_and_decimals`
- `shadow_v2_price_buy_quote_applies_fee_impact_and_min_out`
- `shadow_v2_price_sell_quote_applies_output_fee_and_min_out`
- `shadow_v2_price_buy_rounding_does_not_overstate_output_by_one`
- `shadow_v2_price_sell_rounding_does_not_overstate_output_by_one`
- `shadow_v2_price_amm_quote_uses_real_reserve_formula_label`
- `shadow_v2_price_rejects_invalid_inputs`

Testy PR5:

- `shadow_v2_entry_fill_static_model_reconstructs_buy_fill_from_pool_state`
- `shadow_v2_entry_fill_blocks_missing_reserves_hash_and_bad_temporal_class`
- `shadow_v2_entry_fill_blocks_future_pool_state_by_process_sequence`
- `shadow_v2_entry_fill_blocks_same_slot_incomplete_order`
- `shadow_v2_entry_attempt_keeps_decision_mark_quote_and_min_out_separate`

## D4. Root Cause

Shadow V1 mieszał mark/path evidence z fill-like interpretacją. W praktyce:

- `entry_price` nie miała wystarczającego rozdzielenia na mark, quote i fill;
- slippage, own impact i fee nie były reprezentowane jako jawny kontrakt;
- brakowało mechanizmu `BLOCKED_BY_DATA`, gdy reserve/source evidence nie pozwala
  na rekonstrukcję;
- brakowało centralnej biblioteki formuł niezależnej od runtime writerów i
  lifecycle/replay V1.

## D5. Corrective Action

PR4 ustanawia deterministic formula boundary:

```text
input reserves + decimals + lamports + side + amount + fee_bps + slippage_bps
-> deterministic quote or typed error
```

PR4 blokuje ciche ceny dla:

- zero reserves;
- zero input;
- invalid fee/slippage bps;
- missing SOL normalization;
- unsupported token decimals;
- zero-output quote.

PR5 ustanawia entry fill boundary:

```text
pool_state_sample_v2 + explicit fill config -> FILLED or BLOCKED_BY_DATA
```

`FILLED` oznacza tylko static executable-fill simulation candidate, nie live fill.
`pool_state_sample_v2` musi być przyczynowo wcześniejszy niż entry fill event.
Stan z przyszłości albo same-slot order bez wystarczających komponentów
chain-order nie może zostać użyty jako exact fill evidence.
Rekord `FILLED` zapisuje:

- `fill_price`;
- `fill_price_source`;
- `fill_amount_sol`;
- `fill_amount_tokens`;
- `slippage_bps`;
- `own_impact_bps`;
- `fee_bps`;
- `min_out`;
- `pool_state_before`;
- deterministic derived `pool_state_after`;
- limitations wykluczające live-equivalence.

`BLOCKED_BY_DATA` zapisuje jawne przyczyny, zamiast syntetycznie wytwarzać fill.

## D6. Rejected Alternatives

Odrzucono:

- użycie istniejącego V1 `entry_price` jako fill price;
- podpięcie modelu PR5 do BUY/REJECT albo live TX path;
- domyślne zero slippage, zero fee albo zero own impact;
- ukryte założenie, że `pool_state_after` jest obserwowanym account state;
- promowanie static fill modelu do live-equivalent bez PR14;
- fallback na `ShadowLedgerDiagnostic` jako live truth.

## D7. Consequences

Po PR4/PR5 Shadow V2 ma lokalną podstawę do przyszłej rekonstrukcji entry:

- mark price jest liczony z rezerw i normalizacji;
- quote/fill price jest odseparowany od mark price;
- fee, slippage tolerance i own impact są jawne;
- brak danych daje `BLOCKED_BY_DATA`.

To nadal nie oznacza:

```text
SHADOW_V2_RESEARCH_GRADE
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

Do `SHADOW_V2_RESEARCH_GRADE` nadal potrzebny jest przyszły fidelity validation
run z coverage/reconciliation/density/manifest gates. Do live-equivalence nadal
wymagany jest PR14 live-confirmed calibration dataset.

Granice runtime:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
live_equivalence_claim=false
```

## D8. Verification

Wykonane komendy:

```text
cargo test -q -p ghost-core shadow_v2_price
cargo test -q -p ghost-brain shadow_v2 -- --nocapture
cargo test -q -p ghost-launcher --lib restore_legacy_buy
python3 -m py_compile scripts/guard_restore_shadow_lifecycle.py scripts/test_guard_restore_shadow_lifecycle.py
python3 -m unittest scripts.test_guard_restore_shadow_lifecycle -v
python3 scripts/guard_restore_shadow_lifecycle.py --skip-runtime --output-dir /tmp/restore_guard_static --json
cargo fmt --check
git diff --check
git diff --cached --check
```

Wyniki:

```text
ghost-core shadow_v2_price: 7 passed; 0 failed
ghost-brain shadow_v2: 25 passed; 0 failed
ghost-launcher restore_legacy_buy: 2 passed; 0 failed
guard_restore_shadow_lifecycle.py --skip-runtime: PASS
cargo fmt --check: OK
git diff --check: OK
git diff --cached --check: OK
```

Uwaga:

Repo emituje wiele istniejących warningów z legacy/deprecated/unused paths.
Nie są one wprowadzone przez PR4/PR5 i nie zmieniają wyniku targeted tests.

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_CHANGE
NO_ACTIVE_CLOSE_CHANGE
NO_RUN_STARTED
NO_R51_TOUCH
```
