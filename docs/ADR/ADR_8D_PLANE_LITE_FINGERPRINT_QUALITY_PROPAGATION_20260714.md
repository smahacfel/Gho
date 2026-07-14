# ADR-8D: Plane-Lite — propagacja jakości fingerprintu przez MFS

Status: `IMPLEMENTED / LOCAL VALIDATION PASSED / CI GATED BY PR #66`

Typ: ADR-8D / Decision Plane / SSOT materialization / review fix

Data: 2026-07-14

Repo: `smahacfel/Gho`

PR: `#66`

Poziom ryzyka: `MEDIUM` — zmiana domyka kontrakt jakości kanonicznego
evidence snapshotu, ale nie zmienia progów, Gatekeeper V2 policy, V2.5,
algorytmu V3 ani execution.

## 1. Problem wykryty w follow-up review

Po przepełnieniu bounded fingerprint replay history silnik prawidłowo
zachowywał pełny agregat według poprzedniego anchora i zwracał:

```text
fingerprint_degraded = true
fingerprint_reason = FINGERPRINT_REPLAY_HISTORY_TRUNCATED
```

`PoolObservationSession::try_materialize_features()` kopiował jednak do
`AlphaFingerprintFeatures` wyłącznie wartości liczbowe. `evidence_status.alpha`
wynikał tylko z liczby obecnych pól, dlatego dziewięć dostępnych pól mogło
zostać oznaczonych jako `Clean`, mimo że pochodziły z agregatu, którego nie
udało się bezpiecznie przebudować dla nowego anchora.

Ten sam zdegradowany fingerprint mógł zasilać alpha-dependent
`manipulation_contradictions` oraz legacy `flipper_presence_ratio` w frozen
wejściu PR2B bez kontraktu jakości.

## 2. Red proof

Najpierw dodano test pełnej granicy:

```text
10 fingerprint-eligible events z kompletnymi polami alpha
  -> bounded replay history truncates
  -> late real creation slot / identity update
  -> EarlyFingerprintMetrics = DEGRADED
  -> session.try_materialize_features()
  -> V3 actionability
  -> PR2B compact projection
```

Przed poprawką test odtworzył błąd:

```text
left:  Clean
right: Degraded
```

czyli `EarlyFingerprintMetrics` był degraded, ale
`MaterializedFeatureSet.evidence_status.alpha` był clean.

## 3. Decyzja implementacyjna

### 3.1 Kanoniczny MFS zachowuje jakość źródła

`AlphaFingerprintFeatures` otrzymuje dwa addytywne pola:

```text
fingerprint_degraded: bool
fingerprint_reason: Option<String>
```

Oba są materializowane z tego samego `EarlyFingerprintMetrics`, z którego
pochodzą wartości alpha. Historyczne payloady zachowują kompatybilność przez
`#[serde(default)]`; brak pól oznacza `false` i `None`.

### 3.2 Degradacja ma pierwszeństwo przed kompletnością

`materialize_v3_evidence_status()` zachowuje `Unavailable`, gdy nie istnieje
żadne pole alpha. Jeśli co najmniej jedno pole istnieje, source-level
`fingerprint_degraded` ma pierwszeństwo przed kompletnością: alpha otrzymuje
`EvidenceStatus::Degraded` i `AlphaEvidencePartial`, nawet gdy wszystkie
dziewięć liczbowych pól jest obecnych.

Brak degradacji zachowuje dotychczasową regułę:

- brak pól — `Unavailable`;
- dowolne pola ze zdegradowanego źródła — `Degraded`;
- 9 pól z niezdegradowanego źródła — `Clean`;
- część pól z niezdegradowanego źródła — `Degraded`.

### 3.3 Konsumenci zależni od alpha

`manipulation_contradictions` nie może otrzymać `Clean`, gdy jego alpha input
ma `fingerprint_degraded = true`.

V3 nie otrzymuje nowej reguły policy. Istniejący actionability contract
automatycznie widzi `evidence_status.alpha = Degraded`; source quality i reason
są również częścią deterministycznego V3 feature snapshot hash.

Legacy PR2B `flipper_presence_ratio` nie ma osobnego pola jakości. Dlatego
została wybrana prostsza bezpieczna semantyka: jeśli cały fingerprint jest
degraded, `legacy_flip_ratio` jest pomijane i materializuje się jako
`Unavailable / NotApplicable / Null`. Niezależny FlipV2 pozostaje bez zmian.

## 4. Zakres zmian

- `ghost-core/src/checkpoint/types.rs` — addytywne pola source quality w
  `AlphaFingerprintFeatures`;
- `ghost-launcher/src/session/observation.rs` — propagacja jakości, pierwszeństwo
  degradacji, manipulation inheritance i wyłączenie degraded legacy flip;
- `ghost-launcher/src/components/gatekeeper_v3.rs` — quality/reason w snapshot
  hash oraz test jego feature sensitivity;
- `ghost-launcher/tests/session_lifecycle_tests.rs` — pełny truncation → MFS →
  V3 → PR2B regression contract;
- `ghost-core/tests/feature_builder_tests.rs` — backward-compatible serde;
- istniejące ADR-y Plane-Lite — status CI i aktualny kontrakt MFS.

## 5. Niezmienione kontrakty

- wartości i formuły metryk fingerprintu;
- Gatekeeper V2 policy, thresholds, verdicts i reason codes;
- V2.5, DOW, TAS, PDD i APS;
- algorytm, progi i promotion status Gatekeeper V3;
- Type-5, selector i IWIM;
- FlipV2 oraz jego typed evidence;
- config i defaults;
- DecisionLogger schema;
- postbuy i shadow/live execution.

## 6. Weryfikacja

Warunki akceptacji:

- source-level degraded nigdy nie materializuje alpha jako clean;
- dokładny fingerprint reason przechodzi do MFS;
- alpha-dependent manipulation evidence nie jest clean;
- V3 actionability nie przedstawia degraded alpha jako actionable;
- legacy flip ratio z degraded fingerprintu jest null/unavailable;
- historyczny MFS bez nowych pól nadal się deserializuje;
- V3 snapshot hash zmienia się przy zmianie quality/reason;
- Gatekeeper V2/V2.5/V3 i metric-contract regressions pozostają zielone.

Red/green proof oraz pełna lista wykonanych komend są utrzymywane w treści
PR #66. Aktualny head przed oznaczeniem PR jako ready musi przejść wymagane
GitHub Actions; status PR, a nie statyczny dokument, jest źródłem prawdy o
wyniku CI dla konkretnego SHA.

## 7. Rollback

Rollback to revert follow-up commita propagacji jakości. Nie ma migracji
configu ani danych. Nowe pola serde są addytywne, więc ich usunięcie nie jest
wymagane do odczytu starszych payloadów; wycofanie przywraca jednak błędne
przedstawianie zdegradowanego fingerprintu jako clean i dlatego nie powinno być
wykonywane bez równoważnej naprawy quality contract.

Uwaga: wskazany przez instrukcję repo szablon `ADR_8D_SZABLON.md` nie występuje
w dostępnym checkoutcie; zachowano format zastosowany w ADR-ach Plane-Lite.
