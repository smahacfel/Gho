Zaktualizowany Plan: Restore LegacyBuy Bez Rozszerzania Telemetry Surface

  ## Code-Grounded Corrections

  Sprawdziłem korekty względem aktualnego kodu i mają uzasadnienie:

  - creator_pubkey jest realnie builder-required w obecnym runtime: TriggerComponent::validate_creator_pubkey_for_buy() failuje przy
    braku canonical creator pubkey, a legacy dodatkowo odrzuca creator_pubkey_authoritative == Some(false) (ghost-launcher/src/
    components/trigger/component.rs:2576).

  - DirectBuyBuilder::build_buy_ix_with_accounts_and_remaining() dla legacy używa fixed accounts 0-15 i dokleja
    buy_remaining_accounts jako trailing accounts; bonding_curve_v2 jest dodawany tylko dla routed path (off-chain/components/
    trigger/src/direct_buy_builder.rs:337).

  - Seer parser ma dokładnie restore contract: dla legacy_buy kopiuje accounts po PUMP_BUY_FIXED_ACCOUNT_COUNT, a test pilnuje, że
    legacy index 16 nie jest traktowany jako bonding_curve_v2 (off-chain/components/seer/src/binary_parser.rs:6210, off-chain/
    components/seer/src/binary_parser.rs:10518).

  - Regresja jest jawna: p37_execution_account_contract_failure_tail() przy LegacyBuy i count=2 nadal zwraca
    unsupported_legacy_buy_layout_requires_bcv2 (ghost-launcher/src/oracle_runtime.rs:19366).

  - Fallback resolver ma osobną regresyjną gałąź, która przy legacy_buy.route_ready == true nadal wymusza
    legacy_buy_fallback_unsupported_builder_layout (ghost-launcher/src/oracle_runtime.rs:8376).

  - Fallback overrides obecnie klonują execution_account_contract_status/reason z primary, co może przenieść stale unsupported/
    incomplete status na poprawny legacy fallback (ghost-launcher/src/oracle_runtime.rs:17342).

  ## Implementation Changes

  1. derive_buy_account_overrides()

  - Dodać mapping tx.buy_variant == "legacy_buy" -> PumpfunBuyVariant::LegacyBuy, ale tylko dla observed/complete legacy account
    shape:
      - non-telemetry: !tx.is_nln_program_stream_trade_telemetry_only(),
      - exact tail: tx.buy_remaining_accounts.len() == trigger::PUMPFUN_BUYBACK_REMAINING_ACCOUNT_COUNT,
      - event source ma realne observed route/account evidence: fixed-account fields albo observed trailing accounts, nie feature-
        only/telemetry-only,

      - account handoff zawiera pola wymagane przez aktualny TriggerComponent/DirectBuyBuilder, nie arbitralną listę.

  - Builder-required minimum dla current code:
      - global_config canonical,
      - authorized fee_recipient,
      - token_program,
      - associated_bonding_curve,
      - creator_pubkey,
      - creator_pubkey_authoritative != Some(false) dla LegacyBuy,
      - preserved buy_remaining_accounts exactly in original order.

  - Nie wymagać bonding_curve_v2 dla LegacyBuy.
  - Nie mapować telemetry-only, nln_pumpfun_buy, incomplete legacy_buy, feature-only rows ani unknown labels na executable variant.

  2. p37_execution_account_contract_failure_tail()

  - Dla LegacyBuy usunąć false terminal:
      - complete legacy restore contract -> None,
      - unsupported_legacy_buy_layout_requires_bcv2 nie może wystąpić dla LegacyBuy z observed tail count 2 i builder-required
        handoff.

  - Fail-closed cases zostają:
      - telemetry-only -> telemetry_only_trade_event:route_account_manifest_incomplete,
      - buy_remaining_accounts.len() != 2 -> legacy_buy_missing_buyback_remaining_accounts:count=<n>:expected=2,
      - missing builder-required field -> route_account_manifest_incomplete:<missing_role>,
      - non-authoritative legacy creator -> route/account incomplete reason, zgodny z trigger validation,
      - routed missing/non-route-compatible BCV2 -> primary_route_bcv2_missing albo existing missing-BCV2 incomplete reason.

  - mark_buy_account_overrides_route_contract() ma oznaczać complete legacy jako:
      - execution_account_contract_status = "complete",
      - execution_account_contract_reason = "route_account_contract_complete".

  3. Fallback route resolver

  - W p37_shadow_probe_route_resolution_diagnostics_with_mode() rozdzielić unsupported legacy od validated legacy:
      - jeśli primary routed nie ma executable BCV2,
      - i legacy fallback ma observed complete legacy account contract,
      - i legacy_buy_account_set_status == "ready",
      - i legacy_buy_curve_rpc_load_ready == Some(true),
      - i p37_execution_account_contract_failure_tail(fallback_overrides) == None,
      - wtedy zwrócić legalny fallback:
          - route_resolution_status = "fallback_route_ready",
          - selected_route_kind = "legacy_buy",
          - primary_route_ready = false,
          - fallback_route_kind = "legacy_buy",
          - fallback_route_ready = true,
          - no_executable_route_account_set_reason = None.

  - Jeżeli fallback legacy nie przejdzie contract tail, zachować obecny fail-closed path.
  - W p37_selected_legacy_buy_fallback_overrides() przeliczać contract status dla fallback i nie przenosić stale
    unsupported_legacy_buy_layout/route_account_manifest_incomplete z primary.

  4. No Scope Creep

  - Nie dotykać Gatekeeper thresholds, FSC, R2, P3D, samplerów, Helius/live/P2 ani NLN program streams.
  - Nie zmieniać publicznej schemy eventów ani configów.
  - Nie rozszerzać executable surface na telemetry-only lub feature-only rows.

  ## Tests

  Dodać/zaktualizować testy w ghost-launcher:

  - restore_legacy_buy_with_observed_remaining_accounts_is_executable
      - asercje: LegacyBuy mapped, contract complete, failure tail None, no unsupported_legacy_buy_layout_requires_bcv2, no
        route_account_manifest_incomplete, buy_remaining_accounts preserved exactly in order.

  - legacy_buy_missing_remaining_accounts_is_not_executable
      - asercje: not_executable_route, reason legacy_buy_missing_buyback_remaining_accounts:count=0:expected=2, no dispatch/
        simulation unlock.

  - telemetry_only_pool_transaction_cannot_unlock_legacy_buy
      - asercje: telemetry-only remains blocked, no LegacyBuy executable mapping, dispatch_attempted = false.

  - routed_exact_sol_in_missing_bcv2_still_not_executable
      - asercje: routed missing BCV2 remains no_executable_route_account_set, primary_route_bcv2_missing, no fallback unless
        validated legacy contract exists.

  - Fallback positive test
      - asercje: primary_route_ready = false, fallback_route_kind = "legacy_buy", fallback_route_ready = true, selected_route_kind =
        "legacy_buy", no_executable_route_account_set_reason = None.

  - Zachować bez usuwania:
      - p5_precheck_failure_writes_not_dispatched_lifecycle_record.

  Zaktualizować testy, które obecnie kodują regresję, zwłaszcza:

  - derive_buy_account_overrides_drops_legacy_buy_variant,
  - p37_route_resolver_primary_bcv2_missing_rejects_legacy_fallback,
  - p37_route_resolver_primary_bcv2_manifest_missing_rejects_legacy_fallback_without_precheck_reason,
  - legacy_contract_with_buyback_tail_is_still_unsupported_for_shadow_dispatch.

  ## Verification And Smoke

  Targeted tests:

  cargo test -p ghost-launcher --bin ghost-launcher restore_legacy_buy_with_observed_remaining_accounts_is_executable -- --exact
  cargo test -p ghost-launcher --bin ghost-launcher legacy_buy_missing_remaining_accounts_is_not_executable -- --exact
  cargo test -p ghost-launcher --bin ghost-launcher telemetry_only_pool_transaction_cannot_unlock_legacy_buy -- --exact
  cargo test -p ghost-launcher --bin ghost-launcher routed_exact_sol_in_missing_bcv2_still_not_executable -- --exact
  cargo test -p ghost-launcher --bin ghost-launcher p5_precheck_failure_writes_not_dispatched_lifecycle_record -- --exact

  Restore-only smoke:

  cargo run -p ghost-launcher --bin ghost-launcher -- \
    --config configs/rollout/shadow-burnin.toml \
    --preflight

  timeout 600s cargo run -p ghost-launcher --bin ghost-launcher -- \
    --config configs/rollout/shadow-burnin.toml

  Reporter:

  python3 scripts/shadow_onchain_lifecycle_report.py \
    --config configs/rollout/shadow-burnin.toml \
    --output /tmp/current_restore_shadow_lifecycle_report_after_fix.jsonl \
    --outcome-summary-output /tmp/current_restore_raportneu_after_fix.json

  Smoke status levels:

  - RESTORE PATH REPAIRED: rows_written > 0, resolved truth, final_pnl_pct present, close reason in {Target, StopLoss, TimeStop}.
  - ROUTE REGRESSION FIXED, LIFECYCLE CLOSE NOT OBSERVED: legacy executable, dispatch_attempted=true, simulation_attempted=true,
    unsupported_legacy_buy_layout_requires_bcv2=0, shadow artifacts present, but reporter rows remain 0 inside 600s window.

  - FIX FAILED: validated legacy still ends as not_executable_route or unsupported_legacy_buy_layout_requires_bcv2.

  Commit gate remains strict per dyspozycja: commit only after runtime proof. If result is level B rather than A, report it
  explicitly before committing decision.

  ## Report And Commit Policy

  Create after smoke:

  - reports/selector/restore_lifecycle_regression_fix_<UTC_TS>/RESTORE_LIFECYCLE_REGRESSION_FIX.md
  - reports/selector/restore_lifecycle_regression_fix_<UTC_TS>/restore_lifecycle_regression_fix_v1.json

  Required report section: legacy_contract_matrix

  Fields:

  - legacy_buy_rows
  - legacy_buy_complete_rows
  - legacy_buy_executable_rows
  - legacy_buy_blocked_rows
  - unsupported_legacy_buy_layout_requires_bcv2_rows
  - legacy_buy_missing_buyback_remaining_accounts_rows
  - fallback_route_ready_rows
  - dispatch_attempted_rows
  - simulation_attempted_rows
  - reporter_rows_written

  Allowlist for code commit:

  - ghost-launcher/src/oracle_runtime.rs
  - ghost-launcher/src/components/trigger/component.rs only if current builder validation/test fixture requires it
  - ghost-launcher/src/components/trigger/shadow_run.rs only if preserving lifecycle test requires it
  - ghost-launcher/src/events.rs only if fixture/helper requires it

  Do not stage logs, datasets, runtime artifacts, P3/R2 scripts, Gatekeeper/FSC configs, NLN streams, feature events, or reports
  unless separately instructed.

  Commit message:

  PR-RESTORE: restore executable legacy buy path after route contract gating


