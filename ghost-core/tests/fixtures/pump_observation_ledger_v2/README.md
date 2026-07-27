# Pump Observation Ledger differential corpus v2

Status: `FROZEN EXECUTABLE HARD GATE`

V2 is additive to the immutable V1 corpus. It freezes the material-claim
fields added after V1 (`error_code` and
`reported_post_state_hash_blake3`) plus the auditable
`SecondaryWitnessExpired` contract. Every record is replayed through the
production `PumpObservationLedgerV1`; V1 bytes and their digest remain
unchanged.

The exact BLAKE3 digest of
`pump_observation_differential_corpus_v2.jsonl`, including LF terminators, is:

```text
c81d7b4f0cc3792c2bb2c4e71bfd0634fcfdd69723758d741ee2405770603415
```

It is recorded by the corpus test and must be versioned rather than rewritten.
