# Raport P3.7 Shadow-Burnin Code Discovery

Data: 2026-05-19

Repo HEAD: `7e3e5fc`

Status: **STAGE A COMPLETE / READ-ONLY DISCOVERY / NO P2 / NO LIVE / NO ACTIVE POLICY CHANGE**

## 1. Cel i zakres

Ten raport realizuje Faze A planu:

- `PLANS/PLAN_P3_7_6A_SHADOW_BURNIN_LIFECYCLE_TRUTH_INTEGRATION_20260519.md`

Zakres byl read-only:

- config shadow-burnin,
- launcher runtime,
- Gatekeeper BUY handoff,
- trigger shadow transport,
- shadow entry log,
- post-buy shadow lifecycle,
- DecisionLogger,
- event bus,
- raporty `shadow_run_report.py` i `shadow_onchain_lifecycle_report.py`,
- dokumenty/runbooki/ADR,
- fizyczna dostepnosc artefaktow na obecnym VPS.

Nie wykonano:

- zmian active V2/V2.5,
- zmian IWIM,
- zmian live sendera,
- threshold tuning,
- uruchomienia live,
- uruchomienia P2,
- zmian FSC active gate,
- zmian w runtime feature implementation.

## 2. Werdykt discovery

Shadow-burnin nadal istnieje w repo jako osobna, pierwszoklasowa sciezka runtime.
Nie jest to tylko historyczny koncept dokumentacyjny.

Potwierdzone fakty:

1. Root `config.toml` ma konfiguracje `entry_mode = "shadow_only"`,
   `execution_mode = "shadow"`, `trigger.shadow_run.enabled = true`,
   `trigger.shadow_run.emit_event_bus = true` oraz `execution.shadow.entry_log_path`.
2. `configs/rollout/shadow-burnin.toml` opisuje standalone shadow runtime z
   `entry_mode = "shadow_only"`, `execution_mode = "shadow"`, jawna sciezka
   `shadow_entries.jsonl`, jawna sciezka `shadow_lifecycle.jsonl`, event dir,
   decision log path i trigger shadow transport.
3. Kod launchera waliduje pary `execution_mode` / `entry_mode`; para
   `Shadow + ShadowOnly` jest legalnym profilem.
4. Trigger potrafi wykonac `simulate_transaction_with_config()` przez
   `trigger.shadow_run`, zapisac transport JSONL i wyemitowac
   `ShadowBuySimulated`.
5. OracleRuntime zapisuje canonical `shadow_entries.jsonl` tylko gdy
   `execution_mode == Shadow`.
6. PostBuyRuntime w lane `shadow` rejestruje pozycje w `MonitoringEngine`,
   ktory zapisuje `exit_filled`, `exit_blocked` i `position_closed` do
   `shadow_lifecycle.jsonl`.
7. Ten sam `shadow_lifecycle.jsonl` moze zawierac tez rekordy
   `record_type = "shadow_dispatch"` dla dispatch lifecycle.
8. `scripts/shadow_onchain_lifecycle_report.py` koreluje transport, entry,
   lifecycle, Gatekeeper BUY rows i `DIAG_ACCOUNT_UPDATE_RELAY` z logow systemowych.
9. Obecny P3.7 wniosek `good_executable=0` jest prawdziwy dla R10/R11/R13
   primary-only namespace'ow, ale nie jest globalnym wnioskiem o calym Ghost.

Ograniczenia potwierdzone w discovery:

- Na obecnym VPS nie znaleziono gotowych
  `shadow_onchain_lifecycle_report*.jsonl`.
- R10/R11 primary-only nie maja lokalnego `shadow_entries.jsonl` ani
  `shadow_lifecycle.jsonl`.
- R13 primary-only ma po jednym `shadow_entries`, `shadow_lifecycle` i transport
  row, ale poprzedni audyt klasyfikuje ten przypadek jako fail-closed
  `AccountNotFound` / `data_problem`, nie jako executable success.
- Starsze namespace'y shadow sa czesciowo obecne, ale nie tworza jeszcze
  znormalizowanego P3.7 execution-truth datasetu.

## 3. Warstwy prawdy, ktorych nie wolno mieszac

### 3.1 Shadow simulation

