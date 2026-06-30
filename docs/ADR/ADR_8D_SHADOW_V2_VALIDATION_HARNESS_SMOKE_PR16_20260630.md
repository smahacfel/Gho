# ADR-8D: Shadow V2 Validation Harness Smoke PR16

Data: 2026-06-30

Status:

```text
PR16_SMOKE_FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE
```

## D1. Problem

Po PR15 Shadow V2 ma logging-only validation harness, ale trzeba bylo
zweryfikowac, czy runtime potrafi w realnym smoke przejsc:

```text
preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS
```

Ten etap nie mial dawac research-grade, live-equivalence-grade, RCE proof,
selector proof ani edge proof.

## D2. Decision

Wykonano PR16 smoke jako evidence-only administracyjny test harnessu.

Wynik smoke jest negatywny:

```text
FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE
```

Nie promujemy Shadow V2 do research-grade. Nie odblokowujemy strategii. Nie
zmieniamy zadnych approval flags.

## D3. Evidence

Baseline:

```text
main_head=2368042de5c78cc3f90b40e65b904eb957e14136
tag=shadow-v2-validation-harness-ready-20260630
```

Pre-run:

```text
pre_run_manifest.status=PASS
pre_run_manifest.blockers=[]
strict_pre_run_audit=PASS
```

Runtime/preflight:

```text
launcher_preflight=PASS
seer_grpc_probe=PASS
stream_established=1
subscribe_sent=22
```

Smoke runtime:

```text
all_components_started=1
gatekeeper_evaluations=53
verdict_buy=0
verdict_reject=43
actual_post_buy_handoff=0
exit_code=137
```

Post-run:

```text
post_run_manifest.status=BLOCKED
strict_post_run_audit=FAIL
```

Missing artifacts:

```text
shadow_position_event_v2.jsonl
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_path_density_v2.jsonl
```

Row counts:

```text
shadow_position_event_v2.jsonl rows=0
shadow_replay_v2.jsonl rows=0
shadow_lifecycle_v2.jsonl rows=0
shadow_path_density_v2.jsonl rows=0
```

## D4. Root Cause

PR15 minimal runtime emitter zapisuje `shadow_position_v2` dopiero po accepted
shadow handoff w `PostBuyRuntime`. Smoke uruchomil stream i Gatekeeper
evaluations, ale nie wygenerowal `verdict=BUY` ani realnego `PostBuySubmitted`.

W rezultacie harness byl aktywny, ale nie dostal canonical recordu do zapisu.
Derived replay/lifecycle/density nie mogly powstac bez canonical high-watermark.

Drugi problem operacyjny: oba smoke przebiegi zakonczyly sie kodem `137` po
`timeout --kill-after`, wiec nie udowodniono clean shutdown.

## D5. Corrective Action

Nie wprowadzono zmian runtime w ramach PR16 smoke. Raportuje sie wynik jako
blokade.

Rekomendowana korekta kolejnego PR:

- dodac deterministic logging-only smoke event niezalezny od BUY/REJECT, np.
  `validation_smoke_marker_v2` albo injected `BLOCKED_BY_DATA` position record;
- zachowac go tylko dla `shadow_v2_burnin.enabled=true` i `logging_only=true`;
- oznaczyc `measurement_grade=DIAGNOSTIC_ONLY`;
- utrzymac `runtime_approval=false`, `shadow_close_only_approval=false`,
  `active_close_approval=false`;
- nie podlaczac markerow do Gatekeeper, selector, TX/Jito/live path ani
  `shadow_close_only`.

## D6. Rejected Alternatives

Odrzucono:

- interpretacje smoke jako strategy proof;
- kontynuowanie do pelnego validation burnin mimo braku JSONL V2;
- commitowanie raw JSONL, logow lub lokalnych env overlay;
- traktowanie aktywnego streamu jako rownowaznego z harness evidence success;
- podnoszenie approval flags po zablokowanym post-run manifest.

## D7. Consequences

Obowiazujacy stan po PR16 smoke:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
research_grade=not_granted
live_equivalence=not_granted
```

Maksymalny verdict pozostaje:

```text
CONTRACT_READY / RESEARCH_GRADE_NOT_GRANTED
```

PR17 fidelity validation burnin pozostaje zablokowany, dopoki smoke harness nie
potwierdzi tworzenia canonical JSONL, derived replay/lifecycle, density rows i
`post_run_manifest.status=PASS`.

## D8. Verification

Wykonane sprawdzenia:

```text
git checkout main
git pull --ff-only
git tag shadow-v2-validation-harness-ready-20260630
git push origin shadow-v2-validation-harness-ready-20260630
python3 scripts/shadow_v2_manifest_audit.py --manifest-phase pre_run --strict
target/debug/ghost-launcher --config <temp smoke config> --preflight
target/debug/ghost-launcher --config <temp smoke config>
python3 scripts/shadow_v2_manifest_audit.py --manifest-phase post_run --strict
python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict
python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict
git diff --cached --name-only
```

Wyniki:

```text
pre_run_strict=PASS
preflight=PASS
runtime_stream=PASS
post_run_strict=FAIL
plan_audit=PASS
legacy_downgrade_audit=PASS
staged_files_before_report=0
```

Runtime boundary:

```text
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
NO_R51_TOUCH
NO_RAW_JSONL_STAGED
```
