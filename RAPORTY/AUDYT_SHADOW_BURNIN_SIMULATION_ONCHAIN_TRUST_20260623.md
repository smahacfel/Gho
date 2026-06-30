# Audyt shadow burn-in simulation vs on-chain trust - 2026-06-23

Status: FINAL / AUDIT REPORT
Autor/Agent: Codex
Repo/branch: `/root/Gho` / `backup/pre-refactor-evidence-contract-20260619`
Zakres: aktualna sciezka shadow burn-in simulation, R45/R46 artefakty, przydatnosc do metric-segment discovery i selector edge analysis
Tryb: audit/report only; bez zmian w runtime

## 1. Werdykt

Shadow burn-in simulation jest uzyteczna jako warunkowe zrodlo danych badawczych do metric-segment discovery i selector edge analysis, ale nie jest dowodem realnego on-chain inclusion ani pelnej wykonalnosci live.

Krotka odpowiedz:

- GO warunkowy dla eksploracyjnego metric-segment discovery, jezeli kazdy rekord ma jawna klase zaufania, `truth_source`, `truth_status`, truth-gap, drift, run scope i separacje active shadow vs p37 probe.
- GO warunkowy dla selector edge analysis, ale tylko po odfiltrowaniu rekordow uszkodzonych, nie-terminalnych, `not_dispatched`, bez canonical truth albo z duzym gapem truth.
- NO-GO dla bezposredniej promocji selector edge do aktywnej polityki bez dodatkowej walidacji temporalnej i swiezego clean runa z formalnym lifecycle proof.
- NO-GO dla traktowania shadow simulation jako dowodu, ze realny live transaction wyladowalby on-chain z tym samym czasem, fee, blockhash, account-lock state i confirmation lifecycle.

Najwazniejszy wniosek: obecny shadow jest blizej "RPC bank simulation + canonical observed market truth replay" niz "realny live execution simulator". To wystarcza do ostroznej analizy separowalnosci segmentow, ale nie wystarcza do dowodzenia produkcyjnej wykonalnosci.

## 2. Zakres i non-goals

In scope:

- `trigger.shadow_run` entry simulation i dispatch lifecycle.
- Post-buy shadow lifecycle i TimeStop V2 truth path.
- Launcher/canary/report gates dla selector lifecycle runs.
- Artefakty R45 i R46 z `reports/selector/`.
- `shadow_onchain_lifecycle_report.py` jako most shadow lifecycle vs canonical on-chain/account-state truth.
- Ryzyko uzycia danych shadow do metric-segment discovery i selector edge analysis.

Out of scope:

- Implementacja napraw.
- Zmiany Gatekeeper policy, scoringu, TX buildera, Helius Sendera, sender retry albo live execution.
- Ocena, czy obecne progi selektora sa dobre ekonomicznie.
- Zewnetrzne chain query poza lokalnymi artefaktami i reporterami.

## 3. Klasa zaufania danych

| Klasa | Co oznacza | Czy nadaje sie do discovery | Czy nadaje sie do promocji edge |
|---|---|---:|---:|
| A: strict lifecycle proof | Launcher `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`, canary PASS, clean JSONL, terminal lifecycle, canonical truth | TAK | Tylko po walk-forward/leakage validation |
| B: raw lifecycle telemetry | Sa lifecycle rowy i canonical truth, ale brak pelnego launcher proof albo report NO-GO | TAK, exploratory only | NIE |
| C: p37 shadow probe | Counterfactual/offline probe plane, nie aktywny BUY lifecycle | TAK, osobno | NIE bez osobnej kalibracji |
| D: zero-BUY event canary | Event ingest proof bez BUY lifecycle proof | Tylko ingest diagnostics | NIE |
| E: corrupt/incomplete | bad JSON, missing terminal, unresolved truth, not dispatched | NIE dla strict | NIE |

Aktualna ocena artefaktow:

