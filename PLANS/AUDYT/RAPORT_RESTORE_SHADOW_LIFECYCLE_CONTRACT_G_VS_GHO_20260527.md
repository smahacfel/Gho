# RAPORT RESTORE SHADOW LIFECYCLE CONTRACT G VS GHO 20260527

## Summary

To jest Phase 1: contract verification + deterministic compact outcome projection + fixture.
To nie jest pelny runtime restore.

Cel tej fazy:

```text
shadow_transport_log
-> shadow_entry_log
-> shadow_lifecycle_log
-> DIAG_ACCOUNT_UPDATE_RELAY truth
-> shadow_onchain_lifecycle_report full JSONL
-> raportneu-style compact outcome artifact
```

Werdykt tej fazy:

```text
CONTRACT RESTORE: expected PASS after this patch and validation
RUNTIME PROOF: expected PENDING until complete active artifacts exist
```

`raportneu.json` nie jest zrodlem prawdy. Zrodlem prawdy pozostaje pelny row-level JSONL z
`scripts/shadow_onchain_lifecycle_report.py`. `raportneu.json` jest stratna, deterministyczna
projekcja compact outcome z pelnego lifecycle row.

## Compared Revisions

| Repo | Revision | Worktree status |
| --- | --- | --- |
| `/root/G` | `4210670dc8c86ac105cda96a17557832ab2a523e` | Dirty runtime artifacts: `logs/shadow_run/shadow-burnin/*`; untracked reports including `reports/raportneu.json`. Code reference was read from commit object. |
| `/root/Gho` | `a45fa8e4ad8de1a06bca18f91203b5d4418149f6` | Dirty before this patch: config/X9/BCV2 files and untracked audit docs. This patch does not touch those files. |

Important current `Gho` runtime fact:

```text
logs/shadow_run/shadow-burnin-v3-p1-buys.jsonl: 1 row
logs/shadow_run/shadow-burnin-v3-p1/shadow_entries.jsonl: missing
logs/shadow_run/shadow-burnin-v3-p1/shadow_lifecycle.jsonl: missing
transport row error_class: transport
transport err: Failed to fetch payer balance: RPC request error: cluster version query failed: builder error: relative URL without a base
```

Therefore the current active profile can produce `0` lifecycle report rows. That is a runtime-input blocker, not a successful restore proof.

## G vs Gho Table

| Element | Repo G reference | Repo Gho current | Status | Action |
| --- | --- | --- | --- | --- |
| `scripts/shadow_onchain_lifecycle_report.py` | Exists at commit `4210670`; generates full row-level JSONL from transport/entry/lifecycle/DIAG. | Exists; has additive probe-plane and join metadata support. This patch adds optional compact projection only. | OK / additive | Keep full JSONL as source of truth; add optional `--outcome-summary-output`. |
| `scripts/shadow_onchain_lifecycle_report2.py` | Exists; variant with extra fee/neighbor diagnostics. | Exists; small additive gatekeeper-log path changes vs G. | OK / role-risk | Do not modify in Phase 1 unless `py_compile` fails. |
| `scripts/shadow_run_report.py` | Exists; resolves transport/lifecycle artifacts and burn-in gates. | Exists; additive decision-plane path discovery and no-dispatch classification. | OK / additive | Do not modify unless strictly necessary. |
| `trigger.shadow_run.output_path` | `configs/shadow-burnin.toml` writes `/root/G/logs/shadow_run/shadow-burnin-buys.jsonl`. Default is `logs/shadow_run/buys.jsonl`. | `configs/rollout/shadow-burnin.toml` points at `../../logs/shadow_run/shadow-burnin-v3-p1-buys.jsonl`. | Present | Preserve transport contract. Current v3-p1 transport row is an error. |
| `execution.shadow.entry_log_path` | `/root/G/logs/shadow_run/shadow-burnin/shadow_entries.jsonl`. | `../../logs/shadow_run/shadow-burnin-v3-p1/shadow_entries.jsonl`. | Config present / artifact missing | Runtime proof remains pending until writer produces entries. |
| `execution.shadow.lifecycle_log_path` | `/root/G/logs/shadow_run/shadow-burnin/shadow_lifecycle.jsonl`. | `../../logs/shadow_run/shadow-burnin-v3-p1/shadow_lifecycle.jsonl`. | Config present / artifact missing | Runtime proof remains pending until writer produces lifecycle rows. |
| `DIAG_ACCOUNT_UPDATE_RELAY` | Emitted in `ghost-launcher/src/components/seer.rs`; fields match report regex. | Emitted in `ghost-launcher/src/components/seer.rs`; same marker and field order, plus trailing `replayed_from_session_buffer`. | OK / format fragile | Preserve marker and regex; do not replace with market API. |
| Full lifecycle JSONL output | Historical reports exist under `/root/G/reports/shadow_onchain_lifecycle_report*.jsonl`. | Historical Gho reports exist in older run dirs; active v3-p1 currently writes 0 rows. | Reporter OK / active input broken | Keep full JSONL as canonical source. |
| `raportneu.json` compact artifact | `/root/G/reports/raportneu.json`, 26 compact rows, close reasons Target/TimeStop/StopLoss. | No equivalent current active v3-p1 compact artifact. | Missing derived artifact | Add deterministic compact projection from full JSONL rows. |

