# INCIDENT: rollout evidence deletion and archive-volume write containment

Date: 2026-06-28

Status: P0 DAMAGE CONTAINMENT APPLIED / NO RUNTIME CHANGE / NO COMMIT

Scope:

- Archive/storage volume: `/mnt/HC_Volume_105935807`
- Primary repo worktree: `/root/Gho`
- Clean research worktree: `/root/Gho-tsv2-a1-a2-clean`
- Deleted evidence family: R48/R49 `logs/rollout/...`
- Remaining evidence family: R48/R49 `logs/shadow_run/...`

## 1. Incident summary

This incident was caused by using the archive volume as an active log sink.

Root cause:

1. Active repo paths were symlinked into `/mnt/HC_Volume_105935807/logs`.
2. R50 logging-only validation then wrote active rollout and shadow logs to that same volume.
3. Under disk pressure, old non-R50 rollout directories were deleted from the volume.
4. Those directories included R48/R49 decision logs, including `gatekeeper_v2_decisions.jsonl`, which later became required pre-entry evidence for EIX.

The archive/storage volume should not have been used as a live write target.

Final damage classification:

- R48/R49 `logs/rollout/...` decision evidence is missing locally.
- R48/R49 `logs/shadow_run/...` evidence remains on the volume.
- R50 rollout and shadow_run evidence remains on the volume.
- No runtime research or experiments were run during this containment pass.
- No process was killed during this containment pass.
- No files were deleted or moved during this containment pass.
- Nothing was staged or committed during this containment pass.

## 1.1 Containment applied on 2026-06-28

Applied actions:

- Stopped using `/mnt/HC_Volume_105935807` as an active log sink by removing active repo log symlinks.
- Restored local real log directories in both worktrees.
- Remounted the archive volume read-only.
- Removed only symlinks after `[ -L path ]` checks.
- Did not remove anything under `/mnt/HC_Volume_105935807`.
- Did not use `rm -rf`.
- Did not start any run.
- Did not stage or commit.

Removed active log symlinks:

```text
/root/Gho/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
/root/Gho/logs/shadow_run -> /mnt/HC_Volume_105935807/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
```

Removed R50-specific symlinks from the clean worktree only after confirming they were symlinks:

```text
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl
```

Local directories restored:

```text
/root/Gho/logs/rollout
/root/Gho/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run
```

Readlink validation after containment:

```text
/root/Gho/logs/rollout
/root/Gho/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run
```

Remaining symlink inventory after containment:

```text
/root/Gho-tsv2-a1-a2-clean/.env -> /root/Gho/.env
```

Archive volume mount after containment:

```text
/dev/sdb ext4 ro,relatime,discard
```

Active writer check after containment:

```text
No active ghost-launcher, cargo run, or tmux writer was observed.
```

`fuser` / `lsof` still show one shell with current working directory under the archive volume:

```text
bash 261650 root cwd /mnt/HC_Volume_105935807/logs/rollout
```

That shell is not a Ghost writer, but it should be moved or closed before snapshot, remount, unmount, or recovery work.

Future runs must write to local `logs/` directories or to an explicitly configured non-archive active-log path. The archive volume is read-only evidence storage by default.

## 2. Deleted paths

The following rollout directories were deleted from the archive volume during cleanup around 2026-06-27 22:44 UTC:

```text
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r1
```

Critical lost evidence:

```text
logs/rollout/<R48/R49 scope>/decisions/**/gatekeeper_v2_decisions.jsonl
logs/rollout/<R48/R49 scope>/decisions/**/gatekeeper_v2_buys.jsonl
logs/rollout/<R48/R49 scope>/oracle.log*
logs/rollout/<R48/R49 scope>/system.log*
logs/rollout/<R48/R49 scope>/selector_shadow_score_v1.jsonl
```

Impact:

- EIX cannot evaluate R49 entry+exit intersection with real pre-entry decision-time feature rows.
- The EIX hypothesis is not numerically falsified by this missing data.
- No runtime approval can be derived from incomplete evidence.