- R45 ma pozniejsze raw lifecycle rowy i male probki porownawcze, ale znaleziony launcher proof z `20260621T205038Z` byl zero-BUY/event-only, wiec nie wolno go traktowac jako formalnego lifecycle proof. R45 raw data: klasa B, maly exploratory sample.
- R46 ma duzo danych, ale brak znalezionego formalnego `RUN_LIFECYCLE_LAUNCHER_REPORT`, `shadow_run_report` daje NO-GO, a lifecycle JSONL ma uszkodzone linie. R46: klasa B/E, mozliwe exploratory po kwarantannie uszkodzonych i niekompletnych rekordow.
- p37 probe data ma oddzielna wartosc discovery, ale musi pozostac klasa C i nie moze byc laczone z active shadow BUY lifecycle bez pola `artifact_plane` / `probe_plane`.

## 4. Jak blisko on-chain reality jest obecny shadow

### Entry / buy simulation

Kod wykonuje rzeczywiste RPC `simulate_transaction_with_config` na zbudowanej transakcji buy:

- `ghost-launcher/src/components/trigger/shadow_run.rs:1175` wywoluje `rpc.simulate_transaction_with_config(&request.rpc_buy_tx, sim_config)`.
- Konfiguracja symulacji ustawia `replace_recent_blockhash`, `commitment`, timeout i requested account output.
- `ghost-launcher/src/components/trigger/component.rs:1784` pokazuje, ze shadow run jest wlaczany tylko gdy `shadow_run.enabled` i entry mode jest `shadow_only` albo `live_and_shadow`; audytowane R45/R46 wrappery sa shadow-only.

To daje realna walidacje against RPC bank state: program execution, account metas, slippage/account constraints i runtime errors moga wyjsc juz na symulacji.

To nie daje:

- realnego submitu,
- inclusion/landing confirmation,
- priority fee competition,
- account-lock contention w przyszlym slotcie,
- blockhash expiry lifecycle,
- realnego retry/reconciliation,
- gwarancji, ze wynik entry price bylby identyczny w live landing slot.

Dlatego entry simulation jest dobra do odrzucania oczywiscie niewykonalnych transakcji, ale nie jest dowodem "would have landed".

### Post-buy lifecycle / exit labels

Post-buy shadow lifecycle nie jest realna pozycja live. Jest syntetyczna pozycja po shadow entry, ktorej pozniejsze ceny i close labels sa powiazane z obserwowanym canonical market/account state:

- `ghost-brain/src/guardian/post_buy/engine.rs:769` definiuje `ShadowLifecycleRecord`.
- `ghost-brain/src/guardian/post_buy/engine.rs:748` zawiera `entry_landed_slot_source`.
- Testy runtime potwierdzaja uzycie `truth_source = canonical_account_state_snapshot` (`engine.rs:6112`, `engine.rs:6242`).
- `entry_landed_slot_source` w testach moze byc syntetyczny: `synthetic_next_slot_after_entry_simulation_rpc_slot` (`engine.rs:4819`, asercja `engine.rs:4874`).

To oznacza, ze exit/path labels sa bliskie obserwowanej market truth wtedy, gdy truth jest canonical/resolved i gap jest maly. Nie oznacza to jednak, ze realny live entry zostalby wypelniony w tej samej cenie ani ze realny exit transaction zostalby wykonany.

### On-chain lifecycle reporter

`scripts/shadow_onchain_lifecycle_report.py` tworzy dataset `shadow_burnin_lifecycle_onchain` (`line 38`) i mierzy truth gaps oraz drift:

- entry/exit truth-gap filters sa w `line 1343` i `line 1468`;
- drift summary jest emitowany w okolicach `line 1710`;
- coverage i summary lines sa w `line 2021-2077`.

To jest najwazniejszy element, ktory robi shadow przydatnym do discovery: raport nie udaje live inclusion, tylko mierzy odleglosc shadow labels od canonical observed/executable truth.

## 5. Artefakty i wyniki

### R45

Znaleziony launcher report:

- `reports/selector/shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260621T205038Z/RUN_LIFECYCLE_LAUNCHER_REPORT.md`
- `status=PASS`
- `claim=SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
- `run_state=RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`

Ten PASS nie jest lifecycle proof. To jest event-ingest proof z jawna zgoda na zero-BUY lifecycle. Canary proof dla tego scope mial:

- `shadow_buys_delta=0`
- `shadow_entries_delta=0`
- `shadow_lifecycle_delta=0`
- `legacy_buy executable=0`
- `shadow_dispatch closed=0`
- `position_closed=0`
- `exit_filled=0`
- `truth_status=resolved=0`
- `truth_source=canonical_account_state_snapshot=0`

Pozniejszy raw log scan pokazal jednak lifecycle telemetry:

- `shadow_buys`: 23 rows, `bad_json=0`
- `shadow_entries`: 23 rows, `bad_json=0`
- `shadow_lifecycle`: 835 rows, `bad_json=0`
- lifecycle `record_type`: `time_stop_v2_window=772`, `shadow_dispatch=23`, `position_closed=20`, `exit_filled=19`, `exit_blocked=1`
- dispatch status: `closed=21`, `failed=2`
- truth: `canonical_account_state_snapshot=812`, `resolved=810`, `failure=2`

`shadow_onchain_lifecycle_report.py` dla R45:

- `rows_written=14`
- `close_truth_coverage=14/14 failed=0 pct=100.00`
- `entry_drift_pct`: mean `-0.288895`, median `0`, `p95_abs=22.245354`
- `exit_drift_pct`: mean `-0.000025`, median `-0.000018`, `p95_abs=0.000082`
- `entry_truth_gap_ms`: mean `3260.642857`, median `1159`, `p95_abs=12499`
- `exit_truth_gap_ms`: mean `10836.642857`, median `2.5`, `p95_abs=30732`
- skipped: `missing_position_closed=1`

Interpretacja R45: bardzo mala probka, dobra jakosciowo po stronie JSONL i exit drift, ale formalnie nie wolno jej zamieniac w strict lifecycle proof, bo znaleziony launcher PASS byl zero-BUY/event-only.

### R46

Znaleziony static guard:

- `reports/selector/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260622T105102Z/static_guard/RESTORE_LIFECYCLE_GUARD.md`
- `status=PASS`
- `claim=RESTORE_PATH_STATIC_GUARD_PASS`

Nie znaleziono formalnego `RUN_LIFECYCLE_LAUNCHER_REPORT` dla R46. Static guard nie jest lifecycle proof.

Raw log scan R46:

- `shadow_buys`: 3494 rows, `bad_json=0`
- `shadow_entries`: 3457 rows, `bad_json=1`
- `shadow_lifecycle`: 57717 rows, `bad_json=12`
- lifecycle `record_type`: `time_stop_v2_window=47960`, `shadow_dispatch=3487`, `position_closed=3128`, `exit_filled=3122`, `exit_blocked=8`
- dispatch status: `closed=3133`, `not_dispatched=188`, `failed=166`
- truth: `canonical_account_state_snapshot=54218`, `resolved=54181`, `failure=37`

Bad JSON samples wskazuja na realna korupcje JSONL: sklejone obiekty, linie przeciete w srodku rekordu i bledy typu `Extra data`, `Expecting ':' delimiter`, `Expecting ',' delimiter`. Kod appenduje JSON i newline osobnymi write calls:

- `ghost-brain/src/guardian/post_buy/engine.rs:1461-1468`
- analogiczny shadow dispatch writer: `ghost-launcher/src/components/trigger/shadow_run.rs:1030`

To nie dowodzi samodzielnie root cause, ale jest spojne z ryzykiem braku single-writer/global lock dla wielozadaniowego JSONL.

`shadow_onchain_lifecycle_report.py` dla R46:

- `rows_written=2188`
- `close_truth_coverage=2188/2196 failed=8 pct=99.64`
- `entry_drift_pct`: mean `9.580745`, median `0`, `p95_abs=46.974389`
- `exit_drift_pct`: mean `-0.149888`, median `-0.000015`, `p95_abs=0.000049`
- `entry_truth_gap_ms`: mean `11482.347806`, median `6931`, `p95_abs=38353`
- `exit_truth_gap_ms`: mean `27489.952468`, median `30294`, `p95_abs=57498`
- skipped: `missing_position_closed=253`, `close_truth_not_resolved=8`

`shadow_run_report.py` dla R46 dal `NO-GO`:

- `bad_lifecycle_rows=12`
- `runtime_lifecycle_complete=false`
- `dispatch_without_lifecycle=6`
- `dispatch_without_terminal_lifecycle=127`
- `no_dispatch=1519`
- `inflight=7`
- `trace_correlation=false`
- `mandatory_artifacts=false`
- `recovery_contract=false`
- `economics_not_fatal=false`
- `no_live_side_effects=true`

Interpretacja R46: bardzo uzyteczny exploratory dataset po kwarantannie, ale aktualnie nie jest clean strict dataset. Entry drift/gap sa materialne i musza wejsc jako cecha jakosci labela, nie byc ignorowane.

## 6. Findings

### F1 - HIGH - Shadow simulation nie jest live inclusion proof

RPC simulation potwierdza, ze transakcja moze przejsc symulacje w bank state, ale nie potwierdza ladowania, fee competition, account locks, retry, confirmation ani reconciliation. Kazdy raport selector edge, ktory traktuje `shadow_success` albo `shadow_dispatch=closed` jako "buy landed", zawyza zaufanie.

Wplyw: shadow nadaje sie do research labels, nie do twierdzenia o live execution readiness.

### F2 - HIGH - Formalny lifecycle trust zalezy od launcher/canary proof

Runbook i canary wymagaja dodatnich delt dla `shadow_buys`, `shadow_entries`, `shadow_lifecycle`, zamknietego dispatch, `position_closed`, `exit_filled`, `truth_status=resolved`, `truth_source=canonical_account_state_snapshot` i `final_pnl_pct`. `scripts/check_selector_lifecycle_canary.py:241-268` egzekwuje te warunki.

R45 zero-BUY PASS jest jawnie event-only. R46 ma static guard PASS, ale to tez nie lifecycle proof.

Wplyw: raporty discovery musza rozdzielac "run started / event proof" od "strict lifecycle dataset".

### F3 - HIGH - R46 nie jest czystym formalnym datasetem

R46 ma duzo danych i wysoka close truth coverage po filtrach, ale ma:

- brak znalezionego launcher lifecycle proof,
- `bad_json=12` w lifecycle,
- `bad_json=1` w entries,
- `shadow_run_report` NO-GO,
- 127 dispatch bez terminal lifecycle,
- 1519 no-dispatch/trace-correlation gaps.

Wplyw: R46 moze karmic exploratory segment mining, ale strict analysis musi najpierw odrzucic/oznaczyc uszkodzone i niekompletne rekordy.

### F4 - MEDIUM/HIGH - Active shadow BUY lifecycle i p37 probe plane sa rozne

R45/R46 maja wlaczony `p37_shadow_probe`. Probe data jest counterfactual/offline i moze byc cenne dla discovery, ale nie jest active BUY proof. Komentarze w wrapperach wskazuja, ze labels dla target/stop/time musza byc materializowane offline z canonical R2 market paths, nie z post-buy runtime time-stop.

Wplyw: mieszanie p37 probe z active lifecycle bez `artifact_plane` grozi leakage i falszywym edge.

### F5 - MEDIUM - On-chain closeness jest dobra dla exit quote, slabsza dla entry timing

R45/R46 maja bardzo niski `exit_drift_pct` p95, ale entry truth gaps i entry drift sa istotne:

- R45 `entry_drift_pct p95_abs=22.245354`, `entry_truth_gap_ms p95_abs=12499`
- R46 `entry_drift_pct p95_abs=46.974389`, `entry_truth_gap_ms p95_abs=38353`
- R46 `exit_truth_gap_ms p95_abs=57498`

Wplyw: selector edge analysis musi segmentowac po `entry_truth_gap_ms`, `exit_truth_gap_ms` i drift class. Bez tego discovery moze uczyc sie artefaktow opoznionej truth, nie realnego edge.

### F6 - MEDIUM - JSONL integrity jest warunkiem dataset trust

R46 pokazuje uszkodzone JSONL rows. Dla analizy segmentow `bad_json>0` nie jest kosmetyka: moze gubic terminal rows, laczyc rekordy z roznych pooli albo przerywac correlation chain.

Wplyw: strict dataset contract musi wymagac `bad_json=0`, albo jawnej naprawy z provenance i lista odrzuconych offsetow/linii.

### F7 - MEDIUM - Ekonomia runa i discovery to rozne bramki

`shadow_run_report.py` dla R45 i R46 zwraca NO-GO przez braki artefaktow/recovery/economics. To nie kasuje wartosci danych do discovery, ale blokuje uzycie jako dowodu gotowosci strategii.

Wplyw: wnioski z discovery musza byc opisane jako "candidate signals", nie jako "production selector edge".

## 7. Minimalny kontrakt datasetu do selector edge analysis

Kazdy rekord uzywany do strict analysis powinien miec albo byc joinowalny do:

- `scope` / rollout profile,
- config path i najlepiej hash configu,
- commit HEAD albo launcher report HEAD,
- `run_lifecycle_guard` path i claim,
- `pool_amm_id`, mint, join key, idempotency key,
- `artifact_plane`: `active_shadow_lifecycle` albo `p37_counterfactual_probe`,
- `dispatch_status` i terminal lifecycle status,
- `truth_status` i `truth_source`,
- `entry_landed_slot_source`,
- `entry_truth_gap_ms`, `exit_truth_gap_ms`,
- `entry_drift_pct`, `exit_drift_pct`,
- `bad_json=0` dla calego input file albo quarantine metadata,
- final label fields: close reason, final pnl, target/stop/time-stop class,
- explicit exclusion flags: `not_dispatched`, `failed`, `missing_position_closed`, `close_truth_not_resolved`, `future_only_truth`, `max_truth_gap_exceeded`.

Suggested trust filters for strict analysis:

- include only `truth_status=resolved`;
- include only `truth_source=canonical_account_state_snapshot`;
- exclude `bad_json` files unless repaired with provenance;
- exclude `dispatch_status in [failed, not_dispatched]` for active lifecycle labels;
- exclude rows without `position_closed` and `exit_filled` where final PnL is required;
- split, do not merge, active lifecycle and p37 probe;
- bucket or cap by truth gap before computing edge.

## 8. Recommended workflow dla metric-segment discovery

1. Najpierw zbudowac inventory runow z klasami A/B/C/D/E.
2. Dla strict lifecycle datasetow uzywac tylko klasy A.
3. Dla exploratory mining dopuscic klase B/C, ale wszystkie wyniki oznaczac jako hypotheses.
4. Dla kazdego segmentu raportowac support, temporal split stability, drift/gap buckets i oddzielnie active-vs-probe.
5. Uruchomic anti-leakage checks: zrodla cech musza istniec decision-time, a labels moga uzywac przyszlosci tylko jako outcome.
6. Weryfikowac candidate edge na nowym clean runie, najlepiej z launcher lifecycle proof i `bad_json=0`.
7. Dopiero po tym rozwazac Gatekeeper/selector policy changes.

## 9. Odpowiedz na pytanie glowne

Czy obecna shadow simulation jest godna zaufania do metric-segment discovery / selector edge analysis?

Tak, ale tylko jako kontrolowane, klasowane zrodlo telemetryczne. Najsilniejsza czesc to:

- RPC transaction simulation dla entry failure/success,
- canonical account-state truth dla lifecycle/exit labels,
- reporter mierzacy truth gap i drift,
- brak live side effects w raportach.

Najslabsza czesc to:

- brak realnego inclusion/fee/account-lock proof,
- formalne proof gaps dla R45/R46,
- R46 JSONL corruption,
- duze entry truth gaps/drift,
- ryzyko mieszania active lifecycle z p37 probe.

Najbezpieczniejsza praktyczna decyzja: uzywac aktualnych danych do discovery i hipotez, ale nie uzywac ich samych do zatwierdzenia edge. Dla promotion readiness wymagany jest co najmniej jeden swiezy, clean, formalny lifecycle proof run plus walidacja temporalna i anti-leakage.

## 10. Walidacja wykonana

Read-only/code checks:

- `rg`/`sed` dla `ghost-launcher/src/components/trigger/shadow_run.rs`
- `rg`/`sed` dla `ghost-launcher/src/components/trigger/component.rs`
- `rg`/`sed` dla `ghost-brain/src/guardian/post_buy/engine.rs`
- `rg` dla `scripts/check_selector_lifecycle_canary.py`
- `rg` dla `scripts/shadow_onchain_lifecycle_report.py`

Script/test checks:

- `python3 -m py_compile scripts/shadow_onchain_lifecycle_report.py scripts/shadow_run_report.py scripts/check_selector_lifecycle_canary.py scripts/start_selector_lifecycle_run.py` - PASS.
- `python3 -m unittest scripts/test_shadow_onchain_lifecycle_report_contract.py scripts/test_shadow_run_report.py -v` - PASS, 13 tests.
- `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher p5_shadow_dispatch_lifecycle_writes_closed_with_idempotency_join_key_rollout_profile -- --nocapture` - PASS, 1 test.
- `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain shadow_runtime_time_stop_uses_currently_observed_canonical_state_for_quiet_pool -- --nocapture` - NOT COMPLETED; compilation failed with `No space left on device` before test execution. Temporary target dir was removed after approval.

Reporter runs:

- `python3 scripts/shadow_onchain_lifecycle_report.py --config configs/rollout/shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1.toml --output /tmp/r45_shadow_onchain_lifecycle_report.jsonl --outcome-summary-output /tmp/r45_shadow_onchain_lifecycle_summary.json`
- `python3 scripts/shadow_onchain_lifecycle_report.py --config configs/rollout/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1.toml --output /tmp/r46_shadow_onchain_lifecycle_report.jsonl --outcome-summary-output /tmp/r46_shadow_onchain_lifecycle_summary.json`
- `python3 scripts/shadow_run_report.py --config configs/rollout/shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1.toml --json` - NO-GO.
- `python3 scripts/shadow_run_report.py --config configs/rollout/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1.toml --json` - NO-GO.

## 11. Delegation trace

```yaml
task_classification: "audit / shadow-burnin simulation trust / selector discovery readiness"
routing_performed: true
primary_specialist: "Decision Logging Replay Analyst"
supporting_specialists_considered:
  - "Solana Execution Path Engineer"
  - "Gatekeeper Policy Auditor"
  - "Seer Ingest Event Integrity Specialist"
  - "Config Rollout Safety Reviewer"
  - "Ghost Runtime Coordinator"
