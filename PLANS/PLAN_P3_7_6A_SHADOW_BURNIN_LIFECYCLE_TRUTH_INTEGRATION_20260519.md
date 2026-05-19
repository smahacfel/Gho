# PLAN P3.7.6A - Shadow-Burnin Lifecycle Truth Integration

Data: 2026-05-19

Status: **EXECUTION PLAN / NO P2 / NO LIVE / NO ACTIVE POLICY CHANGE**

## 1. Cel

Celem P3.7.6A jest przywrocenie `shadow-burnin` jako osobnej, pierwszoklasowej
linii prawdy egzekucyjnej dla P3.7.

P3.7 dotychczas operowalo glownie na R10/R11/R13 jako primary-only V3 replay
datasets. Te runy dostarczyly market/path evidence, ale nie dostarczyly lokalnego
shadow entry/lifecycle proof dla tych samych rows. Dlatego raport:

- `PLANS/AUDYT/RAPORT_P3_7_EXECUTION_FEASIBILITY_ARTIFACT_AUDIT_R10_R11_R13_20260518.md`

poprawnie wskazal `good_executable=0` dla R10/R11/R13. To jest wniosek o tych
trzech namespace'ach, nie globalny wniosek o calym Ghost.

Ten plan ma doprowadzic do stanu, w ktorym P3.7 rozroznia i raportuje:

1. `market_outcome_class` - czy pool/token mial dobry path/outcome.
2. `execution_verification_class` - czy shadow execution/lifecycle zgadza sie z
   on-chain executable state i jaka jest jakosc finality.
3. `truth_gap_class` - czy entry/exit truth byly blisko czasowo, zdegradowane,
   czy zbyt odlegle.
4. `buy_quality_class` - czy BUY byl jednoczesnie market-good, wykonawczo
   zweryfikowany i jakosciowo akceptowalny.

## 2. Twarde granice

Ten plan nie autoryzuje:

- P2,
- live,
- active V2/V2.5 behavior changes,
- IWIM changes,
- live sender changes,
- threshold tuning,
- runtime feature extension,
- FSC active gate/ranking,
- uznania shadow simulation za live inclusion,
- traktowania submit jako confirmation,
- traktowania unknown execution status jako success.

FSC pozostaje poza active gate/ranking zgodnie z `docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md`.
Nowy smoke profil dla P3.7.6A ma miec `seer.funding_lane_mode = "disabled"`,
chyba ze osobny ADR jawnie zmieni zakres FSC.

## 3. Obecny stan repo do ktorego plan jest dopasowany

### 3.1 Root config

`config.toml` zawiera aktywna shadow-only sciezke runtime:

- `[trigger].entry_mode = "shadow_only"`,
- `[execution].execution_mode = "shadow"`,
- `[trigger.shadow_run].enabled = true`,
- `[trigger.shadow_run].emit_event_bus = true`,
- `[execution.shadow].entry_log_path = "logs/shadow_run/shadow_entries.jsonl"`,
- `lifecycle_log_path` jest opcjonalny i w shadow mode moze byc wyprowadzony z
  `entry_log_path`.

### 3.2 Rollout shadow-burnin

`configs/rollout/shadow-burnin.toml` opisuje shadow-burnin jako standalone
shadow runtime:

- `entry_mode = "shadow_only"`,
- `execution_mode = "shadow"`,
- `trigger.shadow_run.enabled = true`,
- `trigger.shadow_run.emit_event_bus = true`,
- `execution.shadow.entry_log_path`,
- `execution.shadow.lifecycle_log_path`,
- `execution.shadow.timing_model = "prepared_entry_mirror"`,
- `execution.shadow.stale_policy = "emit_warning"`.

Uwaga operacyjna: obecny kanoniczny profil jest historycznie zwiazany z rodzina
V3 P1 i ma `funding_lane_mode = "full_chain"`. P3.7.6A nie powinno go
bezposrednio uzywac jako nowego V3 smoke profilu, bo aktualna sciezka V3 jest
primary-only/FSC de-scoped pod single-stream provider constraint.

### 3.3 Istniejace narzedzia

Repo juz posiada:

- `scripts/shadow_onchain_lifecycle_report.py`,
- `scripts/shadow_onchain_lifecycle_report2.py`,
- `scripts/shadow_run_report.py`,
- `scripts/v3_p37_lifecycle_join_report.py`,
- `scripts/v3_p37_evidence_availability_report.py`,
- `scripts/v3_p37_temporal_split_report.py`.

`scripts/shadow_onchain_lifecycle_report.py` laduje config, decision/buy log,
transport shadow log, `shadow_entries.jsonl`, `shadow_lifecycle.jsonl`, events
dir i system log, a nastepnie koreluje lifecycle z `DIAG_ACCOUNT_UPDATE_RELAY`
on-chain snapshot truth. Obecne CLI wspiera:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config <config> \
  --all-sessions \
  --output <jsonl> \
  --max-truth-gap-ms <optional_hard_filter>
