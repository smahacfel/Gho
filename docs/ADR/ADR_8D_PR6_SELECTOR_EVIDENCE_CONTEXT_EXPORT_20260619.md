# ADR-8D: PR6 Selector Evidence Context Export

Status: IMPLEMENTED / TARGETED_STATIC_AND_CONTRACT_SMOKE_VERIFIED
Typ: ADR-8D / selector dataset evidence export, no-silent-imputation contract
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: PR6 jako techniczne domkniecie warunku z planu evidence coverage contract: dataset/selector ma widziec wartosc oraz evidence present/status/source/sample/staleness/reason bez cichego mieszania `0`, `null`, degraded i carried-forward
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `scripts/build_selector_gatekeeper_feature_context.py`
- `scripts/build_selector_training_view.py`
- `scripts/test_selector_pipeline.py`
- `docs/ADR/ADR_8D_PR6_SELECTOR_EVIDENCE_CONTEXT_EXPORT_20260619.md`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ADR-ach PR1-PR5.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Po PR5 pozostalo domkniecie warunku datasetowego z planu: selector/offline nie moze dostawac samych suchych liczb, jezeli dla tej samej metryki istnieje evidence status/source/staleness/sample context.

Rzeczywisty przebieg:
- Potwierdzono, ze plan formalnie opisuje maksymalnie PR1-PR5, ale zawiera jawny wymog: `value + present + status + source + staleness` dla selector/dataset, aby nie uczyc artefaktow runtime.
- Zidentyfikowano aktywny offline path:
  - `scripts/build_selector_gatekeeper_feature_context.py`
  - `scripts/build_selector_training_view.py`
- Nie zatrzymywano i nie restartowano aktywnego runa zbierajacego dane.
- Nie zmieniano Gatekeeper policy, runtime scoringu, DecisionLogger runtime mappera, shadow execution ani Solana execution path.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT, DecisionLogger/replay boundary, selector/offline evidence semantics.
- `rust-master`: ostroznosc przy aktywnym runtime i kontrakcie serializacji, mimo ze PR6 dotyka Pythona.
- `trading-systems`: rozdzielenie value, missing evidence, degraded evidence i carried-forward evidence w datasetach.

Zaladowane instrukcje/specjalistyczne:
- Repo-local `AGENTS.md`
- `.agents/skills/ghost-execution/SKILL.md`
- `.agents/skills/rust-master/SKILL.md`
- `.agents/skills/trading-systems/SKILL.md`

Nie ladowano dokumentow:
- `solana-execution-path-engineer`: PR6 nie dotyka TX builderow, sendera, blockhash, retry ani confirmation.
- `seer-ingest-event-integrity-specialist`: PR6 nie dotyka ingestu, parserow, event ordering ani Yellowstone/Geyser.
- `gatekeeper-policy-auditor`: PR6 nie zmienia verdictow, hard gates ani reason policy; policy wiring bylo zakresem PR5.

## 3. Opis problemu - 3W2H

What:
Po PR1-PR5 runtime potrafi emitowac policy/evidence context, CPV evidence i temporal delta evidence, ale selector dataset builder nadal mogl widziec glownie same wartosci `gk_*`. Bez companion fields downstream nie odroznial:
- `0` jako rzeczywistego wyniku,
- `null` jako braku evidence,
- `degraded_low_sample` jako slabej probki CPV,
- `carried_forward_no_event` jako jawnie przeniesionego stanu,
- `not_allowed` jako odmowy carry-forward dla ratio/state metryki.

Where:
- `gatekeeper_feature_context_v1.jsonl`
- `gatekeeper_feature_context_manifest_v1.json`
- `selector_training_view_v1` join summary
- offline selector/model candidate pipeline

Why it matters:
Bez tego PR selector mogl trenowac na wartosciach bez evidence contextu albo traktowac runtime missing jako normalny sygnal rynkowy. To byloby dokladnie ryzyko wskazane w planie: ladniejsze coverage procenty kosztem prawdy decyzyjnej.

