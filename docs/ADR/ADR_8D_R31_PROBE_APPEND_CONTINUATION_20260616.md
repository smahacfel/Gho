# ADR-8D: R31 probe append continuation for existing namespace

Status: Done
Typ: Runtime/config repair
Data: 2026-06-16
Repo/branch: `/root/Gho`, current working tree
Commit/PR: not committed
Zakres: R31 selector lifecycle restart on the existing `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4` namespace
Dotknięte moduły/pliki:
- `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
Powiązane runy/logi/raporty:
- `reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083412Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_lifecycle_guard_20260616T083520Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
Poziom ryzyka: Low/medium operational config risk

## 1. Przygotowanie i działania wstępne

Plan początkowy:
Restart R31 from the same active config and leave the run in `tmux`.

Rzeczywisty przebieg:
The first lifecycle launcher attempt failed during preflight before runtime start. The config pointed at existing R31 r4 probe artifacts while `[p37_shadow_probe].append=false`.

Odchylenia od planu:
The config required one operational continuation change: probe append mode had to be enabled. No namespace, threshold, RPC, Gatekeeper, execution, or artifact path changes were made.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powód użycia: runtime lifecycle restart and shadow evidence preservation.
Zakres użycia: active Ghost/shadow runtime contracts, launcher-owned lifecycle run procedure, shadow/live separation.
Wynik: run restarted through `scripts/start_selector_lifecycle_run.py` and left running after lifecycle proof.
Ograniczenia: no code-level runtime behavior was changed.

## 3. Opis problemu - 3W2H

What:
R31 r4 could not be restarted through the lifecycle launcher with the same config because preflight rejected an existing probe namespace.

Where:
`[p37_shadow_probe]` in `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`.

Why it matters:
Deleting existing artifacts would break continuity. Starting manually would bypass the selector lifecycle runbook. A fresh namespace would no longer be the same R31 r4 continuation.

How observed:
The first launcher attempt wrote `FAIL_PREFLIGHT`.

How many / scale:
The blocking file was the existing probe output namespace, starting with `logs/shadow_run/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/probe_selection.jsonl`.

Evidence:
`preflight.log` contained:
`Error: [p37_shadow_probe] append=false requires a clean probe namespace; output already exists: /root/Gho/logs/shadow_run/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/probe_selection.jsonl`

## 4. Przyczyna źródłowa

Root cause:
The R31 r4 config was still configured as a clean-namespace probe run while the user requested a continuation of an already populated namespace.

Mechanizm błędu:
`ghost-launcher` config validation fails closed when `require_unique_namespace=true`, `append=false`, and any configured probe output path already exists.

Miejsce:
`ghost-launcher/src/config.rs` validation path for `[p37_shadow_probe]`.

Skutek:
The lifecycle launcher refused to start runtime, leaving no `tmux` session after the first attempt.

Dowód:
`run_lifecycle_guard_20260616T083412Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json` has `status=FAIL_PREFLIGHT`, `run_state=NOT_STARTED`.

Odrzucone hipotezy:
- tmux/session conflict: rejected, no tmux server existed.
- storage failure: rejected, storage gate passed with about 30 GB free and `min-free-gb=20`.
- config contract failure outside probe append: rejected, config and scope contracts passed.

## 5. Strategia naprawy

Przyjęta strategia:
Enable append mode for the probe plane while keeping the same namespace, run id, session id, artifact paths, thresholds, and runtime profile.

Zakres ingerencji:
Single config value:
`[p37_shadow_probe].append = true`

Czego nie zmieniano:
- Gatekeeper thresholds
- FSC state
- RPC endpoints
- NLN program streams
- execution mode
- entry mode
- run namespace and artifact paths
- runtime code

Ryzyka:
The continued run now appends to existing R31 r4 probe artifacts. Consumers must use launcher baseline snapshots or timestamps when evaluating only the fresh segment.

Odrzucone alternatywy:
- deleting old probe artifacts: rejected as destructive and not a continuation.
- starting manually outside the launcher: rejected by `RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md`.
- creating a new r5 namespace: rejected because the user requested the same config/run continuation.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
- Co zmieniono: `[p37_shadow_probe].append` from `false` to `true`.
- Dlaczego: allow continuation in the existing R31 r4 probe namespace without deleting existing artifacts.
- Efekt: preflight passed and the lifecycle launcher started runtime in `tmux`.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| First launcher attempt | `python3 scripts/start_selector_lifecycle_run.py ... --scope shadow-burnin-v3-r31-maxwait15000-fsc-off-r4 ...` | `FAIL_PREFLIGHT` before runtime start | PASS for diagnosis | `run_lifecycle_guard_20260616T083412Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json` |
| Config/static/preflight after append | same launcher command | config contract, scope contract, static guard, preflight passed | PASS | `run_lifecycle_guard_20260616T083520Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json` |
| Event canary | launcher-owned event canary | `SELECTOR_EVENT_CANARY_PASS` | PASS | event delta: `NewPoolDetected=18`, `Candidate=14`, `PoolTransaction=495`, `DIAG_ACCOUNT_UPDATE_RELAY=1015`, bad JSON `0` |
| Lifecycle canary | launcher-owned lifecycle canary | `SELECTOR_LIFECYCLE_CANARY_PASS` on shadow plane | PASS | shadow lifecycle rows `18`, `position_closed=7`, `exit_filled=6`, canonical truth rows `13` |
| Run left running | `tmux ls` and process check | `gho-r31` active, `target/release/ghost-launcher` running | PASS | launcher report `run_state=RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF` |

Wniosek walidacyjny:
The R31 r4 continuation is valid under the selector lifecycle launcher contract and remains running in `tmux`.

Ograniczenia walidacji:
This ADR validates startup and lifecycle proof, not long-horizon provider stability or later coverage metrics.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: existing fail-closed config validation
- Co zabezpiecza: prevents accidental reuse of non-append probe namespaces.
- Kiedy się aktywuje: when `require_unique_namespace=true`, `append=false`, and probe output files already exist.
- Jak przetestowano: first launcher attempt failed before runtime start.
- Co pozostaje poza zakresem: long-run gRPC/provider stall behavior.

Guardrail 2:
- Typ: launcher-owned lifecycle proof
- Co zabezpiecza: prevents claiming a selector lifecycle run without event and lifecycle evidence.
- Kiedy się aktywuje: every `scripts/start_selector_lifecycle_run.py` start.
- Jak przetestowano: second launcher attempt passed and wrote `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`.
- Co pozostaje poza zakresem: downstream statistical quality of the collected data.

## Otwarte ryzyka / follow-up

- Monitor whether the earlier gRPC stall pattern reappears in the restarted R31 runtime.
- Use launcher baseline-aware reports when separating the fresh continuation segment from older R31 r4 artifacts.
