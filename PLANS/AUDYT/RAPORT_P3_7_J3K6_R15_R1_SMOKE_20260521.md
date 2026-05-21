# RAPORT P3.7-J3K6 R15-R1 Creator Vault Authority Smoke

Date: 2026-05-21

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1.toml`

Runtime namespace:

`shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1`

## Verdict

`R15-r1 J3K6 smoke = NOT_READY_DIAGNOSED`

J3K6 creator-vault guard działa: rows z nieautorytatywnym `creator_vault` są zatrzymywane jako `probe_skipped` przed symulacją.

Smoke nie jest jednak entry-path PASS, ponieważ wszystkie dispatch/transport rows były `routed_exact_sol_in` i nie miały `entry_token_amount_raw`, więc zostały sklasyfikowane jako `transport_only_missing_token_quantity`.

Collection pozostaje `HOLD`.

Phase B / P2 / live / threshold tuning pozostają `NO-GO`.

## Runtime Status

Run zakończył się kontrolowanie. Proces `ghost-launcher` nie był aktywny po powrocie operatora.

Artifacts:

- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1/probe_selection.jsonl`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1/probe_skips.jsonl`
- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1/probe_transport.jsonl`

Missing as expected for this failed entry smoke:

- `shadow_entries.jsonl`
- `shadow_lifecycle.jsonl`
- `buys.jsonl`

## V3 Replay

`v3_shadow_report.py`:

- `status = ok`
- `v3_rows = 127`
- `bad_rows = 0`
- `full_snapshot_payload_rows = 127`
- `hash_only_rows = 0`
- `stale_against_config = false`
- policy hash coverage = 100%
- feature snapshot hash coverage = 100%

`v3_full_replay_report.py --strict`:

- `replay_status = full_replay_ok`
- `total_rows = 127`
- `v3_rows = 127`
- `bad_rows = 0`
- `full_replay_ok = 127`

## Probe Artifacts

JSONL parse:

- `probe_selection.jsonl`: 18 rows, 0 malformed
- `probe_skips.jsonl`: 612 rows, 0 malformed
- `probe_transport.jsonl`: 10 rows, 0 malformed
- `shadow_entries.jsonl`: missing
- `shadow_lifecycle.jsonl`: missing
- `buys.jsonl`: missing

Probe decision/V3 join:

- `probe_selection`: 18/18 exact decision/V3 join
- `probe_transport`: 10/10 exact decision/V3 join
- feature hash mismatches: 0
- policy hash mismatches: 0

Join-key audit:

- `probe_readiness = not_ready`
- `probe_join_key_acceptance = fail`
- reason: `missing_probe_entry_rows`
- `probe_transport_rows_with_ab_record_id = 10`
- `probe_entry_rows_with_ab_record_id = 0`
- `probe_transport_rows_with_probe_id = 10`
- `probe_entry_rows_with_probe_id = 0`

## Creator Vault Guard

J3K6 guard converted non-authoritative creator-vault cases into explicit skips:

- `probe_skip_reason = creator_vault_source_not_authoritative`: 545
- `creator_vault_authority_status = creator_vault_source_not_authoritative`: 545
- `creator_vault_mismatch_reason = creator_identity_source_not_authoritative`: 545
- `creator_identity_source = detected_pool.creator`: 545

This means J3K6 prevented the previously observed creator-vault ambiguity from reaching simulation.

No `custom_2006` rows were observed in the probe transport artifacts for this smoke.

## Current Blocker

All 10 probe transport rows were:

- `execution_outcome = counterfactual_shadow_probe_simulated`
- `buy_variant = routed_exact_sol_in`
- `token_param_role = min_tokens_out`
- `min_tokens_out = 1`
- `entry_token_amount_raw = null`
- `probe_entry_materialization_status = transport_only_missing_token_quantity`
- `probe_entry_materialization_reason = routed_exact_sol_in_entry_token_amount_raw_null`

Because no token quantity was available for `routed_exact_sol_in`, entry materialization did not produce `shadow_entries.jsonl`.

## Interpretation

J3K6 achieved its narrow repair goal:

- non-authoritative `creator_vault` does not proceed into simulation,
- creator-vault failures are now structured precheck skips,
- exact decision/V3 continuity remained 100% for selected and transported probes,
- active BUY artifacts were not created.

The next blocker is no longer creator-vault Custom(2006). It is routed exact-SOL-in entry materialization:

`routed_exact_sol_in` probe transports need either a simulation-derived token quantity for entry materialization or a strict skip/dispatch eligibility rule that prevents transport-only rows from consuming the entry-path smoke budget.

## Decision

`J3K6 code-level guard = PASS`

`R15-r1 runtime smoke = NOT_READY_DIAGNOSED`

Next narrow step:

`P3.7-J3K7 Routed Exact-SOL-In Entry Materialization / Dispatch Eligibility`

Required outcome for the next smoke:

- strict replay OK,
- creator-vault non-authoritative rows remain skipped,
- routed exact-SOL-in rows either materialize entry quantity or are skipped before consuming dispatch quota,
- probe entries > 0,
- exact decision/V3 join remains 100%,
- active BUY remains 0,
- no live/P2.
