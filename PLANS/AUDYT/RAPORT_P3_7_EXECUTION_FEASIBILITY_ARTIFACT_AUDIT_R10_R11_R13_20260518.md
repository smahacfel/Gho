# Raport P3.7.6 Execution Feasibility Artifact Audit R10/R11/R13

Data: 2026-05-18

Status: **EXECUTION FEASIBILITY BLOCKED / PHASE B NOT ALLOWED**

## Executive Summary

P3.7 ma juz post-decision price path z Chainstack i niezerowe
`good_clean` w R10/R11/R13. Ten audyt rozdziela jednak market-good outcome od
realnej wykonawczosci Ghost.

Wynik jest fail-closed:

- R10/R11 nie maja shadow entry ani shadow lifecycle proof.
- R13 ma tylko jeden shadow dispatch proof i jest to `data_problem` /
  `AccountNotFound`.
- `good_executable=0` dla R10/R11/R13.
- Phase B candidate feature prototype pozostaje zablokowane.

Ten etap nie zmienia P2, live, thresholdow, aktywnej polityki V2/V2.5, IWIM ani
execution sendera.

## Scope

Audit obejmuje historyczne artefakty:

- R10: `shadow-burnin-v3-p32-replay-r10-primary-only`
- R11: `shadow-burnin-v3-p32-replay-r11-primary-only`
- R13: `shadow-burnin-v3-p36-sample-r13-primary-only`

Artefakty historyczne sa traktowane jako immutable evidence. Nowe wyniki sa
addytywne i zapisane jako P3.7 reports / joined labels.

## Artifact Availability

| Run | Decisions | Label v2 Chainstack | Shadow entries | Shadow lifecycle | Execution evidence source |
| --- | ---: | ---: | ---: | ---: | --- |
| R10 | 150 | 150 | 0 / missing | 0 / missing | `proxy_not_available` |
| R11 | 447 | 447 | 0 / missing | 0 / missing | `proxy_not_available` |
| R13 | 2733 | 2733 | 1 | 1 | `proxy_not_available`, `shadow_lifecycle` |

Physical shadow paths checked:

- R10 entry:
  `logs/shadow_run/shadow-burnin-v3-p32-replay-r10-primary-only/shadow_entries.jsonl`
- R10 lifecycle:
  `logs/shadow_run/shadow-burnin-v3-p32-replay-r10-primary-only/shadow_lifecycle.jsonl`
- R11 entry:
  `logs/shadow_run/shadow-burnin-v3-p32-replay-r11-primary-only/shadow_entries.jsonl`
- R11 lifecycle:
  `logs/shadow_run/shadow-burnin-v3-p32-replay-r11-primary-only/shadow_lifecycle.jsonl`
- R13 entry:
  `logs/shadow_run/shadow-burnin-v3-p36-sample-r13-primary-only/shadow_entries.jsonl`
- R13 lifecycle:
  `logs/shadow_run/shadow-burnin-v3-p36-sample-r13-primary-only/shadow_lifecycle.jsonl`

## Execution Classification After Join

| Run | `good_clean` | `good_dirty` | `good_executable` | `no_dispatch_expected` | `execution_infeasible` | Shadow observed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| R10 | 19 | 6 | 0 | 150 | 0 | 0 |
| R11 | 75 | 16 | 0 | 447 | 0 | 0 |
| R13 | 421 | 115 | 0 | 2732 | 1 | 1 |

Interpretacja:

- R10/R11 market-good rows sa `good_not_executable`, bo nie ma realnego dispatch,
  simulation ani lifecycle proof.
- R13 ma jeden dopasowany dispatch/lifecycle artifact, ale jest fail-closed:
  `dispatch_status=failed`, `simulation_outcome=failed`,
  `error_class=data_problem`, `err=AccountNotFound`.
- `AccountNotFound` / simulation fail / data problem nie moze byc traktowane
  jako execution success ani jako proxy dla `good_executable`.

## R13 Matched Failure

R13 shadow lifecycle row jest dopasowany do market-good row:

- `ab_record_id`:
  `kgB6YsTNrDY9izqakSrJZumTngCo5i7KrcP5BTmcfPV:1779090447679:1779090449679:BUY`
- `pool_id`: `kgB6YsTNrDY9izqakSrJZumTngCo5i7KrcP5BTmcfPV`
- `base_mint`: `HG81ynViD24dJxP3RsZiULLen7hpftCurZm73hCpump`
- `market_outcome_class`: `good_clean`
- `execution_quality_class`: `execution_infeasible`
- `decision_quality_class`: `good_not_executable`
- `execution_evidence_source`: `shadow_lifecycle`
- `simulation_error_class`: `data_problem`

To rozstrzyga, ze pojedynczy R13 dispatch nie odblokowuje Phase B; przeciwnie,
jest negatywnym execution evidence.

## Phase A Report Updates

Zregenerowane artefakty operacyjne:

- R10 execution feasibility:
  `logs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only/decisions/p3_7_execution_feasibility_chainstack_20260518.jsonl`
- R11 execution feasibility:
  `logs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only/decisions/p3_7_execution_feasibility_chainstack_20260518.jsonl`
- R13 execution feasibility:
  `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/decisions/p3_7_execution_feasibility_chainstack_20260518.jsonl`
- Temporal split Chainstack:
  `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_temporal_split_chainstack_20260518.json`
- Evidence availability Chainstack:
  `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_evidence_availability_chainstack_20260518.json`

Tracked reports zaktualizowane:

- `PLANS/AUDYT/RAPORT_P3_7_TEMPORAL_SPLIT_BASELINE_R10_R11_R13_20260518.md`
- `PLANS/AUDYT/RAPORT_P3_7_EVIDENCE_AVAILABILITY_R10_R11_R13_20260518.md`

## Gate Decision

P3.7 Phase B candidate feature prototype pozostaje **blocked**.

Powod:

- `no_execution_proof_for_market_good_rows`
- `no_good_executable_rows`
- R13 dispatch proof jest fail-closed `execution_infeasible`

Dozwolony nastepny ruch:

- tylko P3.7.6 execution-feasibility resolution albo appendix-only
  market-quality diagnostic feature audit,
- bez claimu BUY edge,
- bez P2,
- bez live,
- bez threshold tuning,
- bez active policy change.

## Residual Risks

- `good_clean` jest market-quality evidence, nie BUY-executable evidence.
- R10/R11 brak shadow lifecycle nie dowodzi ekonomicznej niewykonalnosci; dowodzi
  tylko braku artefaktu, ktory pozwalalby zrobic clean execution claim.
- R13 pojedynczy failed dispatch nie wystarcza do modelowania execution quality.
- Feature mining na `good_clean` moze byc uzyty najwyzej jako appendix-only
  market-quality diagnostic, dopoki `good_executable=0`.