Shadow simulation oznacza, ze Ghost przeszedl decision path, zbudowal BUY request
i wykonal symulacje przez RPC bez realnego wyslania transakcji.

Dowody tej warstwy:

- `trigger.shadow_run.output_path` / `*buys.jsonl`,
- event `ShadowBuySimulated`,
- `shadow_execution_outcome` w Gatekeeper BUY logu,
- `shadow_dispatch` rows w `shadow_lifecycle.jsonl`.

To nie jest live inclusion.

### 3.2 Shadow-onchain validated

Shadow-onchain validated oznacza, ze shadow entry/lifecycle zostaly skorelowane z
on-chain executable state z `DIAG_ACCOUNT_UPDATE_RELAY`.

Dowody tej warstwy:

- `shadow_entries.jsonl`,
- `shadow_lifecycle.jsonl`,
- transport `*buys.jsonl`,
- Gatekeeper BUY row,
- system log z `DIAG_ACCOUNT_UPDATE_RELAY`,
- output `scripts/shadow_onchain_lifecycle_report.py`.

To nadal nie jest live inclusion.

### 3.3 Live inclusion

Live inclusion wymaga live signature i confirmation/landing proof. Shadow-only
`live_signature = None`, `submit` nie jest confirmation, a
`curve_finality = speculative` nie jest finalized proof.

Starsze dokumenty mocno rozdzielaja `shadow simulation` od `live inclusion`, ale
nazwa `shadow-onchain validated` jako trzeci formalny poziom jest dopiero
kontraktem P3.7.6A. To jest gap terminologiczny do domkniecia w Fazie B/I, nie
brak runtime kodu.

## 4. Config surface

| Obszar | Plik / linie | Znaczenie |
| --- | --- | --- |
| Root trigger | `config.toml:78-83` | `trigger.enabled = true`, `entry_mode = "shadow_only"` |
| Root execution | `config.toml:121-131` | `execution_mode = "shadow"`, `entry_log_path`, lifecycle moze byc derived |
| Root shadow transport | `config.toml:135-148` | `trigger.shadow_run.enabled`, RPC, retry/timeout, `output_path`, event bus |
| Root events | `config.toml:169-175` | Event dataset dir |
| Rollout definition | `configs/rollout/shadow-burnin.toml:1-7` | Standalone shadow runtime; live sender disabled; trigger block jest shadow transport adapterem |
| Rollout FSC caveat | `configs/rollout/shadow-burnin.toml:30-39` | Historyczny profil ma `funding_lane_mode = "full_chain"`; P3.7.6A smoke powinien miec FSC disabled zgodnie z ADR-0130 |
| Rollout trigger | `configs/rollout/shadow-burnin.toml:47-84` | `entry_mode = "shadow_only"`, shadow transport, `payer_strategy = "ephemeral"`, output buys |
| Rollout execution | `configs/rollout/shadow-burnin.toml:86-96` | `execution_mode = "shadow"`, `shadow_entries`, `shadow_lifecycle`, `prepared_entry_mirror`, `emit_warning` |
| Rollout events/decisions | `configs/rollout/shadow-burnin.toml:98-108` | Events dir i oracle decision dir |
| Rollout logs | `configs/rollout/shadow-burnin.toml:125-132` | system/oracle log roots |

Aktualna sciezka dla przyszlego smoke profilu P3.7.6A:

- planowana: `configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml`
- obecny stan: plik jeszcze nie istnieje,
- wniosek Stage A: Stage D musi go utworzyc jako nowy namespace z
  `seer.funding_lane_mode = "disabled"`.

`configs/rollout/shadow-burnin.toml` jest dobrym wzorcem code/config discovery,
ale nie powinien byc bezposrednio uzyty jako P3.7.6A smoke profile, bo jest
historycznie powiazany z full-chain FSC bake.

## 5. Runtime flow

