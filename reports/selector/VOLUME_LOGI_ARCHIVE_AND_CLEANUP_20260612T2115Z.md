# VOLUME_LOGI_ARCHIVE_AND_CLEANUP_20260612T2115Z

Status: COMPLETE
Generated: 2026-06-12T21:15:00Z

## Scope

User request:

- inspect `/mnt/HC_Volume_105935807` ("logi" volume),
- remove clearly useless files there,
- move selected useful logs/artifacts from `/root/Gho/logs` to that volume,
- preserve evidence value by archiving before deletion.

No R26 start was performed in this step.

## Disk State After Operation

```text
Filesystem      Size  Used Avail Use% Mounted on
/dev/sda1       150G  108G   37G  75% /
/dev/sdb         49G   32G   16G  68% /mnt/HC_Volume_105935807
```

Before this operation, after the earlier `/root/Gho/target` cleanup, `/root/Gho`
had about `33G` free. After archiving and deleting selected originals it has
about `37G` free.

## Volume Inspection Summary

`/mnt/HC_Volume_105935807` contains historical Ghost archives:

- `Gho_logs_archive_20260606T103413Z`
- `Gho_logs_archive`
- `Gho-archive`
- `Gho_r22_preflight_archive_20260610T014408Z`
- `Gho_r22_preflight_archive_20260610T015644Z`
- `selector_lifecycle_launcher_r8_20260606T1036Z.tar.zst`

These were not deleted because they look like historical evidence archives.

Clearly useless items removed from the volume:

```text
/mnt/HC_Volume_105935807/test.txt
/mnt/HC_Volume_105935807/archive_check
```

`test.txt` was a zero-byte test file. `archive_check` was an empty directory.

## Archived And Moved: R24 Raw Evidence

Archived source paths:

```text
logs/rollout/shadow-burnin-v3-gk-edge-fresh-validation-r24
logs/nln_capture/shadow-burnin-v3-gk-edge-fresh-validation-r24
datasets/events/shadow-burnin-v3-gk-edge-fresh-validation-r24
```

Original source sizes before archive:

```text
2.9G logs/rollout/shadow-burnin-v3-gk-edge-fresh-validation-r24
965M logs/nln_capture/shadow-burnin-v3-gk-edge-fresh-validation-r24
504M datasets/events/shadow-burnin-v3-gk-edge-fresh-validation-r24
```

Archive directory:

```text
/mnt/HC_Volume_105935807/Gho_r24_raw_evidence_archive_20260612T211110Z
```

Archive files:

```text
/mnt/HC_Volume_105935807/Gho_r24_raw_evidence_archive_20260612T211110Z/R24_RAW_EVIDENCE_ARCHIVE_MANIFEST.txt
/mnt/HC_Volume_105935807/Gho_r24_raw_evidence_archive_20260612T211110Z/r24_raw_rollout_nln_events_20260612T211110Z.tar.zst
/mnt/HC_Volume_105935807/Gho_r24_raw_evidence_archive_20260612T211110Z/r24_raw_rollout_nln_events_20260612T211110Z.tar.zst.sha256
```

Archive size:

```text
485M
```

Checksum verification:

```text
/mnt/HC_Volume_105935807/Gho_r24_raw_evidence_archive_20260612T211110Z/r24_raw_rollout_nln_events_20260612T211110Z.tar.zst: OK
```

The archive was also checked with:

```text
tar --zstd -tf /mnt/HC_Volume_105935807/Gho_r24_raw_evidence_archive_20260612T211110Z/r24_raw_rollout_nln_events_20260612T211110Z.tar.zst
```

After successful checksum and archive listing, original source paths were
deleted from `/root/Gho`.

Confirmed missing after delete:

```text
MISSING logs/rollout/shadow-burnin-v3-gk-edge-fresh-validation-r24
MISSING logs/nln_capture/shadow-burnin-v3-gk-edge-fresh-validation-r24
MISSING datasets/events/shadow-burnin-v3-gk-edge-fresh-validation-r24
```

