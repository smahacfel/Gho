# ADR-8D: Rollout evidence retention guard after archive-volume incident

Date: 2026-06-28

Status: IMPLEMENTED / P0 ACTIVE-SYMLINK CONTAINMENT APPLIED / NO RUNTIME CHANGE / NO COMMIT

## 1. Context

The archive/storage volume `/mnt/HC_Volume_105935807` was used as an active log destination through symlinks from repo log paths:

```text
/root/Gho/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
/root/Gho/logs/shadow_run -> /mnt/HC_Volume_105935807/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
```

Additional R50 shadow_run symlinks existed in the clean worktree:

```text
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl
```

This let R50 write active rollout and shadow_run logs directly into the archive volume.

Under disk pressure, old non-R50 rollout directories were deleted from the volume around 2026-06-27 22:44 UTC. The deleted directories included R48/R49 `logs/rollout/...`, including `gatekeeper_v2_decisions.jsonl`.

Remaining evidence:

- R48/R49 `logs/shadow_run/...` still exists on `/mnt/HC_Volume_105935807`.
- R50 `logs/rollout/...` and `logs/shadow_run/...` still exists on `/mnt/HC_Volume_105935807`.

Missing evidence:

- R48/R49 `logs/rollout/.../decisions/**/gatekeeper_v2_decisions.jsonl`
- R48/R49 `logs/rollout/.../decisions/**/gatekeeper_v2_buys.jsonl`
- R48/R49 rollout raw logs and related decision-side artifacts

## 2. Decision

Adopt a hard separation between active log paths and archive storage.

The archive volume must not be used as an active writer target.

Required policy:

1. Active Ghost runs must write to local real directories or to a explicitly designated active-log volume, not to `/mnt/HC_Volume_105935807`.
2. `/mnt/HC_Volume_105935807` must be treated as archive/storage only.
3. Repo paths `logs/rollout` and `logs/shadow_run` must not symlink to the archive volume.
4. Evidence archival must use copy-first semantics.
5. Deletion of evidence must require a pre-delete manifest and explicit scope allowlist.
6. Cleanup tooling must reject broad deletes and known evidence filenames.

No runtime behavior is changed by this ADR.

## 2.1 Applied containment

Applied on 2026-06-28:

- Archive volume remounted read-only.
- Active repo log symlinks into the archive volume removed.
- Local real log directories restored in both worktrees.
- R50-specific symlinks under the clean worktree were removed only after confirming `[ -L path ]`.
- No files or directories under `/mnt/HC_Volume_105935807` were removed.
- No `rm -rf` was used.
- No run was started.
- Nothing was staged or committed.

Removed active symlinks:

```text
/root/Gho/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
/root/Gho/logs/shadow_run -> /mnt/HC_Volume_105935807/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout -> /mnt/HC_Volume_105935807/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 -> /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl -> /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1-buys.jsonl
```

Restored local directories:

```text
/root/Gho/logs/rollout
/root/Gho/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run
```

Validation after containment:

```text
findmnt -no SOURCE,FSTYPE,OPTIONS /mnt/HC_Volume_105935807
/dev/sdb ext4 ro,relatime,discard

readlink -f /root/Gho/logs/rollout
/root/Gho/logs/rollout

readlink -f /root/Gho/logs/shadow_run
/root/Gho/logs/shadow_run

readlink -f /root/Gho-tsv2-a1-a2-clean/logs/rollout
/root/Gho-tsv2-a1-a2-clean/logs/rollout

readlink -f /root/Gho-tsv2-a1-a2-clean/logs/shadow_run
/root/Gho-tsv2-a1-a2-clean/logs/shadow_run
```

Remaining symlink in the inspected maxdepth-4 set:

```text
/root/Gho-tsv2-a1-a2-clean/.env -> /root/Gho/.env
```

Future runs must write to local `logs/` directories or an explicitly configured non-archive active-log path. `/mnt/HC_Volume_105935807` is read-only evidence storage by default.

## 3. Containment findings

Mount state:

```text
/mnt/HC_Volume_105935807 /dev/sdb ext4 rw,relatime,discard
```

Current capacity:

```text
/dev/sdb ext4 49G size, 17G used, 30G available, 36% used
```

Active writer check before containment:

```text
pgrep -af 'ghost|launcher|python|cargo|tmux'
```

Observed only:

```text
1005 /usr/bin/python3 /usr/share/unattended-upgrades/unattended-upgrade-shutdown --wait-for-signal
```

Open files under the volume before and after containment:

```text
COMMAND    PID USER   FD   TYPE DEVICE SIZE/OFF   NODE NAME
bash    261650 root  cwd    DIR   8,16     4096 131074 /mnt/HC_Volume_105935807/logs/rollout
```

Interpretation:

- No active Ghost writer was observed.
- One shell has current working directory under the volume.
- That shell must be moved or closed before remount, unmount, snapshot, or recovery work.

