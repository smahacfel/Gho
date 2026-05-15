# Walidacja P0 V3 Shadow/Evidence - Clean Rerun - 2026-05-14

## Status

**GO dla czystego P0 shadow/evidence baseline.**

Clean rerun potwierdza, ze V3 Stack dzialal jako additive shadow/evidence sidecar bez promocji do active policy. Caveat z poprzedniego formalnego artefaktu zostal usuniety przez jawne zaakceptowanie `min_market_cap_sol = 30.0` jako rollout baseline przed runem.

Nie potwierdzano live execution ani promotion readiness.

## Scope

- Repo: `/root/Gho`
- SSOT rollout config: `/root/Gho/configs/rollout/shadow-burnin.toml`
- Brain config baseline: `/root/Gho/ghost-brain/ghost_brain_config.toml`
- Runtime mode:
  - `entry_mode = "shadow_only"`
  - `execution_mode = "shadow"`
- Runtime window: `timeout 30m`
- Run start observed in logs: `2026-05-14T10:35:29Z`
- Last decision timestamp: `2026-05-14T11:05:24.795644789+00:00`

## Baseline Config Acceptance

The clean rerun uses the accepted local brain config baseline:

```toml
min_market_cap_sol = 30.0
```

This value is no longer treated as dirty/uncontrolled for this artifact. It was recorded before the rerun in:

```text
/root/Gho/PLANS/AUDYT/P0_V3_CLEAN_RERUN_BASELINE_CONFIG_20260514.md
```

Config fingerprints confirmed after the run:

```text
aacd7b4e0800f2318fb1b72a93198d1b4cb05d5007d0ca700586cd586abd7073  configs/rollout/shadow-burnin.toml
f2039f35b977ab7f075da0fee6e6ed872e497688da60911f945c7bb09ea8b7d8  ghost-brain/ghost_brain_config.toml
```

## Process Hygiene

Before the successful clean rerun:

- no active `ghost-launcher`, `cargo run`, `target/release/ghost-launcher`, or `timeout 30m` process remained
- the failed/interrupted clean-rerun attempt was archived to:

```text
/root/Gho/_archive_p0_clean_rerun_failed_20260514T103523Z
```

After the clean rerun:

- `pgrep -af 'ghost-launcher|cargo run|target/release/ghost-launcher|timeout 30m'` returned no active process
- no runtime leftovers required manual shutdown

## Runtime Command

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Observed runtime behavior:

- event stream delivered live shadow input
- Seer emitted `PoolTransaction`
- DecisionLogger wrote V2.5 shadow decisions
- event dataset files rotated
- shadow ledger snapshots were written
- occasional `ResourceExhausted` warnings appeared on a lane, but did not prevent V2.5 shadow decisions or V3 sidecar evidence rows

## V3 Report

Formal JSON report snapshot:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/reports/v3_p0_clean_rerun_report_20260514T113624Z.json
```

Report result:

```text
status = ok
raw_rows = 141
deduped_rows = 141
v3_rows = 141
bad_rows = 0
duplicate_rows_removed = 0
no_v3_rows = 0
execution.success_count = 0
execution.outcomes.missing = 141
```

Interpretation:

- `missing` execution outcomes are not treated as success
- `execution.success_count = 0` is expected for P0 shadow-only evidence
- `status = ok` with `v3_rows > 0` satisfies the operational P0 V3 report gate

## JSONL Semantic Checks

Primary V2.5/V3 sidecar JSONL:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Semantic checks passed:

```text
rows = 141
v3_rows = 141
decision_planes = {"v25_shadow": 141}
reason_code_versions = {2: 141}
```

Checked invariants:

- all V3 sidecar rows are present
- every V3 sidecar row has `v3_shadow_reason_code`
- every V3 sidecar row has `v3_shadow_notes.p0 == "shadow_only"`
- active `reason_code_version` remains `2`
- active `decision_plane` remains `v25_shadow`
- V3 never appears as `decision_plane = "v3_shadow"`
- active `verdict_type` matches active `reason_code`
- active `decision_verdict_buy` remained `false` for this P0 sample

## Reason-Code Distributions

Active reason-code distribution:

```text
REJECT_PDD_ENTRY_DRIFT = 92
REJECT_PDD_WHALE = 37
REJECT_PDD_RAMPING = 7
REJECT_PDD_SPIKE = 2
REJECT_LOW_TRAJECTORY = 1
REJECT_PDD_FLASH_CRASH = 2
```

V3 sidecar reason-code distribution:

```text
REJECT_V3_MANIPULATION_CONTRADICTION = 96
PENDING_V3_WAIT_EVIDENCE = 38
PENDING_V3_WAIT_SAMPLE = 6
REJECT_V3_LOW_ORGANIC_BROADENING = 1
```

Required comparison points:

```text
PENDING_V3_WAIT_EVIDENCE = 38
REJECT_V3_MANIPULATION_CONTRADICTION = 96
execution.success_count = 0
decision_plane = v25_shadow
reason_code_version = 2
```

## Comparison To Prior P0 Artifact

Prior artifact with dirty-config caveat:

```text
/root/Gho/_archive_p0_clean_rerun_input_20260514T102105Z/logs_rollout/shadow-burnin-v25-repair-r2/reports/v3_p0_shadow_report_20260514T090916Z.json
```

Prior V2.5/V3 JSONL:

```text
/root/Gho/_archive_p0_clean_rerun_input_20260514T102105Z/logs_rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Summary:

