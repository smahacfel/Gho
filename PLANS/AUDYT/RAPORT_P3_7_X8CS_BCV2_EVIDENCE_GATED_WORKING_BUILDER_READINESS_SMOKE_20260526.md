# RAPORT P3.7-X8C-S - BCV2 Evidence-Gated Working Builder Readiness Smoke

Data: 2026-05-26
Status: **PASS-B-EVIDENCE / FAIL-TRANSPORT-STABILITY / R18 NO-GO**

## Cel smoke

Smoke `P3.7-X8C-S` mial zweryfikowac runtime po PR6:

- czy `bonding_curve_v2` przechodzi przez exact-watch / hydration / AccountUpdate / evidence store,
- czy `working-builder` korzysta z role-aware `ExecutionAccountEvidence`,
- czy PR6 fail-closed nie przepuszcza manifest-ready na `ObservedTxMeta`, `ExactWatchRegistered` ani `AccountUpdateReceived`,
- czy legacy/fallback/live/Sender pozostaja nieaktywne.

To nie byl R18 ani pelny burnin decyzyjny.

## Srodowisko

- clean runtime worktree: `/root/Gho-x8c-smoke-cf6593c`
- smoke HEAD: `cf6593c feat: gate working-builder readiness on execution evidence`
- smoke worktree status: clean, detached `HEAD`
- main repo status przed raportem: dirty tracked scripts istnialy w `/root/Gho`; smoke nie byl uruchamiany z tego worktree
- config smoke: `/tmp/gho-x8c-smoke-cf6593c/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke.toml`
- namespace: `shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke`
- generated audit outputs:
  - `/tmp/gho-x8c-smoke-cf6593c/x8c_join_key_audit.json`
  - `/tmp/gho-x8c-smoke-cf6593c/x8c_join_key_audit.md`

Preflight przeszedl po zmianie tylko zewnetrznego configu smoke: `keypair_path` zostal skierowany na istniejacy `/root/Gho/wallets/shadow-burnin-test.json`. Repo nie bylo modyfikowane dla uruchomienia smoke.

Runtime config potwierdzony w logach:

- `execution_mode=Shadow`
- `entry_mode=shadow_only`
- `p37_shadow_probe_enabled=true`
- `p37_shadow_probe_namespace=shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke`
- `LiveSellHandle: skipped (no live transport required at startup) execution_mode=Shadow`

Dowody:

- `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/system.log.2026-05-26:5`
- `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/system.log.2026-05-26:58`

## Czas i zakonczenie runa

Smoke wystartowal runtime o `2026-05-26T17:31:02Z`.

Run nie dobiegl do pelnych 30 minut. Zakonczyl sie fatal watchdogiem o `2026-05-26T17:38:02Z`:

```text
WATCHDOG FATAL: gRPC stalled for 318525ms and transport progress is 318503ms (>300000 ms) - exiting with code 2
```

Dodatkowo tuz przed zatrzymaniem transportu wystapil panic w workerze Tokio:

```text
thread 'tokio-rt-worker' (...) panicked at .../core/src/slice/sort/shared/smallsort.rs:860:5:
user-provided comparison function does not correctly implement a total order
```

Dowody:

- `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/x8c_smoke_console.log:63878`
- `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/system.log.2026-05-26:49060`

Wniosek: wynik evidence/readiness jest uzyteczny diagnostycznie, ale smoke nie jest stabilnym 30-min runtime proofem. Transport/stability pozostaje blockerem przed R18.

## 1. Exact-watch / evidence ingress

Zrodlo: `/tmp/gho-x8c-smoke-cf6593c/x8c_join_key_audit.json`, sekcja `bcv2_exact_watch_coverage`, oraz logi Seera.

| Metryka | Wartosc |
| --- | ---: |
| `bcv2_exact_watch_registered_rows` | 1968 |
| `bcv2_exact_watch_in_subscribe_request_rows` | 1074 |
| `bcv2_resubscribe_sent_rows` | 1070 |
| `bcv2_account_update_received_rows` | 29 |
| `bcv2_rpc_hydration_ready_rows` | 0 |
| `bcv2_rpc_hydration_missing_rows` | 1106 |
| `bcv2_exact_watch_subscribe_dropped_rows` | 0 |

Interpretacja:

- exact-watch registration dzialal,
- BCV2 trafialo do `SubscribeRequest`,
- resubscribe byl wysylany po zmianie BCV2 registry,
- runtime widzial AccountUpdate dla czesci BCV2,
- hydration w tym smoke nie dala zadnego `RpcReady`; dominujacym wynikiem byl `RpcMissing`.

