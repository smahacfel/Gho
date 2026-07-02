# ADR-8D: Shadow V2 Offline Reconstruction Audits PR18D

## Status

Accepted as blocked offline reconstruction/readiness evidence.

Final verdict:

`BLOCKED_EXECUTABLE_FILL_PROVENANCE_MISSING`

## D1. Problem

PR18C potwierdzil runtime emission, post-run manifest flush i clean shutdown dla Shadow V2 45m burnina. To nadal nie odpowiada na pytanie, czy istniejacy PR18C scope jest gotowy do offline reconstruction i ograniczonych research conclusions.

Wymagane byly offline audyty na istniejacych lokalnych artefaktach, bez uruchamiania kolejnego burnina i bez zmian runtime.

## D2. Decyzja

Dodajemy deterministyczne offline audit scripts i raport PR18D.

Akceptujemy nastepujacy stan:

- manifest/retention: pass;
- replay/lifecycle terminal reconciliation: pass;
- entry reconstruction readiness: blocked;
- exit reconstruction readiness: blocked;
- path density horizon evaluability: blocked;
- temporal/no-lookahead: blocked by ambiguity, not failed by violation.

Final decision:

`BLOCKED_EXECUTABLE_FILL_PROVENANCE_MISSING`

Nie przyznajemy:

- `research_grade`;
- `live_equivalence`;
- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- strategy research unblocked.

## D3. Kontekst

Audited scope:

`reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1`

Main baseline after PR28 merge:

`8d804824febaf0ff4d45570319d8009008444a10`

Input artifacts:

- `shadow_position_event_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- `pre_run_manifest.json`;
- `post_run_manifest.json`;
- `shadow_v2_manifest_report.csv`.

No runtime was started. No burnin was started. Raw JSONL/log/runtime scope artifacts remain local and uncommitted.

## D4. Dowody

Added scripts:

- `scripts/shadow_v2_offline_audit_common.py`;
- `scripts/shadow_v2_entry_reconstruction_readiness_audit.py`;
- `scripts/shadow_v2_exit_reconstruction_readiness_audit.py`;
- `scripts/shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py`;
- `scripts/shadow_v2_path_density_horizon_audit.py`;
- `scripts/shadow_v2_temporal_no_lookahead_audit.py`;
- `scripts/shadow_v2_manifest_retention_audit.py`.

Validation:

- `python3 -m py_compile ...`: `PASS`;
- entry audit: `BLOCKED_ENTRY_FILLS_BLOCKED_BY_DATA`;
- exit audit: `BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA`;
- replay/lifecycle audit: `PASS_REPLAY_LIFECYCLE_RECONCILED`;
- path density audit: `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS`;
- temporal/no-lookahead audit: `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`;
- manifest/retention audit: `PASS_MANIFEST_RETENTION_AUDIT`;
- post-run strict manifest audit: `PASS`.

Key metrics:

- `shadow_entry_attempt_v2` rows: `127`;
- `shadow_entry_fill_v2` rows: `127`;
- entry reconstruction ready count: `0`;
- entry fill `BLOCKED_BY_DATA` rows: `127`;
- `shadow_exit_attempt_v2` rows: `127`;
- `shadow_exit_fill_v2` rows: `127`;
- exit reconstruction ready count: `0`;
- exit fill `BLOCKED_BY_DATA` rows: `127`;
- terminal truth rows: `129`;
- terminal truth with `final_pnl_mark_bps`: `128`;
- terminal truth with `final_pnl_executable_bps`: `0`;
- replay/lifecycle terminal exact joins: `129`;
- replay/lifecycle mismatches: `0`;
- density rows: `7154`;
- density evaluable rows: `0`;
- malformed canonical rows: `0`.

## D5. Root Cause Classification

Primary blocker:

`EXECUTABLE_FILL_PROVENANCE_MISSING`

Specific reasons:

- entry fill pool state before/after missing;
- entry fill price missing;
- entry slippage/own impact/fee fields missing;
- exit fill pool state before/after missing;
- exit fill price missing;
- exit slippage/own impact/fee fields missing;
- terminal executable PnL missing.

Secondary blockers:

- path density not evaluable for required horizons;
- explicit UNKNOWN chain-order components remain;
- some event families do not have event_order_key.

## D6. Konsekwencje

Allowed:

- Use PR18C scope for debugging Shadow V2 canonical/derived logging.
- Use replay/lifecycle reconciliation result as evidence that derived terminal snapshots agree with canonical terminal high-watermarks.
- Use manifest/retention result as evidence that raw artifacts are present locally and not required in git.

Not allowed:

- no executable fill claim;
- no live-equivalent PnL claim;
- no live slippage/landing/no-fill claim;
- no research-grade claim;
- no strategy proof;
- no RCE proof;
- no runtime approval;
- no `shadow_close_only`;
- no active close.

## D7. Runtime Boundary

This PR is offline-only.

It does not change:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval flags.

R51 is not touched.

## D8. Required Follow-Up

Before any stronger Shadow V2 research claim:

1. emit or link canonical `pool_state_sample_v2` for entry/exit fill boundaries;
2. populate executable fill fields or typed no-fill/failure fields;
3. provide `final_pnl_executable_bps` where executable fill is available;
4. improve path density coverage for required horizons or mark horizon-specific research blocked;
5. reduce UNKNOWN chain-order components where ordering-sensitive conclusions are required;
6. rerun PR18D audits and require:
   - `PASS_ENTRY_RECONSTRUCTION_READY`;
   - `PASS_EXIT_RECONSTRUCTION_READY`;
   - `PASS_DENSITY_EVALUABLE_FOR_REQUIRED_HORIZONS`;
   - `PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT`;
   - `PASS_REPLAY_LIFECYCLE_RECONCILED`;
   - `PASS_MANIFEST_RETENTION_AUDIT`.