```text
config.toml / configs/rollout/*.toml
  -> LauncherConfig::validate_execution_profile()
  -> OracleRuntime pool observation
  -> MaterializedFeatureSet
  -> Gatekeeper V2/V2.5 BUY
  -> enrich_buy_log_with_shadow_run()
  -> execute_gatekeeper_buy_path()
  -> TriggerComponent::dispatch_prepared_buy_with_shadow()
  -> RpcShadowSimulator::simulate_buy()
  -> TriggerBuyOutcome::ShadowSimulated
  -> GhostEvent::ShadowBuySimulated
  -> trigger shadow transport log / buys.jsonl
  -> OracleRuntime canonical shadow_entries.jsonl
  -> shadow PostBuySubmitted handoff
  -> PostBuyRuntime lane=shadow
  -> MonitoringEngine shadow position
  -> shadow_lifecycle.jsonl
  -> scripts/shadow_onchain_lifecycle_report.py
  -> shadow-onchain lifecycle rows
```

## 6. Code map

| Warstwa | Plik / funkcje | Rola |
| --- | --- | --- |
| Config enums | `ghost-launcher/src/config.rs:383-405` | `ExecutionMode::{Live,Paper,Shadow,Dual}` i `TriggerEntryMode::{Live,DryRunMock,ShadowOnly,LiveAndShadow}` |
| Legacy warnings | `ghost-launcher/src/config.rs:624-642` | Rozdziela canonical shadow runtime od legacy compare-only surfaces |
| Execution profile validation | `ghost-launcher/src/config.rs:660-670`, `805-835` | Legalna para `ExecutionMode::Shadow` + `TriggerEntryMode::ShadowOnly` |
| Live sender guard | `ghost-launcher/src/config.rs:874-913` | Live-capable modes wymagaja osobnych live transport prerequisites |
| Shadow payer guard | `ghost-launcher/src/config.rs:915-943` | Shadow-only moze ominac live keypair tylko przy `payer_strategy = "ephemeral"` |
| Shadow transport validation | `ghost-launcher/src/config.rs:945-970` | Shadow-capable modes wymagaja enabled `trigger.shadow_run` i realnego RPC |
| Trigger config schema | `ghost-launcher/src/config.rs:1499-1564`, `1566-1631` | `entry_mode`, `shadow_run.enabled`, RPC, retry, timeout, output, event bus |
| Runtime path rebasing | `ghost-launcher/src/config.rs:2779-2797` | Rebase sciezek: events, shadow entry/lifecycle, decision log, transport output |
| Lifecycle path derivation | `ghost-launcher/src/main.rs:311-340` | `shadow_lifecycle.jsonl` derived z `shadow_entries.jsonl` tylko w shadow mode |
| Preflight artifact dirs | `ghost-launcher/src/main.rs:653-690` | Sprawdza zapis shadow entry, lifecycle i trigger output dirs |
| Startup artifact fail-closed | `ghost-launcher/src/main.rs:1084-1104` | Launcher konczy start, jesli nie moze pisac shadow entry/lifecycle |
| PostBuy startup | `ghost-launcher/src/main.rs:1821-1864` | Subskrypcja PostBuy przed producentami i przekazanie `shadow_lifecycle_log_path` |
| Trigger startup log | `ghost-launcher/src/main.rs:1963-1968` | Loguje `execution_mode` i `entry_mode` |
| Trigger shadow support | `ghost-launcher/src/components/trigger/component.rs:1702-1728` | Ekspozycja entry mode, output path, event bus i shadow support |
| Shadow-only dispatch | `ghost-launcher/src/components/trigger/component.rs:4189-4218`, `4248-4315` | Rezerwuje slot i wykonuje `simulate_buy()` bez live send |
| Background shadow companion | `ghost-launcher/src/components/trigger/component.rs:4221-4245`, `4523-4630` | Dla live_and_shadow: companion simulation, event bus, transport record |
| Transport write | `ghost-launcher/src/components/trigger/component.rs:4447-4467`, `4477-4520` | Zapis success/failure shadow buy records z lifecycle identity |
| Join key | `ghost-launcher/src/components/trigger/shadow_run.rs:41-43` | `join_key = pool_id:base_mint:first_seen_ts_ms` |
| Rollout profile from path | `ghost-launcher/src/components/trigger/shadow_run.rs:45-55` | Wyprowadza rollout profile z path component po `rollout` |
| Dispatch lifecycle schema | `ghost-launcher/src/components/trigger/shadow_run.rs:57-102`, `120-235` | `shadow_dispatch` rows, `submitted/failed/abandoned/closed`, idempotency |
| Shadow buy schema | `ghost-launcher/src/components/trigger/shadow_run.rs:237-294` | Transport record fields: candidate, pool, mint, timing, amount, errors, live sig |
| Event candidate id | `ghost-launcher/src/events.rs:461-506` | `candidate_id = base_mint_pool_trace_ref` |
| Event bus variants | `ghost-launcher/src/events.rs:626-669`, `750-789` | `PostBuySubmitted`, `ShadowBuySimulated` |
| RPC simulation | `ghost-launcher/src/components/trigger/shadow_run.rs:1044-1132` | `simulate_transaction_with_config`, account state, units, logs, err |
| Gatekeeper BUY enrichment | `ghost-launcher/src/oracle_runtime.rs:5718-5740` | Zapisuje shadow readiness, trigger mode i `shadow_execution_outcome` do BUY logu |
| BUY path dispatch | `ghost-launcher/src/oracle_runtime.rs:5982-6331` | Hydrate metadata, readiness, trigger dispatch, apply receipt |
| Shadow entry creation | `ghost-launcher/src/oracle_runtime.rs:6712-6803` | Buduje i zapisuje canonical shadow entry tylko dla `ExecutionMode::Shadow` |
| Shadow transport/lifecycle append | `ghost-launcher/src/oracle_runtime.rs:6805-6860` | Transport row plus opcjonalny `shadow_dispatch` lifecycle row |
| Shadow outcome application | `ghost-launcher/src/oracle_runtime.rs:7333-7573` | Obsluga `ShadowSimulated`, event bus, post-buy handoff, entry log |
| Join key per dispatch | `ghost-launcher/src/oracle_runtime.rs:7575-7665` | Uzywa `make_shadow_join_key(pool_id, base_mint, pool_data.timestamp_ms)` |
| Gatekeeper buy logging | `ghost-launcher/src/oracle_runtime.rs:9108-9149` | BUY log enriched i zapisany async przez DecisionLogger |
| PostBuy shadow invariant | `ghost-launcher/src/components/post_buy_runtime.rs:1-23` | Shadow lane trafia do ghost-brain MonitoringEngine; ShadowLedger tylko diagnostic compare dla live truth |
| PostBuy shadow runtime | `ghost-launcher/src/components/post_buy_runtime.rs:1658-1765` | `execution_mode == "shadow"` tworzy MonitoringEngine z ShadowPositionBook i lifecycle path |
| Shadow handoff | `ghost-launcher/src/components/post_buy_runtime.rs:1949-2237`, `2244-2380` | Lane `shadow` rejestruje pozycje po walidacji pool/mint/entry price/canonical snapshot |
| Lifecycle schema | `ghost-brain/src/guardian/post_buy/engine.rs:430-492` | `exit_filled`, `exit_blocked`, `position_closed` fields: PnL, truth, slots, status |
| Lifecycle writer | `ghost-brain/src/guardian/post_buy/engine.rs:1035-1053` | Append JSONL do `shadow_lifecycle_log_path` |
| Lifecycle base | `ghost-brain/src/guardian/post_buy/engine.rs:1055-1097` | candidate/pool/mint/position/truth/sample metadata |
| Position close row | `ghost-brain/src/guardian/post_buy/engine.rs:1100-1172` | `position_closed` z final PnL, duration, close reason, truth evidence |
| Position registration | `ghost-brain/src/guardian/post_buy/engine.rs:1202-1310` | Tworzy `position_id`, zapisuje context, initial snapshot i entry economics |
| Exit rows | `ghost-brain/src/guardian/post_buy/engine.rs:3334-3472` | `exit_blocked` oraz `exit_filled` z truth/economics |
| DecisionLogger schema | `ghost-brain/src/oracle/decision_logger.rs:60-84`, `208-315` | Schema v20, join key, rollout/plane/config hash, shadow readiness/outcome |
| `ab_record_id` | `ghost-brain/src/oracle/decision_logger.rs:1207-1209` | Deterministic dedup key dla downstream |
| Plane routing | `ghost-brain/src/oracle/decision_logger.rs:1992-2095` | `legacy_live` i `v25_shadow` routed per rollout/config hash |
| Decision file writes | `ghost-brain/src/oracle/decision_logger.rs:2260-2355` | `gatekeeper_v2_decisions.jsonl` i BUY-only `gatekeeper_v2_buys.jsonl` |

