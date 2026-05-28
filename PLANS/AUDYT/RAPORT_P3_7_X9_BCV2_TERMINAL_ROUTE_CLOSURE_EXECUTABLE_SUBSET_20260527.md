# RAPORT P3.7-X9-S — BCV2 Terminal Route Closure + Executable Subset Burnin

Data: 2026-05-27  
Namespace smoke: `shadow-burnin-v3-p37-x9-bcv2-terminal-route-closure-executable-subset-smoke`  
Baza kodu: `9086beb` + niezatwierdzony diff X9  
Konfiguracja smoke: `/tmp/gho-x9-smoke/shadow-burnin-v3-p37-x9-bcv2-terminal-route-closure-executable-subset-smoke.toml`  
Console log: `/tmp/gho-x9-smoke/x9_runtime_console.log`  
Audit JSON: `/tmp/gho-x9-smoke/x9_audit.json`  
Audit MD: `/tmp/gho-x9-smoke/x9_audit.md`  
Unique BCV2 JSON: `/tmp/gho-x9-smoke/x9_unique_bcv2.json`

## Werdykt

```text
X9 terminal route closure: CODE-LEVEL PASS, runtime partially exercised
X9-S executable subset burnin: FAIL-TRANSPORT-STABILITY
Executable subset verdict A/B: INCONCLUSIVE
R18: NO-GO
P2/live/Sender: NO-GO
Gatekeeper/scoring/threshold tuning: NO-GO
legacy_buy/fallback: NO-GO
```

X9 poprawnie dodał terminalną klasyfikację dla working-builder route wymagającej BCV2, które ma exact negative load evidence i nie ma `RpcReady` / `PrecheckReady`. W partial smoke pojawiły się realne rows z:

```text
route_resolution_terminal_reason = bcv2_not_persistent_or_not_loadable
execution_feasibility_status = not_executable_route
execution_feasibility_reason = bcv2_not_persistent_or_not_loadable
lifecycle_label_eligibility = not_lifecycle_label_eligible
buy_quality_denominator = exclude
```

Nie można jednak zaakceptować smoke jako burnin A/B, bo transport ponownie wszedł w znany failure mode comparator panic, po czym gRPC stream przestał robić postęp.

## Zakres Implementacji X9

Zmiany są ograniczone do warstwy diagnostyczno-klasyfikacyjnej working-builder/shadow:

- `ghost-launcher/src/config.rs`
  - Dodano `p37_shadow_probe.bcv2_terminal_route_closure_enabled`.
  - Default: `false`.
  - X9 smoke uruchomiony z wartością `true`.

- `ghost-launcher/src/oracle_runtime.rs`
  - Dodano terminalny reason `bcv2_not_persistent_or_not_loadable`.
  - Dodano policy helper dla X9 terminal route closure.
  - Terminal classification jest gated configiem i working-builder parity mode.
  - Terminal classification wymaga exact BCV2 execution evidence z negatywną przyczyną loadability, np. `missing_on_rpc` / `RpcMissing` / `PrecheckMissing`.
  - Nie klasyfikuje jako terminalnych sygnałów typu discovery/stale/provider timeout.
  - Route nie przechodzi manifest-ready i nie próbuje probe simulation.

- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
  - Dodano liczenie terminalnego BCV2 exclusion.
  - Dodano denominatory exclusion dla buy-quality i lifecycle.
  - Dodano analogiczne pola prefiksowane `active_shadow_*`.

- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
  - Dodano testy agregacji terminalnego BCV2 route exclusion i denominator removal.

Poza zakresem i nietknięte:

- `off-chain/components/trigger/src/direct_buy_builder.rs`
- Helius Sender / `LiveTxSender`
- Gatekeeper
- scoring / thresholds
- legacy_buy / fallback
- `AccountStateCore`
- `off-chain/components/seer/src/grpc_connection.rs`

## Walidacja Przed Smoke

Wykonane komendy:

```text
cargo check -p ghost-launcher
cargo test -p ghost-launcher --lib p37_working_builder -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo fmt --check
git diff --check
cargo build -p ghost-launcher --bin ghost-launcher
```

Wynik:

```text
PASS
```

Preflight smoke:

```text
[ok] execution_profile: execution_mode=Shadow, entry_mode=shadow_only
[ok] transport.grpc: source_mode=grpc endpoint=yellowstone-solana-mainnet.core.chainstack.com:443
[ok] trigger.balance: 0.047172000 SOL >= 0.007200000 SOL reserve+trade budget
[ok] preflight: all runtime checks passed
```

## Runtime Evidence Z X9-S

Run rozpoczął się poprawnie:

```text
execution_mode=Shadow
entry_mode=shadow_only
p37_shadow_probe_enabled=true
p37_shadow_probe_namespace=shadow-burnin-v3-p37-x9-bcv2-terminal-route-closure-executable-subset-smoke
```

Pierwsze dane napływały. Seer wykrywał BCV2 i hydration nadal pokazywał `missing_on_rpc`:

```text
BCV2_RPC_HYDRATION_MISSING rows: 296
BCV2_RPC_HYDRATION_READY rows: 0
```

X9 terminal policy została runtime'owo przećwiczona:

```text
P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED rows: 11
bcv2_not_persistent_or_not_loadable marker/log occurrences: 11
```

Przykładowy marker runtime:

```text
P37_SHADOW_PROBE_SELECTED_ROUTE_FINAL_MANIFEST_BLOCKED
precheck_failure_reason=no_executable_route_account_set:bcv2_not_persistent_or_not_loadable:bonding_curve_v2:<pubkey>
```

