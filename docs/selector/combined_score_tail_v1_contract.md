# combined_score_tail_v1 Contract

Status: frozen runtime diagnostic contract.

Scope: shadow-only selector diagnostics emitted as a sidecar. This contract is
not a Gatekeeper policy, not an execution readiness check, and not a live send
selector.

## Score

`combined_score_tail_v1` computes:

```text
score = count(pass_conditions)
```

The score has 12 independent conditions:

```text
1. current_market_cap_sol >= 42.6
2. bonding_progress_pct >= 42.5
3. flipper_presence_ratio < 0.4110
4. burst_ratio < 0.337
5. price_change_ratio >= 1.032
6. max_single_tx_price_impact_pct_observed >= 32.3
7. early_slot_volume_dominance_buy < 0.4152
8. timing_entropy >= 1.780
9. flip_ratio_10s < 0.3943
10. dev_tx_ratio < 0.088
11. sol_buy_ratio >= 0.571
12. buy_count >= 10
```

Missing or non-finite feature values are not passes. A missing value must be
reported in both:

```text
combined_score_tail_v1_failed_conditions
combined_score_tail_v1_missing_features
```

## Buckets

Frozen thresholds:

```text
observation_threshold = 9
strict_threshold = 10
ultra_strict_threshold = 11
```

Bucket mapping:

```text
score < 9   => below_observation
score >= 9  => observation
score >= 10 => strict
score >= 11 => ultra_strict
```

## Emission Contract

Each evaluation must emit at least:

```text
combined_score_tail_v1_score
combined_score_tail_v1_bucket
combined_score_tail_v1_passed_conditions
combined_score_tail_v1_failed_conditions
combined_score_tail_v1_missing_features
combined_score_tail_v1_observation_pass
combined_score_tail_v1_strict_pass
combined_score_tail_v1_ultra_strict_pass
score_contract_hash
changes_gatekeeper_decision = false
changes_execution = false
send_path_changed = false
```

`score_contract_hash` is a BLAKE3 hash of the frozen condition list, thresholds,
and claim boundaries embedded in runtime code.

## Boundaries

This score must not:

```text
- change BUY / REJECT / TIMEOUT
- change Gatekeeper policy
- change execution path
- change send path
- change live rollout configs
- use min_phases_to_pass as a selector
- change thresholds of existing Gatekeeper phases
- act as alpha, prosperity, sybil, or IWIM veto
- claim live execution readiness
```

The condition:

```text
max_single_tx_price_impact_pct_observed >= 32.3
```

is intentionally an observed-tail condition with a lower-bound direction. It
must not be implemented as, or interpreted as, the existing Gatekeeper cap
`max_single_tx_price_impact_pct`.

## Explicit Exclusions

The following are not part of `combined_score_tail_v1`:

```text
- FSC / funding-source features
- funding graph features
- vectors_d_price
- live execution readiness
- route readiness
- transaction construction validity
- simulation success
- confirmation or lifecycle labels
```

The score is diagnostic evidence only. It may support offline analysis, but it
does not approve a pool for live execution and does not unlock send-path logic.
