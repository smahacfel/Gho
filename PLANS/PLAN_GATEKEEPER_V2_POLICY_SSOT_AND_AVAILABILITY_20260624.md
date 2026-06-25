# Plan Wykonawczy V2: Gatekeeper Policy SSOT, PDD Availability, Confidence I Top3 Contract

## 1. Podsumowanie

Ten plan zastępuje poprzednią wersję i uwzględnia wszystkie korekty jakościowe. Kolejność PR-ów zostaje bez zmian, ale doprecyzowuję kontrakty tam, gdzie poprzedni plan zostawiał za dużo interpretacji.

Roadmapa:

1. **PR1: Hard-Fail SSOT**
   `run_assessment()` nie produkuje żadnego aktywnego hard-fail reason ani innego pola, które downstream mógłby czytać jako decyzję.

2. **PR2: PDD Availability Status**
   PDD spike/ramping/flash dostają lokalny, typowany status dostępności. Nie używamy `Option<bool>` jako semantyki dostępności.

3. **PR3: Confidence Semantics**
   Alpha disabled nie obniża confidence i nie udaje wysokiej jakości alpha. Alpha skipped/not-run nie daje sztucznego `0.0`.

4. **PR4: Whale Top3 Contract**
   Nowa metryka `top3_signer_volume_ratio` jest `Option<f64>` + helper fallback, żeby stary payload bez pola nie zamieniał się cicho w prawdziwe zero.

Po akceptacji dokument zapisać jako:

`PLANS/PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`

Każdy PR zmieniający pliki repo wymaga osobnego ADR-8D w `docs/ADR/`. Jeśli szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nadal nie istnieje, użyć lokalnego formatu ADR-8D z istniejących dokumentów i jawnie odnotować brak szablonu.

Granice poza zakresem:

- brak zmian wartości progów ryzyka;
- brak live enablement;
- brak TX builder / sender / Seer ingest / Solana execution zmian;
- brak globalnego `Signal<T>`;
- brak usuwania istniejących pól JSONL/logów bez adaptera;
- brak dużego refaktoru do `GatekeeperEvaluation { evidence, decision }`;
- brak stage'owania cudzych dirty-worktree zmian.

---

## 2. PR1: Hard-Fail SSOT

### Cel

Usunąć drugie źródło prawdy dla hard-fail verdictów. Po PR1 każdy hard-fail reason ma pochodzić z warstwy policy/decision, nie z `run_assessment()`.

### Kontrakt

`run_assessment()`:

- zbiera evidence,
- liczy diagnostykę,
- liczy phase pass/fail,
- materializuje fakty pomocnicze,
- **nie produkuje żadnego aktywnego hard-fail reason**.

Zakazane w `run_assessment()`:

- threshold-derived `hard_reject_reason`,
- dev-sold hard reject,
- HHI hard reject,
- bot timing hard reject,
- slow pool hard reject,
- price impact hard reject,
- failed tx ratio hard reject,
- jakiekolwiek nowe pole, które downstream mógłby potraktować jako aktywną decyzję.

`GatekeeperDecision` / policy:

- pozostaje jedynym właścicielem hard-fail verdictu,
- pozostaje jedynym źródłem `hard_fail_reason`,
- pozostaje jedynym źródłem typed reason code.

`assessment.hard_reject_reason`:

- zostaje wyłącznie compatibility/export field,
- może być wypełnione dopiero po decyzji policy,
- nie może być czytane jako warunek aktywnego rejectu.

### Zmiany Implementacyjne

W `ghost-launcher/src/components/gatekeeper.rs`:

- Usunąć z `run_assessment()` wszystkie przypisania do `hard_reject_reason`.
- `run_assessment()` zawsze zwraca `hard_reject_reason: None`, chyba że chodzi o niedecyzyjne legacy/compat pole ustawiane poza tą funkcją. Preferowany wariant: w samej funkcji zawsze `None`.
- Dodać prywatny helper boundary, np.:
  - `attach_policy_decision_compat_fields(assessment, decision, config)`
