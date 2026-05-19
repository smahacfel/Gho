# Plan P3.7 Shadow-Burnin Artifact and Evidence Contract

Data: 2026-05-19

Status: **PHASE B CONTRACT / NO P2 / NO LIVE / NO ACTIVE POLICY CHANGE**

## 1. Cel

Celem tej fazy jest zdefiniowanie kontraktu artefaktow i klas dowodu dla
wlaczenia `shadow-burnin` jako osobnego execution-truth datasetu w P3.7.

Kontrakt ma zapobiec czterem bledom:

1. Mieszaniu market outcome z execution proof.
2. Traktowaniu speculative snapshot jako finalized proof.
3. Traktowaniu jednego truth-gap threshold jako wystarczajacego dla entry i exit.
4. Traktowaniu shadow simulation albo shadow-onchain validation jako live inclusion.

Ten dokument jest kontraktem dla kolejnych faz:

- Faza C: inventory,
- Faza E: shadow-onchain lifecycle report,
- Faza F: shadow lifecycle labeler,
- Faza G/H: integracja z P3.7 truth layer i feature availability.

Nie implementuje jeszcze skryptow.

## 2. Non-goals

Ten kontrakt nie autoryzuje:

- P2,
- live,
- zmian active V2/V2.5,
- zmian IWIM,
- zmian live sendera,
- threshold tuning,
- runtime feature extension,
- FSC active gate/ranking,
- traktowania submit jako confirmation,
- traktowania unknown execution status jako success,
- traktowania speculative finality jako finalized proof,
- uzycia lifecycle outcome jako decision-time feature,
- mieszania R10/R11/R13 primary-only market-path truth z shadow-burnin lifecycle truth bez segmentacji.

## 3. Zrodla kontraktu

Kontrakt opiera sie na:

- `PLANS/PLAN_P3_7_6A_SHADOW_BURNIN_LIFECYCLE_TRUTH_INTEGRATION_20260519.md`,
- `PLANS/AUDYT/RAPORT_P3_7_SHADOW_BURNIN_CODE_DISCOVERY_20260519.md`,
- `scripts/shadow_onchain_lifecycle_report.py`,
- `scripts/v3_p37_lifecycle_join_report.py`,
- `scripts/v3_p37_evidence_availability_report.py`,
- `scripts/v3_p37_temporal_split_report.py`,
- `AUDYT_PIPELINE_GATEKEEPER_V2.md`,
- `docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md`,
- `docs/ADR/ADR-0133-v3-p37-feature-redesign-lifecycle-labels.md`.

Stage A ustalil, ze:

- shadow-burnin code path istnieje,
- obecny VPS ma czesciowe historyczne artefakty,
- nie ma gotowego `shadow_onchain_lifecycle_report*.jsonl`,
- R10/R11/R13 primary-only nie sa globalnym dowodem przeciw Ghost, tylko dowodem
  braku lokalnego execution proof w tych namespace'ach.

## 4. Dataset segmentation

Kazdy row truth-layer musi miec `truth_dataset_kind`.

Dozwolone wartosci:

- `v3_primary_replay_market_path`,
- `shadow_burnin_lifecycle_onchain`,
- `live_lifecycle_onchain`.

Znaczenie:

| `truth_dataset_kind` | Co dowodzi | Czego nie dowodzi |
| --- | --- | --- |
| `v3_primary_replay_market_path` | Market/path outcome po decyzji, np. Chainstack price path | Execution lifecycle Ghosta |
| `shadow_burnin_lifecycle_onchain` | Shadow entry/lifecycle porownany z on-chain executable snapshot truth | Live inclusion |
| `live_lifecycle_onchain` | Live signature + confirmation/finality + lifecycle | Strategicznego edge bez split/PNL analizy |

Reguly:

1. R10/R11/R13 pozostaja `v3_primary_replay_market_path`.
2. Shadow-burnin lifecycle rows sa osobnym `shadow_burnin_lifecycle_onchain`.
3. Nie wolno kopiowac `buy_quality_good` z jednego dataset kind do drugiego.
4. Combined view moze istniec tylko jako widok secondary.
5. Kazdy combined report musi pokazywac counts per `truth_dataset_kind`.