Przykladowe dowody:

- `BCV2_EXACT_WATCH_REGISTERED`: `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/system.log.2026-05-26`
- `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED`: jw.
- `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT`: jw.
- `BCV2_RPC_HYDRATION_MISSING`: jw.

## 2. Role-aware execution evidence

Zrodlo: `probe_transport.jsonl` + audit helper.

| Metryka | Wartosc |
| --- | ---: |
| `working_builder_bcv2_execution_evidence_ready_rows` | 0 |
| `working_builder_bcv2_execution_evidence_exact_pubkey_match_rows` | 22 |
| `working_builder_bcv2_execution_evidence_stale_rows` | 0 |
| `working_builder_bcv2_execution_evidence_conflict_rows` | 22 |
| `working_builder_bcv2_execution_evidence_owner_rows` | 4 |
| `working_builder_bcv2_execution_evidence_data_len_rows` | 4 |
| `working_builder_bcv2_execution_evidence_slot_rows` | 22 |
| `working_builder_bcv2_execution_evidence_context_slot_rows` | 0 |

Status counts:

```json
{"account_update_received": 1, "discovery_hint": 2, "rpc_missing": 19}
```

Source counts:

```json
{"observed_tx_meta": 2, "rpc_hydration": 19, "yellowstone_account_update": 1}
```

Reason counts:

```json
{
  "account_update_received_not_execution_load_ready": 1,
  "missing_on_rpc": 19,
  "not_execution_load_ready:discovery_hint": 2
}
```

Wazne rozroznienie PR6:

- raw PR4 evidence mialo `working_builder_bcv2_evidence_ready_rows = 4`,
- execution evidence mialo `working_builder_bcv2_execution_evidence_ready_rows = 0`,
- czyli PR6 poprawnie nie uznal `AccountUpdateReceived` ani `DiscoveryHint` za execution-load-ready.

Dowody runtime:

- `DIAG_EXECUTION_ACCOUNT_EVIDENCE_UPSERT` z `source="observed_tx_meta"` i `status="discovery_hint"`: `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/oracle.log.2026-05-26:116`
- `DIAG_EXECUTION_ACCOUNT_EVIDENCE_UPSERT` z `source="exact_watch_registered"` i `status="subscription_requested"`: `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/oracle.log.2026-05-26:118`
- `DIAG_EXECUTION_ACCOUNT_EVIDENCE_UPSERT` z `source="rpc_hydration"` i `status="rpc_missing"`: `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/oracle.log.2026-05-26:120`

## 3. Readiness / execution

Zrodlo: audit helper, `probe_transport.jsonl`, `buys.jsonl`, `shadow_entries.jsonl`, `shadow_lifecycle.jsonl`.