## 7. Artefakty emitowane przez shadow-burnin

| Artefakt | Writer / resolver | Funkcja dowodowa |
| --- | --- | --- |
| `gatekeeper_v2_decisions.jsonl` | DecisionLogger, `write_gatekeeper_buy_log()` | Wszystkie decyzje; plane/config scoped |
| `gatekeeper_v2_buys.jsonl` | DecisionLogger, `write_gatekeeper_buy_log()` | BUY-eligible rows; zawiera shadow readiness/outcome |
| `trigger.shadow_run.output_path` / `*buys.jsonl` | `append_shadow_buy_record()` | Transport/simulation row: timings, amount, units, errors, live_signature |
| `execution.shadow.entry_log_path` / `shadow_entries.jsonl` | `maybe_append_canonical_shadow_entry_record()` | Canonical shadow entry price/timestamp/slot/candidate |
| `execution.shadow.lifecycle_log_path` / `shadow_lifecycle.jsonl` | Trigger + MonitoringEngine | Dispatch lifecycle (`shadow_dispatch`) oraz position lifecycle (`exit_filled`, `exit_blocked`, `position_closed`) |
| `execution.events.output_dir` | EventEmitter | Execution event datasets per run/session |
| `logging.file_path` / `system.log*` | launcher/system logging | `DIAG_ACCOUNT_UPDATE_RELAY` on-chain snapshot truth |
| `logging.oracle_log_path` / `oracle.log*` | oracle logging | Operational oracle trace |
| `metrics.prom` | metrics endpoint/snapshot | Hot-path/report gates |
| `shadow_onchain_lifecycle_report*.jsonl` | `scripts/shadow_onchain_lifecycle_report.py` | Correlated shadow lifecycle vs on-chain executable state |

