# Pump Observation Ledger differential corpus v1

Status: `FROZEN PRE-IMPLEMENTATION HARD GATE`

This directory freezes the PR1D raw Yellowstone / parsed NLN differential
contract before the production `PumpObservationLedgerV1` implementation
exists. The corpus is transport-neutral and does not invoke a runtime parser,
ledger, `CandidateIntegrityRegistry`, MFS, Gatekeeper, quote math, or
execution code.

The fixture contains 33 scenarios. Every JSONL row contains:

- `schema_version = 1`;
- one stable `scenario_id`;
- an ordered series of accepted Pump observations;
- exactly one expected classification for each accepted observation;
- an optional deterministic correlation-finalize outcome;
- the exact expected canonical-mutation count, correlation outcome,
  candidate-integrity status, evidence completeness, and NLN canonical-event
  count;
- optional bounded-capacity, account-conflict handoff, or lifecycle inventory
  metadata.

## Frozen bytes

File:

```text
pump_observation_differential_corpus_v1.jsonl
```

Exact BLAKE3 of the complete file bytes, including each LF record terminator:

```text
833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4
```

The file is UTF-8, contains one compact JSON object per line, uses LF rather
than CRLF, and ends in LF. Existing v1 bytes are append-forbidden. A changed
or additional scenario requires a new corpus version and a separately
recorded digest; v1 must not be rewritten to make a later implementation
pass.

The per-observation `payload_hash_blake3` values are stable, lower-case,
32-byte fixture labels representing captured provider payload digests. The
raw and NLN representations intentionally use different hashes in agreement
scenarios. They are not cross-source semantic hashes.

## Frozen authority and identity rules

- Only `source_family = raw_yellowstone` together with
  `provider_role = primary_authority`, a complete locator, complete canonical
  order, explicit raw mutation inventory, and provenance may set
  `canonical_apply = true`.
- Parsed NLN and secondary raw observations are witness-only and always set
  `canonical_apply = false`.
- NLN-first and NLN-only create zero canonical events.
- Raw primary does not wait for NLN.
- Signature, claims, payload formatting, provider time, receive time, and
  receive sequence are not standalone identity or dedupe keys.
- `tx_index = 0` is a present value.
- Missing locator, order, or captured-payload provenance on a purported raw
  primary is `primary_raw_coverage_incomplete`, not a synthesized fact.
- One signature may contain several canonical Pump mutations.
- Unknown optional claims do not conflict with concrete claims.
- Witness saturation degrades evidence completeness but cannot veto a later
  eligible primary raw mutation.

`ExactStructuralMatch` requires the complete source-neutral locator.
`UniqueSignatureSingletonMatch` is frozen only as a deterministic finalize
outcome after the bounded window contains exactly one raw Pump mutation and
exactly one uncorrelated locatorless NLN witness. Raw applies immediately
before that finalize. Multiple raw mutations or multiple locatorless
witnesses make singleton selection unavailable.

## Scenario inventory

Authority, correlation, and ordering:

1. `raw_only_immediate`
2. `exact_raw_then_nln_agreement`
3. `exact_nln_then_raw_agreement`
4. `singleton_raw_then_nln_agreement`
5. `singleton_nln_then_raw_agreement`
6. `nln_only_expiry_unmatchable`
7. `multiple_raw_mutations_same_signature`
8. `locatorless_nln_ambiguous`
9. `exact_locator_among_multiple_raw`
10. `reconnect_raw_exact_duplicate`
11. `secondary_raw_same_payload_agreement`
12. `secondary_raw_different_payload_conflict`
13. `tx_index_zero_preserved`
14. `missing_primary_order_fail_closed`
15. `witness_saturation_no_primary_veto`
16. `create_and_initial_buy_same_signature`
17. `second_locatorless_nln_prevents_singleton`
18. `primary_raw_missing_locator_fail_closed`
19. `primary_raw_missing_provenance_fail_closed`

Semantic agreement/conflict:

20. `material_conflict_curve`
21. `material_conflict_mint`
22. `material_conflict_route_variant`
23. `material_conflict_side`
24. `material_conflict_success`
25. `material_conflict_token_amount_units`
26. `material_conflict_instruction_limit`
27. `material_conflict_reported_curve_quote_lamports`
28. `material_conflict_reported_wallet_delta_lamports`
29. `material_conflict_reported_fee_breakdown`
30. `unknown_claim_vs_concrete`
31. `cross_source_payload_hash_difference_agreement`

Cross-layer inventory metadata:

32. `account_provider_conflict_handoff`
33. `lifecycle_conflict_matrix`

The material-claim matrix covers every field of the frozen
`PumpMutationClaimsV1` contract. Reported quote, wallet-delta, and fee claims
remain provider observations and gain no quote-math authority.

The account handoff metadata freezes:

```text
SameVersionDifferentHashConflict
  -> CandidateIntegrity::AccountProviderConflict
  -> no account canonical apply
  -> no strategic verdict
```

The lifecycle inventory freezes all eight PR1D phases:

- `PRE_MFS`;
- `MFS_MATERIALIZED`;
- `EVALUATION_RUNNING`;
- `TERMINAL_REJECT`;
- `TERMINAL_TIMEOUT`;
- `TERMINAL_BUY_NOT_SUBMITTED`;
- `SUBMIT_STARTED`;
- `CONFIRMED_OPEN_POSITION`.

These are inventory expectations for later integration tests. They do not
implement lifecycle behavior inside `ghost-core`.

## Gate test

Run:

```text
cargo test -p ghost-core --test pump_observation_ledger_corpus_tests -- --nocapture
```

The pre-implementation test validates only:

- exact fixture digest and JSONL framing;
- strict serde schema with unknown fields rejected;
- scenario uniqueness and complete frozen inventory;
- one expected decision per observation;
- valid 32-byte lower-case provenance hashes;
- source/role canonical-authority restrictions;
- locator/order structural consistency;
- all ten material-claim conflict fields;
- Unknown semantics;
- cross-source payload-hash non-equivalence;
- exact and singleton arrival-order symmetry;
- multiple mutations per signature;
- `tx_index = 0`;
- fail-closed missing coverage;
- witness saturation without primary veto;
- account-conflict handoff metadata;
- complete lifecycle inventory.

Executable replay through the production ledger is added after the ledger
implementation without changing this fixture or its digest.
