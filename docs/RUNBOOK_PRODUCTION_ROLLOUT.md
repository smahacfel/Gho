# Production Rollout Runbook

Ten runbook jest autorytatywną procedurą dla canonical shadow burn-in i późniejszego dual mikro-live. Nie uruchamiaj rolloutu ręcznie poza tą sekwencją.

## Wymagania

- wybrany profil rolloutowy z `configs/rollout/` ma jawne:
  - `[execution].execution_mode`
  - `[trigger].entry_mode`
  - `[durability].wal_dir`
  - `[durability].snapshot_dir`
- aktywny baseline revision stamp w `.ghost/baseline_accepted_revision`
- lokalny `.env` albo procesowe env dostarcza wszystkie sekrety runtime opisane w `docs/SECRET_HYGIENE_AND_ROLLOUT_PROFILES.md`
- keypair jest poza tracked state repo lub w ignorowanym katalogu `wallets/`
- funding wallet ma saldo większe niż `emergency_floor_sol + position_size_buffer_sol + max_position_size_sol`

## Mapa konfiguracji

- `configs/rollout/shadow-burnin.toml` - launcherowy SSOT dla pierwszego canonical shadow runu: `execution_mode`, `entry_mode`, wallet, RPC, rozmiar pozycji, sciezki artefaktow i porty.
- `ghost-brain/ghost_brain_config.toml` - progi runtime Guardian/AEM dla prowadzenia i zamykania pozycji shadow (`[post_buy_guardian]`, `[post_buy_guardian.aem]`).
- `off-chain/components/trigger/src/revolver_integration.rs` - obecny domyslny ladder targetow shadow jest jeszcze code-defined, nie operator-configurable: `25% @ 2x`, `25% @ 3x`, `50% @ 5x`, `time_stop = 20 min`.
- `live_exit_take_profit_pct` / `live_exit_stop_loss_pct` z launcherowego configu dotycza tylko sciezki live SELL i nie steruja canonical shadow runtime.

## 1. Preflight

1. Zatwierdź rollout revision po zielonym baseline:
   ```bash
   mkdir -p .ghost
   cargo test --workspace --no-run
   printf '%s\n' "$(git rev-parse HEAD)" > .ghost/baseline_accepted_revision
   ```
2. Uruchom preflight:
   ```bash
   cp .env.example .env
   $EDITOR .env
     ./scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/shadow-burnin.toml
   ```
   Minimalny komplet sekretow dla `shadow-burnin`: `GHOST_SEER_GRPC_ENDPOINT`, `GHOST_SEER_GRPC_X_TOKEN`, `GHOST_SEER_RPC_ENDPOINT`, `GHOST_TRIGGER_RPC_URL`, `GHOST_TRIGGER_KEYPAIR_PATH`, `GHOST_TRIGGER_SHADOW_RPC_URL`.
3. Nie startuj procesu, jeśli którykolwiek check zwróci `[fail]`.

## 2. Start

1. Uruchom launcher z jawnie wskazanym configiem:
   ```bash
    cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml
   ```
2. Potwierdź w logach:
   - `Runtime durability profile resolved`
   - `ShadowLedger restored from disk snapshot` albo świadomy `Snapshot durability disabled`
   - `WAL replay complete`
   - `Runtime recovery complete`
3. Potwierdź metryki z [`docs/RUNBOOK_HOT_PATH_METRICS.md`](/root/Gho/docs/RUNBOOK_HOT_PATH_METRICS.md):
   - `runtime_durability_mode`
   - `shadow_ledger_restore_duration_ms`
   - `wal_replay_duration_ms`
   - `runtime_recovery_mode`

## 3. Stop

1. Dla canonical `shadow-burnin` nie używaj `paper_burnin_closeout_guard.py`; ten guard pozostaje wyłącznie dla legacy `paper-burnin`.
2. Przed wysłaniem `SIGINT` upewnij się, że bieżący run nie ma oczekiwanych jeszcze shadow closeoutów, albo świadomie zaakceptuj, że formalny raport może zwrócić `shadow_lifecycle_complete = failed`.
3. Zachowaj końcowy snapshot metryk **przed** shutdownem:
   ```bash
   curl -fsS http://127.0.0.1:9090/metrics > /root/Gho/logs/rollout/shadow-burnin/metrics.prom
   ```
