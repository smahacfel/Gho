# RAPORT SHADOW V2 PR18E FILL PROVENANCE 20260702

## 1. Werdykt wykonawczy

Finalny werdykt PR30 / PR18E:

`PR30_IMPLEMENTATION_READY_FOR_VALIDATION`

PR18E implementuje canonical pool-state/fill provenance plumbing dla Shadow V2.
Nie jest to report-only PR i nie uruchamia kolejnego runtime burnina.

Najwazniejsza zmiana: Shadow V2 fill records potrafia teraz linkowac
`pool_state_sample_v2`, jezeli taki sample jest realnie dostepny. Jezeli
danych nie ma, fill pozostaje `BLOCKED_BY_DATA` z typed reasons.

## 2. Zakres

Pliki kodowe:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`;
- `ghost-brain/src/guardian/post_buy/engine.rs`;
- `ghost-launcher/src/components/post_buy_runtime.rs`.

Dokumentacja:

- `docs/ADR/ADR_8D_SHADOW_V2_PR18E_FILL_PROVENANCE_20260702.md`;
- `PLANS/AUDYT/RAPORT_SHADOW_V2_PR18E_FILL_PROVENANCE_20260702.md`.

## 3. Co zostalo dodane

### 3.1 Entry fill provenance

Dodano `ShadowEntryFillV2::blocked_with_pool_state`.

Entry adapter ma teraz wewnetrzna sciezke, ktora moze zapisac canonical:

1. `shadow_entry_attempt_v2`;
2. `pool_state_sample_v2`;
3. `shadow_entry_fill_v2`.

Produkcyjny handoff nadal przekazuje `None`, bo obecny runtime handoff nie ma
deterministycznego entry pool-state before/after. To jest swiadomy fail-closed,
nie fake fill.

### 3.2 Exit fill provenance

Dodano `ShadowExitFillV2::blocked_with_pool_state`.

`MonitoringEngine` probuje zbudowac exit-boundary `pool_state_sample_v2` z
`AccountStateCore` przez `current_canonical_state`. Jezeli stan jest dostepny,
canonical stream zapisuje `POOL_STATE_SAMPLE` przed `EXIT_FILL`, a exit fill
linkuje `pool_state_before`.

Poniewaz `CanonicalPoolState` nie niesie raw account data hash w tym miejscu,
sample ma jawne limitation:

`POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME`

i nie zostaje potraktowany jako pelny executable fill proof.

### 3.3 Terminal executable PnL

Dodano helper:

`executable_pnl_bps_from_entry_exit_fills`

Helper zwraca wartosc tylko, gdy entry i exit fill maja status `FILLED` i
kompletne kwoty SOL. Dla `BLOCKED_BY_DATA` wynik pozostaje `None`.

## 4. Pola realnie wypelniane po PR18E

Realnie wypelniane lub linkowane:

- `pool_state_sample_v2` dla exit, jezeli `AccountStateCore` ma canonical state;
- `shadow_entry_fill_v2.pool_state_before`, jezeli caller dostarczy sample;
- `shadow_exit_fill_v2.pool_state_before`, jezeli exit sample zostal utworzony;
- source refs `pool_state_sample_v2:<event_id>`;
- event order key z explicit `UNKNOWN` tam, gdzie chain order nie jest znany;
- `exit_token_amount_raw` w lifecycle record;
- deterministic executable PnL helper dla przypadkow z dwoma `FILLED` fills.

## 5. Pola nadal typed unavailable

Entry runtime nadal moze emitowac:

- `ENTRY_POOL_STATE_BEFORE_UNAVAILABLE`;
- `ENTRY_POOL_STATE_AFTER_UNAVAILABLE`;
- `FILL_PRICE_UNAVAILABLE`;
- `SLIPPAGE_BPS_UNAVAILABLE`;
- `OWN_IMPACT_BPS_UNAVAILABLE`;
- `FEE_BPS_UNAVAILABLE`;
- `LANDING_TELEMETRY_UNAVAILABLE`;
- `QUOTE_FILL_DIVERGENCE_UNAVAILABLE`.

Exit runtime nadal moze emitowac:

- `EXIT_POOL_STATE_BEFORE_UNAVAILABLE`;
- `EXIT_POOL_STATE_AFTER_UNAVAILABLE`;
- `FILL_PRICE_UNAVAILABLE`;
- `SLIPPAGE_BPS_UNAVAILABLE`;
- `OWN_IMPACT_BPS_UNAVAILABLE`;
- `FEE_BPS_UNAVAILABLE`;
- `LANDING_TELEMETRY_UNAVAILABLE`;
- `QUOTE_FILL_DIVERGENCE_UNAVAILABLE`;
- `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME`.

Te pola pozostaja niedostepne, jezeli runtime nie ma deterministycznego zrodla
danych. PR18E nie wpisuje wartosci domyslnych i nie udaje executable fills.

## 6. Inwarianty zachowane

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval;
- research-grade;
- live-equivalence;
- R51.

Nie uruchomiono runtime burnina.
Nie commitowano raw JSONL, logow, runtime scope ani lokalnych configow.

## 7. Testy i walidacja

Wymagane testy PR18E:

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

## 8. Co PR18E odblokowuje

PR18E nie nadaje research-grade.
PR18E nie nadaje live-equivalence.
PR18E nie odblokowuje strategii.

Po merge operator moze uruchomic kolejny validation/fidelity burnin i sprawdzic,
czy:

- canonical `pool_state_sample_v2` pojawia sie dla realnych exit fills;
- entry handoff nadal wymaga dodatkowego zrodla pool-state before/after;
- offline PR18D audits pokazuja poprawiona provenance coverage;
- replay/lifecycle reconciliation pozostaje `PASS`.
