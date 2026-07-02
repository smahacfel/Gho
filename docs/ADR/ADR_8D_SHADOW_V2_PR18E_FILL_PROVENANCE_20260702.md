# ADR-8D: Shadow V2 PR18E Fill Provenance

## Status

Accepted as implementation-ready for validation.

Final verdict:

`PR30_IMPLEMENTATION_READY_FOR_VALIDATION`

## D1. Problem

PR18D zakonczyl offline reconstruction readiness werdyktem:

`BLOCKED_EXECUTABLE_FILL_PROVENANCE_MISSING`

Glowne blokery:

- `entry_reconstruction_ready_count = 0`;
- `exit_reconstruction_ready_count = 0`;
- entry/exit fills byly `BLOCKED_BY_DATA`;
- terminal truth nie mial `final_pnl_executable_bps`;
- replay/lifecycle reconciliation i manifest retention juz przechodzily.

PR18E ma domknac nastepny waski krok: jezeli runtime ma deterministyczny
`pool_state_sample_v2`, fill record musi go linkowac. Jezeli danych nie ma,
rekord ma pozostac `BLOCKED_BY_DATA`, ale z precyzyjnym typed reason zamiast
ogolnego "missing provenance".

## D2. Decyzja

Dodajemy side-by-side provenance plumbing dla Shadow V2:

- `ShadowEntryFillV2::blocked_with_pool_state`;
- `ShadowExitFillV2::blocked_with_pool_state`;
- `executable_pnl_bps_from_entry_exit_fills`;
- entry adapter potrafi zapisac canonical `pool_state_sample_v2` przed
  `shadow_entry_fill_v2`, jezeli caller dostarczy sample;
- exit lifecycle adapter probuje utworzyc `pool_state_sample_v2` z
  `AccountStateCore` dla exit-boundary;
- terminal executable PnL pozostaje `None`, dopoki entry i exit fill nie sa
  realnie `FILLED`.

Nie dodajemy fake executable fill.
Nie wpisujemy fake slippage, fee, own impact, landing telemetry ani quote/fill
divergence.

## D3. Kontekst kodowy

Zmiany sa zawarte w:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`;
- `ghost-brain/src/guardian/post_buy/engine.rs`;
- `ghost-launcher/src/components/post_buy_runtime.rs`.

Zakres jest logging/provenance-only dla Shadow V2.

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval flags;
- R51.

Nie uruchomiono runtime burnina.

## D4. Dowody implementacyjne

Entry:

- entry adapter nadal nie ma produkcyjnie kompletnego pre/post pool state z
  handoffu;
- kiedy testowy caller dostarcza `PoolStateSampleV2`, canonical stream zapisuje:
  `ENTRY_ATTEMPT -> POOL_STATE_SAMPLE -> ENTRY_FILL`;
- `shadow_entry_fill_v2.pool_state_before` wskazuje event id sample;
- fill pozostaje `BLOCKED_BY_DATA`, jezeli brakuje executable fill telemetry.

Exit:

- exit adapter pobiera dostepny `CanonicalPoolState` przez
  `current_canonical_state`;
- emitowany sample ma reserves, price, market cap, bonding progress, slot/time
  i source refs;
- runtime nie ma raw account data hash w tym miejscu, wiec sample niesie
  `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME`;
- exit fill linkuje `pool_state_before`, ale pozostaje `BLOCKED_BY_DATA`, jezeli
  executable fill telemetry jest niepelna.

Terminal:

- `final_pnl_executable_bps` moze byc wyliczony tylko z entry i exit fills o
  statusie `FILLED`;
- blocked/no-data fills nie generuja executable PnL.

## D5. Pola realnie wypelniane

Realnie wypelniane lub linkowane po PR18E:

- canonical `pool_state_sample_v2` dla exit, gdy `AccountStateCore` ma stan;
- `pool_state_before` w entry fill, gdy caller dostarczy `PoolStateSampleV2`;
- `pool_state_before` w exit fill, gdy `AccountStateCore` ma stan;
- source refs do `pool_state_sample_v2:<event_id>`;
- typed temporal/clock metadata z explicit event order;
- `exit_token_amount_raw` przeniesiony do lifecycle record;
- deterministic executable PnL helper dla przypadkow, gdzie oba fills sa
  `FILLED`.

## D6. Pola nadal typed unavailable

Entry runtime handoff nadal nie udowadnia:

- `ENTRY_POOL_STATE_BEFORE_UNAVAILABLE`, gdy caller nie dostarczy sample;
- `ENTRY_POOL_STATE_AFTER_UNAVAILABLE`;
- `FILL_PRICE_UNAVAILABLE`;
- `SLIPPAGE_BPS_UNAVAILABLE`;
- `OWN_IMPACT_BPS_UNAVAILABLE`;
- `FEE_BPS_UNAVAILABLE`;
- `LANDING_TELEMETRY_UNAVAILABLE`;
- `QUOTE_FILL_DIVERGENCE_UNAVAILABLE`.

Exit runtime nadal nie udowadnia:

- `EXIT_POOL_STATE_BEFORE_UNAVAILABLE`, gdy `AccountStateCore` nie ma stanu;
- `EXIT_POOL_STATE_AFTER_UNAVAILABLE`;
- `FILL_PRICE_UNAVAILABLE`;
- `SLIPPAGE_BPS_UNAVAILABLE`;
- `OWN_IMPACT_BPS_UNAVAILABLE`;
- `FEE_BPS_UNAVAILABLE`;
- `LANDING_TELEMETRY_UNAVAILABLE`;
- `QUOTE_FILL_DIVERGENCE_UNAVAILABLE`;
- `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME`.

Te braki sa jawnie zapisane jako limitations/blockers. Nie sa maskowane wartosciami
domyslnymi.

## D7. Runtime Boundary

PR18E nie nadaje zadnego approval:

- `research_grade = false`;
- `live_equivalence = false`;
- `runtime_approval = false`;
- `shadow_close_only_approval = false`;
- `active_close_approval = false`;
- `strategy_research_unblocked = false`.

Ten PR przygotowuje dane do kolejnego validation burnina, ale sam nie jest
wynikiem burnina ani strategy proof.

## D8. Weryfikacja i nastepny krok

Wymagane lokalne testy dla PR18E:

- `cargo test -p ghost-brain shadow_v2_entry_fill_links_pool_state_when_available -- --nocapture`;
- `cargo test -p ghost-brain shadow_v2_exit_fill_links_pool_state_when_available -- --nocapture`;
- `cargo test -p ghost-brain shadow_v2_terminal_truth_sets_executable_pnl_only_when_exit_fill_executable -- --nocapture`;
- `cargo test -p ghost-brain shadow_v2_fill_remains_blocked_when_pool_state_missing -- --nocapture`;
- `cargo test -p ghost-launcher shadow_v2_postbuy_entry_fill_uses_available_pool_state_refs -- --nocapture`;
- `cargo check -p ghost-brain`;
- `cargo check -p ghost-launcher`;
- `cargo fmt --check`;
- `git diff --check`;
- `git diff --cached --check`;
- forbidden staged-file guard.

Po merge PR30 operator moze zdecydowac o kolejnym validation burninie.
PR18E sam nie uruchamia burnina.
