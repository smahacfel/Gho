# FSC Bake Execution Report — 20260417T183800Z

**Final verdict:** `NO-GO`

## Commit / runtime state

- **Git HEAD:** `2afbd5b3f22c46ebae3462118d7ddc29ab1d4021`
- **Baseline stamp:** `2afbd5b3f22c46ebae3462118d7ddc29ab1d4021`
- **Runtime config set used for the neutral run:**
  - `/root/Gho/configs/rollout/paper-burnin.toml`
  - `/root/Gho/ghost-brain/ghost_brain_config.toml`
- **Prepared but not executed:** `/root/Gho/configs/rollout/paper-burnin-fsc.local.toml`

## `.env` / wallet validation

- Root `.env` present and required runtime variables were validated:
  - `GHOST_SEER_GRPC_ENDPOINT`
  - `GHOST_SEER_GRPC_X_TOKEN`
  - `GHOST_SEER_RPC_ENDPOINT`
  - `GHOST_TRIGGER_RPC_URL`
  - `GHOST_TRIGGER_KEYPAIR_PATH`
  - `GHOST_TRIGGER_SHADOW_RPC_URL`
- Rollout wallet path resolved and was operator-confirmed as the intended wallet.
- **Wallet pubkey:** `9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw`
- **Wallet balance:** `0.047172000 SOL`
- **Required minimum:** `0.005010000 SOL`
- **Validation result:** PASS

## Neutral preflight

- **Command:** `./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/paper-burnin.toml`
- **Result:** PASS
- **Fail count:** `0`
- **Warn count:** `0`
- **Resolved runtime profile at preflight:** `execution_mode=Paper`, `entry_mode=shadow_only`, `durability.profile=snapshot_only`

## Neutral run observations

- Startup markers observed:
  - `Runtime durability profile resolved mode="snapshot_only"`
  - `Runtime recovery complete mode="snapshot_only"`
  - `funding_lane_mode: disabled`
  - `Seer: FSC authoritative funding availability remains fail-closed (funding_lane_mode=disabled)`
- Neutral FSC/runtime metrics before closeout:
  - `fsc_authoritative_funding_stream_available = 0`
  - `fsc_warmup_ready = 0`
  - `fsc_lookup_hits_total = 0`
  - `fsc_lookup_misses_total = 0`
  - `fsc_lookup_hit_rate = 0`
- Artifact counts at neutral closeout:
  - `gatekeeper_v2_buys.jsonl`: `6`
  - `gatekeeper_v2_decisions.jsonl`: `84`
  - `shadow_run/paper-burnin-buys.jsonl`: `6`
  - event files: `2`
- BUY-path observations remained fail-closed for FSC:
  - all observed BUY rows carried `FSC_FUNDING_STREAM_UNAVAILABLE`

## Neutral closeout

- **Closeout guard result:** `SAFE_TO_STOP`
- **Guard details:** `shadow_success=6`, `paper_seen=80`, `inflight_candidates=0`
- Metrics snapshot saved to:
  - `/root/Gho/logs/rollout/paper-burnin/metrics.prom`
- Shutdown sequence logged `Shutdown signal received` and component stop markers.
- **Operational anomaly:** the launcher remained hung after component shutdown markers and had to be cleared with `SIGTERM`. `SIGKILL` was **not** used.

## Neutral formal report

- **Command:** `python3 /root/Gho/scripts/shadow_run_report.py --config /root/Gho/configs/rollout/paper-burnin.toml --metrics-text /root/Gho/logs/rollout/paper-burnin/metrics.prom`
- **Exit code:** `2`
- **Verdict:** `NO-GO`
- **Report summary:**
  - `buy_rows=0`
  - `shadow_rows=6`
  - `shadow_success=6`
  - `paper_completed=1`
  - `total_net_pnl_sol=-0.000003362`
- **Failing gate(s):**
  - `recovery_contract: FAIL`
- **Passing gate(s):**
  - `mandatory_artifacts`
  - `safety_violations`
  - `no_eventbus_lag`
  - `trace_correlation`
  - `paper_lifecycle_complete`
  - `no_duplicate_fire`
  - `no_live_side_effects`
  - `economics_not_fatal`

## Neutral artifacts

- **Neutral archive:** `/root/Gho/logs/rollout/fsc-bake/20260417T183800Z-neutral`
- Supporting cleanup archives created before the fresh restart:
  - `/root/Gho/logs/rollout/fsc-bake/20260417T161649Z-restart-aborted`
  - `/root/Gho/logs/rollout/fsc-bake/20260417T182434Z-stale-foreign-run`

## Authoritative phase

- **Authoritative preflight:** not run
- **Authoritative bake:** not run
- **Authoritative metrics:** not observed
- **Reason not executed:** ceremony stopped immediately after neutral formal report returned `NO-GO`

## Replay / diff phase

- **Status:** not executed
- **Reason:** ceremony stopped at neutral `NO-GO` before the authoritative/replay phase

## Final decision

`NO-GO`

The ceremony cannot proceed to authoritative FSC bake because the neutral baseline session already failed the formal go/no-go report.
