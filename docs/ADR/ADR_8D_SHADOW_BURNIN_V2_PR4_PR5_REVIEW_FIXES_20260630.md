# ADR-8D: Shadow Burnin V2 PR4/PR5 Review Fixes

Data: 2026-06-30

Status:

```text
IMPLEMENTED_LOCAL_PENDING_CI
```

## D1. Problem

Review GitHub dla PR #5 (`review_id=4597701109`) zablokował merge przez:

- off-by-one rounding risk w constant-product BUY/SELL quote;
- brak causal-boundary guard między `pool_state_before` i entry fill eventem;
- czerwony GitHub Actions `Restore Lifecycle Guard` na poprzednim headzie PR.

Zakres review-fixu pozostaje inercyjny: PR4/PR5 nadal nie jest podłączony do
BUY/REJECT, Gatekeeper policy, selector runtime ani TX/Jito/live path.

## D2. Decision

PR4 używa teraz bezpiecznego floor-output:

```text
BUY  = floor(token_reserves_raw * effective_sol_in / (sol_reserves_lamports + effective_sol_in))
SELL = floor(sol_reserves_lamports * token_in_raw / (token_reserves_raw + token_in_raw))
```

Zakazany jest wariant:

```text
reserve_before - floor(k / post_reserve)
```

ponieważ może zawyżyć output o jeden raw unit.

PR5 blokuje static entry fill, jeżeli `pool_state_before.event_order_key` nie
jest ściśle przed entry fill boundary. Future process sequence, equal process
sequence, future slot/order, unknown fill slot, brak observed wall-clock dla fill
eventu albo niepełny same-slot order dają `BLOCKED_BY_DATA`.

## D3. Evidence

Pliki implementacyjne:

- `ghost-core/src/shadow_v2_price.rs`
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Artefakty kontraktowe:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_BURNIN_V2_PR4_PR5_PRICE_ENTRY_FILL_20260630.md`
- `reports/selector/shadow_v2_acceptance_gates.csv`
- `reports/selector/shadow_v2_remediation_workbreakdown.csv`
- `reports/selector/shadow_v2_risk_register.csv`

Nowe testy:

- `shadow_v2_price_buy_rounding_does_not_overstate_output_by_one`
- `shadow_v2_price_sell_rounding_does_not_overstate_output_by_one`
- `shadow_v2_entry_fill_blocks_future_pool_state_by_process_sequence`
- `shadow_v2_entry_fill_blocks_same_slot_incomplete_order`

## D4. Root Cause

Pierwotny PR4 liczył output przez post-reserve floor:

```text
post_reserve = floor(k / post_input_reserve)
output = reserve_before - post_reserve
```

Dla integer AMM math ten wariant może wyemitować output większy o jeden raw unit
niż bezpieczny floor z `output_reserve * input / (input_reserve + input)`.

Pierwotny PR5 sprawdzał research-readiness i temporal class `pool_state_before`,
ale nie porównywał tego stanu z event boundary samego fillu. To pozwalało
opakować późniejszy post-entry sample jako `pool_state_before`.

## D5. Corrective Action

PR4:

- dodał helper `floor_constant_product_output`;
- przelicza BUY/SELL output bez odejmowania od `floor(k / post_reserve)`;
- zostawia deterministic post-trade reserves jako pochodną bezpiecznego output;
- ma niezależne off-by-one fixtures z konkretnymi wartościami adversarial.

PR5:

- dodał `entry_fill_causal_boundary_blockers`;
- porównuje process sequence, slot i same-slot chain-order tuple;
- nie używa same-slot incomplete order jako exact fill evidence;
- zwraca `BLOCKED_BY_DATA` zamiast syntetycznego `FILLED` dla future albo
  ambiguous state.

## D6. Rejected Alternatives

Odrzucono:

- tolerowanie różnicy jednego raw unit jako "nieistotnej";
- utrzymanie testów liczonych tą samą formułą co implementacja;
- akceptację `PostEntry` bez sprawdzenia boundary względem fill eventu;
- silent ambiguity label przy entry fill exact reconstruction;
- dotykanie runtime, BUY/REJECT, Gatekeeper policy lub TX/Jito path w ramach
  review-fixu.

## D7. Consequences

Po poprawce PR4/PR5 jest bliżej kontraktu z planu Shadow V2:

- price reconstruction ma jawny integer rounding contract;
- entry fill nie może korzystać z future pool state;
- same-slot incomplete order blokuje exact static fill;
- GitHub CI musi zostać sprawdzony ponownie po pushu nowego headu PR.

To nadal nie oznacza:

```text
SHADOW_V2_RESEARCH_GRADE
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

Shadow V2 pozostaje bez live-confirmed calibration dataset, bez rzeczywistego
landing/failure/no-fill telemetry i bez runtime approval.

## D8. Verification

Wykonane lokalnie:

```text
cargo test -q -p ghost-core shadow_v2_price
cargo test -q -p ghost-brain shadow_v2 -- --nocapture
cargo test -q -p ghost-launcher --lib restore_legacy_buy
python3 -m py_compile scripts/guard_restore_shadow_lifecycle.py scripts/test_guard_restore_shadow_lifecycle.py
python3 -m unittest scripts.test_guard_restore_shadow_lifecycle -v
python3 scripts/guard_restore_shadow_lifecycle.py --skip-runtime --output-dir /tmp/restore_guard_static --json
```

Wyniki lokalne:

```text
ghost-core shadow_v2_price: 7 passed; 0 failed
ghost-brain shadow_v2: 25 passed; 0 failed
ghost-launcher restore_legacy_buy: 2 passed; 0 failed
restore lifecycle static guard: PASS
```

GitHub Actions:

```text
previous PR head d450cae4 failed Restore Lifecycle Guard
local targeted restore command and local static guard no longer reproduce the failure
new PR head must be checked after push
```

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
