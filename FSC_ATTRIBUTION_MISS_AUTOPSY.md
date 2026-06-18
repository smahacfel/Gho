# FSC Attribution Miss Autopsy

## Purpose

Goal: explain why `FSC_NO_RETAINED_RECIPIENT_HISTORY` dominates despite full-chain lane and larger R26B caps.

This autopsy is offline-only. No R26C/R27 canary was started. No Gatekeeper policy, execution, send path, veto, or score behavior was changed.

## Inputs Checked

R26:

- rollout: `shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation`
- decisions: `logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions/.../gatekeeper_v2_decisions.jsonl`
- BUY rows: `logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions/.../gatekeeper_v2_buys.jsonl`
- capture: `logs/nln_capture/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/`

R26B:

- rollout: `shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary`
- decisions: `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/decisions/.../gatekeeper_v2_decisions.jsonl`
- BUY rows: `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/decisions/.../gatekeeper_v2_buys.jsonl`
- capture: `logs/nln_capture/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/`

Code inspected:

- `ghost-launcher/src/tx_intelligence/funding_source.rs`
- `ghost-launcher/src/components/seer.rs`

## Lookup Key Audit

The code path does not look up FSC by mint, pool, ATA, creator, or bonding curve account.

Observed code behavior:

- `compute_for_transactions_at()` builds the buyer cohort from `unique_successful_buyers()` and then calls `lookup_source_for_buy()` for each buyer transaction.
- `lookup_source_for_buy()` calls `funding_lookup_wallets(tx)`.
- `funding_lookup_wallets(tx)` first uses positive `owner_token_deltas[].owner`, then falls back to `tx.signer`.
- `canonical_buyer_identity(tx)` uses the first wallet from `funding_lookup_wallets(tx)`.
- `observe_transfer()` stores funding transfer history by `transfer.recipient_wallet`.
- `lookup_source_for_wallet()` reads `inner.histories[wallet]`.

Line references from current code:

- store by recipient wallet: `ghost-launcher/src/tx_intelligence/funding_source.rs:897`
- history key is `transfer.recipient_wallet`: `ghost-launcher/src/tx_intelligence/funding_source.rs:915`
- buyer lookup loop: `ghost-launcher/src/tx_intelligence/funding_source.rs:1128`
- `lookup_source_for_buy()`: `ghost-launcher/src/tx_intelligence/funding_source.rs:1460`
- `funding_lookup_wallets()`: `ghost-launcher/src/tx_intelligence/funding_source.rs:2014`
- owner-token-delta owner first, signer fallback: `ghost-launcher/src/tx_intelligence/funding_source.rs:2018`
- no-history miss: `ghost-launcher/src/tx_intelligence/funding_source.rs:1795`

Conclusion: the intended key is buyer/user wallet. The code-level lookup key appears directionally correct. However, the artifacts do not let us prove that the buyer wallet emitted by Pump.fun parsing equals the recipient wallet emitted by SystemTransfers parsing for actual R26/R26B rows, because buyer wallet lists are not serialized in the decision rows and raw funding transfer rows were not persisted.

## Raw Funding Availability

Capture artifact counts:

```text
R26:
funding_events_v1.jsonl                 0
system_transfers_raw_v1.jsonl           0
nln_pumpfun_buy_exact_sol_in_raw_v1     15101
nln_pumpfun_buy_raw_v1                  36767
raw_pumpfun_instruction_evidence_v1     95693
route_manifest_evidence_candidates_v1   51871

R26B:
funding_events_v1.jsonl                 0
system_transfers_raw_v1.jsonl           0
nln_pumpfun_buy_exact_sol_in_raw_v1     3297
nln_pumpfun_buy_raw_v1                  8550
raw_pumpfun_instruction_evidence_v1     31385
route_manifest_evidence_candidates_v1   11847
```

R26B runtime metrics prove the funding lane itself was active:

```text
seer_events_received_total{source="grpc_funding_lane_full_chain"} = 2924634
fsc_index_entries = 12403
fsc_lookup_hits_total = 229
fsc_lookup_misses_total = 4106
```

So this is not evidence that the runtime funding lane was absent. It is evidence that durable raw transfer capture for offline attribution is missing or not sampling/storing the transfer rows needed for this autopsy.