## Input Contract

`resolve_inputs()` must continue to map the reporter inputs as follows:

```text
gatekeeper_buys_log = decision_dir / BUY_LOG_NAME
shadow_transport_log = trigger.shadow_run.output_path
shadow_entry_log = execution.shadow.entry_log_path
shadow_lifecycle_log = execution.shadow.lifecycle_log_path albo derive_shadow_lifecycle_log_path
events_dir = execution.events.output_dir
system_log_base = logging.file_path
```

Required runtime files for a non-empty report:

```text
logs/shadow_run/<scope>-buys.jsonl albo trigger.shadow_run.output_path
logs/shadow_run/<scope>/shadow_entries.jsonl
logs/shadow_run/<scope>/shadow_lifecycle.jsonl
logs/rollout/<scope>/system.log*
logs/rollout/<scope>/decisions/gatekeeper_v2_buys.jsonl
datasets/events/<scope>/*
```

Minimal fields consumed by the reporter:

```text
transport: candidate_id, base_mint, pool_amm_id/pool_id, decision_ts_ms, sim_started_ts_ms, sim_finished_ts_ms, amount_lamports, error_class, live_signature
entry: candidate_id, pool_id, mint_id, entry_price, slot, timestamp_ms, execution_outcome
exit_filled: candidate_id, position_id, pool_id, mint_id, timestamp_ms, sample_timestamp_ms, sample_slot, sample_age_ms, fraction_bps, remaining_fraction_bps, entry_price, exit_price, entry_value_sol, exit_value_sol, truth_status, truth_source, sample_price_state
position_closed: candidate_id, position_id, pool_id, mint_id, timestamp_ms, sample_timestamp_ms, sample_slot, entry_price, entry_value_sol, exit_value_sol, gross_pnl_sol, net_pnl_sol, estimated_costs_sol, final_pnl, final_pnl_pct, duration_ms, close_reason, total_exits, truth_status, truth_source, sample_price_state
DIAG_ACCOUNT_UPDATE_RELAY: timestamp, base_mint, bonding_curve, slot, sol_reserves, token_reserves, complete, curve_finality
```

## Semantic Diff Check

Script-level diff summary after this Phase 1 patch:

```text
shadow_onchain_lifecycle_report.py: additive changes only in current Gho:
  - existing probe-plane and join metadata support
  - new optional --outcome-summary-output compact projection
shadow_onchain_lifecycle_report2.py:
  - additive gatekeeper log path resolution vs G
shadow_run_report.py:
  - additive plane-aware log resolution and no-dispatch classification vs G
```

Mandatory economic/truth function check for `scripts/shadow_onchain_lifecycle_report.py`:

| Function / constant | G lines | Gho lines | Result | Classification |
| --- | --- | --- | --- | --- |
| `choose_shadow_price_multiplier` | 417-431 | 504-518 | Same function body hash | semantic-equivalent |
| `simulate_buy_tokens_raw` | 440-451 | 527-538 | Same function body hash | semantic-equivalent |
| `calculate_sell_sol_out_lamports` | 454-464 | 541-551 | Same function body hash | semantic-equivalent |
| `buy_executable_price_sol` | 467-472 | 554-559 | Same function body hash | semantic-equivalent |
| `sell_executable_price_sol` | 475-480 | 562-567 | Same function body hash | semantic-equivalent |
| `find_causal_truth` | 866-876 | 958-968 | Same function body hash | semantic-equivalent |
| `DIAG_ACCOUNT_UPDATE_RELAY_RE` | 36-42 | 50-56 | Same regex text | semantic-equivalent |

Conclusion: Phase 1 does not change the economics or truth matching path. The new compact projection is derived after full rows are produced.

## Points Matching The Historical Contract

- Transport writer exists in `ghost-launcher/src/components/trigger/shadow_run.rs` as `append_shadow_buy_record`.
- Canonical active shadow entry writer exists in `ghost-launcher/src/oracle_runtime.rs` as `append_shadow_entry_record` / `maybe_append_canonical_shadow_entry_record`.
- Dispatch lifecycle writer exists in `ghost-launcher/src/components/trigger/shadow_run.rs` as `append_shadow_dispatch_lifecycle_record`.
- Post-buy lifecycle writer exists in `ghost-brain/src/guardian/post_buy/engine.rs` as `append_shadow_lifecycle_record`.
- Lifecycle record types include `exit_filled` and `position_closed`.
- `DIAG_ACCOUNT_UPDATE_RELAY` still emits `base_mint`, `bonding_curve`, `slot`, `sol_reserves`, `token_reserves`, `complete`, `curve_finality`.
- Full lifecycle JSONL remains the canonical output.

## Broken Or Risky Points

- Active `shadow-burnin-v3-p1` lacks `shadow_entries.jsonl` and `shadow_lifecycle.jsonl`.
- Active `shadow-burnin-v3-p1` has only one transport row and it is `error_class=transport`.
- Dirty `configs/rollout/shadow-burnin.toml` contains hardcoded endpoint changes and a `chainstawck.com` typo. This patch intentionally does not touch that file.
- The DIAG regex is intentionally strict about field order. Current tracing output matches the historical format, but future field-order drift would break the parser.
- `shadow_onchain_lifecycle_report2.py` is not changed in Phase 1; its exact active/probe role remains documented as a risk rather than silently repaired.
- `/root/G/reports/raportneu.json` has no row with `fills.length >= 2`; multi-fill is covered by a synthetic full lifecycle row in the unit test.

## Runtime Restore Gap

This Phase 1 patch should be closed only as:

```text
CONTRACT RESTORE: PASS
RUNTIME PROOF: PENDING
```

Runtime proof requires a later checkpoint with complete active artifacts:

```text
trigger.shadow_run.output_path has successful transport rows without error_class
execution.shadow.entry_log_path exists and contains matching shadow entries
execution.shadow.lifecycle_log_path exists and contains exit_filled and position_closed rows
system.log* contains matching DIAG_ACCOUNT_UPDATE_RELAY rows
shadow_onchain_lifecycle_report.py writes >0 full JSONL rows
optional compact projection writes raportneu-style JSON from those rows
```

Do not claim that shadow-burnin works from this Phase 1 patch alone.

## Minimal Patch Plan Implemented

- Add deterministic compact outcome projection helper functions in `scripts/shadow_onchain_lifecycle_report.py`.
- Add optional `--outcome-summary-output`.
- Add `tests/fixtures/shadow_lifecycle/raportneu_sample.json` from `/root/G/reports/raportneu.json`.
- Add `scripts/test_shadow_onchain_lifecycle_report_contract.py`.
- Add this report.

## Commands