specialist_docs_loaded:
  - "docs/agents/decision-logging-replay-analyst.md"
  - "docs/agents/solana-execution-path-engineer.md"
  - "docs/agents/gatekeeper-policy-auditor.md"
  - "docs/agents/seer-ingest-event-integrity-specialist.md"
  - "docs/agents/config-rollout-safety-reviewer.md"
  - "docs/agents/ghost-runtime-coordinator.md"
skills_used:
  - "ghost-execution"
  - "trading-systems"
  - "solana-pumpfun-architect"
  - "large-data-analytics"
  - "statistical-research-engine"
  - "gatekeeper-shadow-burnin-audit memory skill"
fast_path_used: false
contracts_checked:
  - "shadow/live separation"
  - "DecisionLogger / JSONL auditability"
  - "shadow dispatch lifecycle"
  - "canonical account-state truth source"
  - "launcher/canary lifecycle proof semantics"
  - "active lifecycle vs p37 counterfactual probe separation"
  - "Solana simulation vs live inclusion boundary"
unresolved_routing_uncertainty:
  - "Nie znaleziono lokalnego ADR_8D_SZABLON.md; ADR tworzony wedlug najblizszego lokalnego formatu ADR-8D."
  - "Ghost-brain targeted test nie zostal wykonany do konca przez brak miejsca na filesystemie."
```
