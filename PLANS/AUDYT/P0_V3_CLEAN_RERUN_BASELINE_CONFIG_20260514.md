# P0 V3 Clean Rerun - Baseline Config Acceptance

## Status

Accepted rollout baseline for the clean P0 V3 shadow/evidence rerun.

## Scope

- Repo: `/root/Gho`
- Rollout SSOT config: `/root/Gho/configs/rollout/shadow-burnin.toml`
- Brain config referenced by rollout: `/root/Gho/ghost-brain/ghost_brain_config.toml`
- Runtime mode remains shadow-only:
  - `entry_mode = "shadow_only"`
  - `execution_mode = "shadow"`

## Accepted Baseline Delta

The current local brain config value is accepted as the rerun baseline:

```toml
min_market_cap_sol = 30.0
```

This intentionally accepts the prior local delta:

```diff
-min_market_cap_sol = 41.0
+min_market_cap_sol = 30.0
```

For the clean rerun, this value is no longer treated as an uncontrolled caveat. It is the explicitly accepted P0 rerun baseline.

## Config Fingerprints

Captured before rerun:

```text
aacd7b4e0800f2318fb1b72a93198d1b4cb05d5007d0ca700586cd586abd7073  configs/rollout/shadow-burnin.toml
f2039f35b977ab7f075da0fee6e6ed872e497688da60911f945c7bb09ea8b7d8  ghost-brain/ghost_brain_config.toml
```

## Rerun Purpose

This rerun is not a new code validation pass. P0 plumbing/evidence was already validated.

The purpose is to produce a clean, controlled P0 artifact set with the accepted `min_market_cap_sol = 30.0` baseline, then compare it against the prior P0 validation.

## Required Post-Run Checks

Minimum comparison against the prior P0 artifact:

- row count
- active reason-code distribution
- V3 sidecar reason-code distribution
- `PENDING_V3_WAIT_EVIDENCE` vs `REJECT_V3_MANIPULATION_CONTRADICTION`
- `execution.success_count == 0`
- `decision_plane == "v25_shadow"`
- `reason_code_version == 2`