## Audit Partial-Run

Audyt z `/tmp/gho-x9-smoke/x9_audit.md`:

```text
bcv2_terminal_route_exclusion_rows = 11
bcv2_terminal_route_exclusion_unique_pubkeys = 11
execution_feasibility_reject_bcv2_not_persistent_rows = 11
buy_quality_denominator_excluded_bcv2_rows = 11
lifecycle_denominator_excluded_bcv2_rows = 11
```

Execution feasibility:

```text
route_executable_rows = 0
route_non_executable_rows = 11
execution_feasibility_reject_rows = 11
execution_feasibility_status_counts = {"not_executable_route": 11, "unknown": 1}
execution_feasibility_reason_counts = {
  "bcv2_not_persistent_or_not_loadable": 11,
  "probe_execution_precheck_failed": 1
}
```

Readiness / execution:

```text
working_builder_manifest_ready_rows = 0
working_builder_bcv2_execution_evidence_exact_pubkey_match_rows = 11
working_builder_bcv2_execution_evidence_ready_rows = 0
successful_probe_entry_rows = 0
active_shadow_successful_entry_rows = 0
lifecycle_eligible_rows = 0
post_simulation_account_not_found_rows = 0
```

Exact-watch / evidence ingress:

```text
bcv2_exact_watch_registered_rows = 410
bcv2_exact_watch_in_subscribe_request_rows = 271
bcv2_resubscribe_sent_rows = 271
bcv2_account_update_received_rows = 2
bcv2_rpc_hydration_ready_rows = 0
bcv2_rpc_hydration_missing_rows = 296
```

Unique working-builder BCV2 join:

```text
working_builder_bcv2_unique_pubkeys = 11
working_builder_bcv2_registered_unique_pubkeys = 11
working_builder_bcv2_included_unique_pubkeys = 11
working_builder_bcv2_hydration_missing_unique_pubkeys = 11
working_builder_bcv2_hydration_ready_unique_pubkeys = 0
audit_bucket_unique_pubkeys = {"included_rpc_missing_no_same_update": 11}
```

Safety invariants:

```text
legacy_buy_route_attempted_rows = 0
legacy_fallback_attempted_rows = 0
selected_route_handoff_mismatch_rows = 0
send_transaction markers = 0
LiveTxSender::send_transaction markers = 0
SUBMITTED markers = 0
```

## Transport Failure

Smoke nie doszedł do stabilnego burnina. W logu pojawił się ten sam rodzaj panic, który wcześniej blokował X8C-S:

```text
user-provided comparison function does not correctly implement a total order
```

Lokalizacja w logu:

```text
/tmp/gho-x9-smoke/x9_runtime_console.log:19698
```

Bezpośrednio później:

```text
Ghost/Pump transport profile=primary_global source_label=grpc_global_stream: all workers exited
```

Lokalizacja:

```text
/tmp/gho-x9-smoke/x9_runtime_console.log:19701
```

Następnie watchdog zaczął raportować stall:

```text
WATCHDOG WARN: gRPC stalled for 62621ms (>60000 ms)
WATCHDOG WARN: gRPC stalled for 122622ms (>60000 ms)
WATCHDOG WARN: gRPC stalled for 182622ms (>60000 ms)
WATCHDOG WARN: gRPC stalled for 242621ms (>60000 ms)
```

Nie było `WATCHDOG FATAL` w zebranym oknie, ale po `all workers exited` i narastającym stallu run nie jest wiarygodny jako executable-subset burnin.

Proces X9-S został zatrzymany po rozpoznaniu failure mode, żeby nie dopisywać dalszych danych z uszkodzonego transportowo stanu.

## Interpretacja

X9 jako zmiana logiczna robi to, czego oczekiwaliśmy:

- nie próbuje robić `manifest-ready` z BCV2 bez `RpcReady` / `PrecheckReady`;
- nie traktuje observed tx ani AccountUpdate jako execution-ready;
- klasyfikuje route jako `not_executable_route`, a nie jako złą decyzję, zły scoring ani buy-quality bad;
- usuwa takie rows z buy-quality i lifecycle denominatorów;
- nie ożywia legacy/fallback;
- nie dotyka live Sendera.

Natomiast X9-S nie odpowiada jeszcze na pytanie:

```text
Czy po wycięciu BCV2 non-loadable route universe ma executable subset?
```

Odpowiedź A/B jest nieważna, bo transport padł zanim burnin mógł zebrać stabilne dane.

## Decyzja

```text
GO: uznać X9 code-level terminal closure za zaimplementowane i przetestowane jednostkowo.
GO: uznać X9 runtime classification za częściowo przećwiczoną na live danych.
NO-GO: uznać X9-S za PASS-A.
NO-GO: uznać X9-S za PASS-B executable-subset-empty.
NO-GO: R18.
NO-GO: P2/live/Sender.
NO-GO: Gatekeeper/scoring/threshold tuning.
NO-GO: legacy_buy/fallback.
```

## Następny Bloker

Aktualny bloker nie jest już BCV2 readiness policy. Aktualny bloker to transport comparator total-order panic:

```text
user-provided comparison function does not correctly implement a total order
```

Dopóki ten panic wraca, żaden smoke typu executable-subset burnin nie może być traktowany jako rozstrzygający.

W worktree nadal istnieją stashe spoza X9:

```text
stash@{0}: park X8D-PR2B live diagnostic diff before X9
stash@{1}: park grpc_connection local smoke diff before X8D-PR1 commit
```

Nie były aplikowane ani commitowane w ramach X9.