## 5. Artifact availability vs code availability

Repo code availability i artifact availability sa osobnymi klasami.

### 5.1 `code_availability_class`

Dozwolone wartosci:

- `shadow_burnin_code_present`,
- `shadow_burnin_code_partial`,
- `shadow_burnin_code_missing`,
- `shadow_burnin_code_unknown`.

Minimalne warunki `shadow_burnin_code_present`:

- config schema ma `execution_mode = "shadow"` i `entry_mode = "shadow_only"`,
- trigger ma `trigger.shadow_run`,
- runtime zapisuje `shadow_entries.jsonl`,
- post-buy lifecycle zapisuje `shadow_lifecycle.jsonl`,
- istnieje raport albo skrypt korelacji on-chain.

Stage A klasyfikuje obecne repo jako:

- `code_availability_class = shadow_burnin_code_present`.

### 5.2 `artifact_availability_class`

Dozwolone wartosci:

- `artifact_complete_for_shadow_onchain_labeling`,
- `artifact_complete_for_shadow_runtime_only`,
- `artifact_partial_transport_entry_lifecycle`,
- `artifact_partial_transport_entry_only`,
- `artifact_partial_transport_only`,
- `artifact_primary_market_path_only`,
- `artifact_missing`,
- `artifact_unknown`.

Minimalne warunki `artifact_complete_for_shadow_onchain_labeling`:

- config snapshot,
- Gatekeeper decision log,
- Gatekeeper BUY log,
- trigger shadow transport log,
- shadow entry log,
- shadow lifecycle log,
- system log z `DIAG_ACCOUNT_UPDATE_RELAY`,
- events dir dla session scope,
- `shadow_onchain_lifecycle_report.jsonl`,
- dataset/run metadata: namespace, config path, git head, policy/config hash jesli dostepny.

`artifact_complete_for_shadow_runtime_only` moze nie miec jeszcze
`shadow_onchain_lifecycle_report.jsonl`, ale musi miec komplet artefaktow
potrzebnych do jego wygenerowania.

## 6. Minimalny kontrakt artefaktow per namespace

Kazdy namespace/run, ktory ma byc uzyty jako `shadow_burnin_lifecycle_onchain`,
musi miec ponizsze pola w inventory i reportach.

| Pole | Wymagane | Zrodlo | Uzycie |
| --- | --- | --- | --- |
| `namespace` | tak | path/run manifest | Dataset segmentation |
| `truth_dataset_kind` | tak | labeler/contract | Oddzielenie market/replay od lifecycle |
| `config_path` | tak | rollout/root config | Reprodukowalnosc |
| `config_snapshot_path` | zalecane | copied config | Reprodukowalnosc po zmianach configu |
| `git_head` | zalecane | runtime/report metadata | Code provenance |
| `policy_config_hash` | zalecane | DecisionLogger | Segmentacja config/policy |
| `entry_mode` | tak | config | Shadow/live boundary |
| `execution_mode` | tak | config | Shadow/live boundary |
| `shadow_run_enabled` | tak | config | Dispatch readiness |
| `emit_event_bus` | tak | config | Event evidence expectation |
| `funding_lane_mode` | tak | config | FSC scope |
| `decision_log_path` | tak | config/resolve | Gatekeeper decisions |
| `buy_log_path` | tak | config/resolve | BUY rows |
| `transport_log_path` | tak | `trigger.shadow_run.output_path` | Simulation/transport proof |
| `shadow_entry_log_path` | tak | `execution.shadow.entry_log_path` | Entry proof |
| `shadow_lifecycle_log_path` | tak | `execution.shadow.lifecycle_log_path` albo derived | Lifecycle proof |
| `events_dir` | tak | `execution.events.output_dir` | Session scope |
| `system_log_base` | tak | logging config | `DIAG_ACCOUNT_UPDATE_RELAY` source |
| `oracle_log_base` | zalecane | logging config | Operational diagnostics |
| `metrics_snapshot_path` | opcjonalne | runbook/report | Hot-path diagnostics |
| `shadow_onchain_lifecycle_report_path` | tak dla labelingu | Faza E | On-chain validation input |

