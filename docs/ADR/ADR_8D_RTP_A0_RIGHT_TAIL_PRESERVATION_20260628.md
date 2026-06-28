# ADR-8D: RTP-A0 right-tail preservation offline proof

Status: RTP_DIAGNOSTIC_ONLY / NO_RUNTIME
Typ: ADR-8D / offline research evidence
Data: 2026-06-28
Zakres: PR-RTP-A0

## Decyzja

RTP-A0 zostal wykonany jako offline-only proof. Nie zmieniono runtime.

Final verdict: `RTP_DIAGNOSTIC_ONLY / NO_RUNTIME`

R51 GO/NO-GO: `NO_GO_FOR_R51`

## Dowody

| scope | positions_with_exit_replay | positions_with_tsv2_windows | candidate_positions | exact_join_rate |
| --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | 4748 | 5604 | 5485 | 1.0 |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | 3656 | 4831 | 4746 | 1.0 |

## Wynik

- `passing_fixed_pair_count = 0`
- `scope_pass_count = 0`
- `best_fixed_pair = M0_ALL / 6000 / -6000 / 120000 + G2_RECOVERY_AFTER_EARLY_DRAWDOWN @ 30000ms on shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`
- `runtime_approval = false`
- `shadow_close_only_approval = false`
- `raw_jsonl_committed = false`

## Konsekwencje

Brak zgody na runtime, active close lub `shadow_close_only`. Nie bylo zmian Gatekeeper/BUY/REJECT/selector/TX/Jito/live. RTP-A0 jest tylko testem offline right-tail preservation. Jezeli brak stalej pary przechodzacej R49 i R50, obowiazuje `NO_GO_FOR_R51`, TSV2 pozostaje diagnostic/logging-only i nie startujemy nowego runu na podstawie tego wyniku. ORG/TSV2/EIX/RTP nie daja podstaw do runtime ani `shadow_close_only`.
