# ADR-8D: Shadow V2 Fidelity Validation Burnin PR18C 45M

## Status

Accepted as validation/fidelity-only burnin evidence.

Final verdict:

`PASS`

## D1. Problem

Po PR18/PR17B canonical V2 fill/path/terminal emission oraz PR27/PR18C shutdown + manifest flush fix trzeba bylo sprawdzic runtime w kontrolowanym burninie. Poprzedni problem polegal na tym, ze shutdown mogl nie domykac `PostBuyRuntime` i runtime post-run manifest mogl nie powstac przed koncem launcher component join.

Wymagana byla walidacja, czy po SIGINT runtime potrafi:

- zakonczyc `PostBuyRuntime` bez abortu;
- wygenerowac runtime `post_run_manifest.json`;
- przejsc post-run strict audit;
- domknac wszystkie komponenty;
- zachowac Shadow V2 evidence bez stage'owania raw JSONL/logow.

## D2. Decyzja

Akceptujemy wynik runu:

`shadow-burnin-v2-fidelity-validation-pr18c-45m-r1`

jako:

`PASS`

Zakres akceptacji jest waski:

- validation/fidelity-only;
- logging-only Shadow V2 evidence;
- proof of runtime manifest flush;
- proof of clean shutdown;
- proof of canonical V2 event family presence.

Nie przyznajemy:

- `research_grade`;
- `live_equivalence`;
- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- strategy research unblocked.

## D3. Kontekst

PR27:

- PR head: `000c81def0929bc65e9d18ee16df59b6966a5d35`;
- merge commit: `c44dfc321a72faa34828f81e7fb4aaa8a7fc3422`;
- local main HEAD for run: `c44dfc321a72faa34828f81e7fb4aaa8a7fc3422`.

Run:

- run_id: `shadow-burnin-v2-fidelity-validation-pr18c-45m-r1`;
- scope root: `reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1`;
- local launcher config: `configs/rollout/shadow-v2-fidelity-validation-pr18c-45m-r1.local.toml`;
- local brain config: `configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr18c_45m_r1.local.toml`.

Local validation profile:

- `min_tx_count=5`;
- `min_buy_count=3`;
- `min_unique_signers=3`;
- `min_market_cap_sol=5.0`;
- `shadow_v2_burnin.enabled=true`;
- `shadow_v2_burnin.logging_only=true`;
- `post_run_manifest_drain_timeout_ms=180000`;
- all approval/proof flags false.

## D4. Dowody

Pre-run gates:

- pre-run manifest generation: `PASS`;
- pre-run strict audit: `PASS`;
- validation burnin plan audit: `PASS`;
- `validation_mode=FIDELITY_ONLY`;
- legacy downgrade audit: `PASS`;
- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher `--preflight`: `PASS`.

Runtime:

- run start: `2026-07-02T10:50:10Z`;
- SIGINT sent: `2026-07-02T11:40:01Z`;
- final shutdown: `2026-07-02T11:40:35.459Z`;
- controller exit status: `0`;
- shutdown method: `SIGINT`;
- `SIGTERM=false`.

Post-run:

- runtime `post_run_manifest.status=PASS`;
- runtime manifest blockers: `[]`;
- post-run strict audit: `PASS`;
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`: present;
- `PostBuyRuntime shut down successfully`: present;
- `All components shut down successfully`: present;
- `Ghost Launcher shutdown complete`: present;
- forced component abort: not observed;
- `SHADOW_V2_POST_RUN_MANIFEST_DRAIN_TIMEOUT`: not observed.

Canonical evidence:

- canonical rows: `1022`;
- malformed canonical rows: `0`;
- unique positions total: `130`;
- diagnostic smoke positions: `1`;
- real_shadow_v2_positions: `129`.

Canonical event families:

- `shadow_position_v2`: `130`;
- `shadow_entry_attempt_v2`: `127`;
- `shadow_entry_fill_v2`: `127`;
- `shadow_path_sample_v2`: `255`;
- `shadow_exit_attempt_v2`: `127`;
- `shadow_exit_fill_v2`: `127`;
- `shadow_terminal_truth_v2`: `129`.

Derived evidence:

- `shadow_replay_v2.jsonl`: `1022` rows;
- `shadow_lifecycle_v2.jsonl`: `1022` rows;
- `shadow_path_density_v2.jsonl`: `7154` rows.

## D5. Root Cause Classification

PR27/PR18C fixed the previously blocking shutdown/manifest flush risk.

Observed status in this burnin:

`POST_BUY_RUNTIME_MANIFEST_FLUSH_PASS`

`LAUNCHER_CLEAN_SHUTDOWN_PASS`

`CANONICAL_V2_EVENT_FAMILIES_PRESENT`

Remaining limitation:

`FILL_ROWS_PRESENT_BUT_BLOCKED_BY_DATA`

This means canonical V2 fill schemas are emitted, but they do not yet prove executable entry/exit fills because pool-state provenance and live fill telemetry are incomplete for those records.

## D6. Konsekwencje

Allowed conclusions:

- Shadow V2 logging-only runtime harness can produce V2 canonical evidence under a relaxed validation sampling profile.
- Runtime can flush and strict-verify the post-run manifest before clean shutdown.
- PR27 shutdown/manifest fix is validated under live runtime load.
- V2 event family presence gates for PR18C are satisfied.

Disallowed conclusions:

- no live-equivalent PnL claim;
- no executable fill claim;
- no runtime approval;
- no active close approval;
- no `shadow_close_only` approval;
- no strategy edge proof;
- no RCE proof;
- no research-grade claim without separate reconstruction/reconciliation audit.

## D7. Runtime Boundary

No runtime behavior was approved or promoted by this ADR.

No changes were made to:

- BUY/REJECT code;
- Gatekeeper policy code;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close.

R51 was not touched.

Raw JSONL/log/runtime scopes remain local and are not part of the report-only PR.

## D8. Required Follow-Up

Before any stronger claim, run separate offline audits against this evidence:

1. entry reconstruction readiness and typed `BLOCKED_BY_DATA` reasons;
2. exit reconstruction readiness and typed `BLOCKED_BY_DATA` reasons;
3. replay/lifecycle terminal reconciliation from canonical V2 evidence;
4. path density evaluability by horizon;
5. temporal/no-lookahead evidence audit;
6. manifest/retention audit for the run scope.

Only after those audits may an operator consider:

- `runtime_approval_candidate=true_for_shadow_v2_logging_validation_only`;
- `strategy_research_unblocked_candidate=true_for_offline_reconstruction_only`.

Those remain candidates, not granted approvals.
