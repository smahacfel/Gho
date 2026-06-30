# Raport PR16: Shadow V2 Validation Harness Smoke

Data: 2026-06-30

Status:

```text
FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE
```

## 1. Cel

Ten smoke mial sprawdzic tylko, czy po merge PR15 logging-only harness potrafi
przejsc sekwencje:

```text
preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS
```

To nie byl research-grade burnin, RCE proof, selector proof, edge proof ani
strategia. Wynik smoke nie moze byc cytowany jako dowod edge, PnL ani
wykonalnych filli.

## 2. Baseline

Baseline main po merge PR15:

```text
2368042de5c78cc3f90b40e65b904eb957e14136
```

Tag referencyjny:

```text
shadow-v2-validation-harness-ready-20260630
```

Tag lokalny i tag na `origin` wskazuja na ten sam commit.

## 3. Konfiguracja smoke

Scope root:

```text
reports/selector/shadow-v2-fidelity-validation
```

Kontrakt Shadow V2:

```text
configs/rollout/ghost_brain_shadow_v2_validation_logging_only.toml
```

Flagi kontraktowe pozostaly bez approval:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_proof_enabled=false
rce_proof_enabled=false
selector_proof_enabled=false
edge_proof_enabled=false
```

Uwaga operacyjna: klucz NLN zostal uzyty w lokalnym overlay env poza repo.
Sekret nie zostal zapisany do plikow repo i nie jest czescia tego raportu.

## 4. Wyniki

### Pre-run manifest

```text
status=PASS
blockers=[]
run_id=shadow-burnin-v2-fidelity-validation-logging-only-smoke-r1
```

Strict pre-run audit przeszedl.

### Preflight runtime

Preflight launchera przeszedl po uzyciu lokalnego env overlay dla NLN i temp
pelnego Ghost Brain configu:

```text
preflight=PASS
seer_grpc_probe=PASS
transport_grpc=PASS
gatekeeper_contract=PASS
```

Nie zaobserwowano po podmianie klucza NLN bledu `PermissionDenied` ani
`Account disabled`.

### Runtime smoke

Wykonano dwa krotkie przebiegi smoke. Oba uruchomily proces i polaczyly stream,
ale oba zakonczyly sie kodem `137` po wymuszonym `timeout --kill-after`, a nie
czystym shutdownem.

Najwazniejsze obserwacje z drugiego, lepiej kontrolowanego przebiegu:

```text
all_components_started=1
stream_established=1
subscribe_sent=22
gatekeeper_evaluations=53
verdict_buy=0
verdict_reject=43
actual_post_buy_handoff=0
shadow_v2_jsonl_rows=0
exit_code=137
```

Wzmianka o `PostBuySubmitted` w logu pochodzi z komunikatu shutdown drain, a nie
z realnego handoff eventu. Nie bylo accepted shadow handoff, wiec PR15 nie mial
wyzwalacza do `maybe_emit_shadow_v2_position_created()`.

### Post-run manifest

Post-run manifest jest zablokowany:

```text
status=BLOCKED
blockers=[
  missing required artifact: shadow_lifecycle_v2.jsonl,
  missing required artifact: shadow_path_density_v2.jsonl,
  missing required artifact: shadow_position_event_v2.jsonl,
  missing required artifact: shadow_replay_v2.jsonl
]
```

Strict post-run manifest audit zakonczyl sie kodem `1`, zgodnie z brakami
artefaktow.

### Wymagane pliki evidence V2

```text
shadow_position_event_v2.jsonl: exists=false rows=0
shadow_replay_v2.jsonl: exists=false rows=0
shadow_lifecycle_v2.jsonl: exists=false rows=0
shadow_path_density_v2.jsonl: exists=false rows=0
```

## 5. Werdykt smoke

```text
PR16_SMOKE_FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE
```

To jest fail smoke, nie fail NLN i nie proof strategii.

Co zostalo potwierdzone:

- main po PR15 zostal oznaczony tagiem baseline;
- pre-run manifest przechodzi strict audit;
- launcher preflight przechodzi po poprawnym NLN env overlay;
- runtime startuje i zestawia stream;
- Gatekeeper wykonuje decyzje w smoke;
- brak raw JSONL w stagingu;
- approval flags pozostaja false.

Co nie zostalo potwierdzone:

- canonical `shadow_position_event_v2.jsonl`;
- derived `shadow_replay_v2.jsonl`;
- derived `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- post-run manifest PASS;
- clean shutdown;
- end-to-end harness evidence success.

## 6. Przyczyna blokady

Obecne PR15 runtime wiring emituje minimalny `shadow_position_v2` dopiero po
accepted shadow handoff w `PostBuyRuntime`. W smoke nie bylo `verdict=BUY` ani
realnego `PostBuySubmitted`, wiec harness nie zapisal zadnego canonical eventu.

Wniosek: PR16 pokazuje, ze sam start i aktywny stream nie wystarczaja do
walidacji harnessu. Smoke zalezy obecnie od wystapienia accepted shadow handoff,
czyli od zdarzenia strategii. To jest niepozadane dla czystego fidelity smoke.

## 7. Konsekwencje

Do czasu naprawy lub doprecyzowania harness smoke:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
research_grade=not_granted
live_equivalence=not_granted
```

Shadow V2 pozostaje:

```text
CONTRACT_READY / RESEARCH_GRADE_NOT_GRANTED
```

## 8. Rekomendacja

Nastepny krok nie powinien byc pelnym validation burninem. Najpierw trzeba
usunac zaleznosc smoke od przypadkowego BUY/accepted shadow handoff.

Bezpieczne warianty:

1. Dodac jawny `validation_smoke_marker_v2` lub `harness_started_v2` jako
   diagnostic-only canonical event, emitowany tylko przy
   `shadow_v2_burnin.enabled=true` i `logging_only=true`.
2. Dodac osobny fixture/injected-record smoke path uruchamiany poza decyzjami,
   ktory tworzy canonical position + derived replay/lifecycle/density z
   `BLOCKED_BY_DATA` / `NOT_EVALUABLE_NO_COVERAGE`.
3. Alternatywnie zdefiniowac PR16 smoke jako wymagajacy realnego accepted shadow
   handoff, ale wtedy wynik smoke jest niedeterministyczny i nie powinien byc
   gate'em readiness dla PR17.

Preferowany wariant: `validation_smoke_marker_v2` albo deterministic injected
record, bo lepiej sprawdza writer/materializer bez mieszania z BUY/REJECT.

## 9. Zakres wykluczony

Nie uruchomiono RCE proof.
Nie uruchomiono research-grade validation burnin.
Nie zmieniono BUY/REJECT.
Nie zmieniono Gatekeeper policy.
Nie zmieniono selector runtime.
Nie zmieniono TX/Jito/live path.
Nie wlaczono `shadow_close_only`.
Nie wlaczono active close.
Nie dotykano R51.
Nie stage'owano raw JSONL ani logow.
