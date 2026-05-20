# RAPORT P3.7-J3R2 Counterfactual Probe Simulation and Hash Continuity Repair

Date: 2026-05-20

Base HEAD: `cd520fc` (`Implement P3.7-J3R probe runtime repair`)

## Verdict

`P3.7-J3R2 code-level repair: PASS`

R15-r2 pozostaje `NOT_READY`. Ten etap nie claimuje runtime smoke PASS i nie
autoryzuje collection runu.

Status po J3R2:

```text
R15-r2 runtime smoke: NOT_READY
J3R2 code-level repair: PASS
R15-r3 bounded runtime smoke: next gate
Full/bounded collection: HOLD
Phase B V3/MFS lifecycle feature prototype: HOLD
P2/live/tuning: NO-GO
```

## Problem

R15-r2 wykazał dwa twarde blokery przed bounded collection:

- probe selection/transport exact decision/V3 continuity tylko `1/5`;
- wszystkie `5/5` probe simulations zakończyły się
  `AccountNotFound` / `data_problem`, bez probe entry rows.

Dodatkowo diagnostyka `AccountNotFound` nie identyfikowała brakującego konta:

- `simulation_error_account_pubkey=null`;
- `simulation_error_account_role=null`.

## Implemented Changes

### Post-Serialize V3 Hash Boundary

`P37ShadowProbeCandidate` wylicza teraz `v3_feature_snapshot_hash` przez tę
samą granicę post-serialize JSON, którą DecisionLogger stosuje dla
persisted decision rows.

Zakres:

- `ghost-launcher/src/oracle_runtime.rs`
- funkcja `p37_shadow_probe_serialized_replay_payload_hash(...)`

Semantyka:

- aktywne logowanie DecisionLoggera nie zostało zmienione;
- probe metadata przestaje ufać wcześniejszemu in-memory/pre-serialize hash;
- selection i transport powinny w kolejnym smoke exact-joinować do persisted
  decision/V3 row przez `ab_record_id` + V3 feature/policy hash.

### Probe Required-Account Precheck

Dodano probe-only precheck po zbudowaniu `PreparedBuyRequest` i przed wywołaniem
`shadow_simulator.simulate_buy(...)`.

Zakres:

- `ghost-launcher/src/components/trigger/component.rs`
- `CounterfactualProbeMissingAccount`
- `TriggerComponent::counterfactual_probe_missing_required_account(...)`
- `TriggerComponent::counterfactual_probe_required_account_roles(...)`
- `ghost-launcher/src/oracle_runtime.rs`
- `run_p37_shadow_probe_dispatch(...)`

Precheck sprawdza przygotowany zestaw kont transakcji oraz jawne tożsamości z
requestu. Jeżeli konto jest znane jako brakujące, probe nie przechodzi do
symulacji, tylko zapisuje `probe_skipped`:

```text
probe_skip_reason = probe_execution_precheck_failed
precheck_failure_reason = missing_required_account:<role>:<pubkey>
```

To ma zamienić znane przypadki `AccountNotFound` z nieopisanych
`data_problem` na precyzyjny `NOT_READY_DIAGNOSED`.

Precheck nie traktuje brakującego user ATA jako fatalnego, jeśli przygotowana
transakcja ma idempotentne tworzenie ATA (`attach_idempotent_ata_create=true`
i `ata_missing_pre_submit=true`).

Jeżeli sam precheck ma błąd RPC inny niż `AccountNotFound`, runtime loguje
ostrzeżenie i kontynuuje symulację. To zachowuje fail-open wobec active
decision path i nie mutuje aktywnego verdictu.

## Non-Goals Preserved

J3R2 nie zmienia:

- active Gatekeeper verdicts;
- IWIM;
- runtime thresholds;
- live sender;
- P2/live behavior;
- active BUY semantics;
- MFS schema;
- DecisionLogger active hash semantics.

Probe rows nadal są `counterfactual_shadow_probe`, nie BUY.

## Validation

Uruchomione komendy:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Wyniki:

- `p37_shadow_probe`: `21/21 PASS`;
- `p37_counterfactual_probe`: `5/5 PASS`;
- Python join-key audit unittest: `6/6 PASS`;
- Python compile: `PASS`.

## Next Gate

Następny krok to świeży bounded runtime smoke, np. `R15-r3`, w czystym
namespace.

Minimalne oczekiwane rozstrzygnięcie po R15-r3:

- jeśli probe transport exact decision/V3 continuity = `100%` i powstaną
  probe entry rows: `PASS minimalny`;
- jeśli transport exact join = `100%`, ale probe rows są skipowane przez
  `missing_required_account:<role>:<pubkey>`: `NOT_READY_DIAGNOSED`;
- jeśli nadal występuje hash mismatch albo nieopisany `AccountNotFound`:
  `FAIL / repair required`.

Collection pozostaje HOLD do czasu R15-r3 PASS.