```

Obecny raport on-chain ma juz pola konieczne do kolejnego labelera, m.in.:

- `analysis_status`,
- `candidate_id`,
- `position_id`,
- `mint_id`,
- `pool_id`,
- `close_reason`,
- `truth_status`,
- `truth_source`,
- `timing.*`,
- `shadow.*`,
- `onchain.entry.curve_finality`,
- `onchain.entry.match_delta_ms`,
- `onchain.exit.max_abs_truth_gap_ms`,
- `drift_pct.entry_vs_onchain_executable`,
- `drift_pct.exit_vs_onchain_executable`,
- `exit_fills`.

## 4. Semantyka dowodu

### 4.1 Shadow-burnin nie jest live inclusion

Shadow-burnin dowodzi, ze runtime potrafi przejsc przez mozliwie wierna sciezke
execution bez realnego live exposure:

```text
Gatekeeper BUY
-> trigger.shadow_run
-> shadow transport / buy simulation
-> shadow entry
-> shadow lifecycle
-> on-chain snapshot/finality correlation
-> lifecycle label
```

Nie wolno z tego robic claimu:

```text
live included / finalized live fill / live PnL
```

bez live signature, confirmation/finality proof i osobnej live sciezki dowodowej.

### 4.2 Finality

`curve_finality = "speculative"` oznacza snapshot proof, nie finalized proof.

Mapowanie:

| `curve_finality` | `execution_verification_class` |
| --- | --- |
| `finalized` | `shadow_onchain_finalized_verified` |
| `confirmed` | `shadow_onchain_confirmed_verified` |
| `processed` albo brak silniejszego proofu | `shadow_onchain_snapshot_verified` |
| `speculative` | `shadow_onchain_speculative_snapshot_verified` |
| brak finality info | `shadow_onchain_degraded` albo `shadow_execution_unknown` |

Zakazane:

```text
speculative -> finalized_onchain_verified
```

### 4.3 Truth gap

Entry i exit truth gap musza miec osobne progi.

Minimalne parametry kontraktu:

- `entry_truth_gap_clean_ms`,
- `entry_truth_gap_acceptable_ms`,
- `exit_truth_gap_clean_ms`,
- `exit_truth_gap_acceptable_ms`,
- `exit_truth_gap_max_ms_by_close_reason`.

Przyklad semantyki:

- entry gap kilkaset ms moze byc clean,
- exit gap okolo 30s dla `TimeStop` moze byc `truth_gap_degraded_acceptable`,
- exit gap okolo 30s dla szybkiego stop-loss albo early exit nie jest clean i
  domyslnie powinien byc rejected/degraded poza osobna decyzja.

Nie wolno uzywac jednego globalnego `--max-truth-gap-ms` jako finalnej semantyki
klas. Obecny hard-filter moze byc narzedziem diagnostycznym, ale docelowy labeler
ma klasyfikowac gap per entry/exit/close reason.

### 4.4 Edge strategii

Shadow lifecycle proof nie dowodzi edge strategii.

Dowodzi tylko:

- shadow execution/lifecycle lane dziala,
- shadow lifecycle mozna porownac z on-chain executable state,
- Ghost potrafi generowac execution-verifiable positions w shadow plane.

Edge wymaga nadal:

- wiekszej probki,
- temporal/session split,
- outcome classes,
- PnL/MAE/MFE,
- execution-quality distribution,
- oddzielenia decision policy od execution feasibility.

## 5. Evidence classes

### 5.1 `market_outcome_class`

Dozwolone wartosci:

- `market_good_clean`,
- `market_good_dirty`,
- `market_bad_clean`,
- `market_bad_dirty`,
- `market_neutral`,
- `market_unknown`.

### 5.2 `execution_verification_class`

Dozwolone wartosci:

- `shadow_onchain_finalized_verified`,
- `shadow_onchain_confirmed_verified`,
- `shadow_onchain_snapshot_verified`,
- `shadow_onchain_speculative_snapshot_verified`,
- `shadow_onchain_degraded`,
- `shadow_execution_infeasible`,
- `shadow_execution_unknown`,
- `live_confirmed_verified`.

Reguly:

- `truth_status != resolved` -> nie moze byc verified success,
- `AccountNotFound`, simulation failure, data problem -> `shadow_execution_infeasible`,
- `curve_finality=speculative` -> `shadow_onchain_speculative_snapshot_verified`,
- `live_confirmed_verified` tylko przy live signature + confirmation proof.

### 5.3 `truth_gap_class`

Dozwolone wartosci:

- `truth_gap_clean`,
- `truth_gap_degraded_acceptable`,
- `truth_gap_too_large`,
- `truth_gap_unknown`.

Reguly:

- entry i exit sa oceniane osobno,
- final row dostaje najgorsza klase z entry/exit, chyba ze `close_reason`
  jawnie dopuszcza degraded exit jako acceptable,
- `truth_gap_unknown` nie moze byc success.

### 5.4 `buy_quality_class`

Dozwolone wartosci:

- `buy_quality_good`,
- `buy_quality_dirty_good`,
- `buy_quality_bad`,
- `buy_quality_neutral`,
- `buy_quality_unknown`,
- `buy_quality_not_executable`.

`buy_quality_good` wymaga jednoczesnie:

- `market_outcome_class = market_good_clean`,
- `execution_verification_class` w clean verified set:
  - `shadow_onchain_finalized_verified`,
  - `shadow_onchain_confirmed_verified`,
  - opcjonalnie `shadow_onchain_snapshot_verified`, jezeli plan etapu jawnie to
    dopuszcza jako non-finalized clean shadow proof,
- `truth_gap_class = truth_gap_clean`,
- MAE/exit constraints acceptable,
- brak unknown execution status,
- brak `AccountNotFound`, simulation failure albo data problem.

`buy_quality_dirty_good` moze obejmowac market-good rows z:

- speculative snapshot proof,
- degraded acceptable TimeStop gap,
- non-finalized but resolved on-chain snapshot,
- innym jawnie opisanym degraded proof, ktory nie jest execution failure.

`buy_quality_bad` obejmuje negative PnL / bad path rows z resolved execution truth.

`buy_quality_not_executable` obejmuje rows z infeasible execution, np.
`AccountNotFound`, failed simulation, transport error, brak shadow entry przy
dispatch-required row.

## 6. Faza A - Shadow-burnin code discovery

### Zakres

Read-only discovery calej sciezki shadow-burnin:

- config,
- runtime,
- trigger shadow transport,
- shadow entry,
- post-buy lifecycle,
- DecisionLogger,
- events,
- reports,
- runbooki,
- ADR.

### Pliki startowe

- `config.toml`,
- `configs/rollout/shadow-burnin.toml`,
- `scripts/shadow_onchain_lifecycle_report.py`,
- `scripts/shadow_onchain_lifecycle_report2.py`,
- `scripts/shadow_run_report.py`,
- `scripts/v3_p37_lifecycle_join_report.py`,
- `ghost-launcher/src/main.rs`,
- `ghost-launcher/src/oracle_runtime.rs`,
- `ghost-launcher/src/components/trigger/component.rs`,
- `ghost-launcher/src/components/trigger/shadow_run.rs`,
- `ghost-launcher/src/components/post_buy_runtime.rs`,
- `ghost-brain/src/oracle/decision_logger.rs`,
- `docs/RUNBOOK_PRODUCTION_ROLLOUT.md`,
- `AUDYT_PIPELINE_GATEKEEPER_V2.md`,
- `docs/ADR/`,
- `PLANS/PLAN_P3_7_FEATURE_REDESIGN_AND_LIFECYCLE_LABELS_20260518.md`,
- `PLANS/AUDYT/RAPORT_P3_7_EXECUTION_FEASIBILITY_ARTIFACT_AUDIT_R10_R11_R13_20260518.md`.

### Szukane frazy

```text
execution_mode = "shadow"
entry_mode = "shadow_only"
trigger.shadow_run
execution.shadow
shadow_entries
shadow_lifecycle
shadow_run
shadow_simulated
exit_filled
position_closed
shadow_execution_outcome
emit_event_bus
prepared_entry_mirror
derive_shadow_lifecycle_log_path
shadow_onchain_lifecycle_report
DIAG_ACCOUNT_UPDATE_RELAY
```

### Deliverable

`PLANS/AUDYT/RAPORT_P3_7_SHADOW_BURNIN_CODE_DISCOVERY_20260519.md`

Raport musi zawierac:

- konkretne pliki i funkcje,
- runtime flow,
- mapa `config -> Gatekeeper BUY -> trigger.shadow_run -> shadow entry -> lifecycle -> on-chain truth report`,
- lista emitowanych artefaktow,
- join keys:
  - `candidate_id`,
  - `position_id`,
  - `pool_id`,
  - `mint_id` / `base_mint`,
  - `ab_record_id`, jezeli wystepuje,
- opis `scripts/shadow_onchain_lifecycle_report.py`,
- wskazanie brakujacych historycznych artefaktow po migracji VPS,
- rozdzielenie:
  - shadow simulation,
  - shadow-onchain validated,
  - live inclusion.

### Acceptance

- Raport ma konkretne file/function references.
- Nie ma twierdzen o active/live bez dowodu z configu i kodu.
- Wskazano config path dla P3.7.6A smoke runu.
- Nie zmieniono active policy.

## 7. Faza B - Artifact and evidence contract

### Zakres

Zdefiniowac minimalny kontrakt artefaktow wymaganych do uznania
shadow-burnin lifecycle truth za usable w P3.7.

### Minimalne artefakty

Per namespace/run wymagane sa:

- rollout config albo root config snapshot,
- policy/config hash, jezeli dostepny,
- git head, jezeli dostepny,
- Gatekeeper decision log:
  - `gatekeeper_v2_decisions.jsonl`,
  - `gatekeeper_v2_buys.jsonl`,
- trigger shadow transport log:
  - `buys.jsonl`,
  - albo `shadow-burnin-*-buys.jsonl`,
- `execution.shadow.entry_log_path`:
  - `shadow_entries.jsonl`,
- `execution.shadow.lifecycle_log_path`:
  - `shadow_lifecycle.jsonl`,
- system/oracle log z `DIAG_ACCOUNT_UPDATE_RELAY`,
- events dir dla session scope,
- `shadow_onchain_lifecycle_report.jsonl`,
- opcjonalnie manifest/inventory JSON.

### Deliverable

`PLANS/PLAN_P3_7_SHADOW_BURNIN_ARTIFACT_AND_EVIDENCE_CONTRACT_20260519.md`

### Acceptance

- Nie istnieje jedno mieszane pole `good_executable` jako jedyna prawda.
- Speculative snapshot nie jest finalized proof.
- Entry i exit truth gaps maja osobne progi.
- Shadow-onchain validation nie jest live inclusion.
- Edge strategii nie jest wnioskowany z samego lifecycle proof.
- Kontrakt rozroznia code availability od artifact availability.

## 8. Faza C - Inventory obecnych i historycznych artefaktow

### Zakres

Zbudowac read-only inventory dla artefaktow shadow-burnin:

- na obecnym VPS,
- w opcjonalnych folderach dostarczonych z PC,
- w istniejacych logs/datasets/PLANS.

### Nowy skrypt

`scripts/v3_p37_shadow_burnin_inventory.py`

### CLI

```bash
python3 scripts/v3_p37_shadow_burnin_inventory.py \
  --repo-root /root/Gho \
  --extra-artifact-root /path/to/local/copied/artifacts \
  --output-json logs/rollout/shadow-burnin-p37-lifecycle-smoke/reports/p3_7_shadow_burnin_inventory.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_SHADOW_BURNIN_INVENTORY_20260519.md
