# ADR-8D: Shadow V2 Fidelity Validation Burnin PR17 R2 Relaxed Sampling

## Status

Accepted as positive real-position validation evidence with limitations.

## D1. Problem

PR17-r1 przeszedl bramki infrastrukturalne, ale byl zablokowany przez brak realnej pozycji Shadow V2:

`BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`

Przyczyna byla jasna: lokalny PR17-r1 `ghost_brain_config` mial bardzo konserwatywne progi samplingowe, m.in. `min_tx_count=55`, `min_unique_signers=41`, `min_buy_count=39`, `min_market_cap_sol=115`. To dawalo zero BUY i tylko diagnostyczny marker.

Potrzebny byl osobny relaxed validation sampling burnin, ktory nie jest produkcyjna polityka ani strategy proof, ale potrafi wygenerowac realny accepted shadow handoff dla Shadow V2 fidelity evidence.

## D2. Decyzja

Akceptujemy PR17-r2 jako:

`PASS_REAL_SHADOW_V2_POSITION_EVIDENCE_PRESENT_WITH_LIMITATIONS`

Profil jest jawnie klasyfikowany jako:

- `RELAXED_VALIDATION_SAMPLING_PROFILE`;
- `NOT_PRODUCTION_POLICY`;
- `NOT_STRATEGY_EVIDENCE`;
- `NOT_EDGE_PROOF`;
- `NOT_RUNTIME_APPROVAL`;
- `NOT_LIVE_EQUIVALENT`.

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- strategy research unblocked.

## D3. Kontekst

Baseline:

`286a1f76f5c5fd632800f60afa6b4be98066eec7`

Run id:

`shadow-burnin-v2-fidelity-validation-pr17-r2-relaxed-sampling`

Scope root:

`reports/selector/shadow-v2-fidelity-validation-pr17-r2-relaxed-sampling`

Uzyto wylacznie lokalnych, niestage'owanych configow:

- `configs/rollout/shadow-v2-fidelity-validation-pr17-r2-relaxed-sampling.local.toml`;
- `configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr17_r2_relaxed_sampling.local.toml`.

Runtime code nie zostal zmieniony.

## D4. Dowody

Pre-run:

- pre-run manifest generation: `PASS`;
- pre-run strict audit: `PASS`;
- validation burnin plan audit: `PASS`;
- legacy downgrade audit: `PASS`;
- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher `--preflight`: `PASS`.

Post-run:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- post-run strict audit: `PASS`;
- clean shutdown: `PASS`;
- `SIGTERM=false`;
- reconnect/disconnect flood: `false`.

Shadow V2 artifacts:

- `shadow_position_event_v2.jsonl`: `2` rows;
- `shadow_replay_v2.jsonl`: `2` rows;
- `shadow_lifecycle_v2.jsonl`: `2` rows;
- `shadow_path_density_v2.jsonl`: `14` rows;
- `real_shadow_v2_positions=1`;
- `diagnostic_marker_positions=1`.

Real position:

- pool_id: `2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une`;
- base_mint: `HhCYSPYhNWmYkzpcjdWnVoDfgPbuHGVGSyomeei4pump`;
- position_id: `2ttPHhPR7rF98eug3F9rjaNR96fsDU2AEArGK5q78Une:HhCYSPYhNWmYkzpcjdWnVoDfgPbuHGVGSyomeei4pump:1782949910476`;
- quality: `VALIDATION_HARNESS_POSITION_CREATED`.

Legacy shadow evidence:

- `shadow_entries.jsonl`: `1` row;
- `buys.jsonl`: `1` row;
- `shadow_lifecycle.jsonl`: `3` rows;
- lifecycle record types:
  - `shadow_dispatch`;
  - `exit_filled`;
  - `position_closed`;
- close_reason: `TimeStop`;
- final_pnl_pct: `-17.154999999999998`.

## D5. Root Cause Classification

PR17-r1 blocker:

`NO_REAL_ACCEPTED_SHADOW_HANDOFF_DURING_PR17_WINDOW`

zostal usuniety przez lokalny relaxed validation sampling profile.

Nowy status:

`REAL_SHADOW_V2_POSITION_EVIDENCE_PRESENT`

Pozostale ograniczenie:

`CANONICAL_V2_FILL_PATH_TERMINAL_RECORDS_NOT_EMITTED`

PR15/PR16 harness nadal emituje minimalny V2 `POSITION_CREATED`, a entry/exit/path/terminal fidelity evidence pozostaje w legacy shadow logs albo nie istnieje jako canonical V2 schema.

## D6. Konsekwencje

Udowodnione:

- logging-only relaxed validation profile generuje realny accepted shadow handoff;
- canonical Shadow V2 writer zapisuje realny position event;
- derived replay/lifecycle sa tied to real position high-watermark;
- post-run manifest i strict audit przechodza;
- clean shutdown dziala;
- legacy shadow lifecycle ma entry/exit/close evidence dla tej pozycji.

Nieudowodnione:

- canonical V2 entry fill;
- canonical V2 exit fill;
- canonical V2 terminal truth;
- V2 path samples;
- V2 density evaluability;
- entry/exit independent reconstruction z V2-only evidence;
- live fill/slippage/landing;
- live-equivalence.

## D7. Runtime Boundary

Nie zmieniono:

- BUY/REJECT runtime code;
- Gatekeeper policy code;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close.

Nie dotykano R51.

Raw JSONL/log/runtime artifacts pozostaja lokalne i nie sa commitowane.

## D8. Required Follow-Up

Przed research-grade fidelity validation wymagany jest kolejny waski etap:

1. emitowac canonical `shadow_entry_attempt_v2` i `shadow_entry_fill_v2`;
2. emitowac canonical `shadow_path_sample_v2`;
3. emitowac canonical `shadow_exit_attempt_v2`, `shadow_exit_fill_v2` i `shadow_terminal_truth_v2`;
4. powtorzyc validation burnin;
5. dopiero wtedy uruchomic reconstruction/reconciliation/density audit na V2-only evidence.

Do tego czasu:

- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`.
