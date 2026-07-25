# AccountObservationArbiter differential corpora v1 and v2

This directory is the hard pre-implementation gate for PR1C.  The corpus is
deliberately transport-neutral: each JSONL row describes one ordered series of
accepted `AccountUpdate` observations and the single decision required for
each observation.

`account_key`, `base_mint_key`, and `signature_tag` are stable fixture labels.
The replay test deterministically maps them to `Pubkey` / `Signature` values;
the labels are not runtime identifiers.  `data_hash_blake3` is a 32-byte,
lower-case hexadecimal BLAKE3 digest and is the payload identity used by the
arbiter.

The files are intentionally append-forbidden: changing an existing row or
adding a row requires an explicit corpus-version bump and a new recorded
digest in the replay test. This prevents a unit-test-only implementation from
silently redefining duplicate, ordering, or provider-conflict semantics.

`v1` remains byte-frozen as the original pre-implementation corpus. Its
`secondary_first_then_primary_conflicts` expected outcome exposed a review
defect: it incorrectly let a secondary-first conflicting witness veto the
later raw primary. `v1` is therefore retained as historical evidence and its
digest/inventory are still verified, but it is not used as the behavioral
oracle.

`v2` repeats the complete hard-gate inventory and is the executable PR1C
oracle. Its corrected secondary-first conflict rule is:

```text
secondary (version V, hash B) -> witness only
primary   (version V, hash A) -> apply primary A exactly once
                              + retain primary/secondary conflict evidence
```

Thus a secondary provider cannot become a veto authority through arrival
order. The raw primary remains the only writer of canonical state.

Coverage in both corpus versions:

- exact one-provider duplicate and reconnect/replay duplicate;
- identical primary/secondary observation and primary-secondary agreement;
- same version + same hash, including different transaction signatures;
- same version + different hash conflict;
- older write version after newer write version;
- `write_version = None`, including same-version duplicate;
- `None` versus `Some(0)` without coercing unknown to zero;
- secondary-first and conflicting-secondary-first behavior;
- several ordered mutations of the same account.

The corpus contains no NLN, Observation Ledger, quote, Gatekeeper, strategy,
or execution data.  Those boundaries belong to later PRs.