## 8. Join keys

| Key | Format / source | Uzycie |
| --- | --- | --- |
| `join_key` | `pool_id:base_mint:first_seen_ts_ms`; `make_shadow_join_key()` | Stable observation/dispatch correlation |
| `candidate_id` | `base_mint_pool_trace_ref`; `build_execution_candidate_id()` | Decision, transport, lifecycle, event correlation |
| `position_id` | Default `pool_amm_id:base_mint:now_ms` albo context-provided | Shadow lifecycle identity |
| `pool_id` / `pool_amm_id` | Pubkey string | Gatekeeper, trigger, lifecycle, on-chain truth join |
| `mint_id` / `base_mint` | Pubkey string | Entry/lifecycle/on-chain truth join |
| `idempotency_key` | `blake3(pool_id:join_key:rollout_profile)` | Dispatch dedup and lifecycle correlation |
| `ab_record_id` | `{pool_id}:{t0}:{t_end}:{verdict}` | Downstream dedup for P3/P3.7 labels |
| `rollout_profile` | DecisionLogger routing or derived from path | Dataset segmentation |
| `config_hash` | DecisionLogger routing | Prevents cross-config mixing |
| `decision_plane` | `legacy_live` / `v25_shadow` | Prevents live/shadow plane mixing |

Uwaga: `scripts/shadow_onchain_lifecycle_report.py` obecnie laczy Gatekeeper BUY rows
po `(base_mint, pool_id)`, a lifecycle/transport po `candidate_id`. Dlatego Faza B
powinna utrzymac oba poziomy kluczy i nie zastepowac jednego drugim.

## 9. `scripts/shadow_run_report.py`

Formalny shadow run report:

- laduje config i rozpoznaje `execution_mode`, `entry_mode`, runtime lane oraz
  sciezki decyzji, transportu, lifecycle, eventow, system log i metrics
  (`scripts/shadow_run_report.py:161-230`),
- preferuje `v25_shadow` dla runtime lane `shadow`
  (`scripts/shadow_run_report.py:254-283`),
- derivuje lifecycle path z entry path, jesli brak jawnej sciezki
  (`scripts/shadow_run_report.py:286-291`),
