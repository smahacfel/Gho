# ADR-0132: V3 P3.6 R13 Sample Threshold Closure

Date: 2026-05-18

Status: Accepted

## Context

P3.6 required a larger primary-only, shadow-only sample before drawing further calibration conclusions. Earlier small samples were not sufficient for stable reason/subtrigger and outcome-quality analysis.

R13 was launched under:

- rollout config: `configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml`
- namespace: `shadow-burnin-v3-p36-sample-r13-primary-only`
- mode: primary-only, shadow-only
- V3 replay payload: enabled
- V3 promotion: disabled
- active policy / IWIM / live sender: unchanged

The operational stop condition for this check was:

- if the fresh R13 V3 decision row count exceeds `1500`, stop the run and preserve it as sufficient P3.6 analysis material.

## Decision

R13 exceeded the requested sample threshold and was stopped intentionally.

Observed post-stop state:

- `v3_rows=2733`
- `raw_rows=2733`
- `deduped_rows=2733`
- `bad_rows=0`
- `duplicate_rows_removed=0`
- `replay_status=full`
- `full_snapshot_payload_rows=2733`
- `hash_only_rows=0`
- `stale_against_config=false`
- `v3_feature_snapshot_hash.coverage=1.0`
- `v3_policy_config_hash.coverage=1.0`
- `policy_hash_unique_count=1`
- `snapshot_hash_unique_count=2733`

Strict full replay result:

- `status=ok`
- `replay_status=full_replay_ok`
- `v3_rows=2733`
- `status_counts.full_replay_ok=2733`

R13 therefore provides enough replay-stable material for the next P3.6 analysis step.

## Stop Procedure

The run was first asked to stop through the tmux session with `Ctrl-C`.

Because the launcher process was still present after that request, it was then stopped with `SIGTERM`.

Post-stop checks:

- no active `ghost-launcher` process remained
- no tmux server/session remained for `p36_sample_r13`
- no panic/overflow/runtime-stop/queue-depth/replay-payload-mismatch markers were found in the launcher log

The launcher log contains repeated `Transport channel disconnected` warnings around shutdown. These are treated as shutdown/transport noise for this ADR because:

- the decision log was fresh
- V3 row count exceeded the target threshold
- strict full replay passed for all rows
- no runtime panic or queue-depth failure marker was found

## Current Distribution

V3 reason distribution:

- `REJECT_V3_MANIPULATION_CONTRADICTION=1928`
- `PENDING_V3_WAIT_EVIDENCE=673`
- `PENDING_V3_WAIT_SAMPLE=132`

V3 stage distribution:

- `RISK=1928`
- `EVIDENCE=805`

Confidence cap reasons:

- `hard_risk=1928`
- `insufficient_evidence=805`

Execution outcome fields remain non-success:

- `missing=2732`
- `shadow_data_problem=1`
- `success_count=0`

This ADR does not interpret outcome quality. Outcome labels must be generated separately before economic conclusions are drawn.

## Consequences

R13 should now be used as the main larger-sample input for P3.6 calibration analysis, together with prior R10/R11 evidence where appropriate.

The next step is not another blind runtime expansion. The next step is offline analysis over the captured full-replay rows:

- outcome labeling for R13
- combined R10/R11/R13 outcome-quality report
- reason/subtrigger decomposition
- PENDING evidence-group breakdown
- true full-replay ablation variants
- candidate P3.6 calibrated shadow profile evaluation

No P2 promotion is implied by this ADR.

No live execution change is implied by this ADR.

No active V2/V2.5, IWIM, or live sender behavior was changed.

## Invariants

- `MaterializedFeatureSet` remains the replay SSOT.
- V3 remains shadow-only.
- V3 promotion remains disabled.
- FSC remains de-scoped as an authoritative required dependency under ADR-0130.
- R13 is evidence for calibration analysis, not production promotion.
