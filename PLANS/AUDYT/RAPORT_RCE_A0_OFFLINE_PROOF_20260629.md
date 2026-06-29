# PR-RCE-A0: Offline proof

Data: `2026-06-29`

Final verdict: `RCE_BLOCKED_BY_DATA`

## Decyzja

GO_R51_LOGGING_ONLY wymaga osobnej zgody; bez niej NO_GO_CLOSE_PROJECT.

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

## Boundary

To jest offline-only proof. Skrypt nie zmienia runtime, Gatekeepera, BUY/REJECT, selector runtime, `v25_confidence`, V3 promotion, TX/Jito/live path, `shadow_close_only`, active close, `alpha_31100` ani XGBoost.

## Evidence

| scope | has_decision_log | has_exit_replay | has_pre_entry_path_summary_v1 | has_session_regime_snapshot_v1 | full_rce_surface |
| --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | False | True | False | False | False |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | True | True | False | False | False |
| shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1 | False | False | False | False | False |

Full RCE surface scopes: ``

Full RCE surface scope count: `0`

## Templates

- `T1_BREAKOUT_RETEST_RECLAIM`
- `T2_STAIRSTEP_CONTINUATION`
- `T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY`

## Fixed grid

- target_bps: `600, 900, 1200`
- stop_bps: `-250, -400, -600`
- max_hold_ms: `10000, 20000, 30000`
- costs_bps: `100, 200`

## Best rows

Brak metryk: full RCE evidence surface jest niedostepny.

## Acceptance

Passing fixed rules across two scopes: `0`

Single-scope passing rules: `0`

Best rule: `` on ``

## Required next step

Istniejace R49/R50 logs nie zawieraja wymaganej RCE surface. Jedyny dopuszczalny nastepny krok to osobno zatwierdzony R51 logging-only scope. Bez zgody sponsora na jeden taki scope projektowy trading edge search nalezy zamknac.