4. Dopiero teraz wyślij `SIGINT` (`Ctrl+C`) do procesu launchera.
5. Czekaj na:
   - `Shutdown signal received`
   - `Ghost Launcher shutdown complete`
6. Nie zabijaj procesu `SIGKILL`, dopóki graceful shutdown jest żywy.
7. Jeżeli świadomie zatrzymasz canonical shadow run przed domknięciem pozycji, późniejszy raport formalny może prawidłowo zwrócić `shadow_lifecycle_complete = failed`; taki wynik jest traktowany jako błąd operacyjnego closeoutu, nie jako podstawa do osłabiania kontraktu raportu.
8. Jeśli uruchamiasz legacy `paper-burnin`, wtedy przed `SIGINT` użyj:
   ```bash
   python3 /root/Gho/scripts/paper_burnin_closeout_guard.py \
     --config /root/Gho/configs/rollout/paper-burnin.toml
   ```

## 4. Restart

1. Zrób ponownie pełny preflight.
2. Uruchom ten sam config i ten sam profil rolloutu.
3. Potwierdź recovery:
   - snapshot watermark jest załadowany,
   - replay WAL kończy się bez błędu,
   - `runtime_recovery_mode` jest zgodny z oczekiwanym profilem (`snapshot_plus_wal` dla normalnego restartu).

## 5. Recovery Check

Po restarcie sprawdź:

- katalog WAL rośnie i jest zapisywalny,
- katalog snapshotów zawiera aktualne pliki i rotację,
- `runtime_recovery_watermark_ms` ma sensowną wartość,
- brak `eventbus_lag_total`,
- brak wzrostu `provider_stall_total`,
- brak safety rejectionów, których operator nie rozumie.

Zachowaj też snapshot metryk do raportu końcowego burn-in:

```bash
curl -fsS http://127.0.0.1:9090/metrics > /root/Gho/logs/rollout/shadow-burnin/metrics.prom
```

Jeżeli którykolwiek z tych punktów nie jest spełniony, rollout wraca do stanu `abort`.

## 6. Burn-in Session Report

Po zakończeniu sesji wygeneruj formalny raport go/no-go:

  ```bash
  python3 /root/Gho/scripts/shadow_run_report.py \
  --config /root/Gho/configs/rollout/shadow-burnin.toml \
  --metrics-text /root/Gho/logs/rollout/shadow-burnin/metrics.prom
  ```

Opcjonalnie można jawnie poluzować economics floor, jeżeli operator chce dopuścić mały kontrolowany ujemny wynik netto jako niekatastrofalny:

  ```bash
  python3 /root/Gho/scripts/shadow_run_report.py \
  --config /root/Gho/configs/rollout/shadow-burnin.toml \
  --metrics-text /root/Gho/logs/rollout/shadow-burnin/metrics.prom \
  --min-net-pnl-sol -0.001
  ```

Jeżeli `--min-net-pnl-sol` **nie** jest podany jawnie, raport domyślnie wyprowadza economics floor z `[trigger].position_size_buffer_sol` i traktuje wynik do `-position_size_buffer_sol` jako niekatastrofalny względem rollout wallet budget. To chroni przed fałszywym `NO-GO` wywołanym przez mikroujemny synthetic paper PnL oderwany od realnego safety buffer walleta.

Raport musi spiąć:

Artefakty Gatekeeper verdict są pochodną `[oracle].decision_log_path`; nie wolno ich szukać ani
walidować pod historycznym `logs/decisions.json/...`, jeśli aktywny rollout wskazuje inny root.

- `logs/rollout/shadow-burnin/decisions/gatekeeper_v2_buys.jsonl`,
- `logs/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl`,
- `logs/shadow_run/shadow-burnin*` oraz `logs/shadow_run/shadow-burnin/*`,
- `datasets/events/shadow-burnin/*`,
- log systemowy,
- snapshot metryk hot-path.

Interpretacja wyniku:

