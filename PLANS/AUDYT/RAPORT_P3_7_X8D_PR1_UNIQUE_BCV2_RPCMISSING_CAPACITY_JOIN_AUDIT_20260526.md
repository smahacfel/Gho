# P3.7-X8D-PR1 - Unique BCV2 RpcMissing / Capacity Join Audit

Data: 2026-05-26

## Status

Werdykt PR1:

```text
X8D-PR1: AUDIT-ONLY FORMALIZED
runtime/readiness changes: NO
R18: NO-GO
live/Sender: NO-GO
legacy/fallback revival: NO-GO
```

Ten raport formalizuje etap po X8C-S2. X8C-S2 zamknal stabilizacje transportu jako `PASS-TRANSPORT-STABILITY` i potwierdzil fail-closed PR6, ale nie odblokowal execution:

```text
working_builder_bcv2_execution_evidence_ready_rows = 0
working_builder_manifest_ready_rows = 0
successful_probe_entry_rows = 0
active_shadow_successful_entry_rows = 0
lifecycle_eligible_rows = 0
```

X8D-PR1 nie zmienia runtime. Celem jest przejscie z row-count summary na deduplikowany join po unikalnym `working_builder_bcv2_pubkey`.

## Zrodla

Smoke:

```text
shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke
```

Checkpoint repo:

```text
origin/main = 352db55 docs: add X8C-S2 smoke report
```

Artefakty X8C-S2:

```text
/tmp/gho-x8c-s2-pr7a-smoke/x8c_s2_audit.json
/tmp/gho-x8c-s2-pr7a-smoke/x8c_s2_audit.md
```

Formalne artefakty X8D-PR1 wygenerowane przez `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`:

```text
/tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_audit.json
/tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_audit.md
/tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_unique_bcv2_pubkey_join.formal.json
/tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_unique_bcv2_pubkey_join.formal.csv
```

Schemat joinu:

```text
x8d_pr1_unique_bcv2_pubkey_join_v1
```

## Zakres

In-scope:

- unique join po `working_builder_bcv2_pubkey`,
- rozdzielenie `rows` od `unique pubkeys`,
- join z exact-watch registered/included/resubscribe/drop markers,
- join z same-pubkey `BCV2_ACCOUNT_UPDATE_RECEIVED`,
- join z `BCV2_RPC_HYDRATION_READY/MISSING`,
- join z PR6 execution evidence fields,
- bucketizacja przyczyn blokady bez zmiany policy.

Out-of-scope:

- zmiany `AccountStateCore`,
- zmiany `AccountUpdateEvent`,
- Seer producer changes,
- OracleRuntime readiness changes,
- working-builder readiness policy changes,
- TX builder / Sender / Gatekeeper / scoring,
- R18 / live / submitted path,
- legacy/fallback revival.

## Wynik Glowny

Probe working-builder plane:

```text
working_builder_rows = 405
unique_bcv2_pubkeys = 354
```

Primary bucket split po unique BCV2:

```json
{
  "included_rpc_missing_no_same_update": 273,
  "registered_not_included_rpc_missing": 6,
  "same_pubkey_update_but_not_execution_ready": 75
}
```

Primary bucket split po row count:

```json
{
  "included_rpc_missing_no_same_update": 288,
  "registered_not_included_rpc_missing": 6,
  "same_pubkey_update_but_not_execution_ready": 111
}
```

Non-exclusive audit buckets po unique BCV2:

```json
{
  "dropped_over_cap": 6,
  "included_rpc_missing_no_same_update": 273,
  "registered_not_included_rpc_missing": 6,
  "same_pubkey_update_but_not_execution_ready": 75
}
```

## Evidence / Readiness

Per unique pubkey:

```text
registered_pubkeys = 354
included_in_subscribe_inferred_pubkeys = 348
same_pubkey_account_update_pubkeys = 75
hydration_ready_pubkeys = 0
hydration_missing_pubkeys = 354
execution_ready_pubkeys = 0
execution_evidence_exact_pubkey_match_pubkeys = 354
execution_evidence_conflict_pubkeys = 354
execution_evidence_stale_pubkeys = 0
```

Row-level execution evidence counts z working-builder rows:

```json
{
  "status": {
    "account_update_received": 4,
    "discovery_hint": 13,
    "missing": 51,
    "rpc_missing": 334,
    "subscription_requested": 3
  },
  "source": {
    "exact_watch_registered": 3,
    "missing": 51,
    "observed_tx_meta": 13,
    "rpc_hydration": 334,
    "yellowstone_account_update": 4
  },
  "reason": {
    "account_update_received_not_execution_load_ready": 4,
    "missing": 51,
    "missing_on_rpc": 332,
    "not_execution_load_ready:discovery_hint": 13,
    "not_execution_load_ready:subscription_requested": 3,
    "timeout": 2
  }
}
```

Interpretacja:

- Exact lookup po role-aware BCV2 dziala dla wszystkich `354` unique working-builder pubkeys.
- `RpcReady` nie wystapil dla zadnego z `354` pubkeyow.
- `AccountUpdateReceived`, `ObservedTxMeta` i `ExactWatchRegistered` nie odblokowaly readiness.
- PR6 zachowal fail-closed contract: `execution_evidence_ready=false` mimo raw evidence i exact pubkey match.

## Bucket A - Same-Pubkey Update, Ale Nie Execution-Ready