How observed:
Plan i runtime review wskazywaly problemy z rozroznieniem `null`, `0`, degraded CPV i carried temporal deltas. Dodatkowo `build_selector_training_view.py` klasyfikowal wszystkie `gk_*` poza kilkoma provenance fields jako model feature columns, co mogloby wrzucic stringowe statusy do summary modelowych kolumn.

How many / scale:
Zmiana dotyczy kazdego wygenerowanego Gatekeeper feature context row i kazdego training view joinu, ale nie dotyka aktywnego runtime decision path.

## 4. Przyczyna zrodlowa

Root cause:
Dataset builder nie mial jawnego evidence contractu na poziomie kolumn. Istniejace `model_policy = "missing_not_zero"` bronilo przed jedna klasa bledu, ale nie zapewnialo:
- `gk_<metric>_present`,
- `gk_<metric>_status`,
- `gk_<metric>_source`,
- `gk_<metric>_sample_count`,
- `gk_<metric>_required_*_sample_count`,
- `gk_<metric>_staleness_ms`,
- `gk_<metric>_carried_from_anchor_ms`,
- `gk_<metric>_reason` / `gk_<metric>_degraded_reason`.

Dodatkowo training view summary nie rozroznial wartosci modelowych od evidence companion fields.

## 5. Strategia naprawy

Przyjeta strategia:
- Zachowac istniejace wartosci modelowe `gk_<metric>` jako wartosci.
- Dodac evidence companion fields addytywnie.
- Nie zamieniac `null` w `0`.
- Jezeli top-level nie ma wartosci, ale embedded `v3_materialized_feature_snapshot` ma kanoniczna wartosc dla allowlistowanej metryki, builder moze wyemitowac wartosc z embedded SSOT z jawnie oznaczonym source/status.
- CPV evidence czytac z `sybil_resistance.cpv_evidence`.
- Temporal evidence czytac z `temporal_deltas.delta_evidence`.
- Policy context czytac z top-level `evidence_policy_context`.
- Manifest rozdziela:
  - `model_feature_columns`,
  - `evidence_feature_columns`,
  - `evidence_value_present_rates`.
- Training view kopiuje evidence fields do rows, ale nie klasyfikuje ich jako `model_feature_columns`.

Granice:
- Brak zmian Gatekeeper policy.
- Brak zmian verdictow BUY/REJECT/TIMEOUT.
- Brak zmian DecisionLogger runtime schema.
- Brak zmian shadow/live behavior.
- Brak zmian CPV denominatora.
- Brak future-fill.
- Brak `null -> 0`.
- Brak uzywania status stringow jako domyslnych cech modelowych.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: evidence contract version i manifest
- Plik: `scripts/build_selector_gatekeeper_feature_context.py`
- Dodano:
  - `EVIDENCE_CONTRACT_VERSION = "gatekeeper_feature_context_evidence_v1"`
  - `evidence_policy = "value_present_status_source_no_silent_imputation"`
  - `evidence_feature_columns`
  - `evidence_feature_presence`
  - `evidence_value_present_rates`
  - `evidence_value_fields`
  - `evidence_policy_context_fields`

Zmiana 2: canonical `burst_ratio` w selector context
- Plik: `scripts/build_selector_gatekeeper_feature_context.py`
- Dodano `burst_ratio` do `RAW_FEATURES`.
- Builder emituje `gk_burst_ratio`, jezeli top-level lub embedded tx-intel SSOT ma wartosc.

Zmiana 3: evidence companion fields
- Plik: `scripts/build_selector_gatekeeper_feature_context.py`
- Dla krytycznych metryk dodano companion fields w stylu:
  - `gk_<metric>_present`
  - `gk_<metric>_status`
  - `gk_<metric>_source`
  - `gk_<metric>_sample_count`
  - `gk_<metric>_required_sample_count`
  - `gk_<metric>_required_clean_sample_count`
  - `gk_<metric>_required_degraded_sample_count`
  - `gk_<metric>_rolling_state_available`
  - `gk_<metric>_degraded_reason`
  - `gk_<metric>_reason`
  - `gk_<metric>_staleness_ms`
  - `gk_<metric>_carried_from_anchor_ms`

