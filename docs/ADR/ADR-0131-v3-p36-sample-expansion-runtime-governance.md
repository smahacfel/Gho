# ADR-0131: V3 P3.6 Sample Expansion Runtime Governance

Date: 2026-05-17

Status: Accepted

## Context

P3.6 is the current V3 shadow-only calibration phase. Earlier P3.6 offline
analysis over R10+R11 showed that the current V3 stack has useful replay and
outcome tooling, but the candidate `p36_candidate_organic_relaxed` produced only
23 BUY candidates:

- `bad_entry=12`
- `good_entry=9`
- `neutral_entry=2`

That sample is too small for threshold tuning, precision estimation, or
promotion. It is still enough to falsify that specific candidate, because it
unblocked more bad entries than good entries and violated the P3.6 safety gate:
`bad_unblocked <= 0.5 * good_unblocked`.

The next operational need is therefore larger primary-only sample collection,
not P2 promotion and not another small offline-only conclusion.

## Current Runtime Constraint

The provider exposes only one usable Yellowstone/Geyser stream for the current
endpoint. There is no dedicated stream for FSC/full-chain funding collection.

Therefore, current V3 validation must remain primary-only:

- `funding_lane_mode="disabled"`
- no authoritative FSC dependency
- no negative interpretation of missing/degraded FSC evidence
- no attempt to run a second concurrent funding-chain stream

This preserves ADR-0130.

## Current P3.6 Runtime Shape

The active sample-expansion rollout profile is:

```text
configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml
```

Its intent is sample expansion only. It is not a calibrated-candidate rollout.

Required runtime invariants:

- `entry_mode="shadow_only"`
- `execution_mode="shadow"`
- `funding_lane_mode="disabled"`
- `gatekeeper_v3.enabled=false`
- `gatekeeper_v3.shadow_emit_enabled=true`
- `gatekeeper_v3.replay_payload_enabled=true`
- `gatekeeper_v3.promotion.enabled=false`
- `gatekeeper_v3.evidence_requirements.fsc=false`
- no active V2/V2.5 policy change
- no IWIM change
- no live sender change
- no P2 promotion

## Operational Issue Found

The tmux session was initially launched with stdout/stderr redirected to:

```text
logs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only/tmux_launcher.log
```

That made `tmux attach -t p36_sample_r12` look idle even though the launcher was
running and writing logs. This is an operator-footgun, not evidence that the
runtime was stopped.

To make the run visibly observable, the tmux session now contains a `monitor`
window that tails the launcher log:

```bash
tmux attach -t p36_sample_r12
tmux select-window -t p36_sample_r12:monitor
```

## Current Evidence Snapshot

At the time this ADR was written, the sample-expansion run was active:

```text
tmux session: p36_sample_r12
launcher pid: 1450578
rollout namespace: shadow-burnin-v3-p36-sample-r12-primary-only
```

Early health check:

- `v3_shadow_report.py`: `status=ok`
- `v3_rows=74`
- `replay_status=full`
- `stale_against_config=false`
- `full_snapshot_payload_rows=74`
- `v3_policy_config_hash.coverage=1.0`
- `v3_feature_snapshot_hash.coverage=1.0`

This is not final outcome evidence. It only proves that the run is producing
fresh full-replay V3 rows in the intended namespace.

## Decision

Continue the P3.6 sample-expansion run for several hours, then stop it
manually and run the normal offline evidence pipeline:

1. stop the tmux session cleanly
2. run strict full replay validation
3. generate outcome labels on the configured Chainstack RPC
4. produce a P3.6 sample-expansion report
5. merge R10+R11+sample-r12 analytically, without mutating historical evidence
6. re-run candidate BUY analysis on the larger sample
7. decide whether a new shadow-only candidate is justified

No P2 promotion is allowed from this run alone.

## Stop Procedure

```bash
tmux send-keys -t p36_sample_r12:0 C-c
```

Then confirm shutdown:

```bash
pgrep -af 'ghost-launcher|target/release/ghost-launcher|cargo run'
tmux ls
```

## Validation Procedure After Stop

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --json

python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml \
  --strict \
  --json
```

Outcome labels must be generated only after enough rows are available and the
full replay validator remains clean.

## Non-Goals

- no P2 promotion
- no live trading
- no active V2/V2.5 threshold change
- no IWIM change
- no live sender change
- no FSC/full-chain funding claim under single-stream constraints
- no threshold tuning from the 23-row candidate sample

## Consequences

Positive:

- larger sample collection can proceed without consuming a second provider stream
- runtime evidence remains replayable and namespace-isolated
- operator visibility is improved by a tmux monitor window
- ADR-0130 remains intact

Residual risks:

- current evidence is still primary-only and lacks authoritative full-chain FSC
- outcome quality remains unknown until labeling finishes
- a larger sample may still falsify every current candidate
- `PENDING` remains an effective economic block unless later evidence proves it
  transitions to viable BUY candidates

## Final Rule

P3.6 sample expansion is evidence gathering, not promotion. A larger sample may
justify a new offline candidate, but it must pass full replay, outcome labeling,
and R10+R11+sample-r12 combined safety checks before any R12-calibrated candidate
run or P2 discussion.