PODSUMOWUJĄC:

Zakres: wyłącznie restore LegacyBuy execution contract Commit: dopiero po runtime proof Nie dotykać: Gatekeeper, FSC, R2, P3D, sampler, NLN, live/P2 

To ma być chirurgiczny fix w OracleRuntime, nie kolejna przebudowa.

Co musi zostać zachowane

Najważniejsza granica:

complete observed legacy_buy: może być executable telemetry-only / feature-only / incomplete route: nadal fail-closed 

Nie wolno naprawić restore path kosztem ponownego odblokowania śmieciowych route’ów. 62daa00 miał sens, bo zatrzymał fałszywe executable=true -> simulation -> AccountNotFound dla incomplete contracts. Teraz trzeba tylko wyjąć z tego zbyt szerokiego fail-closed gate’a validated LegacyBuy.

Implementacja — zaakceptowany zakres

1. derive_buy_account_overrides()

Tak, dodać:

buy_variant == "legacy_buy" -> PumpfunBuyVariant::LegacyBuy 

ale tylko przy kompletnym observed legacy shape:

!tx.is_nln_program_stream_trade_telemetry_only() buy_remaining_accounts.len() == PUMPFUN_BUYBACK_REMAINING_ACCOUNT_COUNT global_config present fee_recipient present token_program present associated_bonding_curve present creator_pubkey present creator_pubkey_authoritative != Some(false) buy_remaining_accounts preserved exactly in original order 