- skanuje `shadow_lifecycle.jsonl` i rozroznia `exit_filled`, `exit_blocked`,
  `position_closed` oraz `shadow_dispatch`
  (`scripts/shadow_run_report.py:649-737`),
- rozdziela expected dispatch, actual dispatch, lifecycle terminal rows oraz
  `no_dispatch`
  (`scripts/shadow_run_report.py:884-990`),
- traktuje `no_dispatch` jako osobna klasyfikacje, nie lifecycle failure
  (`scripts/shadow_run_report.py:1020-1065`),
- sprawdza brak live side effects przez `live_signature_count == 0`
  (`scripts/shadow_run_report.py:1052-1058`),
- raportuje profile, artefakty i summary
  (`scripts/shadow_run_report.py:1095-1145`).

To narzedzie odpowiada za formalny go/no-go shadow-burnin, ale nie wykonuje
shadow-onchain proof z finality/truth-gap semantics. Do tego sluzy nastepny
raport.

## 10. `scripts/shadow_onchain_lifecycle_report.py`

Raport on-chain lifecycle:

- parsuje `DIAG_ACCOUNT_UPDATE_RELAY`, w tym `curve_finality`
  (`scripts/shadow_onchain_lifecycle_report.py:38-44`),
- definiuje rekordy `ShadowTransportRecord`, `ShadowEntryRecord`,
  `LifecycleBundle`, `GatekeeperBuyRow`, `DiagUpdate`
  (`scripts/shadow_onchain_lifecycle_report.py:47-176`),
- CLI przyjmuje `--config`, `--output`, `--session-start-ms`,
  `--session-end-ms`, `--all-sessions`, `--max-truth-gap-ms`
  (`scripts/shadow_onchain_lifecycle_report.py:202-246`),
- z configu resolveruje decision dir, shadow entry log, lifecycle log,
  transport log, events dir, system log base i output
  (`scripts/shadow_onchain_lifecycle_report.py:249-307`),
- laduje transport i entry rows
  (`scripts/shadow_onchain_lifecycle_report.py:490-535`),
- laduje lifecycle rows z `exit_filled` i `position_closed`
  (`scripts/shadow_onchain_lifecycle_report.py:538-616`),
- laduje Gatekeeper BUY rows po `(base_mint, pool_id)`
  (`scripts/shadow_onchain_lifecycle_report.py:679-718`),
- laduje `DIAG_ACCOUNT_UPDATE_RELAY` z system logow, uzywajac `rg` gdy mozliwe
  (`scripts/shadow_onchain_lifecycle_report.py:721-817`),
- analizuje tylko lifecycle candidates z `position_closed`, resolved close truth,
  resolved exit fills i bez transport error
  (`scripts/shadow_onchain_lifecycle_report.py:886-917`),
- znajduje entry truth i exit truth, a `--max-truth-gap-ms` dziala obecnie jako
  hard filter dla entry i exit
  (`scripts/shadow_onchain_lifecycle_report.py:988-995`,
  `scripts/shadow_onchain_lifecycle_report.py:1069-1077`),
- wylicza executable entry/exit price, drifty, timing i final PnL
  (`scripts/shadow_onchain_lifecycle_report.py:1187-1287`),
- zapisuje JSONL i drukuje summary z driftami oraz truth gap stats
  (`scripts/shadow_onchain_lifecycle_report.py:1290-1415`).

Wazny wniosek dla Faz B/F:

- skrypt ma juz `curve_finality`,
- ma entry i exit gap measurements,
- ale nie klasyfikuje jeszcze `speculative` jako osobnej klasy dowodu,
- ma jeden wspolny `--max-truth-gap-ms`, a plan wymaga osobnych progow entry/exit
  oraz exit-by-close-reason.

`scripts/shadow_onchain_lifecycle_report2.py` jest wariantem bliskim primary
reportowi, ale zawiera dodatkowa logike fee bps (`PUMP_FUN_FEE_BPS = 100`).
Faza B/E powinna zdecydowac, ktory wariant jest kanoniczny dla P3.7.6A.

## 11. Dokumenty i ADR wspierajace Stage A

