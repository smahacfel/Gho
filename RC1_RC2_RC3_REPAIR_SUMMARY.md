# RC1 / RC2 / RC3 Repair Summary

Ten dokument opisuje **wyłącznie zmiany, które realnie wylądowały w repo** dla problemów,
które w trakcie audytu nazywałem `RC1`, `RC2`, `RC3` (zakładam, że Twoje `RS1/RS2/RS3`
odnosi się do tej samej trójki).

Nie opisuję tu planów „na później”, tylko:

1. **jaki był problem,**
2. **jaki był root cause,**
3. **co zostało zmienione w kodzie,**
4. **co jeszcze jest już tylko follow-upem / walidacją rerunu.**

---

## RC1 — split-brain Gatekeeper artifact routing

### Problem

Gatekeeper decyzje i BUY logi nie trafiały do tego samego clean namespace co reszta
artefaktów rolloutu. W praktyce clean rerun mógł zapisywać część artefaktów pod
`[oracle].decision_log_path`, a Gatekeeper verdict artifacts dalej lądowały pod starym,
historycznym rootem.

### Root cause

Problem nie był w samym `DecisionLogger` v17, tylko w **runtime wiring**:

- `ghost-launcher/src/oracle_runtime.rs` budował `DecisionLoggerConfig` z historycznego rootu,
  zamiast z `[oracle].decision_log_path`,
- `DecisionLoggerConfig::default()` nadal niósł rollout-specific assumptions,
  które maskowały błędną konfigurację runtime.

### Co zostało naprawione

#### 1. Runtime przestał hardcodować stary root

W `ghost-launcher/src/oracle_runtime.rs` został dodany helper:

- `build_decision_logger_config(...)`

Ten helper:

- bierze **runtime `decision_log_path`** jako SSOT,
- ustawia:
  - `log_dir`,
  - `gatekeeper_log_dir`,
  - `gatekeeper_rollout_profile`,
  - `gatekeeper_config_hash`,
- i na tej bazie konstruuje `DecisionLoggerConfig`.

To oznacza, że Gatekeeper verdict artifacts są już routowane z tego samego runtime root family,
z którego korzysta reszta artefaktów clean rerunu.

#### 2. Default biblioteczny przestał wciskać rollout-specific path

W `ghost-brain/src/oracle/decision_logger.rs`:

- `DecisionLoggerConfig::default().gatekeeper_log_dir == log_dir`,
- rollout profile default to teraz `unknown_rollout`,
- config default nie zakłada już `shadow-burnin`.

To przywraca prosty kontrakt:

> runtime podaje root, biblioteka go nie nadpisuje własnym rollout-specific domysłem.

#### 3. Zostały dodane regresje i poprawiona dokumentacja operatorska