Nie wymagać bonding_curve_v2 dla LegacyBuy.

Nie mapować:

nln_pumpfun_buy nln_pumpfun_sell feature-only PoolTransaction telemetry-only PoolTransaction unknown buy_variant legacy_buy bez tail count=2 

2. p37_execution_account_contract_failure_tail()

Tak, poprawić gałąź LegacyBuy.

Dla kompletnego restore contract:

return None 

Nie może wystąpić:

unsupported_legacy_buy_layout_requires_bcv2 

Dla incomplete legacy nadal fail-closed:

legacy_buy_missing_buyback_remaining_accounts:count=<n>:expected=2 route_account_manifest_incomplete:<missing_role> telemetry_only_trade_event:route_account_manifest_incomplete creator_pubkey_not_authoritative / equivalent trigger-compatible reason 

Dla routed path nic nie luzować:

missing BCV2 => primary_route_bcv2_missing / no_executable_route_account_set 

3. Fallback route resolver

Tak, to jest drugi krytyczny punkt.

Jeżeli:

primary routed path missing/non-executable BCV2 legacy fallback route_ready == true legacy account set ready legacy curve rpc/load ready fallback overrides complete p37_execution_account_contract_failure_tail(fallback_overrides) == None 

to ma być:

route_resolution_status = fallback_route_ready selected_route_kind = legacy_buy primary_route_ready = false fallback_route_kind = legacy_buy fallback_route_ready = true no_executable_route_account_set_reason = None 

Nie wolno przenosić stale unsupported_legacy_buy_layout ani route_account_manifest_incomplete z primary routed overrides na poprawny legacy fallback.

4. Contract status

Dla kompletnego legacy:

execution_account_contract_status = complete execution_account_contract_reason = route_account_contract_complete 

