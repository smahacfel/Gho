# P3.7-E4R3S Final Selected-Route Manifest Smoke

Data: 2026-05-25
HEAD: `8ba4236`
Worktree: `HEAD + lokalne zmiany E4R3`
Config lokalny: `configs/rollout/shadow-burnin-v3-p37-e4r3s-final-selected-route-manifest-smoke.toml`
Namespace: `shadow-burnin-v3-p37-e4r3s-final-selected-route-manifest-smoke`
Runtime: `timeout 20m`, shadow-only, promotion/P2/live off

## Werdykt

`E4R3S` nie odblokowal execution.

To nie jest jednak powrot do starego bledu E4R2, gdzie system twierdzil, ze handoff jest applied, a finalnie szedl primary manifest. E4R3 zadzialal jako safety gate: wykryl, ze finalny selected legacy manifest nadal zawiera `bonding_curve_v2`, oznaczyl handoff jako mismatch i zatrzymal row przed realna symulacja.

Najkrotszy wynik:

```text
strict replay: PASS / full_replay_ok
diagnostic_quality: PASS
identity/hash: PASS
IWIM overflow panic: 0
AccountNotFound: 0
post-simulation BCV2 AccountNotFound: 0

legacy_buy route ready rows:
  active = 7
  probe = 14

selected legacy handoff claimed:
  active = 7
  probe = 14

selected legacy handoff validated:
  active = 0
  probe = 0

selected legacy handoff mismatch:
  active = 7
  probe = 14

selected legacy final manifest contains BCV2:
  active = 7
  probe = 14

no_executable_route_but_simulated:
  active = 0
  probe = 0

successful entries: 0
lifecycle eligible rows: 0

overall:
  safety/enforcement: PASS-B / correct fail-closed
  execution unlock: FAIL / NO-GO
```

Interpretacja operacyjna:

```text
E4R3 zamknal falszywy marker "handoff applied".
Nie zamknal executable legacy_buy.
Finalny builder/simulation manifest dla selected legacy_buy nadal ma BCV2.
Nie wolno odpalac kolejnego runtime smoke bez decyzji o prawdziwym legacy manifest/ABI.
```

## Run

Komenda:

```bash
timeout 20m env RUST_LOG=info ./target/release/ghost-launcher \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-e4r3s-final-selected-route-manifest-smoke.toml
```

Proces zakonczyl sie kodem `124`, czyli przez oczekiwany limit `timeout 20m`. Po runie nie zostal aktywny proces `ghost-launcher`.

Artefakty fizyczne:

```text
logs/shadow_run/.../buys.jsonl                  7 rows
logs/shadow_run/.../shadow_entries.jsonl        7 rows
logs/shadow_run/.../shadow_lifecycle.jsonl      7 rows
logs/shadow_run/.../probe_selection.jsonl       14 rows
logs/shadow_run/.../probe_skips.jsonl           228 rows
logs/shadow_run/.../probe_transport.jsonl       14 rows
logs/shadow_run/.../probe_shadow_entries.jsonl  0 rows
logs/shadow_run/.../probe_shadow_lifecycle.jsonl 0 rows
seer_runtime_coverage_audit.jsonl               242 rows
```

Logi runtime:

```text
system.log.2026-05-25
oracle.log.2026-05-25
```

## Replay / Diagnostics

`v3_full_replay_report.py --strict --json`:

```text
status = ok
replay_status = full_replay_ok
total_rows = 242
v3_rows = 242
bad_rows = 0
```

`v3_shadow_report.py --json`:

```text
status = ok
artifact_freshness.stale_against_config = false
raw_rows = 242
deduped_rows = 242
v3_rows = 242
bad_rows = 0
execution.success_count = 0
execution.outcomes.selected_route_handoff_mismatch = 7
execution.outcomes.missing = 235
```

`v3_p37_l1_reject_diagnostics.py --json`:

```text
diagnostic_quality.status = PASS
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
decision_rows = 242
active_shadow_buys = 7
active_shadow_entries = 7
active_shadow_lifecycle = 7
probe_selection = 14
probe_transport = 14
probe_entries = 0
probe_lifecycle = 0
active_shadow_dispatch_failure_rows = 14
active_shadow_precheck_status_counts = {"precheck_failed": 14}
active_shadow_lifecycle_eligibility_status_counts = {"not_lifecycle_eligible": 14}
active_shadow_account_not_found_rows = 0
active_shadow_simulation_error_category_counts = {}
shadow_payer_strategy = ephemeral
shadow_payer_account_status = ephemeral_not_rpc_required
shadow_payer_account_error =
  selected_route_handoff_mismatch:selected_legacy_buy_final_manifest_contains_primary_bcv2
```

`v3_p37_mfs_lifecycle_join_key_audit.py`:

```text
readiness = not_ready
join_key_acceptance = fail
probe_readiness = not_ready
probe_decision_join_acceptance = fail
probe_required_exact_decision_v3_join_coverage = 1.0
probe_entry_materialization_status_counts = {"simulation_error": 14}
full_chain_ab_record_id_coverage = 1.0
probe_chain_ab_record_id_coverage = 1.0
probe_chain_probe_id_coverage = 1.0
```

Uwaga o denominatorach: `v3_full_replay_report` liczy 242 terminal decision rows. Join-key audit raportuje `decision_rows_total = 249`, bo agreguje glowny decision log razem z aktywnymi BUY decision artifacts.

## Log Health

Grep po `system.log.2026-05-25` i `oracle.log.2026-05-25`:

```text
panic / thread panicked = 0
IWIM overflow = 0
AccountNotFound = 0
selected_route_handoff_mismatch = 42
selected_legacy_buy_final_manifest_contains_primary_bcv2 = 42
```

To potwierdza, ze S1/IWIM safety guard nie regresowal i ze E4R3S nie wprowadzil powrotu AccountNotFound.

## Execution Feasibility

Join-key audit:

```text
decision_rows_total = 249
probe_selected_rows = 14
route_executable_rows = 0
route_non_executable_rows = 209
execution_feasibility_reject_rows = 35
active_buy_execution_infeasible_rows = 21
successful_entry_rows = 0
lifecycle_eligible_rows = 0
lifecycle_labeled_rows = 7
execution_feasibility_rate = 0.0
entry_materialization_rate = None
lifecycle_label_rate = None
```

Wniosek: E4R3S nie daje denominatora do L2 ani R18. Dataset pozostaje chroniony, ale executable universe dalej jest zerowy.

## Legacy Buy State

Active path:

```text
legacy_buy_route_attempted_rows = 7
legacy_buy_route_ready_rows = 7
legacy_buy_route_not_ready_rows = 0
legacy_buy_curve_authoritative_and_load_ready_rows = 7
legacy_buy_curve_source_counts = {"account_state_core": 7}
legacy_buy_curve_authority_status_counts = {"authoritative_account_state": 7}
legacy_buy_curve_rpc_load_status_counts = {"present_on_rpc_precheck": 7}
legacy_buy_account_set_status_counts = {"ready": 7}
legacy_buy_fallback_account_set_ready_rows = 7
legacy_buy_route_ready_after_account_set_separation_rows = 7
legacy_buy_successful_entry_rows = 0
```

Probe path:

```text
legacy_buy_route_attempted_rows = 14
legacy_buy_route_ready_rows = 14
legacy_buy_route_not_ready_rows = 0
legacy_buy_curve_authoritative_and_load_ready_rows = 14
legacy_buy_curve_source_counts = {"account_state_core": 14}
legacy_buy_curve_authority_status_counts = {"authoritative_account_state": 14}
legacy_buy_curve_rpc_load_status_counts = {"present_on_rpc_precheck": 14}
legacy_buy_account_set_status_counts = {"ready": 14}
legacy_buy_fallback_account_set_ready_rows = 14
legacy_buy_route_ready_after_account_set_separation_rows = 14
legacy_buy_successful_entry_rows = 0
```

To oznacza, ze E3/E4 readiness side dalej jest zdrowy: core curve jest authoritative + load-ready, a legacy route readiness na warstwie resolvera przechodzi.

## Final Selected-Route Manifest Enforcement

Active path:

```text
selected_legacy_handoff_claimed_rows = 7
selected_legacy_handoff_validated_rows = 0
selected_legacy_handoff_mismatch_rows = 7
selected_legacy_final_manifest_contains_bcv2_rows = 7
selected_legacy_final_manifest_contains_primary_route_builder_rows = 0
selected_legacy_request_variant_not_legacy_rows = 0
selected_legacy_precheck_hash_mismatch_rows = 0
selected_legacy_simulation_hash_mismatch_rows = 0
no_executable_route_but_simulated_rows = 0
legacy_buy_selected_but_primary_bcv2_terminal_rows = 0
legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows = 7
legacy_buy_selected_and_precheck_uses_legacy_account_set_rows = 0
legacy_buy_selected_and_simulation_uses_legacy_account_set_rows = 0
```

Probe path:

```text
selected_legacy_handoff_claimed_rows = 14
selected_legacy_handoff_validated_rows = 0
selected_legacy_handoff_mismatch_rows = 14
selected_legacy_final_manifest_contains_bcv2_rows = 14
selected_legacy_final_manifest_contains_primary_route_builder_rows = 14
selected_legacy_request_variant_not_legacy_rows = 0
selected_legacy_precheck_hash_mismatch_rows = 0
selected_legacy_simulation_hash_mismatch_rows = 0
no_executable_route_but_simulated_rows = 0
legacy_buy_selected_but_primary_bcv2_terminal_rows = 0
legacy_buy_selected_but_primary_bcv2_in_selected_manifest_rows = 14
legacy_buy_selected_and_precheck_uses_legacy_account_set_rows = 0
legacy_buy_selected_and_simulation_uses_legacy_account_set_rows = 0
```

Najwazniejszy kontrakt E4R3:

```text
no_executable_route_but_simulated_rows = 0
```

To jest dobry wynik safety. Runtime nie przepchnal route z finalnym manifest mismatch do realnej symulacji jako lifecycle-eligible entry.

## Active Row Shape

Aktywne BUY rows maja spojnosc safety:

```text
selected_route_source = selected_fallback_route_execution_handoff
selected_route_handoff_status = selected_route_handoff_mismatch
selected_route_handoff_reason = selected_legacy_buy_final_manifest_contains_primary_bcv2
precheck_failure_reason =
  selected_route_handoff_mismatch:selected_legacy_buy_final_manifest_contains_primary_bcv2
execution_outcome = selected_route_handoff_mismatch
lifecycle_eligibility_status = not_lifecycle_eligible
```

W starej wersji E4R2 ten sam obszar potrafil raportowac `selected_route_handoff_applied` mimo finalnego BCV2. Po E4R3 juz tego nie robi.

## Probe Row Shape

Probe transport rows:

```text
execution_outcome = selected_route_handoff_mismatch: 14
route_resolution_status = no_executable_route_account_set: 14
selected_route_source = selected_fallback_route_execution_handoff: 14
selected_route_handoff_status = selected_route_handoff_mismatch: 14
selected_route_handoff_reason = selected_legacy_buy_final_manifest_contains_primary_bcv2: 14
precheck_failure_reason =
  selected_route_handoff_mismatch:selected_legacy_buy_final_manifest_contains_primary_bcv2: 14
buy_variant = legacy_buy: 14
simulation_error_kind = simulation_error: 14
```

Istotna roznica: audit klasyfikuje `no_executable_route_but_simulated_rows = 0`, bo `selected_route_handoff_mismatch:*` jest precheckowym fail-closed reason, nie realna proba wykonania skażonego manifestu.

## Kod / Kontrakt

E4R3 w lokalnym worktree dotyka:

```text
ghost-launcher/src/oracle_runtime.rs
scripts/v3_p37_mfs_lifecycle_join_key_audit.py
scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
```

Kluczowe miejsca:

```text
ghost-launcher/src/oracle_runtime.rs:9722
  p37_selected_route_handoff_diagnostics

ghost-launcher/src/oracle_runtime.rs:9804
  p37_selected_route_final_manifest_failure_reason

ghost-launcher/src/oracle_runtime.rs:10910
  probe dispatch blocks selected route final manifest mismatch before readiness wait / simulate

ghost-launcher/src/oracle_runtime.rs:13235
  active shadow precheck returns failed receipt on selected route final manifest mismatch

scripts/v3_p37_mfs_lifecycle_join_key_audit.py:1035
  no_executable_route_but_simulated_rows

scripts/v3_p37_mfs_lifecycle_join_key_audit.py:1141
  selected_legacy_handoff_validated_rows / mismatch counters
```

Targeted validation przed smoke:

```text
cargo check -p ghost-launcher
cargo test -p ghost-launcher --lib selected_legacy_buy -- --nocapture
cargo test -p ghost-launcher --lib p37_route_resolver -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
cargo fmt -p ghost-launcher -- --check
git diff --check
cargo build --release -p ghost-launcher --bin ghost-launcher
```

## Analiza

E4R3 odpowiadal na jeden konkretny problem: marker `selected_route_handoff_applied` nie mogl byc ustawiany na podstawie intencji lub pol diagnostycznych. Musial oznaczac, ze finalny request/precheck/simulation manifest realnie odpowiada selected route.

Po E4R3 mamy:

```text
selected_route_handoff_applied = 0
selected_route_handoff_mismatch = 21
final manifest contains BCV2 = 21
no executable route but simulated = 0
```

To jest poprawne fail-closed zachowanie. System nie udaje juz, ze selected legacy handoff jest applied.

Jednoczesnie runtime pokazal, ze sam `legacy_buy` final manifest nadal nie jest czysty:

```text
legacy_buy route readiness/resolver: ready
final execution manifest: contains bonding_curve_v2
selected handoff validation: mismatch
execution: blocked before lifecycle
```

To przesuwa problem nizej niz resolver i niz handoff marker. Pozostaly blocker jest w finalnym build/simulation manifest contract:

```text
albo DirectBuyBuilder legacy layout nadal buduje extended route z bonding_curve_v2,
albo route nazwana legacy_buy w tym builderze nie jest prawdziwa legacy route bez BCV2,
albo selected route account-set jest poprawny diagnostycznie, ale final transaction builder ignoruje jego semantyczna liste kont.
```

Bez rozstrzygniecia ABI/layoutu nie ma sensu dodawac kolejnej latki handoffu.

## Decyzja

```text
E4R3 code-level: PASS jako safety enforcement
E4R3S runtime safety: PASS-B / correct fail-closed
E4R3S execution unlock: FAIL / NO-GO
legacy_buy executable route: NOT VALIDATED / still blocked
post-simulation AccountNotFound: 0
IWIM overflow: 0
L2D2 / R18 / thresholds / Phase B / P2 / live: NO-GO
```

Nie robimy kolejnego smoke bez nowej decyzji o builderze.

## Recommended Next

Nastepny krok nie powinien byc kolejnym E4R handoff patchem.

Waski, uczciwy next step:

```text
P3.7-E5A -- DirectBuyBuilder legacy_buy final manifest / ABI audit
```

Cel E5A:

```text
1. Sprawdzic, czy faktyczna pump.fun legacy_buy instruction layout wymaga bonding_curve_v2.
2. Porownac observed legacy_buy tx account metas z finalnym DirectBuyBuilder manifestem.
3. Jesli observed true legacy route nie wymaga BCV2:
     implement true legacy_buy account layout w builderze.
4. Jesli builderowy legacy_buy semantycznie nadal wymaga extended/BCV2 account:
     oznaczyc legacy_buy jako unsupported under current route support
     i wrocic do E1 route matrix / nastepny route target.
```

Acceptance dla E5A:

```text
direct_buy_legacy_manifest_requires_bcv2 = true/false
observed_legacy_buy_account_position_map_complete = true/false
builder_legacy_manifest_matches_observed_legacy_layout = true/false
recommended_next_path =
  implement_true_legacy_buy_layout
  OR legacy_buy_unsupported_select_next_route
  OR audit_gap
```

Do czasu E5A:

```text
no runtime
no Gatekeeper changes
no threshold tuning
no L2D2
no R18
no P2/live
```