- exit code `0` = `GO`,
- exit code `2` = `NO-GO`.

` safety_violations ` w raporcie obejmuje tylko jawne bulkhead violations / safety rejections widoczne w metrykach i logach. Ocena, czy safety odrzuca poprawne setupy z powodu driftu configu, pozostaje obowiązkową oceną operatora.

Nie wolno przejść do `dual-micro-live`, jeżeli raport kończy się `NO-GO`.

### FSC authoritative-lane bake package

**Status 2026-05-16:** ten bake jest wstrzymany dla obecnego providera przez
`docs/ADR/ADR-0130-v3-fsc-scope-decision-single-stream.md`. Obecny endpoint pozwala tylko na jeden
stream, więc dedicated `full_chain` lane konkuruje z primary streamem i może uniemożliwić zebranie
decision rows. Poniższa procedura pozostaje historycznym runbookiem / future-only, a nie aktywnym
krokiem V3.

Ten bake pozostaje **data-plane only**. Nie zmieniaj:

- `soft_penalty_high_fsc`,
- `soft_penalty_high_fsc_high_cpv_combo`,
- `enable_sybil_combo_veto`.

W trackowanym repo wszystkie te guardraile pozostają zamrożone (`0`, `0`, `false`).

1. **Neutral replay diff (lane disabled)**
   - Jeśli potrzebujesz neutral-disabled control artifact, zrób lokalną kopię committed `shadow-burnin.toml` i zmień wyłącznie lane switch z powrotem na `disabled`.
   - Przygotuj dwa porównywalne artefakty `gatekeeper_v2_buys.jsonl` z tego samego wejścia replay i z tym lokalnym disabled configiem.
   - Uruchom:
     ```bash
     python3 /root/Gho/scripts/fsc_replay_diff.py \
       --mode neutral-disabled \
       --baseline /path/to/baseline_gatekeeper_v2_buys.jsonl \
       --candidate /path/to/candidate_gatekeeper_v2_buys.jsonl
     ```
   - Wynik `PASS` oznacza: zero verdict drift i zero driftu w `funding_source_concentration` / `sybil_metric_degraded_reasons`.

2. **Authoritative shadow-burnin config**
   - `configs/rollout/shadow-burnin.toml` jest teraz committed profilem operatorskim dla authoritative FSC bake i canonical standalone shadow runtime.
   - Trackowany profil już zawiera:
      ```toml
      [seer]
      funding_lane_mode = "full_chain"
      ```
   - Nie twórz dodatkowej tymczasowej kopii tylko po to, żeby włączyć lane.

3. **Authoritative shadow-burnin run**
   - Start:
      ```bash
      cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml
      ```
   - W trakcie runu potwierdź metryki z `docs/RUNBOOK_HOT_PATH_METRICS.md`:
     - `ghost.pump.*{source_label="grpc_funding_lane_full_chain"}`
     - `seer_funding_transfer_observations_total{lane=...,coverage=...}`
     - `fsc_authoritative_funding_stream_available`
     - `fsc_warmup_ready`
      - `fsc_coverage_window_ready`
      - `fsc_coverage_window_remaining_ms`
      - `fsc_authoritative_buy_gate_open`
     - `fsc_lookup_hit_rate`
     - `fsc_lookup_hits_total` / `fsc_lookup_misses_total`
     - Zachowaj artefakty:
       - `logs/rollout/shadow-burnin/decisions/gatekeeper_v2_buys.jsonl`
       - `logs/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl`
       - `logs/rollout/shadow-burnin/metrics.prom`
       - formalny raport `shadow_run_report.py`

4. **Authoritative replay diff**
   - Porównaj neutralny artefakt replay z artefaktem replay, w którym authoritative lane był jawnie włączony:
     ```bash
     python3 /root/Gho/scripts/fsc_replay_diff.py \
       --mode authoritative-enabled \
       --baseline /path/to/neutral_gatekeeper_v2_buys.jsonl \
       --candidate /path/to/authoritative_gatekeeper_v2_buys.jsonl
     ```
   - Wynik `PASS` oznacza:
     - zero verdict drift,
     - brak driftu poza powierzchnią `FSC`,
     - jeśli występuje drift, dotyczy on wyłącznie `funding_source_concentration` i/lub `FSC_*` w `sybil_metric_degraded_reasons`.