```text
prior.status = ok
clean.status = ok

prior.raw_rows = 72
clean.raw_rows = 141
delta.raw_rows = +69

prior.v3_rows = 72
clean.v3_rows = 141
delta.v3_rows = +69

prior.execution.success_count = 0
clean.execution.success_count = 0

prior.decision_plane = v25_shadow only
clean.decision_plane = v25_shadow only

prior.reason_code_version = 2 only
clean.reason_code_version = 2 only
```

Active reason-code comparison:

```text
REJECT_PDD_ENTRY_DRIFT: 51 -> 92
REJECT_PDD_WHALE: 16 -> 37
REJECT_PDD_RAMPING: 2 -> 7
REJECT_PDD_SPIKE: 1 -> 2
REJECT_PDD_FLASH_CRASH: 2 -> 2
REJECT_LOW_TRAJECTORY: 0 -> 1
```

V3 sidecar reason-code comparison:

```text
PENDING_V3_WAIT_EVIDENCE: 12 -> 38
PENDING_V3_WAIT_SAMPLE: 2 -> 6
REJECT_V3_LOW_ORGANIC_BROADENING: 1 -> 1
REJECT_V3_MANIPULATION_CONTRADICTION: 57 -> 96
```

## Relic Paths

Decision logs:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions
```

V2.5/V3 sidecar JSONL:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Legacy mirror JSONL:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.2/legacy_live/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl
```

Coverage audit:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/decisions/seer_runtime_coverage_audit.jsonl
```

System log:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/system.log.2026-05-14
```

Oracle log:

```text
/root/Gho/logs/rollout/shadow-burnin-v25-repair-r2/oracle.log.2026-05-14
```

Event dataset:

```text
/root/Gho/datasets/events/shadow-burnin-v25-repair-r2
```

Snapshot data:

```text
/root/Gho/data/rollout/shadow-burnin-v25-repair-r2/snapshots
```

Shadow run dir:

```text
/root/Gho/logs/shadow_run/shadow-burnin-v25-repair-r2
```

Input archive for prior dirty-caveat artifact:

```text
/root/Gho/_archive_p0_clean_rerun_input_20260514T102105Z
```

Failed clean-rerun attempt archive:

```text
/root/Gho/_archive_p0_clean_rerun_failed_20260514T103523Z
```

## Artifact Sizes

```text
322M  /root/Gho/logs/rollout/shadow-burnin-v25-repair-r2
4.0K  /root/Gho/logs/shadow_run/shadow-burnin-v25-repair-r2
204K  /root/Gho/datasets/events/shadow-burnin-v25-repair-r2
184K  /root/Gho/data/rollout/shadow-burnin-v25-repair-r2
```

Event files:

```text
exec_launcher-1778754929050_20260514_103529_0000.jsonl 0
exec_launcher-1778754929105_20260514_103529_0000.jsonl 30760
exec_launcher-1778754929105_20260514_104032_0001.jsonl 23226
exec_launcher-1778754929105_20260514_104532_0002.jsonl 32510
exec_launcher-1778754929105_20260514_105037_0003.jsonl 29447
exec_launcher-1778754929105_20260514_105541_0004.jsonl 27710
exec_launcher-1778754929105_20260514_110044_0005.jsonl 25891
```

Snapshot files:

```text
shadow_ledger_snapshot_1778756549052.bin 55560
shadow_ledger_snapshot_1778756609051.bin 57938
shadow_ledger_snapshot_1778756669051.bin 60383
```

## Conclusion

Clean rerun closes the prior config caveat for P0 shadow/evidence.

P0 remains GO with a controlled `min_market_cap_sol = 30.0` baseline:

- runtime produced V2.5 shadow decisions
- V3 report is `status = ok`
- `v3_rows > 0`
- JSONL semantic checks passed
- V3 stayed additive sidecar only
- no `decision_plane = "v3_shadow"`
- active `reason_code_version` stayed `2`
- active verdict/reason fields were not promoted or overwritten by V3
- `execution.success_count = 0`, as expected for shadow-only P0
- no active runtime leftovers remained after the run

This artifact supersedes the dirty-config caveat in the earlier P0 formal report for baseline purposes. It does not authorize P2 or active promotion. The next recommended stage remains P1 planning for calibrated shadow funnel, config-driven thresholds/caps, policy hash, feature snapshot hash, and replay/ablation reporting.