Dla telemetry/incomplete zostaje obecne fail-closed.

Testy — zatwierdzone

Te testy są właściwe i muszą wejść:

restore_legacy_buy_with_observed_remaining_accounts_is_executable legacy_buy_missing_remaining_accounts_is_not_executable telemetry_only_pool_transaction_cannot_unlock_legacy_buy routed_exact_sol_in_missing_bcv2_still_not_executable fallback positive test dla validated legacy fallback p5_precheck_failure_writes_not_dispatched_lifecycle_record 

Szczególnie ważne asercje:

no unsupported_legacy_buy_layout_requires_bcv2 dla complete LegacyBuy no route_account_manifest_incomplete dla complete LegacyBuy buy_remaining_accounts preserved exactly in order fallback_route_ready true tylko dla complete LegacyBuy telemetry-only nadal dispatch_attempted=false 

Testy, które kodują regresję, trzeba zaktualizować, nie obchodzić:

derive_buy_account_overrides_drops_legacy_buy_variant p37_route_resolver_primary_bcv2_missing_rejects_legacy_fallback p37_route_resolver_primary_bcv2_manifest_missing_rejects_legacy_fallback_without_precheck_reason legacy_contract_with_buyback_tail_is_still_unsupported_for_shadow_dispatch 

Smoke gate

Smoke tylko na:

configs/rollout/shadow-burnin.toml 

Nie sampler. Nie NLN. Nie P3D. Nie R2-only.

Kolejność:

cargo run -p ghost-launcher --bin ghost-launcher -- \ --config configs/rollout/shadow-burnin.toml \ --preflight 

Potem:

timeout 600s cargo run -p ghost-launcher --bin ghost-launcher -- \ --config configs/rollout/shadow-burnin.toml 

Reporter:

python3 scripts/shadow_onchain_lifecycle_report.py \ --config configs/rollout/shadow-burnin.toml \ --output /tmp/current_restore_shadow_lifecycle_report_after_fix.jsonl \ --outcome-summary-output /tmp/current_restore_raportneu_after_fix.json 

Commit gate

Tu przyjmuję Twój strict gate:

Commit dopiero po runtime proof. 

Najlepszy wynik:

RESTORE PATH REPAIRED: rows_written > 0 close_truth_coverage > 0 truth_status = resolved gatekeeper_buy_context_found = true final_pnl_pct present close_reason in {Target, StopLoss, TimeStop} 

Jeżeli będzie tylko:

ROUTE REGRESSION FIXED, LIFECYCLE CLOSE NOT OBSERVED 

czyli legacy executable, dispatch/simulation attempted, brak unsupported_legacy_buy_layout_requires_bcv2, ale reporter rows nadal 0 w 600s — wtedy nie commitować automatycznie. Najpierw raport i decyzja. To może oznaczać, że naprawiliśmy route regression, ale smoke window nie domknął pozycji.

Jeżeli dalej:

validated legacy -> not_executable_route validated legacy -> unsupported_legacy_buy_layout_requires_bcv2 

fix failed.

Raport po smoke

Tak, wygenerować:

reports/selector/restore_lifecycle_regression_fix_<UTC_TS>/RESTORE_LIFECYCLE_REGRESSION_FIX.md reports/selector/restore_lifecycle_regression_fix_<UTC_TS>/restore_lifecycle_regression_fix_v1.json 

Wymagana sekcja legacy_contract_matrix jest bardzo dobra. Dodałbym jeszcze dwa pola:

complete_legacy_contract_rows complete_legacy_contract_with_dispatch_attempted_rows 

Czyli pełna minimalna macierz:

legacy_buy_rows legacy_buy_complete_rows complete_legacy_contract_rows legacy_buy_executable_rows legacy_buy_blocked_rows unsupported_legacy_buy_layout_requires_bcv2_rows legacy_buy_missing_buyback_remaining_accounts_rows fallback_route_ready_rows dispatch_attempted_rows complete_legacy_contract_with_dispatch_attempted_rows simulation_attempted_rows reporter_rows_written 

Commit allowlist

Zgoda:

ghost-launcher/src/oracle_runtime.rs 

plus tylko jeśli realnie wymaga kompilacja/test fixture:

ghost-launcher/src/components/trigger/component.rs ghost-launcher/src/components/trigger/shadow_run.rs ghost-launcher/src/events.rs 

Nie stage’ować:

P3/R2 scripts Gatekeeper configs FSC configs NLN program streams feature events datasets logs reports smoke artifacts 

Commit message:

PR-RESTORE: restore executable legacy buy path after route contract gating 

Finalna dyspozycja

Implementować. Nie rozszerzać scope. Nie dotykać P3D/R2/samplera. Nie commitować bez restore smoke proof. Cel: validated LegacyBuy wraca jako executable, telemetry/incomplete nadal fail.
