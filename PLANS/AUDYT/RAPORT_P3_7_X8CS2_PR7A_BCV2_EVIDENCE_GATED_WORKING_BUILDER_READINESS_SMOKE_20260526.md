# P3.7-X8C-S2 / PR7A — BCV2 Evidence-Gated Working Builder Readiness Smoke

Data: 2026-05-26

Namespace:

```text
shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke
```

## Cel

Powtórzyć smoke po PR7A comparator/transport repair na czystym detached worktree i sprawdzić, czy po PR6 working-builder readiness pozostaje evidence-gated oraz czy poprzedni runtime panic/watchdog wraca.

To nie jest R18, nie jest live run i nie jest pełny burnin decyzyjny.

## Izolacja runu

Run został uruchomiony z izolowanego detached worktree:

```text
/root/Gho-x8c-s2-pr7a-smoke
```

Snapshot testowany:

```text
6244ae1
```

Konfiguracja:

```text
/tmp/gho-x8c-s2-pr7a-smoke/shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke.toml
```

Artefakty:

```text
/root/Gho-x8c-s2-pr7a-smoke/logs/rollout/shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke/
/root/Gho-x8c-s2-pr7a-smoke/logs/shadow_run/shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke/
/root/Gho-x8c-s2-pr7a-smoke/datasets/events/shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke/
```

Wygenerowany audit P3.7:

```text
/tmp/gho-x8c-s2-pr7a-smoke/x8c_s2_audit.json
/tmp/gho-x8c-s2-pr7a-smoke/x8c_s2_audit.md
```

## Konfiguracja safety

Potwierdzone:

```text
execution_mode = Shadow
entry_mode = shadow_only
p37_execution_builder_mode = working_builder_parity
execution_account_evidence_freshness_ms = 10000
metrics.enabled = false
no live Sender
no R18
no Gatekeeper/scoring/threshold tuning
no legacy/fallback revival
```

Preflight przed smoke przeszedł. Run zakończył się po 30-min timeout window; końcowe logi dalej pokazywały normalny ingest, resubscribe i evidence relay.

## Werdykt

```text
PASS-B-EVIDENCE
PASS-TRANSPORT-STABILITY-FOR-SMOKE
NO-GO: PASS-A
NO-GO: R18
NO-GO: P2/live/Sender
NO-GO: Gatekeeper/scoring/threshold tuning
NO-GO: legacy_buy/fallback
```

PR6 nadal działa fail-closed: role-aware evidence path jest runtime’owo ćwiczony, ale brak świeżego execution-load-ready `RpcReady` / `PrecheckReady` dla exact BCV2 pubkey blokuje manifest-ready. Po PR7A nie wrócił panic total-order comparator ani watchdog fatal w 30-min smoke.

## 1. Exact-Watch / Evidence Ingress

| metryka | wartość |
| --- | ---: |
| `bcv2_exact_watch_registered_rows` | 19616 |
| `bcv2_exact_watch_in_subscribe_request_rows` | 11320 |
| `bcv2_exact_watch_subscribe_dropped_rows` | 9144 |
| `bcv2_resubscribe_sent_rows` | 11221 |
| `bcv2_account_update_received_rows` | 471 |
| `bcv2_rpc_hydration_ready_rows` | 0 |
| `bcv2_rpc_hydration_missing_rows` | 11365 |

Working-builder pubkey join:

| metryka | wartość |
| --- | ---: |
| `working_builder_bcv2_rows` | 405 |
| `working_builder_bcv2_unique_pubkeys` | 354 |
| `working_builder_bcv2_registered_unique_pubkeys` | 354 |
| `working_builder_bcv2_included_unique_pubkeys` | 348 |
| `working_builder_bcv2_hydration_ready_unique_pubkeys` | 0 |
| `working_builder_bcv2_hydration_missing_unique_pubkeys` | 354 |
| `working_builder_bcv2_account_update_same_pubkey_unique_pubkeys` | 75 |

Classification:

```json
{
  "working_builder_bcv2_hydration_missing_after_include": 399,
  "working_builder_bcv2_included_no_update": 288,
  "working_builder_bcv2_true_missing_or_not_loadable": 288,
  "working_builder_bcv2_update_received_other_pubkey_only": 294,
  "working_builder_bcv2_update_received_unmapped": 111
}
```

Uwaga transportowa: exact watch payload cap zaczął przycinać BCV2 exact accounts pod koniec runu:

```text
tracked_bcv2 = 914
bcv2_sent = 199
bcv2_dropped = 715
```

To nie jest panic/watchdog, ale jest osobny sygnał pojemnościowy dla kolejnego etapu.

## 2. Role-Aware Evidence

Probe working-builder evidence:

| metryka | wartość |
| --- | ---: |
| `working_builder_bcv2_evidence_rows` | 354 |
| `working_builder_bcv2_evidence_ready_rows` | 36 |
| `working_builder_bcv2_evidence_conflict_rows` | 354 |
| `working_builder_bcv2_evidence_owner_rows` | 36 |
| `working_builder_bcv2_evidence_data_len_rows` | 36 |
| `working_builder_bcv2_evidence_slot_rows` | 354 |
| `working_builder_bcv2_evidence_context_slot_rows` | 0 |

Raw evidence status/source:

```json
{
  "status_counts": {
    "account_update_received": 4,
    "discovery_hint": 13,
    "rpc_missing": 334,
    "subscription_requested": 3
  },
  "source_counts": {
    "exact_watch_registered": 3,
    "observed_tx_meta": 13,
    "rpc_hydration": 334,
    "yellowstone_account_update": 4
  },
  "reason_counts": {
    "missing_on_rpc": 332,
    "positive_account_update_received_conflicts_with_negative_rpc_missing": 10,
    "positive_subscription_requested_conflicts_with_negative_rpc_missing": 10,
    "timeout": 2
  }
}
```

Execution evidence gate:

| metryka | wartość |
| --- | ---: |
| `working_builder_bcv2_execution_evidence_ready_rows` | 0 |
| `working_builder_bcv2_execution_evidence_exact_pubkey_match_rows` | 354 |
| `working_builder_bcv2_execution_evidence_stale_rows` | 0 |
| `working_builder_bcv2_execution_evidence_conflict_rows` | 354 |

Execution evidence status/source/reason:

```json
{
  "status_counts": {
    "account_update_received": 4,
    "discovery_hint": 13,
    "rpc_missing": 334,
    "subscription_requested": 3
  },
  "source_counts": {
    "exact_watch_registered": 3,
    "observed_tx_meta": 13,
    "rpc_hydration": 334,
    "yellowstone_account_update": 4
  },
  "reason_counts": {
    "account_update_received_not_execution_load_ready": 4,
    "missing_on_rpc": 332,
    "not_execution_load_ready:discovery_hint": 13,
    "not_execution_load_ready:subscription_requested": 3,
    "timeout": 2
  }
}
```

Kluczowy wynik PR6: raw PR4 evidence mogło mieć `ready=true` dla account-update evidence, ale execution readiness pozostało `0`, ponieważ `AccountUpdateReceived`, `ObservedTxMeta` i `ExactWatchRegistered` nie są execution-load-ready.

## 3. Readiness / Execution

| metryka | wartość |
| --- | ---: |
| `decision_rows_total` | 646 |
| `probe_selected_rows` | 354 |
| `working_builder_manifest_ready_rows` | 0 |
| `working_builder_manifest_missing_required_rows` | 354 |
| `successful_probe_entry_rows` | 0 |
| `active_shadow_successful_entry_rows` | 0 |
| `lifecycle_eligible_rows` | 0 |
| `active_shadow_lifecycle_eligible_rows` | 0 |
| `post_simulation_account_not_found_rows` | 0 |
| `active_shadow_account_not_found_rows` | 0 |
| `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows` | 0 |

Execution feasibility:

```json
{
  "route_executable_rows": 0,
  "route_non_executable_rows": 405,
  "execution_feasibility_reject_rows": 405,
  "active_buy_execution_infeasible_rows": 51,
  "probe_execution_feasibility_status_counts": {
    "not_executable_route": 354,
    "unknown": 275
  },
  "active_shadow_execution_feasibility_status_counts": {
    "not_executable_route": 51
  }
}
```

Active-shadow path:

```json
{
  "active_shadow_transport_rows": 17,
  "active_shadow_entry_rows": 17,
  "active_shadow_lifecycle_rows": 17,
  "active_shadow_dispatch_failure_rows": 51,
  "active_shadow_precheck_failed_rows": 51,
  "active_shadow_working_builder_manifest_ready_rows": 0,
  "active_shadow_working_builder_manifest_missing_required_rows": 51,
  "active_shadow_working_builder_bcv2_rpc_fetch_missing_rows": 51,
  "active_shadow_working_builder_bcv2_rpc_fetch_ready_rows": 0
}
```

Interpretacja: to nie jest PASS-A, bo nie ma manifest-ready, successful entries ani lifecycle-eligible rows. To jest poprawny fail-closed z konkretną dominującą klasą: `missing_on_rpc` / `rpc_missing`.

## 4. Safety Invariants

| invariant | wartość |
| --- | ---: |
| `legacy_buy_route_attempted_rows` | 0 |
| `legacy_fallback_attempted_rows` | 0 |
| `selected_route_handoff_mismatch_rows` | 0 |
| `active_shadow_legacy_buy_route_attempted_rows` | 0 |
| `active_shadow_legacy_fallback_attempted_rows` | 0 |
| `active_shadow_selected_route_handoff_mismatch_rows` | 0 |
| `active_shadow_route_fallback_attempted_rows` | 0 |
| `active_shadow_probe_working_builder_selected_legacy_handoff_rows` | 0 |
| `send_transaction` markers | 0 |
| `LiveTxSender::send_transaction` markers | 0 |
| `SUBMITTED` markers | 0 |
| `panic` markers | 0 |
| `WATCHDOG FATAL` markers | 0 |

## Transport / Runtime Stability

Poprzedni blocker:

```text
panic: user-provided comparison function does not correctly implement a total order
Ghost/Pump transport all workers exited
WATCHDOG FATAL: gRPC stalled
```

W tym 30-min smoke nie wrócił:

```text
panic rows = 0
WATCHDOG FATAL rows = 0
all workers exited rows = 0
comparison function rows = 0
```

Run zakończył się w oknie 30-min timeout, a końcówka logów nadal zawierała aktywny ingest, `BCV2_EXACT_WATCH_REGISTERED`, `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED`, `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT`, account updates oraz `DIAG_EXECUTION_ACCOUNT_EVIDENCE_UPSERT`.

## Decyzja Operacyjna

```text
GO: uznać PR7A transport panic repair za smoke-validated na 30-min próbie.
GO: uznać PR6 gate za runtime-exercised fail-closed.
GO: zamknąć X8C-S2 jako PASS-B-EVIDENCE / PASS-TRANSPORT-STABILITY.
NO-GO: PASS-A.
NO-GO: R18.
NO-GO: P2/live/Sender.
NO-GO: Gatekeeper/scoring/threshold tuning.
NO-GO: legacy_buy/fallback.
```

## Następny Krok

Ponieważ runtime był stabilny, a `RpcReady = 0` przy `RpcMissing` dominującym i exact pubkey match zachowanym, następny etap nie powinien już być naprawą comparatora. Następny etap powinien być:

```text
P3.7-X8D — BCV2 RpcMissing / Layout / Provider Truth Audit
```

Zakres X8D powinien rozstrzygnąć, czy BCV2 oznaczone przez observed route-compatible tx są faktycznie `missing/not-loadable`, czy problem leży w commitment/timingu/providerze/hydration/layout. Osobno trzeba uwzględnić nowy sygnał pojemnościowy: `tracked_bcv2=914`, `bcv2_sent=199`, `bcv2_dropped=715`.