- Helper może:
  - ustawić `assessment.decision`,
  - przeliczyć/cache'ować `v25_confidence`,
  - ustawić `assessment.hard_reject_reason = decision.hard_fail_reason.clone()` dla logów/replay/starych API.
- Helper nie może być używany przed decyzją ani jako warunek decyzji.

W `evaluate_phases()`:

- Dla `use_three_layer_decision=true`: zachować flow `assessment -> compute_decision -> terminal decision`.
- Dla `use_three_layer_decision=false`:
  - legacy branch nadal może mieć legacy phase-count BUY/TIMEOUT semantics,
  - ale hard-fail precheck musi wywołać to samo `compute_decision(&assessment)`,
  - jeśli policy zwraca hard fail, legacy branch ma zwrócić ten sam verdict/reason/code,
  - branch legacy nie może czytać `assessment.hard_reject_reason`.

W feature/compat/replay path:

- `evaluate_from_features()` i `evaluate_compat_from_features()` podejmują decyzję z `GatekeeperDecision`, nie z `assessment.hard_reject_reason`.
- `build_snapshot_assessment_for_current_state()` może wypełniać compatibility fields dopiero po policy decision.
- `record_policy_decision_eval_snapshot()` i `to_buy_log()` mogą emitować `hard_reject_reason`, ale źródłem ma być `decision.hard_fail_reason`, z `assessment.hard_reject_reason` tylko jako stary fallback przy kompatybilności.

W `gatekeeper_policy.rs`:

- `build_assessment_from_features()` nie może ustawiać aktywnego `assessment.hard_reject_reason`.
- `refresh_assessment_thresholds()`:
  - usunąć z aktywnego path albo ograniczyć do wyraźnie nazwanej funkcji compatibility/export,
  - nie może wpływać na verdict.
- `evaluate_policy_from_assessment()` pozostaje policy SSOT dla feature-driven path.

### Testy PR1

Dodać obowiązkową macierz hard-fail parity.

Przypadki obowiązkowe:

- dev sold,
- HHI,
- bot timing,
- slow pool,
- tx price impact,
- failed tx ratio.

Ścieżki obowiązkowe:

- three-layer on,
- `use_three_layer_decision=false`,
- feature-driven policy path,
- compat/replay assessment path, jeśli istniejący helper pozwala to zbudować bez runtime.

Assertować dla każdego przypadku:

- ten sam hard-fail verdict,
- ten sam `hard_fail_reason`,
- ten sam albo kontraktowo zgodny typed reason code,
- `run_assessment().hard_reject_reason.is_none()`,
- żaden path nie decyduje na podstawie `assessment.hard_reject_reason`.

Zaktualizować stare unit testy, które dziś oczekują `run_assessment().hard_reject_reason.is_some()`:

- test ma sprawdzać evidence z `run_assessment()`,
- hard-fail ma być assertem na `compute_decision()` albo `evaluate_policy_from_assessment()`.

### DoD PR1

- Brak wyjątków: nawet dev sold nie jest decyzją z `run_assessment()`.
- `rg "hard_reject_reason = Some" ghost-launcher/src/components/gatekeeper.rs` nie pokazuje przypisań wewnątrz `run_assessment()`.
- `use_three_layer_decision=false` używa tej samej policy dla hard-faili.
- `assessment.hard_reject_reason` nie jest aktywnym inputem decyzji.
- Macierz hard-fail parity przechodzi dla wszystkich sześciu przypadków.
- ADR-8D opisuje nowy kontrakt: assessment = evidence, policy = decision.