Kontrakt fail-closed:

- brak configu oznacza `artifact_unknown` albo `artifact_missing`,
- brak transport/entry/lifecycle oznacza brak shadow executable label,
- brak system logu z `DIAG_ACCOUNT_UPDATE_RELAY` oznacza brak on-chain validation,
- brak `shadow_onchain_lifecycle_report.jsonl` oznacza, ze labeler nie moze
  nadac klas `shadow_onchain_*_verified`.

## 7. Wymagane row-level identifiers

Kazdy row po labelingu powinien niesc mozliwie pelny zestaw identyfikatorow.

| Pole | Wymagane | Uwagi |
| --- | --- | --- |
| `schema_version` | tak | Wersja labelera/kontraktu |
| `truth_dataset_kind` | tak | `shadow_burnin_lifecycle_onchain` dla tego lane |
| `namespace` | tak | Run namespace |
| `candidate_id` | tak | Glowny join dla transport/lifecycle |
| `position_id` | tak, jesli lifecycle ma position | Wymagane dla position lifecycle |
| `pool_id` | tak | Pubkey string |
| `base_mint` / `mint_id` | tak | Pubkey string |
| `join_key` | zalecane | `pool_id:base_mint:first_seen_ts_ms` |
| `idempotency_key` | zalecane | Dispatch dedup |
| `ab_record_id` | opcjonalne | Jesli laczymy z P3/P3.7 market label |
| `rollout_profile` | tak, jesli dostepny | Segmentacja |
| `decision_plane` | tak, jesli dostepny | `v25_shadow` vs `legacy_live` |
| `policy_config_hash` | tak, jesli dostepny | Segmentacja |
| `git_head` | zalecane | Provenance |

Brak identyfikatora nie musi automatycznie kasowac row, ale musi obnizyc
`label_quality` albo ustawic `unknown_reason`.

## 8. Minimalny row-level evidence payload

Labeler Fazy F powinien produkowac co najmniej:

```text
schema_version
truth_dataset_kind
namespace
candidate_id
position_id
pool_id
base_mint
decision_ts_ms
entry_execution_ts_ms
close_ts_ms
market_outcome_class
execution_verification_class
truth_gap_class
buy_quality_class
truth_status
truth_source
curve_finality_entry
curve_finality_exit
entry_truth_gap_ms
exit_truth_gap_ms
entry_drift_vs_onchain_executable_pct
exit_drift_vs_onchain_executable_pct
final_pnl_sol
final_pnl_pct
duration_ms
close_reason
total_exits
label_quality
unknown_reason
```

Zalecane pola dodatkowe:

```text
artifact_availability_class
code_availability_class
gatekeeper_buy_context_found
shadow_execution_outcome
dispatch_status
simulation_outcome
simulation_error_class
entry_price_source
exit_price_source
onchain_entry_match_direction
onchain_exit_match_direction
entry_onchain_match_slot
exit_onchain_match_slot
decision_to_execution_ms
detection_to_execution_ms
```

## 9. Klasy market outcome

Pole: `market_outcome_class`.

Dozwolone wartosci:

- `market_good_clean`,
- `market_good_dirty`,
- `market_bad_clean`,
- `market_bad_dirty`,
- `market_neutral`,
- `market_unknown`.

Mapowanie z obecnych P3.7 labeli:

| Obecne pole | Nowe pole |
| --- | --- |
| `good_clean` | `market_good_clean` |
| `good_dirty` | `market_good_dirty` |
| `bad_clean` | `market_bad_clean` |
| `bad_dirty` | `market_bad_dirty` |
| `neutral_clean`, `neutral`, brak silnego outcome | `market_neutral` |
| `unknown`, brak path/outcome | `market_unknown` |

Reguly:

1. Market class mowi tylko o post-decision market/path outcome.
2. Market class nie mowi, czy Ghost mogl wykonac BUY.
3. `market_good_clean` bez execution proof nie moze stac sie
   `buy_quality_good`.

## 10. Klasy execution verification

Pole: `execution_verification_class`.