Full lifecycle JSONL:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --output /tmp/restore_shadow_lifecycle_report.jsonl
```

Full lifecycle JSONL plus compact outcome projection:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --output /tmp/restore_shadow_lifecycle_report.jsonl \
  --outcome-summary-output /tmp/restore_raportneu.json
```

Expected current active v3-p1 result:

```text
0 rows is expected until runtime inputs are complete.
0 rows is not a successful runtime restore proof.
```

## Verification

Required Phase 1 validation:

```bash
python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py
python3 -m py_compile scripts/shadow_onchain_lifecycle_report2.py
python3 -m py_compile scripts/shadow_run_report.py
python3 -m py_compile scripts/test_shadow_onchain_lifecycle_report_contract.py
python3 -m unittest scripts/test_shadow_onchain_lifecycle_report_contract.py -v
git diff --check
```

Executed validation result:

```text
python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py: PASS
python3 -m py_compile scripts/shadow_onchain_lifecycle_report2.py: PASS
python3 -m py_compile scripts/shadow_run_report.py: PASS
python3 -m py_compile scripts/test_shadow_onchain_lifecycle_report_contract.py: PASS
python3 -m unittest scripts/test_shadow_onchain_lifecycle_report_contract.py -v: PASS, 4 tests
git diff --check: PASS
```

Optional runtime-input smoke, only meaningful when active artifacts are complete:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --output /tmp/restore_shadow_lifecycle_report.jsonl \
  --outcome-summary-output /tmp/restore_raportneu.json
```

Executed current active v3-p1 smoke result:

```text
exit_code=0
rows_written=0
/tmp/restore_shadow_lifecycle_report.jsonl: 0 lines
/tmp/restore_raportneu.json: []
interpretation: runtime input blocker, not runtime proof
```

## Non-Goals

Do not touch:

```text
BCV2 readiness
route universe
builder
Sender / Helius Sender
Gatekeeper
V3/scoring
execution route
buy/sell math
configs/rollout/shadow-burnin.toml dirty endpoint changes
shadow_onchain_lifecycle_report2.py, unless py_compile fails
```

## Delegation Trace

```yaml
delegation_trace:
  task_classification: "phase-1 restore implementation for shadow lifecycle reporting contract"
  routing_performed: true
  primary_specialist: "Decision Logging Replay Analyst"
  supporting_specialists_considered:
    - "Ghost Runtime Coordinator"
    - "Oracle Session Runtime Engineer"
    - "Seer Ingest Event Integrity Specialist"
    - "Config Rollout Safety Reviewer"
    - "Solana Execution Path Engineer"
  specialist_docs_loaded:
    - "/root/Gho/AGENTS.md"
    - "/root/Gho/docs/agents/decision-logging-replay-analyst.md"
    - "/root/Gho/docs/agents/oracle-session-runtime-engineer.md"
    - "/root/Gho/docs/agents/seer-ingest-event-integrity-specialist.md"
    - "/root/Gho/docs/agents/config-rollout-safety-reviewer.md"
  specialist_docs_not_loaded:
    - name: "docs/agents/solana-execution-path-engineer.md"
      reason: "Execution builder and Sender changes are explicitly out of scope."
    - name: "docs/agents/gatekeeper-policy-auditor.md"
      reason: "Gatekeeper policy changes are explicitly out of scope."
    - name: "docs/agents/ssot-feature-materialization-guardian.md"
      reason: "No MaterializedFeatureSet or feature ownership change is planned."
  skills_used:
    - "ghost-execution"
    - "rust-master"
    - "solana-pumpfun-architect"
    - "trading-systems"
    - "abstract-reasoning"
  fast_path_used: false
  contracts_checked:
    - "shadow/live separation"
    - "trigger.shadow_run transport artifact"
    - "execution.shadow canonical entry artifact"
    - "execution.shadow lifecycle artifact"
    - "DIAG_ACCOUNT_UPDATE_RELAY parse contract"
    - "Gatekeeper BUY context lookup as report input"
    - "full lifecycle JSONL as source of truth"
    - "raportneu-style compact projection as derived artifact"
  unresolved_routing_uncertainty:
    - "Runtime proof remains pending until complete active artifacts exist."
```