Zmiana 4: policy context export
- Plik: `scripts/build_selector_gatekeeper_feature_context.py`
- Z top-level `evidence_policy_context` eksportowane sa m.in.:
  - `gk_strict_metric_missing_policy`
  - `gk_cpv_low_sample_policy`
  - `gk_temporal_carried_forward_policy`
  - CPV clean/degraded thresholds
  - temporal carry-forward flags
  - `gk_top_level_features_from_materialized_ssot`

Zmiana 5: embedded SSOT fallback
- Plik: `scripts/build_selector_gatekeeper_feature_context.py`
- Dla allowlistowanych metryk builder czyta wartosc z embedded SSOT, gdy top-level nie ma wartosci:
  - CPV: `sybil_resistance.cpv_evidence`
  - temporal deltas/rates: `temporal_deltas` + `delta_evidence`
  - base tx-intel/alpha: `tx_intel_features` i `alpha_fingerprint`
- To nie jest imputacja. Wartosci pochodza z materialized snapshotu, a source/status ujawnia ich pochodzenie.

Zmiana 6: no-silent-imputation behavior
- Plik: `scripts/build_selector_gatekeeper_feature_context.py`
- Jezeli metryka nie ma wartosci ani top-level, ani embedded, `gk_<metric>` nie jest emitowane.
- W takim przypadku moze nadal pojawic sie:
  - `gk_<metric>_present = false`
  - `gk_<metric>_status = not_allowed | unavailable | stale | ...`
  - `gk_<metric>_reason = ...`
- Przyklad: `delta_jito_tip_intensity_1s_to_3s = null` + `not_allowed` evidence nie tworzy `gk_delta_jito_tip_intensity_1s_to_3s = 0`.

Zmiana 7: training view evidence classification
- Plik: `scripts/build_selector_training_view.py`
- Dodano rozpoznawanie evidence companion columns.
- `load_gatekeeper_feature_context()` nadal kopiuje fields do training rows, ale summary:
  - nie wklada evidence/status/source do `model_feature_columns`,
  - dodaje osobna liste `evidence_feature_columns`.

Zmiana 8: test coverage
- Plik: `scripts/test_selector_pipeline.py`
- Dodano test:
  - `test_gatekeeper_feature_context_exports_evidence_without_imputation`
- Rozszerzono test:
  - `test_training_view_joins_gatekeeper_feature_context`
- Testy pokrywaja:
  - CPV degraded low-sample value + status/source/sample counts,
  - embedded fallback dla `cpv_other_pool_activity`,
  - carried-forward temporal delta rowna `0`,
  - `not_allowed` temporal ratio bez wartosci modelowej,
  - policy context export,
  - evidence columns poza training `model_feature_columns`.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Python syntax compile | `python3 -m py_compile scripts/build_selector_gatekeeper_feature_context.py scripts/build_selector_training_view.py scripts/test_selector_pipeline.py` | passed | PASS |
| PR6 builder contract smoke | custom `PYTHONPATH=scripts python3 - <<'PY' ...` | `PR6 evidence builder contract smoke: PASS` | PASS |
| PR6 training view classification smoke | custom `PYTHONPATH=scripts python3 - <<'PY' ...` | `PR6 training view evidence classification smoke: PASS` | PASS |

### Test limitation

Proba uruchomienia pojedynczego unittest z `scripts/test_selector_pipeline.py` nie przeszla przez import modulu testowego:

```text
ModuleNotFoundError: No module named 'build_selector_route_manifest_reuse_projection'
```

To jest istniejacy problem importu/obecnosci modulu w tym checkoutcie, niezalezny od PR6. Z tego powodu wykonano bezposrednie smoke'i kontraktowe na izolowanych tymczasowych danych, obejmujace nowe sciezki PR6.

### Runtime/log proof status

Nie wykonywano nowego runtime runa dla PR6. Aktywny run uzytkownika zostal pozostawiony do zbierania danych zgodnie z dyspozycja. PR6 dotyka offline dataset/export path i wymaga pozniejszego long proof na swiezym `gatekeeper_feature_context_v1.jsonl` / training view, a nie restartu runtime.

## 8. Ryzyka regresji i jak zostaly ograniczone