Landed files:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-brain/examples/oracle_decision_dry_run.rs`
- `docs/RUNBOOK_PRODUCTION_ROLLOUT.md`

#### 4. Follow-up po review: kontrakt katalogowy został zamrożony wyraźniej

Po review został jeszcze dopięty niski follow-up:

- launcher default dla `[oracle].decision_log_path` to teraz jawnie katalog:
  - `logs/decisions`
- legacy wartości wyglądające jak plik, np.:
  - `logs/decisions.jsonl`
  są normalizowane migracyjnie do katalogu `logs/decisions`

To usuwa operatorską dwuznaczność typu „wartość wygląda jak plik, ale runtime traktuje ją jak katalog”.

### Co zweryfikować w kodzie

#### Runtime

Sprawdź:

- `ghost-launcher/src/oracle_runtime.rs`
  - `derive_gatekeeper_rollout_profile(...)`
  - `build_decision_logger_config(...)`
  - miejsce, gdzie runtime wywołuje `build_decision_logger_config(&decision_log_path, ...)`

#### Decision logger default

Sprawdź:

- `ghost-brain/src/oracle/decision_logger.rs`
  - `DecisionLoggerConfig`
  - `impl Default for DecisionLoggerConfig`

### Status

**Implemented:** tak  
**Current:** root-dir split-brain został usunięty  
**Pending:** tylko hygiene/empirical verification na świeżych rerunach

### Ważna uwaga

`gatekeeper_rollout_profile` jest nadal wyprowadzany z komponentów ścieżki,
więc konfigi powinny używać **znormalizowanych rollout roots** zamiast ścieżek z `../`.
To nie cofa RC1, ale jest ważne przy weryfikacji rerunów.

---

## RC3 — deterministic shadow execution failure / `AccountNotFound`

### Problem

Shadow BUY dispatch wywalał się deterministycznie, a dominujące failure surface przez długi czas
nie dawały rzetelnej odpowiedzi, czy problem jest:

- transportowy,
- payer-related,
- builder/account-contract related,
- czy stricte semantyczny po stronie programu.

W praktyce cień działał jak wydmuszka diagnostyczna: było wiadomo, że BUY verdict powstał,
ale nie było pewnej odpowiedzi, **dlaczego shadow dispatch nie doszedł do sensownej symulacji**.

### Root cause

Najważniejsza naprawiona część RC3 była kontraktowa:

- shadow-only był za mocno spleciony z live payer contract,
- failure reporting było zbyt grube (`transport|semantic|unknown`),
- runtime nie emitował failure-side evidence przez ten sam kontrakt co sukcesy,
- przez to `AccountNotFound` i podobne błędy były trudne do rozdzielenia od problemów z payerem,
  przygotowaniem requestu albo samą symulacją.

### Co zostało naprawione

#### 1. Jawny kontrakt payera dla shadow-only

W `ghost-launcher/src/config.rs` została dodana jawna strategia:

- `trigger.shadow_run.payer_strategy = "configured" | "ephemeral"`

Znaczenie:

- `configured` — shadow używa istniejącego payer-backed path,
- `ephemeral` — shadow-only **nie zależy** od live trigger payer contract
  i może działać z launcher-local signerem.

Fail-closed validation:

- live / live_and_shadow dalej wymagają poprawnego configured payer path,
- `shadow_only + configured` bez keypair ma failować jawnie,
- `shadow_only + ephemeral` może działać bez tego zależenia.

#### 2. Payer provenance jest teraz częścią artefaktu

Landed fields:

- `payer_provenance`

trafiają do:

- `ghost-launcher/src/components/trigger/shadow_run.rs`
  - `ShadowBuySimulationReport`
  - `ShadowBuySimulationRecord`
- `ghost-launcher/src/events.rs`
  - `ShadowBuySimulationEvent`

To usuwa dawną ślepotę: z logu widać już, czy dany shadow attempt szedł przez payer configured,
czy ephemeral.

#### 3. Shadow failure jest klasyfikowany deterministycznie

Zamiast grubych kubełków, landed contract niesie:

- `error_class`
- opcjonalnie także bardziej szczegółowy `error_code` / `error_detail_class`

Dzięki temu failure evidence przestał być „transport/semantic/unknown”,
a stał się audytowalny.

#### 4. Runtime emituje failure-side evidence tym samym kontraktem co success-side

W `ghost-launcher/src/oracle_runtime.rs` shadow dispatch failures też przechodzą przez
`ShadowBuySimulated`, a nie tylko udane symulacje.

To było ważne, bo dopiero wtedy jeden rerun może spiąć:

`BUY verdict -> prepared shadow attempt -> simulation result / failure class`

bez rozrywania evidence chain.

#### 5. Follow-up po review: event bus dostał pełny kontrakt diagnostyczny

Po review dopięty został jeszcze brakujący detal symetrii diagnostycznej:

- `ShadowBuySimulationEvent` niesie już addytywnie:
  - `error_code`
  - `error_detail_class`
- klasyfikacja błędu została zebrana do wspólnego helpera w
  `ghost-launcher/src/components/trigger/shadow_run.rs`
- `spawn_background_shadow_event(...)` przestał ręcznie budować event z
  `error_class: None` i używa wspólnego buildera report->event
- `TriggerDispatchFailureContext` niesie teraz także:
  - `payer_pubkey: Option<String>`

To oznacza, że event bus i JSONL nie są już rozjechane kontraktowo na poziomie szczegółowości
RC3 diagnostics.

### Co zweryfikować w kodzie

Sprawdź:

- `ghost-launcher/src/config.rs`
  - `TriggerShadowPayerStrategy`
  - `TriggerShadowRunConfig::payer_strategy`
  - `validate_execution_profile()`
- `ghost-launcher/src/components/trigger/component.rs`
  - payer provenance w prepared/failure contexts
- `ghost-launcher/src/components/trigger/shadow_run.rs`
  - `ShadowBuySimulationReport`
  - `ShadowBuySimulationRecord`
- `ghost-launcher/src/events.rs`
  - `ShadowBuySimulationEvent`
- `ghost-launcher/src/oracle_runtime.rs`
  - emission path dla shadow failures

### Status

**Implemented:** tak  
**Current:** payer/evidence-side deterministic failure contract jest naprawiony  
**Pending:** jedynie dalsza walidacja konkretnych program error classes na świeżych rerunach

### Co dokładnie uważam za naprawione w RC3

Nie twierdzę, że każdy możliwy błąd BUY path przestał istnieć. Twierdzę coś precyzyjniejszego:

- shadow dispatch przestał być ślepy kontraktowo,
- `AccountNotFound` przestał być mieszany z problemem payer contract,
- runtime i shadow artifacts niosą już wystarczającą informację,
  żeby odróżniać payer/setup/builder/semantic failures.

To jest powód, dla którego uznałem RC3 za rozwiązane na poziomie kontraktu wykonania
i diagnostyki.

---

## RC2 — observation starvation / freshness semantics

### Problem

RC2 nie było po prostu „V2.5 za ostre”. Dominujący problem polegał na tym, że runtime
często **nie widział wystarczająco świeżej chain-truth obserwacji w realnym oknie decyzyjnym**.

Objawy:

- dominacja `stale_or_late_arrival`,
- silna korelacja z:
  - `seer_account_updates_before_mapping_total > 0`
  - `seer_account_updates_pending_replay_total > 0`,
- dominacja `ingress_wall` jako effective time source,
- watch registration częściej z `account_update` niż z `create`.

Innymi słowy: system miał aktywność, ale zbyt często docierała ona do Gatekeepera
przez **late mapping + replay path**, a nie przez zdrowy live path.

### Root cause cluster

RC2 miało dwa poziomy:

#### Poziom A — starvation / replay path

- curve->mint mapping i watch activation następowały za późno,
- wczesne `AccountUpdate` wpadały do pending replay,
- pending replay stawał się głównym recovery path.

#### Poziom B — freshness semantics były błędne

Nawet kiedy replay niósł realną prawdę z chainu, runtime traktował to zbyt często jak coś
„wall-clock shaped” albo wprost staleness proxy.

To właśnie ten **poziom B** został domknięty w ostatniej fali implementacji.

### Co zostało naprawione w kodzie

#### 1. `AccountUpdate` dostał własne `EventTimeMetadata` end-to-end

Landed fields:

- `off-chain/components/seer/src/types.rs`
  - `GeyserEvent::AccountUpdate { event_time }`
- `off-chain/components/seer/src/ipc.rs`
  - `DetectedAccountUpdateEvent.event_time`
- `ghost-launcher/src/events.rs`
  - `AccountUpdateEvent.event_time`

To oznacza, że `AccountUpdate` przestał być „korektą bez własnego czasu”.

#### 2. AccountUpdate semantics przestały być hardcoded jako wall-clock

W `ghost-core/src/event_semantics.rs`:

- `normalize_account_update_semantics(...)` przyjmuje teraz `EventTimeMetadata`
- `TimestampQuality` jest wyprowadzany z provenance:
  - `Chain`, jeśli jest `chain_event_ts_ms`
  - `Adapter`, jeśli jest `ingress_wall_ts_ms`
  - `WallClock` tylko jako real fallback

To eliminuje dawny błąd, w którym replayed/live account updates z prawdziwą ingress provenance
były i tak wrzucane do worka `WallClock`.

#### 3. Replay przestał gubić oryginalną provenance czasu

W `off-chain/components/seer/src/lib.rs`:

- `PendingCurveUpdateSnapshot` przechowuje już `event_time`,
- `queue_pending_curve_update(...)` zachowuje oryginalną provenance,
- replayed `AccountUpdate` używa oryginalnego `event_time`,
  zamiast udawać, że jedyny czas to chwila replayu.

To jest krytyczne, bo bez tego runtime nie odróżnia świeżego replayed truth
od naprawdę spóźnionego replayu.

#### 4. Runtime dostał jawny account-update time-source contract

W `ghost-launcher/src/oracle_runtime.rs` dodany został:

- `runtime_account_update_time_source_info(...)`

oraz account-update latency/freshness logic, które:

- preferuje explicit event-axis time,
- dopiero potem replay dwell,
- a `detected_at` zostawia jako fallback ostatniej szansy.

#### 5. Coverage audit dostał account-update freshness diagnostics

W `ghost-core/src/coverage_audit.rs` doszły m.in.:

- `account_update_runtime_seen_total`
- `account_update_runtime_accepted_total`
- `account_update_runtime_seen_by_effective_time_source`
- `account_update_runtime_seen_by_fallback_class`
- `record_account_update_runtime_seen(...)`

To było konieczne, bo wcześniej tx-path miał znacznie bogatszą diagnostykę niż account-update path.

#### 6. `pending_replay` przestał sam z siebie oznaczać staleness

W `ghost-core/src/coverage_audit.rs::classify_timeout_window(...)`
zostało usunięte stare sprzężenie:

> `pending_replay_total > 0` => `StaleOrLateArrival`

Teraz replay sam w sobie nie wystarcza. Potrzebne jest realne lateness evidence
albo fallback-heavy freshness path.

To była najważniejsza semantyczna naprawa RC2 wave 2.

#### 7. Follow-up po review: fallback detection jest już jawniejszy

Po review został jeszcze dopięty niski hardening:

- `classify_timeout_window(...)` nie używa już kruchego
  `source.contains(\"fallback\")`
- fallback source jest rozpoznawany przez jawny helper oparty o znane,
  explicit runtime source values

To nie zmienia semantyki raportu, ale usuwa stringową heurystykę, która mogła być myląca
przy przyszłych rename'ach source labels.

### Co zweryfikować w kodzie

Sprawdź:

- `ghost-core/src/event_semantics.rs`
  - `normalize_account_update_semantics(...)`
- `ghost-core/src/coverage_audit.rs`
  - `CoverageAuditWindowDiagnostics`
  - `record_account_update_runtime_seen(...)`
  - `classify_timeout_window(...)`
- `off-chain/components/seer/src/types.rs`
  - `GeyserEvent::AccountUpdate`
- `off-chain/components/seer/src/ipc.rs`
  - `DetectedAccountUpdateEvent`
- `off-chain/components/seer/src/lib.rs`
  - replay / pending snapshot path
- `off-chain/components/seer/src/grpc_connection.rs`
  - live AccountUpdate producer
- `ghost-launcher/src/events.rs`
  - `AccountUpdateEvent`
- `ghost-launcher/src/oracle_runtime.rs`
  - `runtime_account_update_time_source_info(...)`

### Walidacja, którą zrobiłem

Zielone targeted validations dla tej fali:

- `cargo test -p seer account_update_ --lib`
- `cargo test -p ghost-core account_update_ --lib`
- `cargo test -p ghost-core recorder_keeps_phase3_canonical_ingest_diagnostics --lib`
- `cargo test -p ghost-core recorder_does_not_mark_timeout_stale_for_fresh_replay_alone --lib`
- `cargo check -p ghost-launcher --lib`

### Status

**Implemented:** tak, dla runtime freshness / provenance bug  
**Current:** stary replay-guilt-by-association bug jest usunięty  
**Pending:** tylko empiryczna ocena świeżego rerunu + secondary ingest starvation cleanup

#### Ważne doprecyzowanie kontraktu RC4 / coverage v5

Obecny repair **nie** oznacza, że każdy opcjonalny element pełnego schema surface v5 jest
zawsze serializowany.

Oznacza coś węższego i intencjonalnego:

- pola wymagane przez promotion validator są zawsze obecne:
  - `timeout_primary_cause`
  - `timeout_flags`
  - `filtered_reason_keys`
  - `dominant_runtime_effective_time_source`
- mogą wystąpić jako `null` albo `[]`, ale nie znikają z JSONL przez
  `skip_serializing_if`
- inne addytywne pola v5, np. puste mapy diagnostyczne, nadal mogą pozostać warunkowo
  pomijane, jeśli validator ich nie wymaga

Czyli obecny landed kontrakt to:

> **validator-required v5 surface is always present**,  
> a nie: **entire v5 schema is always materialized in every row**.

### Co dokładnie uważam za rozwiązane w RC2

Nie twierdzę, że już każdy TIMEOUT zniknął. Twierdzę coś węższego, ale technicznie ważniejszego:

- runtime **już nie fałszuje** freshness semantics dla `AccountUpdate`,
- replayed truth nie jest automatycznie uznawany za stale,
- coverage wreszcie rozróżnia:
  - real lateness,
  - fallback-heavy timing,
  - brak explicit provenance,
  - zwykły replay, który nadal może być świeży.

To znaczy, że **core runtime bug w RC2 siedzący w kodzie został naprawiony**.
Pozostały już tylko pytania empiryczne:

- ile realnego starvation jeszcze zostało po tej naprawie,
- ile z tego to secondary ingest loss,
- a ile to już po prostu genuine no-interest / explicit filter causes.

---

## Podsumowanie w jednym miejscu

### RC1

**Naprawione:** split-brain root dla Gatekeeper verdict artifacts  
**Jak:** runtime i defaulty loggera wróciły do `[oracle].decision_log_path` jako SSOT

### RC3

**Naprawione:** shadow execution/payer/evidence contract  
**Jak:** jawny payer strategy + payer provenance + structured failure classes + symmetric event emission

### RC2

**Naprawione:** runtime freshness/time-source semantics dla `AccountUpdate`  
**Jak:** `EventTimeMetadata` end-to-end + provenance-derived semantics + runtime freshness diagnostics + usunięcie replay-as-stale shortcut

---

## Ostatnia ważna uwaga

Jeżeli podczas Twojej weryfikacji zobaczysz, że jakiś obszar dalej zachowuje się źle,
to rozdziel proszę dwie rzeczy:

1. **czy kodowy kontrakt został naprawiony**,  
2. **czy świeży rerun już pokazał oczekiwany efekt liczbowy**.

W RC1 i RC3 naprawa kontraktu jest zasadniczo zakończona.  
W RC2 naprawa kontraktu runtime freshness też jest zakończona, ale końcowe potwierdzenie
liczbowe nadal wymaga oceny świeżych artefaktów rerunu.