5. **Go/no-go gate przed ewentualnym future FSC policy follow-up**
   - `fsc_authoritative_funding_stream_available` jest stabilne,
   - `fsc_warmup_ready` realnie osiąga `1`,
   - `fsc_lookup_hit_rate` i surowe hit/miss counters są akceptowalne,
   - replay diff pokazuje expected-only drift,
   - `FSC_FUNDING_STREAM_UNAVAILABLE` przestaje dominować tam, gdzie lane był świadomie włączony.

## 7. Rollback / Abort

Abort jest obowiązkowy, gdy wystąpi co najmniej jedno z poniższych:

- preflight nie przechodzi,
- `runtime_durability_mode` nie jest oczekiwanym profilem,
- `WAL replay failed` albo `ShadowLedger restore failed`,
- `runtime_recovery_mode=cold_start` przy oczekiwanym restarcie z durability,
- `eventbus_lag_total` rośnie,
- provider circuit breaker przechodzi w `open`,
- podczas authoritative bake `fsc_authoritative_funding_stream_available` pozostaje `0` albo flappuje bez wyjaśnienia,
- podczas authoritative bake `fsc_warmup_ready` nie osiąga `1` mimo realnej próbki authoritative funding transferów,
- podczas authoritative bake replay diff pokazuje verdict drift albo drift wykraczający poza powierzchnię `FSC`,
- saldo spada do `emergency_floor_sol`,
- safety odrzuca BUY z powodów, których operator nie potrafi wyjaśnić,
- divergence shadow/paper/live wygląda na semantyczną, nie losową.

Procedura abort:

1. Zatrzymaj proces graceful shutdownem.
2. Zachowaj bieżące logi i artefakty eventów.
3. Nie zmieniaj ręcznie WAL ani snapshotów.
4. Oznacz rollout jako `aborted`, dopóki nie ma wyjaśnienia przyczyny.

## 8. Kill Switch

Natychmiastowe wyłączenie jest wymagane przy:

- `runtime_recovery_mode` niezgodnym z oczekiwanym kontraktem,
- utracie reachability RPC/gRPC,
- powtarzającym się `pending_curve` timeout,
- nieoczekiwanym realnym BUY w profilu shadow/paper,
- każdym symptomie, że launcher działa bez aktywnego bulkheada albo bez aktywnego durability.

## 9. Profile i granice faz

- `configs/rollout/shadow-burnin.toml` jest canonical profilem do użycia po PR-6.
- `configs/rollout/paper-burnin.toml` pozostaje legacy profilem kompatybilnościowym / compare-only.
- `configs/rollout/dual-micro-live.toml` pozostaje przygotowany, ale nie może być uruchamiany przed formalnym GO po shadow-burnin.
- `configs/rollout/future-live.toml` pozostaje przygotowany, ale nie może być uruchamiany przed domknięciem PR-7.
- Trackowany `config.toml` jest tylko bezpiecznym szablonem referencyjnym, nie nośnikiem sekretów produkcyjnych.
- `configs/rollout/shadow-burnin.toml` i `configs/rollout/paper-burnin.toml` uruchamiają `seer.funding_lane_mode = "full_chain"`.
- Pod obecnym single-stream provider constraint profile `full_chain` są paused/future-only dla FSC.
  Dla bieżącej walidacji V3 używaj primary-only profili z `funding_lane_mode = "disabled"` zgodnie
  z `ADR-0130`.
- `config.toml`, `configs/rollout/dual-micro-live.toml` i `configs/rollout/future-live.toml` pozostają na `funding_lane_mode = "disabled"`.
- Jeśli potrzebujesz lane-disabled control artifact albo rollbacku bake, wyprowadź lokalną kopię `shadow-burnin.toml` z pojedynczym flipem `seer.funding_lane_mode = "disabled"`; nie cofaj PR1–PR3, nie ruszaj persisted state i nie „naprawiaj” bake przez włączanie FSC penalty.