Relevant writer/code path:

- artifact files are opened for `system_transfers_raw_v1.jsonl` and `funding_events_v1.jsonl`: `ghost-launcher/src/components/seer.rs:1021`
- writer handles `SystemTransfersRaw`: `ghost-launcher/src/components/seer.rs:1077`
- writer handles `FundingEvent`: `ghost-launcher/src/components/seer.rs:1089`
- SystemTransfers branch converts to native funding transfer and may write `FundingEvent`: `ghost-launcher/src/components/seer.rs:1861`

The persisted result is still zero transfer rows in both R26 and R26B.

## R26 Vs R26B Coverage

R26 decisions:

- valid rows: 2907
- clean/degraded/unavailable: 36 / 2207 / 664
- clean decision FSC coverage: 36 / 2907 = 1.24%
- `funding_source_concentration` non-null: 36
- summed buyer samples: 15628
- summed known buyers: 820
- summed unknown buyers: 14808

R26 BUY rows:

- valid rows: 354
- clean/degraded/unavailable: 6 / 348 / 0
- clean BUY FSC coverage: 6 / 354 = 1.69%
- summed BUY buyer samples: 6356
- summed BUY known buyers: 303
- summed BUY unknown buyers: 6053

R26B decisions:

- valid rows after pre-start exclusion: 763
- clean/degraded/unavailable: 5 / 606 / 152
- clean decision FSC coverage: 5 / 763 = 0.66%
- `funding_source_concentration` non-null: 5
- summed buyer samples: 4478
- summed known buyers: 232
- summed unknown buyers: 4246

R26B BUY rows:

- valid rows after pre-start exclusion: 136
- clean/degraded/unavailable: 4 / 132 / 0
- clean BUY FSC coverage: 4 / 136 = 2.94%
- summed BUY buyer samples: 2265
- summed BUY known buyers: 123
- summed BUY unknown buyers: 2142

Interpretation:

- R26B removed global cap evictions mechanically.
- Per-recipient overflow pressure dropped but did not disappear.
- Coverage did not reach the target and lookup hit rate did not improve.
- Therefore global cap/per-recipient cap were not the main blocker.

## Miss Reason Breakdown

R26 decision top misses:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 13942
FSC_GLOBAL_RECIPIENT_EVICTED = 340
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 189
FSC_RELATIVE_FUNDING_TOO_SMALL = 169
FSC_ABS_ATTRIBUTION_TOO_SMALL = 125
FSC_LOW_ATTRIBUTION_CONFIDENCE = 33
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 10
```

R26 BUY top misses:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 5677
FSC_GLOBAL_RECIPIENT_EVICTED = 132
FSC_RELATIVE_FUNDING_TOO_SMALL = 88
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 75
FSC_ABS_ATTRIBUTION_TOO_SMALL = 68
FSC_LOW_ATTRIBUTION_CONFIDENCE = 8
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 5
```

R26B decision top misses:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 4123
FSC_ABS_ATTRIBUTION_TOO_SMALL = 63
FSC_RELATIVE_FUNDING_TOO_SMALL = 54
FSC_LOW_ATTRIBUTION_CONFIDENCE = 4
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 2
```

R26B BUY top misses:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 2070
FSC_ABS_ATTRIBUTION_TOO_SMALL = 38
FSC_RELATIVE_FUNDING_TOO_SMALL = 32
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 2
```