```

`--extra-artifact-root` ma byc opcjonalne i powtarzalne.

### Skanowane korzenie

- `logs/shadow_run/**`,
- `logs/rollout/**`,
- `datasets/events/**`,
- `data/rollout/**`,
- `PLANS/AUDYT/**`,
- kazdy `--extra-artifact-root`.

### Szukane artefakty

- `shadow_entries.jsonl`,
- `shadow_lifecycle.jsonl`,
- `buys.jsonl`,
- `shadow-burnin-*-buys.jsonl`,
- `shadow_onchain_lifecycle_report*.jsonl`,
- `system.log*`,
- `oracle.log*`,
- `gatekeeper_v2_decisions.jsonl`,
- `gatekeeper_v2_buys.jsonl`,
- `p3_7_*shadow*lifecycle*.jsonl`.

### Output per run

W JSON i Markdown per detected namespace:

- `namespace`,
- `artifact_root`,
- `config_path`,
- `entry_mode`,
- `execution_mode`,
- `shadow_run_enabled`,
- `emit_event_bus`,
- `funding_lane_mode`,
- `entry_log_exists`,
- `entry_rows`,
- `lifecycle_log_exists`,
- `lifecycle_rows`,
- `position_closed_count`,
- `exit_filled_count`,
- `transport_log_exists`,
- `transport_rows`,
- `decision_log_exists`,
- `decision_rows`,
- `buy_log_exists`,
- `buy_rows`,
- `system_log_exists`,
- `oracle_log_exists`,
- `diag_account_update_relay_count`,
- `events_dir_exists`,
- `event_file_count`,
- `session_scope_detected`,
- `truth_report_exists`,
- `truth_report_rows`,
- `artifact_availability_class`,
- `notes`.

### Acceptance

- Jezeli stare artefakty nie istnieja na VPS, raport mowi to wprost.
- Jezeli operator dostarczy folder z PC, inventory obejmuje go bez kopiowania do
  aktywnego namespace.
- Inventory niczego nie modyfikuje.
- Raport rozroznia:
  - repo code availability,
  - current VPS artifact availability,
  - external/restored artifact availability.

## 9. Faza D - Controlled shadow-burnin smoke run

### Zakres

Utworzyc nowy, izolowany profil smoke runu, ktory sprawdza czy shadow-burnin
lifecycle lane dziala obecnie. Ten smoke run nie jest P2, nie jest live i nie
jest threshold tuningiem.

### Nowy profil

`configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml`

### Kontrakt profilu

Profil ma zachowac shadow-only runtime:

```toml
mode = "production"
ghost_brain_config_path = "../../ghost-brain/ghost_brain_config.toml"

[seer]
enabled = true
source_mode = "grpc"
stream_mode = "single_global"
tx_filter_strategy = "per_pool"
funding_lane_mode = "disabled"

[trigger]
enabled = true
entry_mode = "shadow_only"

[trigger.shadow_run]
enabled = true
emit_event_bus = true
output_path = "../../logs/shadow_run/shadow-burnin-p37-lifecycle-smoke/buys.jsonl"

[execution]
execution_mode = "shadow"

[execution.shadow]
entry_log_path = "../../logs/shadow_run/shadow-burnin-p37-lifecycle-smoke/shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/shadow-burnin-p37-lifecycle-smoke/shadow_lifecycle.jsonl"
timing_model = "prepared_entry_mirror"
stale_policy = "emit_warning"

[execution.events]
output_dir = "../../datasets/events/shadow-burnin-p37-lifecycle-smoke"

[oracle]
decision_log_path = "../../logs/rollout/shadow-burnin-p37-lifecycle-smoke/decisions"

[durability]
wal_enabled = false
wal_dir = "../../data/rollout/shadow-burnin-p37-lifecycle-smoke/wal"
snapshot_dir = "../../data/rollout/shadow-burnin-p37-lifecycle-smoke/snapshots"

[logging]
file_enabled = true
file_path = "../../logs/rollout/shadow-burnin-p37-lifecycle-smoke/system.log"
oracle_log_enabled = true
oracle_log_path = "../../logs/rollout/shadow-burnin-p37-lifecycle-smoke/oracle.log"
```

Dokladne RPC/env/secrets maja byc pobierane przez istniejacy mechanizm `.env` /
zmienne srodowiskowe. Nie wolno wpisywac nowych sekretow do profilu.

### Runtime commands

Przed runem:

```bash
cargo test --workspace --no-run
```

Preflight:

```bash
bash scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml
```

Kontrolowany smoke:

```bash
timeout 30m env RUST_LOG=info \
  cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml
```

### Acceptance

- Proces konczy sie kontrolowanie.
- Powstaje decision log albo raport jasno wskazuje, czemu nie powstal.
- Jesli pojawi sie BUY/shadow dispatch, powstaja:
  - `shadow_entries.jsonl`,
  - `shadow_lifecycle.jsonl`,
  - transport `buys.jsonl`.
- Brak panic.
- Brak queue-depth failure.
- Brak replay-payload-mismatch regression.
- Brak live transaction requirement.
- Artefakty sa w dedykowanym namespace:
  - `shadow-burnin-p37-lifecycle-smoke`.

Brak BUY w 30-min smoke runie nie jest porazka. Wtedy run potwierdza tylko
config/runtime readiness, ale nie dostarcza lifecycle proof. Dalsza decyzja
przechodzi do sciezki "kod dziala, ale trzeba zebrac lifecycle run".

## 10. Faza E - Shadow-onchain lifecycle report

### Zakres

Uruchomic `scripts/shadow_onchain_lifecycle_report.py` na:

1. nowym smoke runie,
2. wszystkich dostepnych historycznych shadow-burnin artefaktach,
3. opcjonalnych artefaktach dostarczonych przez operatora z PC.

### Smoke command

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml \
  --all-sessions \
  --output logs/shadow_run/shadow-burnin-p37-lifecycle-smoke/shadow_onchain_lifecycle_report.jsonl
```

### Historyczny run

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config <historyczny_config_lub_odtworzony_config> \
  --all-sessions \
  --output <historyczny_output_shadow_onchain_lifecycle_report.jsonl>
```

### Zasady gap filtering

`--max-truth-gap-ms` moze byc uzyty diagnostycznie, ale nie powinien byc jedyna
logika klasyfikacji. Preferowane jest zachowanie rows i pozniejsze nadanie:

- `truth_gap_clean`,
- `truth_gap_degraded_acceptable`,
- `truth_gap_too_large`,
- `truth_gap_unknown`.

### Deliverable

- `logs/shadow_run/<namespace>/shadow_onchain_lifecycle_report.jsonl`,
- `PLANS/AUDYT/RAPORT_P3_7_SHADOW_ONCHAIN_LIFECYCLE_SMOKE_20260519.md`.

### Raport musi zawierac

- `rows`,
- `analysis_status` counts,
- `truth_status` counts,
- `truth_source` counts,
- `curve_finality` counts,
- `execution_outcome` counts,
- `gatekeeper_buy_context_found` count,
- `position_closed` count,
- `exit_filled` count,
- `final_pnl_pct` distribution,
- positive/negative PnL counts,
- entry truth gap distribution,
- exit truth gap distribution,
- entry drift vs on-chain executable distribution,
- exit drift vs on-chain executable distribution,
- `decision_to_execution_ms` distribution,
- `detection_to_execution_ms` distribution,
- skipped reason counts.

### Acceptance

- Jezeli byl BUY, przynajmniej jeden row z `analysis_status=ok` i
  `truth_status=resolved` potwierdza lane.
- `curve_finality` jest raportowane.
- `speculative` nie jest promowane do finalized proof.
- Exit gap okolo 30s moze byc `degraded_acceptable` dla `TimeStop`, ale nie
  `clean`.
- `AccountNotFound`, simulation failure i data problem nie sa success.

## 11. Faza F - Shadow lifecycle labeler

### Zakres

Zbudowac labeler, ktory przeksztalca `shadow_onchain_lifecycle_report.jsonl` w
P3.7 lifecycle labels z rozdzielonymi osiami:

- market,
- execution verification,
- truth gap,
- buy quality.

### Nowy skrypt

`scripts/v3_p37_shadow_lifecycle_labeler.py`

### Test

`scripts/test_v3_p37_shadow_lifecycle_labeler.py`

### CLI

```bash
python3 scripts/v3_p37_shadow_lifecycle_labeler.py \
  --shadow-onchain-lifecycle logs/shadow_run/<namespace>/shadow_onchain_lifecycle_report.jsonl \
  --output logs/shadow_run/<namespace>/p3_7_shadow_lifecycle_labels.jsonl \
  --entry-truth-gap-clean-ms 750 \
  --entry-truth-gap-acceptable-ms 2000 \
  --exit-truth-gap-clean-ms 2000 \
  --exit-truth-gap-acceptable-ms 10000 \
  --exit-truth-gap-timestop-acceptable-ms 30000 \
  --max-entry-drift-pct-clean 0.50 \
  --max-exit-drift-pct-clean 2.00
```

### Output fields

- `schema_version`,
- `candidate_id`,
- `position_id`,
- `pool_id`,
- `base_mint`,
- `decision_ts_ms`,
- `entry_execution_ts_ms`,
- `close_ts_ms`,
- `market_outcome_class`,
- `execution_verification_class`,
- `truth_gap_class`,
- `buy_quality_class`,
- `truth_status`,
- `truth_source`,
- `curve_finality_entry`,
- `curve_finality_exit`,
- `entry_truth_gap_ms`,
- `exit_truth_gap_ms`,
- `entry_drift_vs_onchain_executable_pct`,
- `exit_drift_vs_onchain_executable_pct`,
- `final_pnl_sol`,
- `final_pnl_pct`,
- `duration_ms`,
- `close_reason`,
- `total_exits`,
- `label_quality`,
- `unknown_reason`,
- `source_report_path`.

### Classification examples

Positive PnL + resolved truth + speculative finality + acceptable TimeStop gap:

```json
{
  "market_outcome_class": "market_good_clean",
  "execution_verification_class": "shadow_onchain_speculative_snapshot_verified",
  "truth_gap_class": "truth_gap_degraded_acceptable",
  "buy_quality_class": "buy_quality_dirty_good"
}
```

Positive PnL + resolved truth + confirmed/finalized finality + clean gaps:

```json
{
  "market_outcome_class": "market_good_clean",
  "execution_verification_class": "shadow_onchain_finalized_verified",
  "truth_gap_class": "truth_gap_clean",
  "buy_quality_class": "buy_quality_good"
}
```

Negative PnL + resolved truth:

```json
{
  "market_outcome_class": "market_bad_clean",
  "execution_verification_class": "<verified_or_degraded_class>",
  "buy_quality_class": "buy_quality_bad"
}
```

Unresolved truth, data problem, failed simulation:

```json
{
  "execution_verification_class": "shadow_execution_unknown",
  "buy_quality_class": "buy_quality_unknown"
}
```

albo:

```json
{
  "execution_verification_class": "shadow_execution_infeasible",
  "buy_quality_class": "buy_quality_not_executable"
}
```

### Acceptance

- Market, execution verification, truth gap i buy quality sa oddzielnymi polami.
- Negative PnL row nie jest good.
- Speculative finality nie jest finalized proof.
- Exit truth gap powyzej clean threshold nie jest clean.
- TimeStop moze byc degraded acceptable tylko w osobnym progu.
- Unknown execution status nie jest success.
- `AccountNotFound` i `data_problem` nie sa executable.

## 12. Faza G - Integracja z P3.7 truth layer

### Zakres

Rozszerzyc istniejace P3.7 raporty tak, aby obslugiwaly drugi typ datasetu:

```text
truth_dataset_kind:
  - v3_primary_replay_market_path
  - shadow_burnin_lifecycle_onchain
```

### Pliki do rozszerzenia

- `scripts/v3_p37_lifecycle_join_report.py`,
- `scripts/v3_p37_evidence_availability_report.py`,
- `scripts/v3_p37_temporal_split_report.py`.

### Nowy input

```bash
--shadow-lifecycle-labels <p3_7_shadow_lifecycle_labels.jsonl>
```

### Reguly integracji

- R10/R11/R13 pozostaja `v3_primary_replay_market_path`.
- Shadow-burnin labels sa osobnym `shadow_burnin_lifecycle_onchain`.
- Nie mieszac datasetow bez segmentacji:
  - namespace,
  - config,
  - policy hash,
  - run/session,
  - `truth_dataset_kind`.
- Shadow lifecycle outcome to label/outcome truth, nie decision-time feature.
- `live_confirmed_verified` tylko przy live signature + confirmation proof.
- Combined counts sa secondary; primary report musi pokazac per dataset kind.

### Acceptance

- `buy_quality_good` z shadow-burnin nie jest przypisywane do R10/R11/R13.
- Raport pokazuje counts per `truth_dataset_kind`.
- Raport ma combined-only tylko jako secondary context.
- Phase B nadal wymaga feature availability i temporal/session split.
- Brak zmiany active policy.

## 13. Faza H - Feature availability dla executable lifecycle rows

### Zakres

Sprawdzic, czy shadow-burnin executable rows maja decision-time features
wystarczajace do P3.7 feature prototype.

### Nowy skrypt

`scripts/v3_p37_shadow_lifecycle_feature_availability.py`

### CLI

```bash
python3 scripts/v3_p37_shadow_lifecycle_feature_availability.py \
  --shadow-lifecycle-labels logs/shadow_run/<namespace>/p3_7_shadow_lifecycle_labels.jsonl \
  --decisions logs/rollout/<namespace>/decisions/gatekeeper_v2_decisions.jsonl \
  --output-json logs/shadow_run/<namespace>/p3_7_shadow_lifecycle_feature_availability.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_SHADOW_LIFECYCLE_FEATURE_AVAILABILITY_20260519.md
```

### Raportowane counts

- rows total,
- `market_good_clean`,
- `market_bad_clean`,
- `buy_quality_good`,
- `buy_quality_dirty_good`,
- `buy_quality_bad`,
- rows with `v3_materialized_feature_snapshot`,
- rows with tx-intel fields,
- rows with checkpoint/TAS fields,
- rows with PDD fields,
- rows with organic fields,
- rows with alpha fields,
- rows with Gatekeeper V2 phase fields,
- rows with only legacy fields,
- policy/version/config distribution,
- missing join key counts.

### Acceptance

- Jezeli MFS/V3 features sa dostepne, mozna rozpatrywac P3.7 feature prototype
  na executable lifecycle truth.
- Jezeli dostepne sa tylko legacy fields, uzyc datasetu jako diagnostic/execution
  truth, nie jako bezposredniego V3 selector candidate.
- Jezeli coverage jest niska, Phase B pozostaje blocked z poprawnym powodem.

## 14. Faza I - Dokumentacja i ADR

### Nowy ADR

`docs/ADR/ADR-0134-v3-p37-shadow-burnin-lifecycle-truth-integration.md`

Sekcje:

- Context,
- Decision,
- Shadow-burnin definition,
- Evidence classes,
- Curve finality semantics,
- Truth gap semantics,
- Market vs execution vs buy quality classes,
- Dataset segmentation,
- Invariants,
- Non-goals,
- Rejected alternatives,
- Acceptance criteria.

### Erraty do raportow P3.7

Dopisac scope note do:

- `PLANS/AUDYT/RAPORT_P3_7_EXECUTION_FEASIBILITY_ARTIFACT_AUDIT_R10_R11_R13_20260518.md`,
- `PLANS/AUDYT/RAPORT_P3_7_EVIDENCE_AVAILABILITY_R10_R11_R13_20260518.md`,
- `PLANS/AUDYT/RAPORT_P3_7_TEMPORAL_SPLIT_BASELINE_R10_R11_R13_20260518.md`.

Tekst scope note:

```text
This report audits execution evidence only inside R10/R11/R13 local primary-only
namespaces. It does not include root/canonical shadow-burnin lifecycle truth.
The conclusion good_executable=0 applies only to those datasets, not to Ghost
as a whole.
```

### Plan aktualizowany

Ten plik jest kanonicznym planem wykonawczym:

- `PLANS/PLAN_P3_7_6A_SHADOW_BURNIN_LIFECYCLE_TRUTH_INTEGRATION_20260519.md`.

## 15. Faza J - Decyzja po integracji

### Sciezka 1 - lifecycle truth + features dostepne

Warunki:

- `buy_quality_good >= minimum_sample`,
- `buy_quality_bad >= minimum_sample`,
- decision-time feature coverage sufficient,
- temporal/session split possible.

Decyzja:

```text
P3.7 Phase B may start on shadow-burnin executable truth.
```

### Sciezka 2 - lifecycle truth jest, ale brak V3/MFS features

Decyzja:

```text
Use lifecycle truth to design a future data collection run with V3 payload +
lifecycle enabled. Phase B V3 selector remains blocked.
```

### Sciezka 3 - kod dziala, ale brak artefaktow

Decyzja:

```text
Run new shadow-burnin lifecycle collection profile. No P2. No live.
```

### Sciezka 4 - shadow-burnin code nie dziala

Decyzja:

```text
Repair shadow-burnin first. Do not continue feature mining.
```

## 16. Test plan

### Python compile

```bash
python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py
python3 -m py_compile scripts/v3_p37_shadow_burnin_inventory.py
python3 -m py_compile scripts/v3_p37_shadow_lifecycle_labeler.py
python3 -m py_compile scripts/v3_p37_shadow_lifecycle_feature_availability.py
```

### Unit tests

```bash
python3 -m unittest scripts/test_v3_p37_shadow_lifecycle_labeler.py -v
python3 -m unittest scripts/test_v3_p37_lifecycle_join_report.py -v
python3 -m unittest scripts/test_v3_p37_evidence_availability_report.py -v
python3 -m unittest scripts/test_v3_p37_temporal_split_report.py -v
```

### Rust/config readiness

```bash
cargo test --workspace --no-run
bash scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml
```

### Smoke report

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml \
  --all-sessions \
  --output logs/shadow_run/shadow-burnin-p37-lifecycle-smoke/shadow_onchain_lifecycle_report.jsonl
```

### Markdown / diff hygiene

```bash
git diff --check
```

## 17. Implementation order

Zalecana kolejnosc commitow:

1. **Commit A - Discovery report**
   - read-only report under `PLANS/AUDYT/`.
2. **Commit B - Artifact/evidence contract**
   - plan/contract under `PLANS/`.
3. **Commit C - Inventory script**
   - `scripts/v3_p37_shadow_burnin_inventory.py`,
   - inventory report.
4. **Commit D - Smoke rollout profile**
   - `configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml`,
   - no active policy changes.
5. **Commit E - Smoke/report run artifacts**
   - generated report paths only,
   - no mutation of historical R10/R11/R13.
6. **Commit F - Shadow lifecycle labeler**
   - script + tests,
   - evidence class semantics.
7. **Commit G - P3.7 truth report integration**
   - `truth_dataset_kind`,
   - segmented counts.
8. **Commit H - Feature availability audit**
   - decision-time feature coverage for executable rows.
9. **Commit I - ADR-0134 and erratas**
   - documentation only.
10. **Commit J - Final decision report**
    - Phase B allowed/blocked with explicit reason.

## 18. Final expected state

Po wykonaniu P3.7.6A repo powinno miec:

- pelna mape shadow-burnin code/config/runtime,
- kontrakt artefaktow shadow-burnin,
- inventory obecnych i historycznych artefaktow,
- nowy albo historyczny `shadow_onchain_lifecycle_report.jsonl`,
- `v3_p37_shadow_lifecycle_labeler.py`,
- label classes rozdzielajace market/execution/truth gap/buy quality,
- klasy dowodu z finality semantics,
- klasy gap z entry/exit/close-reason semantics,
- P3.7 reports rozszerzone o `shadow_burnin_lifecycle_onchain`,
- ADR-0134,
- erraty do R10/R11/R13 raportow,
- decyzje:
  - Phase B moze uzyc shadow-burnin executable truth,
  - albo Phase B pozostaje blocked z jasnym powodem,
  - albo wymagany jest nowy lifecycle collection run.

## 19. Definition of Done

P3.7.6A jest domkniete tylko jezeli zachodzi jedno z dwoch:

### DoD A - Integrated lifecycle truth

- Shadow-burnin lifecycle truth jest zintegrowane jako osobny dataset kind.
- Istnieja `buy_quality_good` i `buy_quality_bad` labels.
- Feature availability dla tych rows jest znana.
- Temporal/session split jest mozliwy albo jawnie oceniony.
- Phase B ma formalny GO/NO-GO.

### DoD B - Insufficient artifacts documented

- Kod/config/report flow jest opisany.
- Inventory dowodzi, ktorych artefaktow brakuje.
- Brak artefaktow jest odrozniony od braku kodu.
- Jest jednoznaczny runbook nowego shadow-burnin lifecycle collection runu.
- Phase B pozostaje blocked bez falszywego globalnego claimu, ze Ghost nie ma
  executable-opportunity lane.