Dozwolone wartosci:

- `shadow_onchain_finalized_verified`,
- `shadow_onchain_confirmed_verified`,
- `shadow_onchain_snapshot_verified`,
- `shadow_onchain_speculative_snapshot_verified`,
- `shadow_onchain_degraded`,
- `shadow_execution_infeasible`,
- `shadow_execution_unknown`,
- `live_confirmed_verified`.

### 10.1 Reguly finality

| Warunek | Klasa |
| --- | --- |
| live signature + confirmation/finality proof | `live_confirmed_verified` |
| shadow truth resolved, entry/exit finality finalized | `shadow_onchain_finalized_verified` |
| shadow truth resolved, entry/exit finality confirmed | `shadow_onchain_confirmed_verified` |
| shadow truth resolved, finality snapshot but not confirmed/finalized | `shadow_onchain_snapshot_verified` |
| shadow truth resolved, `curve_finality = speculative` | `shadow_onchain_speculative_snapshot_verified` |
| resolved but missing partial finality/gap/metadata quality | `shadow_onchain_degraded` |
| simulation/data problem/account not found/error class present | `shadow_execution_infeasible` |
| missing artifacts, unknown status, unresolved truth | `shadow_execution_unknown` |

### 10.2 Finality precedence

Jesli entry i exit maja rozne finality, klasa musi byc nie mocniejsza niz
najslabszy komponent.

Porzadek od najmocniejszego do najslabszego:

1. `finalized`,
2. `confirmed`,
3. snapshot non-speculative,
4. `speculative`,
5. missing/unknown.

Przyklad:

- entry finalized + exit speculative -> `shadow_onchain_speculative_snapshot_verified`,
- entry confirmed + exit missing finality -> `shadow_onchain_degraded`,
- entry speculative + exit speculative -> `shadow_onchain_speculative_snapshot_verified`.

### 10.3 Zakazy

- `curve_finality = speculative` nie moze dac
  `shadow_onchain_finalized_verified`.
- `truth_status != resolved` nie moze dac klasy verified.
- `AccountNotFound`, `data_problem`, semantic simulation failure, missing entry
  price albo missing exit proof nie moga byc executable success.
- `live_confirmed_verified` wymaga live signature i confirmation proof; shadow
  transport row nie wystarcza.

## 11. Klasy truth gap

Pole: `truth_gap_class`.

Dozwolone wartosci:

- `truth_gap_clean`,
- `truth_gap_degraded_acceptable`,
- `truth_gap_too_large`,
- `truth_gap_unknown`.

### 11.1 Wymagane progi

Labeler musi przyjmowac osobne progi:

```text
entry_truth_gap_clean_ms
exit_truth_gap_clean_ms
exit_truth_gap_acceptable_ms
exit_truth_gap_timestop_acceptable_ms
exit_truth_gap_by_close_reason
```

Minimalny CLI Fazy F:

```bash
python3 scripts/v3_p37_shadow_lifecycle_labeler.py \
  --shadow-onchain-lifecycle <jsonl> \
  --output <jsonl> \
  --entry-truth-gap-clean-ms <int> \
  --exit-truth-gap-clean-ms <int> \
  --exit-truth-gap-timestop-acceptable-ms <int>
```

Zalecane rozszerzenie:

```bash
  --exit-truth-gap-acceptable-ms <int> \
  --exit-truth-gap-by-close-reason TimeStop=30000,StopLoss=1000,TakeProfit=1000
```

### 11.2 Reguly klasyfikacji

`truth_gap_clean`:

- `abs(entry_truth_gap_ms) <= entry_truth_gap_clean_ms`,
- `abs(exit_truth_gap_ms) <= exit_truth_gap_clean_ms`,
- oba gaps znane.

`truth_gap_degraded_acceptable`:

- entry jest clean albo jawnie dopuszczony degraded przez kontrakt,
- exit przekracza clean threshold, ale miesci sie w acceptable threshold dla
  `close_reason`,
- typowy przypadek: `TimeStop` z exit gap okolo 30s.

`truth_gap_too_large`:

- entry przekracza entry clean/acceptable threshold,
- albo exit przekracza threshold dla swojego `close_reason`,
- albo close_reason wymaga ostrego gapu, a gap jest szeroki.

`truth_gap_unknown`:

- brak entry albo exit gap,
- brak `close_reason`, gdy potrzebny jest threshold per close reason,
- brak timestampow truth.

### 11.3 Close reason policy

Domyslna polityka progow:

| `close_reason` | Clean | Acceptable degraded | Uzasadnienie |
| --- | ---: | ---: | --- |
| `TimeStop` / `time_stop` | krotki clean threshold | do okolo 30000 ms | TimeStop moze miec wolniejsza finalna obserwacje |
| `StopLoss` / early loss | ostry threshold | bardzo krotki degraded albo brak | Wczesny exit wymaga bliskiej truth |
| `TakeProfit` / ladder exit | ostry threshold | krotki degraded | Exit quality zalezy od bliskiego price state |
| unknown close reason | brak clean | unknown/too_large | Nie zgadywac |

Konkretnych wartosci progow nie zamrazamy w tym kontrakcie. Musza byc jawne w
CLI/config labelera i raportowane w output metadata.

## 12. Klasy buy quality

Pole: `buy_quality_class`.

Dozwolone wartosci:

- `buy_quality_good`,
- `buy_quality_dirty_good`,
- `buy_quality_bad`,
- `buy_quality_neutral`,
- `buy_quality_unknown`,
- `buy_quality_not_executable`.

### 12.1 `buy_quality_good`

Wymaga jednoczesnie:

- `market_outcome_class = market_good_clean`,
- `execution_verification_class` w:
  - `shadow_onchain_finalized_verified`,
  - `shadow_onchain_confirmed_verified`,
  - opcjonalnie `live_confirmed_verified` dla live datasetu,
- `truth_gap_class = truth_gap_clean`,
- `final_pnl_pct` znane i dodatnie albo spelniajace jawny market outcome contract,
- MAE/exit constraints acceptable, jesli dostepne,
- brak unknown execution status,
- brak simulation/data problem.

### 12.2 `buy_quality_dirty_good`

Dopuszczalne gdy:

- market outcome jest dobry (`market_good_clean` albo `market_good_dirty`),
- execution jest verified, ale proof ma nizsza jakosc:
  - speculative snapshot,
  - degraded but acceptable truth gap,
  - dirty market label,
  - drobny brak drugorzednego pola nie narusza executable proof,
- `truth_gap_class` jest `truth_gap_clean` albo
  `truth_gap_degraded_acceptable`.

Przyklad:

- positive PnL,
- resolved truth,
- `curve_finality = speculative`,
- TimeStop exit gap okolo 30s i w osobnym acceptable threshold:
  `buy_quality_dirty_good`.

### 12.3 `buy_quality_bad`

Wymaga:

- execution proof jest resolved/verified albo co najmniej nie unknown,
- market outcome jest `market_bad_clean` albo `market_bad_dirty`,
- negative PnL albo zly market outcome.

Negative PnL row nie moze byc `buy_quality_good`.

### 12.4 `buy_quality_neutral`

Uzywane gdy:

- market outcome jest neutralny,
- execution proof jest znany,
- row nie wspiera ani good ani bad targetu.

### 12.5 `buy_quality_unknown`

Uzywane gdy:

- brakuje truth,
- brakuje finality,
- brakuje key identifiers,
- execution status jest unknown,
- artifact availability jest niewystarczajace do oceny.

### 12.6 `buy_quality_not_executable`

Uzywane gdy:

- dispatch byl expected, ale brak shadow artifacts,
- simulation/data problem,
- AccountNotFound,
- missing entry price,
- missing exit proof,
- unresolved truth,
- execution_verification_class = `shadow_execution_infeasible`.

## 13. Label quality i unknown reasons

Pole: `label_quality`.

Dozwolone wartosci:

- `label_quality_clean`,
- `label_quality_degraded`,
- `label_quality_unknown`,
- `label_quality_invalid`.

Pole: `unknown_reason`.

Dozwolone przyklady:

- `missing_shadow_onchain_report`,
- `missing_shadow_entry`,
- `missing_shadow_lifecycle`,
- `missing_transport_record`,
- `missing_gatekeeper_buy_context`,
- `truth_status_not_resolved`,
- `missing_entry_truth`,
- `missing_exit_truth`,
- `entry_truth_gap_too_large`,
- `exit_truth_gap_too_large`,
- `curve_finality_unknown`,
- `speculative_finality_only`,
- `simulation_error`,
- `account_not_found`,
- `data_problem`,
- `missing_identifier`,
- `unsupported_schema_version`.

Reguly:

- `unknown_reason` jest wymagany dla `buy_quality_unknown`,
  `buy_quality_not_executable` i `label_quality_invalid`.
- Jezeli jest kilka powodow, output moze uzyc listy `unknown_reasons` albo
  glownego `unknown_reason` plus `diagnostics`.

## 14. Mapowanie ze starych P3.7 klas

Obecne skrypty P3.7 uzywaja:

- `execution_quality_class`,
- `decision_quality_class`,
- `good_executable`,
- `good_not_executable`,
- `execution_feasible_clean`,
- `execution_feasible_degraded`.

Nowy kontrakt nie usuwa ich natychmiast, ale degraduje do compatibility layer.

| Stare pole / wartosc | Nowa interpretacja |
| --- | --- |
| `execution_quality_class = execution_feasible_clean` | Nie wystarcza; musi byc rozbite na `execution_verification_class` + `truth_gap_class` |
| `execution_quality_class = execution_feasible_degraded` | Kandydat na degraded proof, wymaga finality/gap reason |
| `execution_quality_class = execution_infeasible` | `execution_verification_class = shadow_execution_infeasible` |
| `execution_quality_class = execution_unknown` | `execution_verification_class = shadow_execution_unknown` |
| `decision_quality_class = good_executable` | Deprecated compatibility; zastapic `buy_quality_good` albo `buy_quality_dirty_good` |
| `decision_quality_class = good_not_executable` | `buy_quality_not_executable` albo `buy_quality_unknown` zalezne od powodu |

Regula przejsciowa:

- raporty moga nadal pokazywac `good_executable` jako secondary compatibility,
  ale nie moze to byc jedyna prawda ani gate Phase B.

## 15. Reguly dla `shadow_onchain_lifecycle_report.py`

Obecny output ma pola:

- `analysis_status`,
- `candidate_id`,
- `position_id`,
- `mint_id`,
- `pool_id`,
- `close_reason`,
- `truth_status`,
- `truth_source`,
- `sample_price_state`,
- `timing.*`,
- `shadow.*`,
- `onchain.entry.curve_finality`,
- `onchain.entry.match_delta_ms`,
- `onchain.exit.max_abs_truth_gap_ms`,
- `exit_fills[*].onchain_curve_finality`,
- `drift_pct.*`.

Kontrakt Fazy F musi:

1. Przepisac `onchain.entry.match_delta_ms` do `entry_truth_gap_ms`.
2. Przepisac `onchain.exit.max_abs_truth_gap_ms` do `exit_truth_gap_ms`.
3. Wyprowadzic `curve_finality_entry` z `onchain.entry.curve_finality`.
4. Wyprowadzic `curve_finality_exit` z najgorszej finality w `exit_fills`.
5. Nie dropowac rows wylacznie przez jeden globalny `--max-truth-gap-ms`, jesli
   celem jest klasyfikacja degraded/too-large. Hard filter moze istniec jako
   opcja diagnostyczna, ale labeler powinien preferowac klasy.
6. Traktowac `analysis_status != ok` jako nie-success.
7. Traktowac `truth_status != resolved` jako nie-verified.
8. Traktowac `shadow.execution_outcome` inne niz zdrowe shadow outcome jako
   potencjalny `shadow_execution_infeasible`.

## 16. Reguly finality

`curve_finality` jest jakoscia dowodu, nie ozdoba raportu.

| `curve_finality` | Klasa dowodu |
| --- | --- |
| `finalized` | finalized proof |
| `confirmed` | confirmed proof |
| `processed` / snapshot-like | snapshot proof |
| `speculative` | speculative snapshot proof |
| missing/unknown | degraded albo unknown |

