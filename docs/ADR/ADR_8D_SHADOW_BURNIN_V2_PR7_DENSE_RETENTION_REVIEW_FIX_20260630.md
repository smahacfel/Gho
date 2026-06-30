# ADR-8D: Shadow Burnin V2 PR7 Dense Retention Review Fix

Data: 2026-06-30

Status:

```text
COMPLETED_ON_PR_BRANCH_PENDING_REVIEW
```

## D1. Problem

Review GitHub dla PR #6 (`review_id=4598808922`) zablokował merge przez
niespójność kontraktu PR7:

- spec, ADR i acceptance gate mówiły, że `shadow_path_dense_3s` zachowuje każdy
  `EVENT_SAMPLE`;
- `select_path_samples_v2()` po zebraniu próbek robił `truncate(max_path_points)`;
- pierwotny test dense-retention tworzył 5 `EVENT_SAMPLE`, ustawiał
  `max_path_points = 3` i oczekiwał tylko 3 próbek w wyniku.

To oznaczało, że dense path mógł gubić próbki wymagane do 2s/3s fidelity
research i nadal przechodzić test o nazwie sugerującej pełną retencję.

## D2. Decision

Wybieramy Opcję A z review: strict dense retention.

`max_path_points` pozostaje storage capem, ale nie może usuwać protected samples:

```text
LEVEL_HIT
TERMINAL
EVENT_SAMPLE, gdy keep_every_event_sample = true
```

Jeżeli liczba protected samples przekracza `max_path_points`, sampler zachowuje
protected samples i dopisuje limitation:

```text
PATH_SAMPLER_STORAGE_BUDGET_EXCEEDED_PROTECTED_SAMPLES_RETAINED
```

Przycinanie przez `max_path_points` może dotyczyć tylko próbek opcjonalnych.

## D3. Evidence

Plik implementacyjny:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Artefakty kontraktowe:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_BURNIN_V2_PR6_PR7_EXIT_PATH_20260630.md`
- `reports/selector/shadow_v2_acceptance_gates.csv`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_risk_register.csv`

Fixture:

- `shadow_v2_path_sampler_dense_keeps_all_event_samples_over_max_points_cap`

## D4. Root Cause

Pierwotna implementacja myliła dwa osobne kontrakty:

- storage budget powinien ograniczać koszt próbek opcjonalnych;
- dense fidelity dla 2s/3s wymaga kompletnej retencji event samples.

`truncate(max_path_points)` nie rozróżniał tych klas i mógł usunąć
`EVENT_SAMPLE`, `LEVEL_HIT` albo `TERMINAL`.

## D5. Corrective Action

Dodano protected-sample cap policy:

- `sample_protected_from_path_cap()` klasyfikuje protected samples;
- `select_path_samples_v2()` zachowuje wszystkie protected samples;
- `max_path_points` przycina tylko próbki opcjonalne;
- przekroczenie capu przez protected samples emituje storage-budget limitation;
- test dense retention oczekuje `5 EVENT_SAMPLE in -> 5 EVENT_SAMPLE out` przy
  `max_path_points = 3`.

## D6. Rejected Alternatives

Odrzucono:

- pozostawienie hard capu, który usuwa dense `EVENT_SAMPLE`;
- zmianę gate na słabszy claim bez naprawy kodu;
- traktowanie utraty eventów jako zwykłego `truncated=true`, bo to nadal może
  fałszować 2s/3s target/stop/timeout evidence;
- zmiany runtime, BUY/REJECT, Gatekeeper policy, selector runtime lub TX/Jito
  path.

## D7. Consequences

Po poprawce PR7 nie może twierdzić, że dense mode zachowuje każdy event sample,
a następnie usuwać go przez storage cap.

To nadal nie oznacza:

```text
SHADOW_V2_RESEARCH_GRADE
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

Shadow V2 nadal wymaga PR12 validation burnin, manifestów, density reports,
>=99% reconstruction/reconciliation gates i PR14 live-confirmed calibration
dataset dla live-equivalence.

## D8. Verification

Wykonano lokalnie na `/root/Gho`, branch
`codex/shadow-v2-pr6-pr7-exit-path`:

```text
cargo test -q -p ghost-brain shadow_v2_path_sampler_dense_keeps_all_event_samples_over_max_points_cap
OK: 1 passed; 0 failed; 1718 filtered out

cargo test -q -p ghost-brain shadow_v2_path
OK: 6 passed; 0 failed; 1713 filtered out

cargo test -q -p ghost-brain shadow_v2
OK: 38 passed; 0 failed; 1681 filtered out

cargo fmt --check
OK

git diff --check
OK

awk CSV column checks for shadow_v2_required_schema_manifest.csv,
shadow_v2_acceptance_gates.csv, shadow_v2_risk_register.csv
OK
```

Cargo test output contains existing repository warnings unrelated to this PR7
review fix; no new warning-specific remediation was in scope.

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_CHANGE
NO_ACTIVE_CLOSE_CHANGE
NO_RUN_STARTED
NO_R51_TOUCH
```