```text
same_pubkey_update_but_not_execution_ready = 75 unique pubkeys
working_builder_rows = 111
included_in_subscribe_inferred = 75 unique pubkeys
dropped_over_cap_inferred = 0 unique pubkeys
raw_evidence_ready_pubkeys = 36
execution_ready_pubkeys = 0
```

Znaczenie:

- Dla tych BCV2 istnieje globalny same-pubkey account update.
- PR6 slusznie nie traktuje `AccountUpdateReceived` jako execution-load-ready.
- Ten bucket jest kandydatem do pozniejszego layout/provider truth audit: czy update zawiera wystarczajace owner/data/layout, aby przyszly osobny PR mogl bezpiecznie tworzyc `PrecheckReady`, czy ma pozostac tylko diagnostyka.

## Bucket B - Included + RpcMissing, Bez Same-Pubkey Update

```text
included_rpc_missing_no_same_update = 273 unique pubkeys
working_builder_rows = 288
included_in_subscribe_inferred = 273 unique pubkeys
dropped_over_cap_inferred = 0 unique pubkeys
hydration_missing = 273 unique pubkeys
hydration_ready = 0 unique pubkeys
```

Znaczenie:

- Te pubkeye byly registered i auditowo inferred jako included/resubscribed.
- Nie ma same-pubkey account update.
- Hydration konczy jako `rpc_missing` / `timeout`.
- To jest glowny korpus dla X8D-PR2: provider/commitment/timing/layout truth audit.

## Bucket C - Registered, Ale Nie Included

```text
registered_not_included_rpc_missing = 6 unique pubkeys
working_builder_rows = 6
included_in_subscribe_inferred = 0 unique pubkeys
dropped_over_cap_inferred = 6 unique pubkeys
hydration_missing = 6 unique pubkeys
hydration_ready = 0 unique pubkeys
```

Znaczenie:

- Te pubkeye byly zarejestrowane, ale nie maja inferred include/resubscribe.
- Wszystkie sa powiazane z capacity-drop inference.
- To jest maly, ale wazny bucket: capacity moze znieksztalcac evidence dla koncowki requestu.

## Exact-Watch Capacity

Formalny capacity summary:

```json
{
  "drop_marker_rows": 3094,
  "max_tracked_bcv2": 914,
  "max_bcv2_sent": 199,
  "max_bcv2_dropped": 715,
  "max_exact_payload_cap": 199
}
```

Interpretacja:

- Capacity jest realnym czynnikiem runtime, nie pobocznym szumem.
- Dla working-builder pubkeyow w tym smoke tylko `6/354` unique pubkeys trafia do bucketu `registered_not_included_rpc_missing` oraz `dropped_over_cap`.
- Glowny korpus problemu pozostaje w bucketcie `included_rpc_missing_no_same_update = 273`, czyli sama capacity nie tlumaczy dominujacego `RpcMissing`.
- Capacity musi jednak wejsc do X8D-PR2 jako osobna oś kontroli, bo `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED` jest nadal atrybuowane auditowo, a nie bezposrednim markerem per pubkey.

## Decyzja

```text
GO: X8D-PR1 audit-only unique BCV2 join
GO: uzyc outputow JSON/CSV jako wejscia do X8D-PR2
NO-GO: R18
NO-GO: PASS-A
NO-GO: live/Sender
NO-GO: Gatekeeper/scoring/threshold tuning
NO-GO: legacy/fallback revival
```

X8D-PR1 zawęża problem do dwóch osi:

1. Hydration truth dla `273` included/resubscribed BCV2 bez same-pubkey update.
2. Capacity/selection truth dla `6` registered-but-not-included BCV2.

## Rekomendowany X8D-PR2

Nastepny PR powinien pozostac audit-only albo diagnostics-only:

```text
P3.7-X8D-PR2 - Multi-Commitment / Delayed Hydration Truth Audit
```

Zakres rekomendowany:

- dla unique `working_builder_bcv2_pubkey` z X8D-PR1 sprawdzic delayed hydration,
- rozdzielic `processed`, `confirmed`, `finalized`, jezeli konfiguracja/RPC to umozliwia,
- porownac timing: observed slot, precheck context slot, hydration attempt time, account update order,
- zachowac osobny bucket dla capacity-dropped pubkeys,
- nie zmieniac readiness policy bez twardego dowodu `RpcReady`/`PrecheckReady` equivalent.

Warunek przed jakimkolwiek readiness unlock pozostaje bez zmian:

```text
fresh exact ExecutionAccountEvidence(BondingCurveV2, pubkey)
status in {RpcReady, PrecheckReady}
owner present
data_len present
no newer negative evidence
freshness window satisfied
```

## Weryfikacja PR1

Komendy do odtworzenia:

```bash
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config /tmp/gho-x8c-s2-pr7a-smoke/shadow-burnin-v3-p37-x8c-s2-pr7a-comparator-smoke.toml \
  --output-json /tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_audit.json \
  --output-md /tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_audit.md \
  --output-bcv2-unique-json /tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_unique_bcv2_pubkey_join.formal.json \
  --output-bcv2-unique-csv /tmp/gho-x8c-s2-pr7a-smoke/x8d_pr1_unique_bcv2_pubkey_join.formal.csv
```

Targeted checks:

```bash
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
git diff --check -- scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py PLANS/AUDYT/RAPORT_P3_7_X8D_PR1_UNIQUE_BCV2_RPCMISSING_CAPACITY_JOIN_AUDIT_20260526.md
```