Zakazy:

- `speculative` nie moze byc nazwane `finalized_onchain_verified`.
- snapshot proof nie moze byc opisany jako live inclusion.
- brak finality nie moze przejsc jako clean verified bez explicit degraded label.

## 17. Reguly truth-gap

Entry i exit gap maja rozne znaczenie:

- entry truth gap mierzy jak blisko executable account state bylo do shadow BUY,
- exit truth gap mierzy jak blisko exit lifecycle bylo do on-chain executable
  state przy zamknieciu lub fillu.

Wymagane zasady:

1. Entry i exit gap musza miec osobne progi.
2. Exit gap musi miec opcjonalne progi per `close_reason`.
3. TimeStop moze dopuszczac wiekszy degraded acceptable gap.
4. StopLoss/early exit musi miec ostrzejszy threshold.
5. Gap zbyt duzy nie znaczy automatycznie execution infeasible; znaczy, ze proof
   nie jest clean i moze byc degraded albo too_large.
6. `truth_gap_too_large` nie moze dawac `buy_quality_good`.

## 18. Edge strategii

Shadow-burnin lifecycle proof dowodzi executable/lifecycle porownania z on-chain
state. Nie dowodzi strategii edge.

Do edge nadal potrzeba:

- wystarczajacej probki,
- temporal split,
- holdout,
- outcome classes,
- PnL/MAE/MFE distribution,
- execution quality distribution,
- oddzielenia decision policy od execution feasibility,
- braku leakage z lifecycle outcome do decision-time features.

Raporty musza uzywac sformulowan typu:

- `execution proof available`,
- `buy-quality label available`,
- `Phase B candidate may be evaluated`.

Nie wolno uzywac:

- `strategy edge proven`,
- `live-ready`,
- `P2-ready`,
- `confirmed executable` bez confirmation/finality dowodu.

## 19. Dataset mixing rules

Zakazy:

1. Nie przypisywac `buy_quality_good` z shadow-burnin do R10/R11/R13 rows.
2. Nie laczyc rows roznych `config_hash` bez segmentacji.
3. Nie laczyc rows roznych `rollout_profile` bez segmentacji.
4. Nie laczyc `legacy_live` i `v25_shadow` jako jednego decision plane.
5. Nie trenowac feature prototype na lifecycle outcome jako feature.
6. Nie uzywac combined-only result jako gate pass.

Dopuszczalne:

- raport secondary combined z jawna kolumna `truth_dataset_kind`,
- diagnostyka coverage miedzy market path a lifecycle truth,
- planowanie nowego collection runu z V3 payload + lifecycle enabled.

## 20. Acceptance dla przyszlego labelera

Labeler Fazy F przechodzi acceptance, jesli:

- negative PnL row nie jest `buy_quality_good`,
- `curve_finality = speculative` nie jest finalized proof,
- exit truth gap powyzej clean threshold nie jest clean,
- TimeStop moze byc `truth_gap_degraded_acceptable`, jesli miesci sie w
  osobnym progu,
- unknown execution status nie jest success,
- `AccountNotFound` / `data_problem` nie jest executable,
- brak `shadow_onchain_lifecycle_report.jsonl` daje unknown/missing, nie success,
- output ma oddzielne:
  - `market_outcome_class`,
  - `execution_verification_class`,
  - `truth_gap_class`,
  - `buy_quality_class`.

## 21. Acceptance dla raportow P3.7 po integracji

Raporty P3.7 przechodza acceptance, jesli:

- pokazuja counts per `truth_dataset_kind`,
- nie uzywaja `good_executable` jako jedynej prawdy,
- maja compatibility counts tylko jako secondary,
- rozrozniaja market path truth od shadow lifecycle truth,
- pokazuja finality distribution,
- pokazuja entry truth gap distribution i exit truth gap distribution osobno,
- pokazuja degraded/unknown reasons,
- nie wnioskuja edge z samego lifecycle proof.

## 22. Przykladowe klasyfikacje

### 22.1 Positive PnL, speculative finality, TimeStop degraded acceptable