| Metryka | Wartosc |
| --- | ---: |
| `working_builder_manifest_ready_rows` | 0 |
| `working_builder_manifest_missing_required_rows` | 22 |
| `successful_probe_entry_rows` | 0 |
| `active_shadow_successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `active_shadow_lifecycle_eligible_rows` | 0 |
| `post_simulation_account_not_found_rows` | 0 |
| `active_shadow_account_not_found_rows` | 0 |
| `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows` | 0 |

Przyklady fail-closed:

```text
working_builder_final_manifest_execution_evidence_not_ready:bonding_curve_v2:<pubkey>:missing_on_rpc
```

Dowody:

- `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/oracle.log.2026-05-26:2696`
- `/root/Gho-x8c-smoke-cf6593c/logs/rollout/shadow-burnin-v3-p37-x8c-bcv2-evidence-gated-readiness-smoke/oracle.log.2026-05-26:11718`

Interpretacja:

- PR6 gating zadzialal fail-closed,
- `ObservedTxMeta` nie odblokowal manifest-ready,
- `AccountUpdateReceived` nie odblokowal execution-load-ready,
- nie pojawil sie powrot starego post-simulation `AccountNotFound` po manifest-ready, bo manifest-ready nie zostal osiagniety.

## 4. Safety invariants

| Invariant | Wartosc |
| --- | ---: |
| `legacy_buy_route_attempted_rows` | 0 |
| `active_shadow_legacy_buy_route_attempted_rows` | 0 |
| `legacy_fallback_attempted_rows` | 0 |
| `active_shadow_legacy_fallback_attempted_rows` | 0 |
| `selected_route_handoff_mismatch_rows` | 0 |
| `active_shadow_selected_route_handoff_mismatch_rows` | 0 |
| `send_transaction(` | 0 |
| `LiveTxSender::send_transaction` | 0 |
| `SUBMITTED` | 0 |

Uwaga: w logach parsera wystepuja obserwowane transakcje z `buy_variant=legacy_buy`, ale audit nie pokazuje proby wykonania legacy route. To jest metadata/provenance, nie revival legacy execution path.

## Klasyfikacja PASS / FAIL

### PASS-A

Nie.

Powody:

- `working_builder_bcv2_execution_evidence_ready_rows = 0`
- `working_builder_manifest_ready_rows = 0`
- `successful_probe_entry_rows = 0`
- `active_shadow_successful_entry_rows = 0`

### PASS-B evidence/readiness

Tak, diagnostycznie.

Warunki spelnione:

- role-aware evidence path dziala,
- runtime store przyjmuje evidence,
- exact pubkey match w probe rows: `22/22`,
- readiness fail-closed z jawna przyczyna:
  - `missing_on_rpc`,
  - `not_execution_load_ready:discovery_hint`,
  - `account_update_received_not_execution_load_ready`,
- raw evidence nie odblokowalo execution readiness,
- legacy/fallback/handoff/live invariants sa czyste,
- post-simulation AccountNotFound nie wrocil.

### FAIL / blocker

Tak, dla stabilnosci transportu.

Run zakonczyl sie:

- panic w workerze Tokio: comparator sortowania nie implementuje total order,
- potem `Ghost/Pump transport ... all workers exited`,
- potem watchdog fatal `gRPC stalled ... exiting with code 2`.

To oznacza, ze smoke nie jest stabilnym 30-min dowodem runtime. Wynik evidence/readiness jest wazny, ale nie zamyka transportowej czesci X8C jako stabilnej.

## Decyzja operacyjna

```text
GO: uznac PR6 gate za runtime-exercised w trybie fail-closed.
GO: traktowac X8C-S jako PASS-B-EVIDENCE.
NO-GO: PASS-A.
NO-GO: R18.
NO-GO: P2/live/Sender.
NO-GO: Gatekeeper/scoring/threshold tuning.
NO-GO: legacy_buy/fallback.
NO-GO: pelny lifecycle burnin, dopoki transport/panic nie zostana wyjasnione.
```

## Rekomendowany nastepny krok

Nastepny krok powinien byc **X8C-S transport follow-up / PR7-prep**, nie R18:

1. Zlokalizowac panic `user-provided comparison function does not correctly implement a total order`.
2. Sklasyfikowac transport failure jako `source_label=grpc_global_stream`, po sekwencji h2 internal errors i `all workers exited`.
3. Dodac/wykorzystac diagnostyke PR7: global stream stall vs funding lane, last tx/account/blockmeta osobno.
4. Powtorzyc `P3.7-X8C-S` w czystym namespace po poprawce lub po jednoznacznej klasyfikacji transportu.

R18 pozostaje **NO-GO**.

## Delegation trace

```yaml
delegation_trace:
  task_classification: "runtime smoke closure report for X8C PR6 evidence-gated readiness"
  routing_performed: true
  primary_specialist: "Oracle Session Runtime Engineer"
  supporting_specialists_considered:
    - "Seer Ingest Event Integrity Specialist"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
    - "Solana Execution Path Engineer"
  specialist_docs_loaded:
    - "docs/agents/oracle-session-runtime-engineer.md"
  specialist_docs_not_loaded:
    - name: "seer-ingest-event-integrity-specialist.md"
      reason: "report consumes existing Seer markers; no parser/subscription code change was performed"
    - name: "decision-logging-replay-analyst.md"
      reason: "report uses existing audit script output and does not change durable schema"
    - name: "config-rollout-safety-reviewer.md"
      reason: "config was smoke-only outside repo; no rollout config was committed"
    - name: "solana-execution-path-engineer.md"
      reason: "no transaction builder, Sender, live submission, blockhash, retry, or confirmation behavior was changed"
  skills_used:
    - "ghost-execution"
    - "rust-master"
  fast_path_used: false
  contracts_checked:
    - "clean worktree runtime proof"
    - "shadow-only execution"
    - "role-aware ExecutionAccountEvidence separate from AccountStateCore"
    - "ObservedTxMeta does not unlock readiness"
    - "AccountUpdateReceived does not unlock execution-load-ready"
    - "legacy/fallback/handoff/live paths remain inactive"
    - "R18 remains blocked"
  unresolved_routing_uncertainty:
    - "exact source comparator causing Tokio panic not identified in this smoke report"
```