Walidacja:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher test_evaluate_hard_reject -- --nocapture
git diff --check -- ghost-launcher/src/components/gatekeeper.rs ghost-launcher/src/components/gatekeeper_policy.rs ghost-launcher/tests docs/ADR
```

---

## 3. PR2: PDD Availability Status

### Cel

Naprawić `unknown as false` dla PDD sequence signals bez globalnego modelu `Signal<T>` i bez łamania starych bool fields.

### Nowe Typy Lokalne

Dodać lokalny typ PDD-only, np. w `gatekeeper_pdd_sequence.rs` albo bliskim module:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PddSequenceUnavailableReason {
    MissingSequence,
    InsufficientDuration,
    InsufficientTxPerSegment,
    FlashCrashUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PddSignalStatus {
    NotApplicable,
    Available,
    Unavailable(PddSequenceUnavailableReason),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PddSignalObservation {
    pub detected: bool,
    pub status: PddSignalStatus,
}
```

Kontrakt:

- `Available + detected=false` znaczy: sprawdzono i sygnału nie było.
- `Unavailable(reason) + detected=false` znaczy: bool jest tylko aliasem kompatybilnościowym, nie "clean".
- `NotApplicable + detected=false` znaczy: sygnał nie był wymagany albo PDD nie było uruchomione.

Nie używać `Option<bool>` jako semantyki availability.

### Zmiany Implementacyjne

W `PddDiagnostics`:

- Zachować stare pola:
  - `spike_detected: bool`,
  - `ramping_detected: bool`,
  - `flash_crash_risk: bool`.
- Dodać nowe pola:
  - `spike_signal: PddSignalObservation`,
  - `ramping_signal: PddSignalObservation`,
  - `flash_crash_signal: PddSignalObservation`.
- `PddDiagnostics::not_run()` ustawia:
  - stare bool-e na `false`,
  - nowe statusy na `NotApplicable`.

W feature/materialized path:

- Jeśli `tx_segment_sequence` jest `None`, a sygnał jest enabled:
  - status = `Unavailable(MissingSequence)`,
  - detected = `false`,
  - stare bool pole = `false`.
- Jeśli sekwencja istnieje, ale nie spełnia warunków:
  - status = `Unavailable(InsufficientDuration)` albo `Unavailable(InsufficientTxPerSegment)`,
  - detected = `false`.
- Jeśli flash crash nie może być oceniony przez obecny snapshot:
  - status = `Unavailable(FlashCrashUnavailable)`,
  - detected = `false`,
  - zachować istniejący reason string `pdd_flash_crash_unavailable` przy eksporcie.
- Jeśli sygnał jest ocenialny:
  - status = `Available`,
  - detected = realny wynik.

W APS:

- Nie przekazywać już `bool` ani `Option<bool>`.
- Przekazywać `PddSignalObservation` dla spike.
- APS może podnieść high-volatility regime tylko dla:
  - `status == Available`,
  - `detected == true`.
- `Unavailable(_)` nie jest clean false. APS diagnostic musi zachować:
  - `pdd_spike_signal_status`,
  - `pdd_spike_unavailable_reason`.
- `NotApplicable` i `Unavailable(_)` mają być rozróżnialne w diagnostyce.

W DecisionLogger / JSONL:

- Dodać opcjonalne pola tekstowe:
  - `pdd_spike_signal_status`,
  - `pdd_spike_unavailable_reason`,
  - `pdd_ramping_signal_status`,
  - `pdd_ramping_unavailable_reason`,
  - `pdd_flash_crash_signal_status`,
  - `pdd_flash_crash_unavailable_reason`.
- Zachować stare bool fields bez zmiany nazwy.
- Serializowane statusy muszą być stabilne:
  - `not_applicable`,
  - `available`,
  - `unavailable`.
- Serializowane reasony muszą mapować do istniejących stringów:
  - `missing_sequence`,
  - `insufficient_duration`,
  - `insufficient_tx_per_segment`,
  - `pdd_flash_crash_unavailable`.

### Testy PR2

Dodać testy:

- Missing sequence:
  - `spike_detected == false`,
  - `spike_signal.status == Unavailable(MissingSequence)`,
  - APS nie traktuje tego jako known clean false.
- Valid sequence, no spike:
  - `spike_detected == false`,
  - `spike_signal.status == Available`.
