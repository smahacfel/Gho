# ADR-8D: R37 CPV degraded low-sample top-level emission repair

Status: IMPLEMENTED / TARGETED_RUNTIME_SMOKE_VERIFIED
Typ: ADR-8D / runtime evidence repair
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: residual PR2/PR5 runtime gap dla CPV degraded low-sample emission; bez zmiany denominatora CPV i bez promocji degraded CPV jako clean policy evidence
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/tests/cpv_successful_buy_contract_tests.rs`

Powiazane plany i ADR:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`
- `docs/ADR/ADR_8D_PR2_CPV_SUCCESSFUL_BUY_COVERAGE_CONTRACT_20260619.md`
- `docs/ADR/ADR_8D_PR5_EVIDENCE_POLICY_WIRING_AND_REASON_CODES_20260619.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Zadanie:
- ubic poprzedni aktywny R37 runtime, jesli zyje;
- uruchomic swiezy R37 po rebuildzie;
- sprawdzic artefakty runtime;
- naprawic pozostale luki, jesli wystapia.

Wykonano:
- Zatrzymano aktywny tmux runtime `r37-pr5-runtime-proof-20260619`.
- Potwierdzono swiezy release binary:
  - `target/release/ghost-launcher`
  - mtime UTC: `2026-06-19T13:42:15.429471Z`
- Uruchomiono swiezy lifecycle run:
  - scope: `shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1`
  - tmux session: `r37-pr5-cpv-proof-20260619`
  - lifecycle report: `reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260619T134238Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- Guardrail status:
  - `status=PASS`
  - `claim=SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
  - `run_state=RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: aktywna sciezka Gatekeeper/MaterializedFeatureSet/DecisionLogger, SSOT i shadow/live boundary.
- `rust-master`: bezpieczna zmiana Rust w hot path materializacji, targeted tests.
- `trading-systems`: semantyka evidence quality i brak promocji degraded value jako clean decision proof.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/gatekeeper-policy-auditor.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md`

Powod:
Zmiana dotyka aktywnej materializacji `MaterializedFeatureSet`, top-level JSONL convenience fields, strict policy context i runtime proof. Najwieksze ryzyko bylo semantyczne: zwiekszyc coverage CPV bez zmiany denominatora i bez udawania, ze low-sample evidence jest clean.

## 3. Opis problemu - 3W2H

What:
Swiezy runtime po PR5 pokazal, ze embedded `v3_materialized_feature_snapshot.sybil_resistance.cpv_evidence` zawiera poprawne degraded low-sample CPV values, ale top-level pola:
- `signer_cross_pool_velocity`
- `cpv_other_pool_activity`

pozostawaly `null` dla `quality=degraded_low_sample`.

Where:
- `PoolObservationSession::materialize_features()`
- materializacja `MaterializedFeatureSet.sybil_resistance`
- DecisionLogger JSONL top-level convenience projection

Why it matters:
Planowy kontrakt PR2/PR5 dopuszcza liczenie CPV przy 2 successful-buy signerach tylko jako jawne `degraded_low_sample`, jezeli config `cpv_emit_degraded_low_sample=true`. Wartosci nie moga byc clean, ale nie powinny znikac z top-level convenience fields, bo wtedy offline selector/top-level consumer widzi falszywy brak metryki mimo istnienia embedded evidence.

How observed:
Przed poprawka runtime pokazywal przypadki:
- embedded `cpv_evidence.quality = degraded_low_sample`
- embedded `signer_cross_pool_velocity = Some(...)`
- embedded `cpv_other_pool_activity = Some(...)`
- top-level `signer_cross_pool_velocity = null`
- top-level `cpv_other_pool_activity = null`

How many / scale:
W swiezym runtime po poprawce, w snapshotcie audytu po starcie `2026-06-19T13:45:39Z`:
- `legacy_live`: 39 rekordow `degraded_low_sample`, 39/39 top-level present, 0 mismatch.
- `v25_shadow`: 18 rekordow `degraded_low_sample`, 18/18 top-level present, 0 mismatch.

## 4. Przyczyna zrodlowa

Root cause:
`PoolObservationSession::materialize_features()` liczyl CPV przez rolling index, ale canonical `MaterializedFeatureSet.sybil_resistance` wypelnial wartosci tylko gdy CPV bylo `Clean`.

Efekt:
- `cpv_evidence` bylo prawdziwe i zawieralo degraded low-sample value,
- top-level convenience projection gubila wartosc,
- coverage top-level wygladal gorzej niz embedded SSOT,
- downstream czytajacy tylko top-level mogl pomylic degraded evidence z missing/unavailable.

Kontrakt, ktory nalezalo zachowac:
- CPV nadal liczy tylko successful-buy signers.
- Failed tx, sell-only wallets i wszyscy signerzy nie sa denominator CPV.
- Low-sample CPV nie staje sie clean.
- Strict policy nadal nie moze uzyc degraded CPV jako clean pass, chyba ze jawny config policy na to pozwoli.

## 5. Strategia naprawy

Przyjeta strategia:
- Nie zmieniac denominatora CPV.
- Nie zmieniac progow CPV.
- Nie zmieniac verdict taxonomy.
- Nie zmieniac strict policy defaultow.
- Zmienic tylko emission gate dla canonical sybil CPV value:
  - `Clean` -> emit value.
  - `DegradedLowSample` -> emit value tylko gdy `cross_pool_velocity_config.emit_degraded_low_sample == true`.
  - `InsufficientSample`, `Unavailable`, `UnavailableSource`, `NotConfigured`, `NotAllowed`, `Stale` -> nie emituj value.
- Zachowac `cpv_evidence` jako nosnik statusu, sample_count, required counts i reasons.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: CPV top-level emission gate
- Plik: `ghost-launcher/src/session/observation.rs`
- Zastapiono clean-only materializacje CPV warunkiem `cpv_can_emit_value`.
- Dla `MetricEvidenceQuality::DegradedLowSample` value jest emitowane tylko przy `emit_degraded_low_sample=true`.
- Dla pozostalych nie-clean stanow value pozostaje `None`.

Zmiana 2: test kontraktu degraded low-sample
- Plik: `ghost-launcher/tests/cpv_successful_buy_contract_tests.rs`
- Test `degraded_low_sample_cpv_emits_value_with_degraded_evidence` sprawdza:
  - top-level canonical sybil fields maja wartosc,
  - embedded `cpv_evidence.quality` pozostaje `degraded_low_sample`,
  - `sample_count`, required sample counts i `CPV_LOW_SAMPLE_DEGRADED` pozostaja widoczne,
  - JSON projection zawiera value i evidence.

## 7. Walidacja dzialan naprawczych

### Targeted tests

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| CPV degraded low-sample contract | `cargo test -q -p ghost-launcher --test cpv_successful_buy_contract_tests degraded_low_sample_cpv_emits_value_with_degraded_evidence -- --exact` | 1 passed | PASS |
| CPV low-sample lib tests | `cargo test -q -p ghost-launcher cpv_low_sample --lib` | 4 passed | PASS |
| Strict metric policy lib tests | `cargo test -q -p ghost-launcher strict_metric --lib` | 4 passed | PASS |
| Rustfmt | `cargo fmt --package ghost-launcher` | passed | PASS |
| Diff hygiene | `git diff --check` | clean | PASS |
| Release build | `cargo build --release -p ghost-launcher --bin ghost-launcher` | passed | PASS |

### Runtime smoke

Lifecycle launcher:
- `status=PASS`
- `claim=SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
- `binary_mtime_utc=2026-06-19T13:42:15.429471+00:00`
- `tmux_session=r37-pr5-cpv-proof-20260619`