| Dokument | Linie | Znaczenie |
| --- | --- | --- |
| `docs/RUNBOOK_PRODUCTION_ROLLOUT.md` | `1-19`, `43-46`, `60-71`, `106-137` | Canonical shadow runbook, config SSOT, launch command, stop/closeout, report artifacts |
| `AUDYT_PIPELINE_GATEKEEPER_V2.md` | `55-75`, `77-94` | Full pipeline i non-negotiable: shadow simulation nie jest live inclusion; submit nie jest confirmation |
| `AUDYT_PIPELINE_GATEKEEPER_V2.md` | `1018-1067` | Shadow-burnin flow oraz co shadow dowodzi / czego nie dowodzi |
| `AUDYT_PIPELINE_GATEKEEPER_V2.md` | `1069-1100` | Join key, idempotency key, dispatch status, lifecycle fields |
| `AUDYT_PIPELINE_GATEKEEPER_V2.md` | `1188-1218` | Raporty i interpretacja BUY jako hipotezy, nie live inclusion |
| `AUDYT_PIPELINE_GATEKEEPER_V2.md` | `1304-1338`, `1340-1352` | Hot files, tests, caveat: shadow-burnin nie dowodzi live inclusion |
| `AUDYT_PIPELINE_GATEKEEPER_V2.md` | `1408-1494` | Shadow-first active path, metrics i false BUY/degraded semantics |
| `docs/ADR/ADR-0127-p5-shadow-execution-lifecycle-20260508.md` | `9-12`, `16-30`, `45-47` | P5 lifecycle evidence chain, ephemeral payer, idempotency, no_dispatch vs failed_reconciliation |
| `docs/ADR/ADR-0128-p6-validation-gates-promotion-readiness-20260509.md` | `18-36`, `40-55`, `75-78` | Plane separation, shadow-only no live payer, lifecycle reconciliation, live remains out of scope |
| `docs/ADR/ADR-0067-shadow-only-buy-outcome-diagnosis-2026-04-01.md` | `24-69`, `81-92` | `shadow_skipped_not_ready`, `shadow_simulation_error`, `shadow_simulated` sa rozne klasy |
| `docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md` | `32-49`, `73-84` | FSC de-scoped under single-stream; P3.7 smoke profile musi miec funding lane disabled |
| `docs/ADR/ADR-0133-v3-p37-feature-redesign-lifecycle-labels.md` | `60-69`, `182-213` | Execution feasibility join przed BUY-quality claims; good outcome bez feasible execution nie jest true good entry |

## 12. Fizyczna dostepnosc artefaktow na obecnym VPS

Read-only scan wykazal:

| Namespace / rodzina | Entry rows | Lifecycle rows | Transport rows | Uwagi |
| --- | ---: | ---: | ---: | --- |
| `shadow-burnin-v25-repair` | 6 | 0 found | 6 | Partial: entry + transport, brak lifecycle path w `logs/shadow_run` scan |
| `shadow-burnin-v3-p36-sample-r13-primary-only` | 1 | 1 | 1 | Znany R13 fail-closed z audytu P3.7 |
| `shadow-burnin-buy-heavy` | 4227 | 0 found | 4233 | Partial: duzy transport/entry set, brak lifecycle path w scan |
| `shadow-burnin-buy-heavy-rerun` | 7785 | 4776 | 7816 | Najpelniejszy lokalny historical shadow artifact set |
| `shadow-burnin` | 0 found | 0 found | 3405 | Flat transport log jest, brak entry/lifecycle pod oczekiwanym shadow_run scan |
| `R10 primary-only` | 0 | 0 | 0 found | Zgodne z P3.7 audit: no local shadow proof |
| `R11 primary-only` | 0 | 0 | 0 found | Zgodne z P3.7 audit: no local shadow proof |

Znalezione system/oracle logi sa glownie rotowane jako `system.log.YYYY-MM-DD`
i `oracle.log.YYYY-MM-DD`; `shadow_onchain_lifecycle_report.py` powinien je
widziec, bo globuje `system_log_base.name*`.

Nie znaleziono:

- `logs/shadow_run/**/shadow_onchain_lifecycle_report*.jsonl`,
- `configs/rollout/*p37*`,
- `configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml`.