Warunki:

- positive PnL,
- `truth_status = resolved`,
- `curve_finality_entry = speculative`,
- `curve_finality_exit = speculative`,
- entry gap clean,
- exit gap okolo 30s,
- `close_reason = TimeStop`,
- exit gap miesci sie w `exit_truth_gap_timestop_acceptable_ms`.

Klasy:

```text
market_outcome_class = market_good_clean
execution_verification_class = shadow_onchain_speculative_snapshot_verified
truth_gap_class = truth_gap_degraded_acceptable
buy_quality_class = buy_quality_dirty_good
```

### 22.2 Positive PnL, confirmed/finalized, clean gaps

Klasy:

```text
market_outcome_class = market_good_clean
execution_verification_class = shadow_onchain_confirmed_verified
truth_gap_class = truth_gap_clean
buy_quality_class = buy_quality_good
```

albo:

```text
execution_verification_class = shadow_onchain_finalized_verified
```

jesli entry i exit maja finalized finality.

### 22.3 Negative PnL, resolved truth

Klasy:

```text
market_outcome_class = market_bad_clean
execution_verification_class = shadow_onchain_confirmed_verified
truth_gap_class = truth_gap_clean
buy_quality_class = buy_quality_bad
```

### 22.4 AccountNotFound / data_problem

Klasy:

```text
market_outcome_class = market_good_clean albo market_unknown
execution_verification_class = shadow_execution_infeasible
truth_gap_class = truth_gap_unknown
buy_quality_class = buy_quality_not_executable
unknown_reason = account_not_found albo data_problem
```

### 22.5 Missing artifacts

Klasy:

```text
execution_verification_class = shadow_execution_unknown
truth_gap_class = truth_gap_unknown
buy_quality_class = buy_quality_unknown
unknown_reason = missing_shadow_lifecycle albo missing_shadow_onchain_report
```

## 23. Required output metadata

Kazdy output JSON/Markdown powinien miec metadata block:

```json
{
  "schema_version": 1,
  "contract": "p3_7_shadow_burnin_artifact_and_evidence_contract",
  "contract_date": "2026-05-19",
  "repo_head": "<git_head_or_unknown>",
  "truth_dataset_kind": "shadow_burnin_lifecycle_onchain",
  "namespace": "<namespace>",
  "config_path": "<path>",
  "entry_truth_gap_clean_ms": "<int>",
  "exit_truth_gap_clean_ms": "<int>",
  "exit_truth_gap_timestop_acceptable_ms": "<int>",
  "finality_policy": "weakest_entry_exit_component",
  "live_inclusion_claimed": false
}
```

Markdown reports powinny jawnie drukowac:

- used thresholds,
- finality counts,
- truth gap counts,
- artifact availability class,
- dataset kind counts,
- blockers.

## 24. Invariants

1. Shadow simulation is not live inclusion.
2. Shadow-onchain validation is not live inclusion.
3. Submit is not confirmation.
4. Unknown execution status is not success.
5. Speculative snapshot is not finalized proof.
6. Market-good is not executable-good.
7. Execution proof is not strategy edge.
8. Lifecycle outcome is not a decision-time feature.
9. Combined-only evidence is secondary.
10. R10/R11/R13 primary-only conclusions stay scoped to their namespaces.

## 25. Decyzja po Fazie B

Faza B jest kontraktowo gotowa, gdy ten dokument zostanie zaakceptowany jako
SSOT dla:

- inventory schema w Fazie C,
- smoke profile artifact expectations w Fazie D,
- `shadow_onchain_lifecycle_report.py` usage w Fazie E,
- `v3_p37_shadow_lifecycle_labeler.py` output schema w Fazie F,
- P3.7 truth-layer integration w Fazie G.

Nastepny etap wykonawczy:

- Faza C: `scripts/v3_p37_shadow_burnin_inventory.py` oraz raport inventory.

Phase B feature prototype pozostaje zablokowane do czasu:

- dostepnych `buy_quality_good` i `buy_quality_bad` rows,
- wystarczajacego decision-time feature coverage,
- mozliwego temporal/session split.
