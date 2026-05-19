# P3.7 Phase E Shadow-Onchain Report Recovery

Date: 2026-05-19
Status: active execution plan

## Decision

GO: run Phase E on existing shadow-burnin runtime artifacts.

HOLD: controlled Phase D smoke run.

NO-GO: Phase B feature prototype, P2/live changes, threshold tuning, IWIM changes, live sender changes, FSC active gate.

## Goal

Generate `shadow_onchain_lifecycle_report` for existing discovered shadow-burnin namespaces before running a new smoke profile. The immediate target is to recover execution/lifecycle truth from already available runtime artifacts.

This phase must not treat shadow simulation as live inclusion and must not infer strategy edge from lifecycle proof.

## Primary Target

Namespace: `shadow-burnin-buy-heavy-rerun`

Config:

Runtime/source config:

`configs/rollout/shadow-burnin-buy-heavy.local.toml`

Report-only recovery config:

`configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml`

The report-only config is required because the runtime config points
`oracle.decision_log_path` to `logs/rollout/shadow-burnin-buy-heavy-rerun/decisions`, while the historical `gatekeeper_v2_buys.jsonl` files currently available on the VPS are stored directly under `logs/rollout/shadow-burnin-buy-heavy-rerun/v2.2/...` and `logs/rollout/shadow-burnin-buy-heavy-rerun/v2.5/...`.

The report-only config must not be used for runtime.

Existing artifacts:

- shadow entries: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_entries.jsonl`
- shadow lifecycle: `logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_lifecycle.jsonl`
- shadow transport: `logs/shadow_run/shadow-burnin-buy-heavy-rerun-buys.jsonl`
- decision dir: `logs/rollout/shadow-burnin-buy-heavy-rerun/decisions`
- events dir: `datasets/events/shadow-burnin-buy-heavy-rerun`
- system log base: `logs/rollout/shadow-burnin-buy-heavy-rerun/system.log`

Expected output:

`logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all.jsonl`

## Secondary Target

Namespace: `shadow-burnin-v3-p36-sample-r13-primary-only`

Config:

`configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml`

Use only as a small sanity/fail-closed case after the primary namespace. It is not sufficient as the main evidence lane because it has only one entry/lifecycle/transport row.

## Fallback

If no existing namespace can produce resolved lifecycle truth rows, keep Phase E as blocked and run Phase D controlled lifecycle smoke collection.

## Commands

Primary report recovery, no hard truth-gap filter:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml \
  --all-sessions \
  --output /root/Gho/logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all.jsonl
```

Primary summary:

```bash
python3 scripts/v3_p37_shadow_onchain_lifecycle_summary.py \
  --input logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all.jsonl \
  --namespace shadow-burnin-buy-heavy-rerun \
  --config configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml \
  --output-md PLANS/AUDYT/RAPORT_P3_7_SHADOW_ONCHAIN_LIFECYCLE_RECOVERY_20260519.md \
  --output-json logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_all_summary.json
```

Optional sensitivity runs, after the full unfiltered report:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml \
  --all-sessions \
  --max-truth-gap-ms 1000 \
  --output /root/Gho/logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_gap_1000ms.jsonl

python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml \
  --all-sessions \
  --max-truth-gap-ms 5000 \
  --output /root/Gho/logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_gap_5000ms.jsonl

python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-buy-heavy-rerun-report-only.toml \
  --all-sessions \
  --max-truth-gap-ms 30000 \
  --output /root/Gho/logs/shadow_run/shadow-burnin-buy-heavy-rerun/shadow_onchain_lifecycle_report_gap_30000ms.jsonl
```

The sensitivity runs are not the acceptance source. The first source is the full unfiltered report, because entry/exit gap semantics must later be labeler-aware and close-reason-aware.

## Summary Requirements

The recovery summary must report at minimum:

- rows total
- `analysis_status` counts
- `truth_status` counts
- `truth_source` counts
- entry `curve_finality` counts
- exit `curve_finality` counts
- position-closed row count
- exit-filled row count
- positive, negative, and neutral final PnL row counts
- entry truth-gap distribution
- exit truth-gap distribution
- close-reason counts
- `gatekeeper_buy_context_found` count
- shadow `execution_outcome` counts
- entry drift vs on-chain executable distribution
- exit drift vs on-chain executable distribution
- `decision_to_execution_ms` distribution
- `detection_to_execution_ms` distribution

## Finality Semantics

`curve_finality=speculative` or other non-confirmed/non-finalized values are snapshot proof only. They must not be promoted to finalized proof.

Allowed interpretation:

- `finalized` => `shadow_onchain_finalized_verified`
- `confirmed` => `shadow_onchain_confirmed_verified`
- `speculative` => `shadow_onchain_speculative_snapshot_verified`
- missing or non-standard finality => degraded/unknown proof until classified by the labeler

## Truth-Gap Semantics

The first report must run without `--max-truth-gap-ms` to expose the full distribution.

Later labeler work must use separate thresholds for:

- entry truth gap
- exit truth gap
- exit truth gap by close reason

An exit gap around 30s can be degraded acceptable for TimeStop, but it is not clean. The hard CLI `--max-truth-gap-ms` is only a sensitivity tool.

## Acceptance

Phase E recovery is accepted if the primary target or another large existing namespace produces:

- generated `shadow_onchain_lifecycle_report`
- `rows_total > 0`
- `analysis_status.ok > 0`
- `truth_status.resolved > 0`
- `position_closed > 0`
- `gatekeeper_buy_context_found > 0`
- entry and exit truth-gap distributions
- curve finality distribution
- positive and negative PnL row counts

Phase E recovery is not accepted if:

- config path mismatch prevents loading artifacts
- system log or DIAG truth is missing
- shadow lifecycle is missing
- shadow entries are missing
- no resolved truth rows are produced
- all rows fail analysis

## Next Decision

If recovery succeeds, proceed to the shadow lifecycle labeler and feature availability audit.

If recovery fails, run Phase D controlled shadow-burnin lifecycle smoke collection.