Interpretacja:

- repo code availability: **present**,
- current artifact availability: **partial**,
- current P3.7/R10/R11/R13 executable proof: **not sufficient**,
- historical shadow artifacts: **present but require inventory + onchain report regeneration**,
- przyczyna brakow po migracji VPS nie jest dowodzona przez kod; discovery
  potwierdza jedynie fizyczny brak lub niekompletnosc artefaktow na obecnym VPS.

## 13. R10/R11/R13 scope note

Raport:

- `PLANS/AUDYT/RAPORT_P3_7_EXECUTION_FEASIBILITY_ARTIFACT_AUDIT_R10_R11_R13_20260518.md`

audytowal tylko lokalne primary-only namespace'y:

- R10: `shadow-burnin-v3-p32-replay-r10-primary-only`,
- R11: `shadow-burnin-v3-p32-replay-r11-primary-only`,
- R13: `shadow-burnin-v3-p36-sample-r13-primary-only`.

Wynik tamtego raportu:

- R10/R11: no shadow entry/lifecycle proof,
- R13: 1 matched lifecycle row, ale `dispatch_status=failed`,
  `simulation_outcome=failed`, `error_class=data_problem`, `AccountNotFound`,
- `good_executable=0` dla R10/R11/R13.

To jest poprawny fail-closed wniosek o tych datasetach. Nie jest to globalny
wniosek, ze Ghost nie posiada shadow-burnin executable lane.

## 14. Braki do domkniecia w kolejnych fazach

Faza B powinna zdefiniowac kontrakt artefaktow i klas dowodu:

- `market_outcome_class`,
- `execution_verification_class`,
- `truth_gap_class`,
- `buy_quality_class`.

Faza C powinna zrobic pelny inventory skryptem, bo obecny Stage A scan byl tylko
bounded discovery.

Faza D powinna utworzyc nowy smoke config:

- `configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml`

z:

- `entry_mode = "shadow_only"`,
- `execution_mode = "shadow"`,
- `trigger.shadow_run.enabled = true`,
- `trigger.shadow_run.emit_event_bus = true`,
- `seer.funding_lane_mode = "disabled"`,
- dedykowanym namespace `shadow-burnin-p37-lifecycle-smoke`.

Faza E powinna uruchomic:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-p37-lifecycle-smoke.toml \
  --all-sessions \
  --output logs/shadow_run/shadow-burnin-p37-lifecycle-smoke/shadow_onchain_lifecycle_report.jsonl
```

Dla historycznych artefaktow trzeba uzyc odpowiadajacego configu albo
odtworzonego configu, z zachowaniem dataset segmentation.

## 15. Acceptance checklist

- [x] Wskazano konkretne pliki i funkcje.
- [x] Zmapowano runtime flow.
- [x] Zmapowano `config -> Gatekeeper BUY -> trigger.shadow_run -> shadow entry -> lifecycle -> on-chain truth report`.
- [x] Wypisano emitowane artefakty.
- [x] Wypisano join keys: `candidate_id`, `position_id`, `pool_id`,
  `mint_id` / `base_mint`, `ab_record_id`, `join_key`, `idempotency_key`,
  `rollout_profile`, `config_hash`, `decision_plane`.
- [x] Opisano `scripts/shadow_onchain_lifecycle_report.py`.
- [x] Wskazano braki i czesciowa dostepnosc historycznych artefaktow na obecnym VPS.
- [x] Rozdzielono shadow simulation, shadow-onchain validated i live inclusion.
- [x] Wskazano config path dla P3.7.6A smoke runu oraz fakt, ze plik jeszcze nie istnieje.
- [x] Nie zmieniono active policy.

## 16. Decyzja po Fazie A

Stage A jest wykonany.

Go do Fazy B/C:

- Faza B: artifact and evidence contract, z osobnymi klasami market/execution/truth-gap/buy-quality.
- Faza C: inventory script dla obecnych i opcjonalnie PC-supplied historycznych artefaktow.

No-go dla:

- Phase B feature prototype,
- P2,
- live,
- active policy change,
- FSC active gate,
- mieszania R10/R11/R13 primary-only truth z historical shadow-burnin lifecycle truth bez dataset segmentation.