Fresh decision files:
- `logs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/45137ae410c1ab231b457abed6a34f99b4086136f912e6de64c7dd703d6850d8/gatekeeper_v2_decisions.jsonl`
- `logs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/v2.5/v25_shadow/45137ae410c1ab231b457abed6a34f99b4086136f912e6de64c7dd703d6850d8/gatekeeper_v2_decisions.jsonl`

Runtime audit snapshot after `2026-06-19T13:45:39Z`:

| Gate | legacy_live | v25_shadow | Status |
|---|---:|---:|---|
| Fresh records | 155 | 67 | INFO |
| `log_schema_version=28` | 155/155 | 67/67 | PASS |
| `evidence_policy_context` present and config-matching | 155/155 | 67/67 | PASS |
| CPV degraded low-sample top-level present | 39/39 | 18/18 | PASS |
| CPV top-level vs embedded mismatch | 0 | 0 | PASS |
| top-level burst_ratio vs embedded canonical mismatch | 0 | 0 | PASS |
| top-level vectors vs embedded nullable decision series mismatch | 0 | 0 | PASS |
| negative decision-series intervals | 0 | 0 | PASS |
| top-level numeric `delta_*`/`rate_*` vs embedded mismatch | 0 | 0 | PASS |
| top-level numeric `delta_*`/`rate_*` embedded-only fields | 0 | 0 | PASS |

Observed reason/evidence behavior:
- `HARD_FAIL_STRICT_METRIC_THRESHOLD` present in runtime rows.
- strict value failure details visible in decision/reason-chain fields.
- temporal carried-forward log-only annotations visible where applicable.
- CPV degraded values are visible as values plus evidence status, not as clean evidence.

## 8. Ryzyka resztkowe / czego ten ADR nie zamyka

- Runtime smoke jest krotki; dluzszy proof moze ujawnic rzadsze branch cases.
- `rate_mcap_sol_per_s_*` nie zostalo cwiczone w tym snapshotcie, bo embedded runtime nie wyemitowal numeric mcap rate values w badanym oknie. Mapping dla wszystkich numeric embedded delta/rate fields, ktore wystapily, byl czysty.
- Brak missing price samples w badanym oknie nie dowodzi, ze market-cap/carry-forward price source zawsze wystapi w dluzszym runie.
- Ten ADR nie zmienia policy defaultu `cpv_low_sample_policy=reason_only` ani `cpv_allow_degraded_in_strict_policy=false`.
- Ten ADR nie twierdzi, ze degraded CPV jest rownowazne clean CPV.

## 9. Scope out

Poza zakresem pozostaly:
- zmiana CPV denominatora;
- uzycie all-signers, failed tx albo sell-only wallets do CPV;
- promocja degraded CPV do clean strict policy evidence;
- tuning thresholdow;
- zmiana shadow/live behavior;
- zmiana execution path;
- restart lub porzadkowanie starszych rollout artefaktow.

## 10. Wniosek

Residual runtime gap zostal naprawiony w sposob zgodny z kontraktem evidence:
- CPV low-sample value moze byc widoczne, gdy config to jawnie pozwala;
- status `degraded_low_sample`, sample count i reason pozostaja widoczne;
- top-level convenience fields nie klamia przez ukryte wyciecie wartosci;
- strict policy nie dostaje degraded value jako clean pass.

Swiezy R37 runtime smoke po rebuildzie potwierdzil oczekiwana poprawe dla badanych artefaktow. Run `r37-pr5-cpv-proof-20260619` zostal pozostawiony aktywny do dalszego zbierania danych.