R26B runtime top misses:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 3990
FSC_ABS_ATTRIBUTION_TOO_SMALL = 58
FSC_RELATIVE_FUNDING_TOO_SMALL = 52
FSC_LOW_ATTRIBUTION_CONFIDENCE = 4
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 2
```

## Offline Buyer Funding Search

Requested windows:

- 5 min
- 15 min
- 30 min
- 60 min

Result: not computable from current durable artifacts.

Reason:

- decision and BUY rows serialize aggregate FSC diagnostics, not the actual buyer wallet list used by `funding_lookup_wallets()`.
- R26 and R26B `funding_events_v1.jsonl` contain 0 rows.
- R26 and R26B `system_transfers_raw_v1.jsonl` contain 0 rows.

Representative rows demonstrate the limitation:

| run | row_type | pool_id | decision_ts_ms | buyer_wallet | found_inbound_transfer_5m | found_inbound_transfer_15m | found_inbound_transfer_30m | found_inbound_transfer_60m | latest_funding_age_ms | funding_amount_lamports | source_wallet | miss_reason |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R26B | BUY | ARmHbDbbebD7pkXrNrivft57KUK23dNAgmYsmGU9AuHN | 1781346786745 | NOT_SERIALIZED_IN_DECISION_ROW | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | NOT_COMPUTABLE | NOT_COMPUTABLE | NOT_COMPUTABLE | FSC_NO_RETAINED_RECIPIENT_HISTORY |
| R26B | BUY | Ev4qqagNfHptwgfyV4cRCocXA85vWFnmnhJ24osSXpCk | 1781346817912 | NOT_SERIALIZED_IN_DECISION_ROW | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | NOT_COMPUTABLE | NOT_COMPUTABLE | NOT_COMPUTABLE | FSC_NO_RETAINED_RECIPIENT_HISTORY |
| R26B | BUY | BG8MDyTpt8rcVhx1MpRT78E7NxmWTnEE55jCn31M6nSb | 1781346820964 | NOT_SERIALIZED_IN_DECISION_ROW | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | NOT_COMPUTABLE | NOT_COMPUTABLE | NOT_COMPUTABLE | FSC_NO_RETAINED_RECIPIENT_HISTORY |
| R26B | decision | AM4B1ReoxZ5o9LuLCWVTTt6oJ7NkPJNqjnTm9q6Rv57E | 1781346751528 | NOT_SERIALIZED_IN_DECISION_ROW | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | NOT_COMPUTABLE | NOT_COMPUTABLE | NOT_COMPUTABLE | FSC_NO_RETAINED_RECIPIENT_HISTORY |
| R26 | BUY | FddajhEpSMn3Qc6YP72HVjuhBWyr19X9DyoT8ZR3Tgrn | 1781301980470 | NOT_SERIALIZED_IN_DECISION_ROW | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | UNVERIFIABLE_NO_RAW_FUNDING_ARTIFACT | NOT_COMPUTABLE | NOT_COMPUTABLE | NOT_COMPUTABLE | FSC_NO_RETAINED_RECIPIENT_HISTORY |

This is a hard evidence limitation, not a claim that no funding existed within 60 minutes.

## Subreason Classification

`FSC_NO_RETAINED_RECIPIENT_HISTORY` means the lookup wallet had no retained entry in `inner.histories` and was not globally evicted. In R26B, global cap eviction is no longer the main blocker because `fsc_index_global_cap_evictions_total = 0`.

Subreason status:

| subreason | R26B assessment |
| --- | --- |
| `NO_INBOUND_TRANSFER_OBSERVED` | Not provable. Raw transfer artifacts are empty. Runtime lane was active globally, but no per-buyer persisted join exists. |
| `INBOUND_EXISTS_BUT_OLDER_THAN_300S` | Not computable. Need durable funding transfers or a longer wallet profile. |
| `INBOUND_EXISTS_BUT_PRUNED_BY_WINDOW` | Possible, not proven. `fsc_index_window_prunes_total = 137223`, but there is no per-buyer raw evidence to connect prunes to BUY/decision misses. |
| `INBOUND_EXISTS_BUT_BELOW_ABS_THRESHOLD` | Secondary, not dominant. R26B decision count 63; BUY count 38; runtime count 58. |
| `INBOUND_EXISTS_BUT_BELOW_REL_THRESHOLD` | Secondary, not dominant. R26B decision count 54; BUY count 32; runtime count 52. |
| `ADDRESS_KEY_MISMATCH` | Not proven by code. Code uses buyer/user wallet semantics, but actual Pump.fun buyer wallet vs SystemTransfer recipient equality cannot be verified from persisted artifacts. |
| `SAME_SLOT_ORDERING` | Not dominant in R26B. It was visible in R26 but absent from R26B top misses. |
| `PARSER_DID_NOT_STORE_TRANSFER_KIND` | Confirmed for durable capture artifacts: raw transfer/funding event files are empty in both R26 and R26B. Not the same as saying runtime index stored nothing, because R26B had index entries and lookup hits. |
| `UNKNOWN` | Residual dominant bucket after cap fix. Requires buyer-wallet and raw funding transfer persistence to split. |

## Threshold Check

Thresholds observed:

```text
min_abs_store_lamports = 1000000
min_abs_attribution_lamports = 10000000
min_rel_to_buy = 0.20
lookback_window_s = 300
```

Answer by candidate cause:

- too short window: possible but not proven. R26B has high window prune count and a 300s TTL, but there is no raw per-buyer transfer history to prove that most funding exists at 15/30/60 minutes.
- thresholds too strict: not the main blocker. ABS/REL threshold misses are small compared with `FSC_NO_RETAINED_RECIPIENT_HISTORY`.
- wrong lookup key: no obvious code bug found. The implemented key is buyer owner wallet with signer fallback. Actual parser-to-transfer key equality is unverified because durable artifacts do not contain the needed raw transfer rows and decisions do not serialize buyer wallet candidates.
- no transfers in stream: not supported. R26B full-chain lane had 2.9M funding-lane raw events and 12,403 index entries. The issue is not global lane absence.
- sparse direct funding evidence: possible. Current direct single-hop, 300s retained-history model may be intrinsically sparse for these buyers, but this cannot be separated from lookup/capture semantics without persisted transfer joins.

## Why Retention Delta Did Not Fix Coverage

The dominant R26B miss remains no retained recipient history, while:

- global cap evictions were zero,
- per-recipient cap was increased,
- lookup hit rate stayed about 5.28%,
- clean coverage stayed far below target,
- threshold misses stayed secondary,
- raw transfer artifacts were empty.

This points away from cap pressure as the first-order bottleneck and toward attribution-window, lookup-semantics, or durable-capture semantics.

## Next Concrete Variant

Recommended next variant: `FSC_LOOKUP_KEY_BUG_FIX`

Scope clarification: this should be treated as a lookup/capture semantics fix or audit fix, not as policy use and not as another canary in the dark.

Required contents before any R26C/R27:

- persist enough full-chain funding transfer evidence to make `buyer_wallet -> recipient_wallet` joins auditable;
- persist or sample the actual `funding_lookup_wallets(tx)` candidates for BUY/relevant decision rows;
- verify whether Pump.fun buyer owner/signature equals SystemTransfer recipient for the same wallets;
- make the offline 5/15/30/60 minute join computable from durable artifacts;
- keep Gatekeeper policy unchanged;
- keep execution unchanged;
- keep send path unchanged;
- do not add FSC as veto/score.

Why not `R26C_FSC_LOOKBACK_WINDOW_CANARY` yet:

- we do not yet know whether the missing evidence is older than 300s;
- raw funding transfer capture is empty, so a longer-window canary would still be partly blind;
- increasing retention without proving key/capture semantics already failed to improve coverage in R26B.

If `FSC_LOOKUP_KEY_BUG_FIX` proves that funding exists but falls outside 300s, the next canary should become `R26C_FSC_LOOKBACK_WINDOW_CANARY` with `lookback_window_s = 1800` or `3600` and a larger global cap. If it proves direct single-hop funding is inherently sparse, the next model should be `FSC_WALLET_ATTRIBUTION_PROFILE_CACHE`:

```text
wallet -> top funding sources, last_seen_ts, total_lamports, count, confidence
```

That profile model would target coverage without retaining a giant raw transfer history.

## Autopsy Verdict

```text
FSC_COVERAGE_NOT_FIXED_BY_RETENTION_DELTA
GLOBAL_CAP_NOT_MAIN_BLOCKER
PER_RECIPIENT_CAP_NOT_MAIN_BLOCKER
THRESHOLDS_SECONDARY_NOT_DOMINANT
LOOKUP_KEY_CODE_PATH_APPEARS_BUYER_WALLET_BASED
OFFLINE_BUYER_FUNDING_JOIN_BLOCKED_BY_MISSING_DURABLE_TRANSFER_ARTIFACTS
NEXT_BOTTLENECK_ATTRIBUTION_WINDOW_OR_LOOKUP_SEMANTICS
NEXT_VARIANT_FSC_LOOKUP_KEY_BUG_FIX
```