- Valid sequence, spike:
  - `spike_detected == true`,
  - `spike_signal.status == Available`.
- Flash crash unavailable:
  - `flash_crash_risk == false`,
  - `flash_crash_signal.status == Unavailable(FlashCrashUnavailable)`,
  - confidence fail-closed behavior zostaje zachowane.
- APS:
  - `Unavailable(MissingSequence)` i `NotApplicable` dają różne diagnostics.
- JSONL:
  - stare bool fields nadal istnieją,
  - nowe status/reason fields są obecne i stabilne.

### DoD PR2

- Nie ma gołego `bool` ani `Option<bool>` jako pełnego kontraktu PDD spike w APS.
- Missing PDD sequence nie może wyglądać jak checked-clean false.
- Stare bool-e zostają kompatybilnościowe.
- Existing fail-closed tests dla PDD sequence nadal przechodzą.
- ADR-8D opisuje lokalny PDD-only availability model.

Walidacja:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression p1_ -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher gatekeeper_pdd_sequence -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher gatekeeper_adaptive_prosperity -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test session_lifecycle_tests -- --nocapture
git diff --check -- ghost-launcher/src/components ghost-launcher/tests ghost-brain/src/oracle/decision_logger.rs docs/ADR
```

---

## 4. PR3: Confidence Semantics

### Cel

Usunąć mylące confidence semantics bez zmiany progów decyzyjnych.

Dwa invarianty:

- Alpha disabled nie obniża confidence i nie udaje wysokiej jakości alpha.
- Alpha enabled but not-run/skipped/missing nie daje `0.0`; daje unavailable reason.

### Aktualny Kontekst Kodu

Obecny V2.5 confidence jest modelem multiplikatywnym:

```text
base_quality * alpha_quality * pdd_modulator * tas_modulator * sybil_modulator
```

Dlatego neutralnym elementem dla wyłączonego komponentu alpha jest `1.0`, ale plan nie ma twierdzić, że alpha ma jakość `1.0`. To jest neutralizacja disabled component w formule.

### Zmiany Implementacyjne

Dodać helper, np.:

```rust
enum AlphaConfidenceInput {
    NeutralDisabled,
    Available { quality: f64 },
    Unavailable { reason: &'static str },
}
```

Kontrakt:

- `enable_alpha_gate=false`:
  - alpha component jest neutralny w formule,
  - log/diagnostic ma rozróżniać `NeutralDisabled` od realnej jakości `1.0`.
- `enable_alpha_gate=true` i alpha actionable:
  - liczyć quality z realnych momentum/demand/joint.
- `enable_alpha_gate=true` i alpha skipped/not-run/missing:
  - `v25_confidence_breakdown()` zwraca `None`,
  - availability reason wskazuje konkretną przyczynę:
    - `alpha_not_run`,
    - `alpha_insufficient_sample`,
    - `alpha_missing_inputs`,
    - albo istniejący stabilny `skip_reason`.

Usunąć `unwrap_or(0.0)` dla alpha metrics w canonical V2.5 confidence path.

Shadow Early/Normal:

- Extended path zostaje canonical.
- Early/Normal:
  - jeśli canonical confidence jest dostępne, można je cache'ować jako `v25_confidence`,
  - jeśli canonical confidence jest niedostępne, nie wpisywać uproszczonego score jako `v25_confidence`,
  - jeśli uproszczony score jest nadal potrzebny do stage behavior, nadać mu oddzielną semantykę, np. local `stage_confidence` / `legacy_shadow_score`, bez udawania canonical V2.5 confidence.
- Nie zmieniać BUY/REJECT thresholds przy okazji tego PR.

### Testy PR3

Dodać testy:

- Alpha disabled:
  - confidence nie jest obniżone przez alpha,
  - diagnostic rozróżnia disabled-neutral od realnej alpha quality.
- Alpha enabled + insufficient sample:
  - confidence unavailable,
  - reason `alpha_insufficient_sample` albo istniejący stabilny odpowiednik,
  - brak `0.0` jako canonical confidence.
- Alpha enabled + missing inputs:
  - confidence unavailable,
  - reason `alpha_missing_inputs`.
- Alpha enabled + actionable:
  - quality liczona z realnych wartości.
- Early/Normal:
  - simplified score nie trafia do `assessment.v25_confidence`, gdy canonical confidence unavailable.
- Extended:
  - nadal używa canonical confidence.

### DoD PR3

- Brak `unwrap_or(0.0)` dla alpha quality w canonical confidence.
- Disabled alpha neutralizuje komponent, ale nie jest raportowane jako realna jakość alpha.
- Alpha skipped/not-run/missing daje unavailable, nie zero.
- Early/Normal shadow score i canonical V2.5 confidence są rozdzielone.
- ADR-8D opisuje semantykę neutral disabled vs unavailable.

Walidacja:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression confidence -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests alpha -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression -- --nocapture
git diff --check -- ghost-launcher/src/components/gatekeeper.rs ghost-launcher/src/components/gatekeeper_policy.rs ghost-launcher/tests docs/ADR
```

---

## 5. PR4: Whale Top3 Semantic Contract

### Cel

Nazwać i zabezpieczyć rzeczywistą semantykę obecnej metryki: top3 signer-volume ratio.

### Nowy Kontrakt

Nowe pole:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub top3_signer_volume_ratio: Option<f64>
```

Nie używać `f64 #[serde(default)]` jako nowego pola, bo stary payload bez pola dostałby `0.0`, co zlewa brak danych z prawdziwym zerem.

Dodać helper na `TxIntelFeatures`:

```rust
pub fn effective_top3_signer_volume_ratio(&self) -> f64 {
    self.top3_signer_volume_ratio.unwrap_or(self.top3_volume_pct)
}
```

Kontrakt helpera:

- nowe payloady używają `Some(ratio)`,
- stare payloady fallbackują do starego aliasu `top3_volume_pct`,
- ratio ma skalę `0.0..1.0`,
- helper jest jedynym preferowanym odczytem w nowym code path.

### Zmiany Implementacyjne

W tx intelligence analysis/engine:

- Dodać `top3_signer_volume_ratio` do `SignerDiversityProfile`.
- Obecne obliczenie pozostaje:
  - agregacja wolumenu po signerze,
  - sort descending,
  - suma top3 signer volumes,
  - podział przez total volume.
- Nowe materializacje ustawiają:
  - `top3_signer_volume_ratio = Some(ratio)`,
  - `top3_volume_pct = ratio` jako alias kompatybilnościowy.

W policy:

- Nowy code path czyta `effective_top3_signer_volume_ratio()`.
- Hard-fail/top3 dominance thresholds, które dziś są ratio, dalej porównują ratio.
- Nie zmieniać nazw config thresholds w tym PR, chyba że testy pokażą lokalny komentarz jako wystarczający kontrakt.

W PDD:

- PDD `whale_top3_pct` pozostaje procentem `0.0..100.0`.
- Wejście do PDD musi jawnie konwertować:

```rust
let whale_top3_pct = effective_top3_signer_volume_ratio() * 100.0;
```

- Jeśli config `whale_top3_max_pct = 60.0`, to jest threshold procentowy `0..100`, nie ratio.
- Komentarz przy konwersji musi to powiedzieć wprost.

W loggerze:

- Dodać nowe opcjonalne pole `top3_signer_volume_ratio`.
- Zachować:
  - `top3_volume_pct`,
  - `gk_top3_volume_pct`,
  - `pdd_whale_top3_pct`.
- Stare pola pozostają compatibility aliasami.

### Testy PR4

Dodać testy:

- Signer-volume semantic:
  - top3 liczone po signerach i wolumenie, nie po tx count.
- Scale:
  - 60% koncentracji => ratio `0.60`,
  - PDD input => pct `60.0`.
- New payload:
  - `top3_signer_volume_ratio == Some(ratio)`,
  - `top3_volume_pct == ratio`.
- Old payload:
  - brak `top3_signer_volume_ratio`,
  - `effective_top3_signer_volume_ratio()` fallbackuje do `top3_volume_pct`,
  - nie powstaje ciche `0.0`.
- Logger:
  - nowe pole jest emitowane dla nowych payloadów,
  - stare pola nadal istnieją.

### DoD PR4

- Nowe pole jest `Option<f64>`, nie defaultowane gołe `f64`.
- Istnieje i jest używany `effective_top3_signer_volume_ratio()`.
- PDD jawnie konwertuje ratio `0..1` do pct `0..100`.
- Stare pola są zachowane jako aliasy kompatybilnościowe.
- Testy obejmują serde fallback i skalę.
- ADR-8D opisuje różnicę signer-volume ratio vs percent threshold in PDD.

Walidacja:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher tx_intelligence -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests top3 -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression top3 -- --nocapture
cargo test -p ghost-core tx_intelligence -- --nocapture
git diff --check -- ghost-core/src/tx_intelligence ghost-launcher/src/tx_intelligence ghost-launcher/src/components ghost-brain/src/oracle/decision_logger.rs docs/ADR
```

---

## 6. Finalna Walidacja Po PR4

Po zakończeniu wszystkich PR-ów:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression -- --nocapture
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test session_lifecycle_tests -- --nocapture
cargo test -p ghost-core -- --nocapture
git diff --check
```

Ręczne gates:

- `run_assessment()` nie produkuje aktywnej decyzji.
- `assessment.hard_reject_reason` jest tylko compatibility/export.
- `use_three_layer_decision=false` nie zmienia hard-fail semantics.
- Dev sold jest w macierzy hard-fail parity.
- PDD missing sequence ma typed unavailable status.
- APS odróżnia unavailable od not applicable i known false.
- Alpha disabled jest neutralized, nie raportowane jako real alpha quality.
- Alpha unavailable nie daje canonical zero.
- `top3_signer_volume_ratio` jest preferowanym polem, a `top3_volume_pct` jest aliasem.
- PDD top3 używa procentu `0..100`, nie ratio threshold przez pomyłkę.

---

## 7. Delegation Trace

```yaml
delegation_trace:
  task_classification: "cross-cutting Gatekeeper policy/SSOT/availability planning"
  routing_performed: true
  primary_specialist: "Gatekeeper Policy Auditor"
  supporting_specialists_considered:
    - "SSOT Feature Materialization Guardian"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
    - "Ghost Runtime Coordinator"
  specialist_docs_loaded:
    - "docs/agents/gatekeeper-policy-auditor.md"
  specialist_docs_not_loaded:
    - name: "ssot-feature-materialization-guardian.md"
      reason: "MaterializedFeatureSet boundary was inspected directly; no broad SSOT refactor is planned."
    - name: "decision-logging-replay-analyst.md"
      reason: "Logging changes are additive compatibility fields; replay redesign is out of scope."
    - name: "config-rollout-safety-reviewer.md"
      reason: "No config threshold/default changes are planned."
    - name: "ghost-runtime-coordinator.md"
      reason: "Routing was clear after Gatekeeper policy inspection; no multi-runtime orchestration change."
  skills_used:
    - "ghost-execution"
    - "abstract-reasoning"
    - "rust-master"
    - "gatekeeper-shadow-burnin-audit memory skill"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet remains canonical decision snapshot"
    - "No duplicate hard-fail threshold authority"
    - "Policy owns verdict/reason"
    - "use_three_layer_decision=false does not become alternate risk engine"
    - "DecisionLogger/replay compatibility remains additive"
    - "PDD unavailable is not silently clean"
    - "Shadow/live boundary unchanged"
    - "No legacy HyperPrediction/Chaos revival"
  unresolved_routing_uncertainty: []
```