## 4. Guard plan

The active symlink removal and read-only remount were applied. Remaining guard items are proposed and not yet applied.

### 4.1 Remove active symlinks: applied

Applied:

- Replaced `/root/Gho/logs/rollout` symlink with a real local directory.
- Replaced `/root/Gho/logs/shadow_run` symlink with a real local directory.
- Replaced `/root/Gho-tsv2-a1-a2-clean/logs/rollout` symlink with a real local directory.
- Removed R50-specific shadow_run symlinks from the clean worktree after `[ -L path ]` checks.

Rules:

- Do not use `git reset --hard`.
- Do not delete evidence directories.
- Do not use recursive deletion without explicit allowlist.
- Confirm with `readlink -f` that active log dirs no longer resolve into `/mnt/HC_Volume_105935807`.

### 4.2 Archive layout: not applied

Use:

```text
/mnt/HC_Volume_105935807/archive/
/mnt/HC_Volume_105935807/archive/logs/
/mnt/HC_Volume_105935807/archive/reports/
```

Do not use:

```text
/mnt/HC_Volume_105935807/logs/rollout
/mnt/HC_Volume_105935807/logs/shadow_run
```

as active writer paths.

### 4.3 Archive copy rule: not applied

Allowed archival operation:

```text
rsync -a --ignore-existing --protect-args <source>/ /mnt/HC_Volume_105935807/archive/<scope>/
```

Forbidden for evidence archival:

```text
rsync --remove-source-files
rm -rf <scope>
rm -rf /mnt/HC_Volume_105935807/logs/*
rm -rf logs/rollout/*
rm -rf logs/shadow_run/*
```

### 4.4 Cleanup guard script: not applied

Add a cleanup guard script after review.

Proposed path:

```text
scripts/guard_rollout_evidence_cleanup.py
```

Minimum behavior:

- dry-run by default
- explicit scope allowlist required
- refuse broad roots
- refuse paths containing critical evidence filenames
- require pre-delete manifest
- require explicit second confirmation token
- print file count, byte count, and exact paths
- no wildcard deletes without scope match

Critical filenames to protect:

```text
gatekeeper_v2_decisions.jsonl
gatekeeper_v2_buys.jsonl
shadow_exit_replay_v1.jsonl
shadow_lifecycle.jsonl
probe_shadow_lifecycle.jsonl
selector_shadow_score_v1.jsonl
coordination_risk_evidence.jsonl
```

### 4.5 Volume README: not applied

Add after approval:

```text
/mnt/HC_Volume_105935807/README_DO_NOT_WRITE_ACTIVE_LOGS_HERE.md
```

Purpose:

- make the volume role explicit
- warn against symlinking active log paths into archive storage
- document allowed archival copy command
- document deletion guard requirements

## 5. Recovery stance

No recovery was attempted as part of this ADR.

Because the filesystem is ext4 and R50 continued writing after deletion, recovery of full paths/names may fail. Some blocks may have been overwritten.

If recovery is attempted:

1. Stop all writers.
2. Move/close shell PID `261650`.
3. Snapshot the volume or create a block-level image.
4. Run recovery tools only on the image/copy.
5. Prefer raw JSONL carving strings for decision rows if path-level recovery fails.

## 6. Consequences

Positive:

- Stops accidental active writes to archive storage.
- Makes future evidence cleanup auditable.
- Preserves remaining R48/R49 shadow_run evidence.
- Prevents future R50/R51 runs from using archive volume as a sink.

Negative:

- Requires manual cleanup of symlink state in two worktrees.
- Requires a separate recovery decision for deleted R48/R49 rollout logs.
- Does not restore lost `gatekeeper_v2_decisions.jsonl`.

## 7. Runtime decision

Runtime approval: false

Shadow close only approval: false

Active close approval: false

R50/R51 start approval: false

Research continuation approval: false until path guards are applied and evidence retention policy is explicit.

## 8. Verification required before closing this ADR

Before this ADR can be marked implemented:

```text
find /root/Gho /root/Gho-tsv2-a1-a2-clean -maxdepth 4 -type l -ls
readlink -f /root/Gho/logs/rollout
readlink -f /root/Gho/logs/shadow_run
readlink -f /root/Gho-tsv2-a1-a2-clean/logs/rollout
readlink -f /root/Gho-tsv2-a1-a2-clean/logs/shadow_run
pgrep -af 'ghost|launcher|python|cargo|tmux'
lsof +D /mnt/HC_Volume_105935807 2>/dev/null | head -200
git diff --check -- <changed-files>
git diff --cached --name-only
```

Expected:

- no active repo log symlink resolves to `/mnt/HC_Volume_105935807`
- no Ghost writer has open files under `/mnt/HC_Volume_105935807`
- staged set remains empty unless an explicit commit is approved