Ryzyko 1: `null` staje sie `0`.
- Kiedy: helper uzywa `or 0`, `unwrap_or_default` albo fillna.
- Mitigacja: `gk_<metric>` jest emitowane tylko dla finite/bool wartosci. `present=false` nie tworzy wartosci modelowej.

Ryzyko 2: degraded CPV wyglada jak clean.
- Kiedy: builder eksportuje tylko liczbe CPV bez statusu.
- Mitigacja: CPV companion fields niosa `status/source/sample_count/required_*_sample_count/degraded_reason`.

Ryzyko 3: carried-forward delta wyglada jak fresh observed.
- Kiedy: `delta=0` bez statusu.
- Mitigacja: temporal companion fields niosa `status`, `source`, `staleness_ms`, `carried_from_anchor_ms`.

Ryzyko 4: embedded fallback staje sie cicha imputacja.
- Kiedy: builder zgaduje wartosci z dowolnych pol.
- Mitigacja: fallback jest allowlistowany do znanych SSOT sciezek i zawsze towarzyszy mu evidence source/status.

Ryzyko 5: status string trafia jako model feature.
- Kiedy: training view summary bierze wszystkie `gk_*` poza provenance jako modelowe.
- Mitigacja: `build_selector_training_view.py` rozdziela `model_feature_columns` i `evidence_feature_columns`.

Ryzyko 6: downstream nie wie, jak liczono coverage.
- Kiedy: manifest pokazuje tylko obecnosc kolumn, a nie present-rate wartosci.
- Mitigacja: dodano `evidence_value_present_rates`, liczace `gk_<metric>_present == true`.

Ryzyko 7: runtime/shadow regresja.
- Kiedy: offline PR dotyka Gatekeeper runtime, loggera runtime lub execution.
- Mitigacja: PR6 ograniczony do scripts/dataset/test docs. Brak zmian w runtime Rust path.

## 9. Stan po zmianie

Zamkniete w PR6:
- selector Gatekeeper context eksportuje value + evidence companion fields;
- CPV degraded low-sample moze byc analizowane jako degraded, a nie clean;
- carried-forward delta moze byc liczba z jawna jakoscia evidence;
- `not_allowed`/missing ratio nie zamienia sie w `0`;
- `burst_ratio` jest dostepny jako `gk_burst_ratio`;
- manifest ma evidence contract i evidence present rates;
- training view nie klasyfikuje evidence/status/source jako default model columns.

Nie zamkniete / wymagany pozniejszy proof:
- long proof na realnych swiezych artefaktach R37/R38 po wygenerowaniu datasetow;
- walidacja, czy konkretne downstream modele chca uzywac statusow jako osobnych, jawnie zakodowanych cech;
- ewentualne formalne odblokowanie pelnego `scripts/test_selector_pipeline.py`, jezeli brakujacy modul testowy ma zostac przywrocony.

## 10. Decyzja

PR6 zostaje zaimplementowany jako offline selector evidence export layer.

Decyzja architektoniczna:
- wartosc liczbowa i evidence context sa rozdzielone,
- embedded SSOT moze uzupelnic wartosc tylko dla allowlistowanych metryk,
- brak wartosci pozostaje brakiem wartosci,
- evidence companion fields sa dostepne do audytu/selektora, ale nie sa automatycznie traktowane jako model feature columns.

## 11. DoD status

- [x] Brak `null -> 0`.
- [x] CPV low-sample ma status/source/sample metadata.
- [x] Temporal carried-forward ma status/source/staleness/carried anchor.
- [x] `not_allowed` temporal ratio nie emituje wartosci modelowej.
- [x] Policy config context jest eksportowany do dataset rows, gdy istnieje w decision logu.
- [x] Manifest rozdziela model/evidence columns.
- [x] Training view summary nie klasyfikuje evidence jako model columns.
- [x] Python syntax compile przechodzi.
- [x] Targeted contract smoke przechodzi.
- [ ] Pelny unittest `scripts/test_selector_pipeline.py` - zablokowany przez istniejacy brak modulu importowanego w tym pliku.
- [ ] Long runtime dataset proof - do uruchomienia po zebraniu swiezych danych.