## 3. Step 1 inventory

Command: `findmnt /mnt/HC_Volume_105935807`

```text
TARGET                   SOURCE   FSTYPE OPTIONS
/mnt/HC_Volume_105935807 /dev/sdb ext4   rw,relatime,discard
```

Command: `df -hT`

```text
Filesystem     Type   Size  Used Avail Use% Mounted on
/dev/sda1      ext4   150G  103G   42G  72% /
/dev/sdb       ext4    49G   17G   30G  36% /mnt/HC_Volume_105935807
```

Command: `lsblk -f`

```text
NAME    FSTYPE FSVER UUID                                 FSAVAIL FSUSE% MOUNTPOINTS
sda
|-sda1  ext4   1.0   5e6f1593-c3dd-45e2-bf7a-27b3cd9ef023   41.4G    68% /
`-sda15 vfat   FAT32 3ED7-13F3                             251.9M     0% /boot/efi
sdb     ext4   1.0   957e91dd-8ca4-441d-ad25-b313924efa48   29.7G    34% /mnt/HC_Volume_105935807
```

Command: `find /root/Gho /root/Gho-tsv2-a1-a2-clean -maxdepth 4 -type l -ls`

```text
/root/Gho/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
/root/Gho/logs/shadow_run -> /mnt/HC_Volume_105935807/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl -> /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 -> /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
/root/Gho-tsv2-a1-a2-clean/.env -> /root/Gho/.env
```

Command: `du -sh /mnt/HC_Volume_105935807/* /mnt/HC_Volume_105935807/logs/* 2>/dev/null`

Current observed top-level usage:

```text
17G /mnt/HC_Volume_105935807/logs
```

Detailed current evidence usage:

```text
15G  /mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
489M /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1
279M /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
430M /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
```

Command: `pgrep -af 'ghost|launcher|python|cargo|tmux'`

```text
1005 /usr/bin/python3 /usr/share/unattended-upgrades/unattended-upgrade-shutdown --wait-for-signal
```

Observed result:

- No active `ghost-launcher` process.
- No active `cargo` run.
- No active R50/R51 process.
- No active `tmux` session matching Ghost was reported by this command.

## 4. Step 2 active writer check

Command: `lsof +D /mnt/HC_Volume_105935807 2>/dev/null | head -200`

```text
COMMAND    PID USER   FD   TYPE DEVICE SIZE/OFF   NODE NAME
bash    261650 root  cwd    DIR   8,16     4096 131074 /mnt/HC_Volume_105935807/logs/rollout
```

Follow-up:

```text
UID          PID    PPID  C STIME TTY          TIME CMD
root      261650  261594  0 15:26 pts/1    00:00:00 -bash
```

The shell has `cwd` under the volume, but no file descriptor is open for writing under the volume. No active Ghost writer was observed.

Containment implication:

- Do not remount or unmount while that shell has cwd on the volume unless it is moved/closed.
- Do not start new Ghost runs until log paths are fixed.
- Do not write new active logs to `/mnt/HC_Volume_105935807`.

## 5. Step 3 remaining evidence manifest

No moving was performed. This is a manifest of remaining evidence only.

### R49 remaining shadow_run evidence

```text
2026-06-26T19:46:19.4769871050Z 27343702  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl
2026-06-26T19:46:37.4745845500Z 11742516  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl
2026-06-26T19:46:37.9726564370Z 280671496 /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl
2026-06-26T19:46:39.3348530460Z 18442283  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_transport.jsonl
2026-06-26T19:46:39.3348530460Z 5930788   /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_entries.jsonl
2026-06-26T19:46:48.3611558770Z 9652943   /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_entries.jsonl
2026-06-26T19:46:48.3641563100Z 18893198  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1-buys.jsonl
2026-06-26T19:48:38.2680254870Z 11540048  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_selection.jsonl
2026-06-26T19:48:38.2750264990Z 147309362 /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_skips.jsonl
```

### R48 remaining shadow_run evidence

```text
2026-06-25T16:31:14.4168510170Z 5721102   /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1/shadow_lifecycle.jsonl
2026-06-25T16:43:29.0199305350Z 212023    /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r1/shadow_lifecycle.jsonl
2026-06-26T07:37:55.4874764940Z 40259676  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_lifecycle.jsonl
2026-06-26T07:38:37.8615620400Z 10973969  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_exit_replay_v1.jsonl
2026-06-26T07:34:51.9811224790Z 21164153  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2-buys.jsonl
```

Additional R48 files remain under:

```text
/mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1
/mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r1
/mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
```

### R50 remaining evidence

```text
15G  /mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
430M /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
13M  /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl
```

Critical R50 decision files still present:

```text
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/decisions/**/gatekeeper_v2_decisions.jsonl
/mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/decisions/**/gatekeeper_v2_buys.jsonl
```

### Critical small report hashes

Hashes were computed in `/root/Gho-tsv2-a1-a2-clean` for small CSV/JSON/MD reports only. Raw JSONL logs were not hashed in this pass.

```text
cc0e217b57a5ed18847d453836be4e27b57e2d2634ddcb1592cf8e4399098101  reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_NOHARM_PROOF_A1.md
b67031bb1abde9b2e6f6e5d49444bf0ef52d9708d2f011d9505b750d398dc9d5  reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_TARGET_CUT_ATTRIBUTION_A2.md
ff29693dca5656268703137aadc2531aeb106c3e9c1e46a48c5910a36fd36dec  reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv
0d85b6281c1e25bfa71a19fd225e032d4013cb095011415eb64c009d05b6cdb2  reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_summary.csv
28a772bd011a7f621a2d164b94d78342c66646976bbebf9ccaba480cf4c934ca  reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_thresholds.csv
894b14c098e620eaddfd5a53a7d20f9e39dda03f79af38d0dd22fa1cd6aa6c27  reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/organic_candidate_policy_inventory.csv
b9a8b5eaa5148cc29609a60933894f31d66fe663e5e5a6c7efdeb1fb9cd3aae4  reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/TARGET_STOP_HOLD_MATRIX_REPORT.md
18ecf1e533356c59dc4b3e22b8e9432e0a3d32445e9683cf8100c18a8fa18338  reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/TIME_STOP_V2_TARGET_CUT_ATTRIBUTION_A2.md
490b4b383b7f99d56bee2b650a31ef0295ec2215426b06f4408063d633b52692  reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv
46a2abae74db34ceb0a125cdeb1099a14154ba4eff370a7d86c3dc3d7550410c  reports/selector/tsv2_a3_two_scope_validation_summary.csv
bad84ca1a5246111824bec1c18d39471e2c7307bf50105cd03ea44b4f2c2e69a  reports/selector/tsv2_a3_two_scope_fixed_cell_intersection.csv
55e09f18e62d27a7d78929e5dace073be08bcbf2f57f68fea70fb2c5a1ecc813  PLANS/AUDYT/RAPORT_TSV2_A3_TWO_SCOPE_VALIDATION_20260628.md
a278e71fac1178b28695bcf5503a81e814c7d4824fd425318078268549e13be7  docs/ADR/ADR_8D_TSV2_A3_TWO_SCOPE_VALIDATION_20260628.md
```

## 6. Fix plan status

The active symlink containment portion of this plan was applied on 2026-06-28. The archive layout, README guard, cleanup guard script, and any recovery operation remain not applied and require explicit approval.

### P0.0 freeze

- Do not start R50/R51 or any research run.
- Do not run experiments.
- Do not write active logs to `/mnt/HC_Volume_105935807`.
- Confirm no active `ghost-launcher`, `cargo`, `tmux`, or analysis job is writing to the volume.
- Move or close shell PID `261650` before any remount/snapshot operation, because its `cwd` is under the volume.

### P0.1 archive layout: not applied

Use the volume as archive only:

```text
/mnt/HC_Volume_105935807/archive/
/mnt/HC_Volume_105935807/archive/logs/
/mnt/HC_Volume_105935807/archive/reports/
/mnt/HC_Volume_105935807/README_DO_NOT_WRITE_ACTIVE_LOGS_HERE.md
```

The volume root and `/mnt/HC_Volume_105935807/logs` must not be used as active writer paths.

### P0.2 remove active symlinks: applied

Active symlink usage was removed from both worktrees and local real directories were recreated:

```text
/root/Gho/logs/rollout
/root/Gho/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run
```

Rules observed:

- `git reset --hard` was not used.
- `rm -rf` was not used.
- Only symlink paths were removed after `[ -L path ]`.
- Local directories were recreated as real directories.
- `readlink -f` confirms active log dirs no longer point to `/mnt/HC_Volume_105935807`.

### P0.3 archive copies: not applied

Old evidence movement must use copy-first semantics:

```text
rsync -a --ignore-existing --protect-args <source>/ /mnt/HC_Volume_105935807/archive/<scope>/
```

Rules:

- Never delete source evidence in the same operation as archival copy.
- Never use `--remove-source-files` for evidence archival.
- Generate manifest before any cleanup.
- Generate manifest after archival copy.
- Compare counts, sizes, and hashes for small files before considering cleanup.

### P0.4 cleanup guard: not applied

Add a cleanup guard script only after review. Required behavior:

- Explicit scope allowlist is mandatory.
- Dry-run default.
- Refuse broad roots such as `/mnt/HC_Volume_105935807`, `/mnt/HC_Volume_105935807/logs`, `/root/Gho/logs`, or any `logs/rollout` root.
- Refuse deletion of paths containing:
  - `gatekeeper_v2_decisions.jsonl`
  - `gatekeeper_v2_buys.jsonl`
  - `shadow_exit_replay_v1.jsonl`
  - `shadow_lifecycle.jsonl`
  - `probe_shadow_lifecycle.jsonl`
- Require pre-delete manifest.
- Require explicit `--allow-delete-scope <scope>` and a second confirmation token.
- Print exact file count, byte count, and candidate paths before deletion.
- No wildcard deletion without scope match.

Candidate path:

```text
scripts/guard_rollout_evidence_cleanup.py
```

### P0.5 README guard: not applied

Add a volume-root README only after approval:

```text
/mnt/HC_Volume_105935807/README_DO_NOT_WRITE_ACTIVE_LOGS_HERE.md
```

Required message:

```text
This volume is archive/storage only.
Do not point active repo log paths here.
Do not run Ghost launchers with logs/rollout or logs/shadow_run symlinked here.
Use rsync --ignore-existing for archival copies.
Never run rm -rf on archived evidence without a manifest and explicit scope allowlist.
```

## 7. Recovery note

The deleted evidence was on ext4. After deletion, R50 continued to write active logs to the same volume. This means some freed blocks may have been overwritten.

Recovery status:

- No recovery attempt was run in this containment pass.
- Do not run recovery tools on the mounted RW filesystem.
- If recovery is attempted, first stop all writers and create a provider snapshot or block-level image.
- Work only on the image/copy.

Potential recovery methods after snapshot:

```text
extundelete
ext4magic
debugfs
testdisk
photorec
raw carving for JSONL fragments
```

Useful carving strings:

```text
gatekeeper_v2_decisions
materialized_feature_snapshot
buy_count
sol_buy_ratio
current_market_cap_sol
bonding_progress_pct
price_change_ratio
rollout_namespace
shadow-burnin-v3-r49-r48
shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
```

## 8. Runtime decision

No runtime decision is allowed from this incident state.

- Runtime approval: false
- Shadow close only approval: false
- Active close approval: false
- R50/R51 start approval: false
- Research continuation approval: false until evidence paths are fixed

## 9. Immediate next approvals needed

Do not apply these without explicit approval:

1. Stop or move the shell currently holding cwd under the volume.
2. Snapshot or block-image the volume if recovery is desired.
3. Remove active symlinks and recreate local log directories.
4. Add volume README guard.
5. Add cleanup guard script.
6. Decide whether R50 evidence should be copied into archive layout or left in place until recovery decision.