## Archived And Moved: Funding Lane Smoke R1 Logs

Archived source paths:

```text
logs/rollout/shadow-burnin-v3-funding-lane-smoke-r1
logs/nln_capture/shadow-burnin-v3-funding-lane-smoke-r1
logs/shadow_run/shadow-burnin-v3-funding-lane-smoke-r1
logs/shadow_run/shadow-burnin-v3-funding-lane-smoke-r1-buys.jsonl
```

Original source sizes before archive:

```text
99M logs/rollout/shadow-burnin-v3-funding-lane-smoke-r1
22M logs/nln_capture/shadow-burnin-v3-funding-lane-smoke-r1
60K logs/shadow_run/shadow-burnin-v3-funding-lane-smoke-r1
24K logs/shadow_run/shadow-burnin-v3-funding-lane-smoke-r1-buys.jsonl
```

Archive directory:

```text
/mnt/HC_Volume_105935807/Gho_funding_lane_smoke_r1_archive_20260612T211320Z
```

Archive files:

```text
/mnt/HC_Volume_105935807/Gho_funding_lane_smoke_r1_archive_20260612T211320Z/FUNDING_LANE_SMOKE_R1_ARCHIVE_MANIFEST.txt
/mnt/HC_Volume_105935807/Gho_funding_lane_smoke_r1_archive_20260612T211320Z/funding_lane_smoke_r1_logs_20260612.tar.zst
/mnt/HC_Volume_105935807/Gho_funding_lane_smoke_r1_archive_20260612T211320Z/funding_lane_smoke_r1_logs_20260612.tar.zst.sha256
```

Archive size:

```text
13M
```

Checksum verification:

```text
/mnt/HC_Volume_105935807/Gho_funding_lane_smoke_r1_archive_20260612T211320Z/funding_lane_smoke_r1_logs_20260612.tar.zst: OK
```

The archive was also checked with:

```text
tar --zstd -tf /mnt/HC_Volume_105935807/Gho_funding_lane_smoke_r1_archive_20260612T211320Z/funding_lane_smoke_r1_logs_20260612.tar.zst
```

After successful checksum and archive listing, original source paths were
deleted from `/root/Gho`.

Confirmed missing after delete:

```text
MISSING logs/rollout/shadow-burnin-v3-funding-lane-smoke-r1
MISSING logs/nln_capture/shadow-burnin-v3-funding-lane-smoke-r1
MISSING logs/shadow_run/shadow-burnin-v3-funding-lane-smoke-r1
MISSING logs/shadow_run/shadow-burnin-v3-funding-lane-smoke-r1-buys.jsonl
```

## Deliberately Not Moved / Not Deleted

Large but preserved:

```text
logs/rollout/shadow-burnin-v3-score-tail-v1-r1
logs/rollout/shadow-burnin-v3-gk-edge-fresh-validation-r25
logs/nln_capture/shadow-burnin-v3-score-tail-v1-r1
logs/nln_capture/shadow-burnin-v3-gk-edge-fresh-validation-r25
datasets/events/shadow-burnin-v3-score-tail-v1-r1
datasets/events/shadow-burnin-v3-gk-edge-fresh-validation-r25
```

Reason:

- R23/R25 raw evidence remains high-value for current selector/business-label
  analysis lineage.
- No direct matching archive/checksum proof was found on the volume during this
  pass.
- The volume has only about `16G` free, so it cannot safely receive the largest
  raw R23/R25 folders without a separate archive/compression plan.

## Result

Operation PASS:

- R24 raw evidence moved to `/mnt/HC_Volume_105935807` as compressed archive
  with manifest and checksum.
- Funding-lane smoke R1 logs moved to `/mnt/HC_Volume_105935807` as compressed
  archive with manifest and checksum.
- Useless zero-byte/test volume items removed.
- Current `/root/Gho` free space: about `37G`.
- Current `/mnt/HC_Volume_105935807` free space: about `16G`.

