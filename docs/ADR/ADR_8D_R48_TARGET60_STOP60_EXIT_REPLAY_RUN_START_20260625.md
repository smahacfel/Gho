# ADR-8D: R48 target60 stop60 exit replay rollout start

Status: IMPLEMENTED / R2_RUN_STARTED
Typ: ADR-8D / rollout config and shadow research run
Data: 2026-06-25
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `research/alpha-31100-validation-harness-v1`
HEAD podczas pracy: `f618d8e8ae09858cbcaf7a2efcd8eb1017927b49`
Commit/PR: local runtime/config change, not committed at ADR creation time
Zakres: restart R48-derived shadow run z progami `+60%/-60%` i wlaczonym `shadow_exit_replay_v1`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r48_target60_stop60_exit_replay_maxwait31100_fsc_off.toml`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/run_lifecycle_guard_20260625T164401Z/runtime.log`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zatrzymac poprzedni R48 `target24/stop3` i wystartowac nowy R48-derived shadow run z progami biznesowego lifecycle `+60%/-60%`, przy wlaczonym pasywnym `shadow_exit_replay_v1`.

Warunki brzegowe:
- nie zmieniac Gatekeeper thresholds,
- nie zmieniac `v25_confidence`,
- nie zmieniac selector/alpha,
- nie zmieniac BUY/REJECT,
- nie wlaczac live execution,
- nie mieszac nowych artefaktow z katalogiem starego R48 `target24-stop3`.

## 2. Dzialania

1. Wypchnieto commit sidecara:
   - branch remote: `origin/research/alpha-31100-validation-harness-v1`,
   - HEAD: `f618d8e8ae09858cbcaf7a2efcd8eb1017927b49`.

2. Zatrzymano poprzedni R48:
   - tmux session: `r48-r38-repeat`,
   - proces: `/root/Gho/target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1.toml`,
   - zatrzymanie: `tmux kill-session -t r48-r38-repeat`.

3. Utworzono izolowany R48-derived brain config:
   - `configs/rollout/ghost_brain_selector_dataset_sampler_r48_target60_stop60_exit_replay_maxwait31100_fsc_off.toml`,
   - bazuje na R38 maxwait31100 FSC off,
   - dodaje tylko:

```toml
[post_buy_guardian.exit_replay_v1]
enabled = true
flush_on_shutdown = false
shutdown_flush_budget_ms = 3000
levels_bps = [
  -6000, -5000, -3000, -2000, -1500, -1000, -700, -500, -300, -200, -100,
  100, 200, 300, 400, 500, 700, 1000, 1500, 2000, 3000, 5000, 6000, 7500, 10000,
]
```

4. Utworzono osobny rollout config:
   - `configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2.toml`,
   - `live_exit_take_profit_pct = 0.60`,
   - `live_exit_stop_loss_pct = 0.60`,
   - nowe run/log namespace:
     `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2`.

5. Zbudowano release binary:

```bash
cargo build -p ghost-launcher --release
```

Wynik: PASS, `Finished release profile`, z istniejacymi repo-wide warningami.

6. Uruchomiono nowy run:

```bash
tmux new -d -s r48-r38-target60-exit-replay ...
```

Runtime log:
`reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/run_lifecycle_guard_20260625T164401Z/runtime.log`

Uwaga:
Pierwszy start `target60-stop60-exit-replay-r1` zostal przerwany po wykryciu, ze domyslna siatka `levels_bps` nie zawiera `+6000/-6000`. Dla r2 dodano jawne `levels_bps` z `-6000` i `6000`, aby wariant `+60%/-60%` mogl korzystac z exact `first_hit_ms`.

## 3. Walidacja startu

Potwierdzone:
- proces `ghost-launcher` zyje jako PID do weryfikacji po starcie R2,
- tmux session: `r48-r38-target60-exit-replay`,
- config runtime:
  `/root/Gho/configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2.toml`,
- brain config:
  `/root/Gho/configs/rollout/ghost_brain_selector_dataset_sampler_r48_target60_stop60_exit_replay_maxwait31100_fsc_off.toml`,
- `max_wait_ms=31100`,
- execution mode: `Shadow`,
- entry mode: `shadow_only`,
- p37 namespace:
  `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2`,
- decision logger path:
  `/root/Gho/logs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/decisions`.

## 4. Ryzyka resztkowe

- `shadow_exit_replay_v1.jsonl` pojawi sie dopiero po shadow entry i finalizacji/horizon/shutdown trackera.
- Stary `target24-stop3` run zostal zatrzymany, ale jego artefakty pozostaja w oddzielnym katalogu.
- Przerwany `target60-stop60-exit-replay-r1` moze miec szczatkowe artefakty i nie powinien byc traktowany jako finalny R48 replay run.
- Nowe configi sa lokalne i nie byly commitowane/pushowane w tym kroku.
- Runtime log ostrzega `PRODUCTION MODE`, ale `execution_mode=Shadow` i `entry_mode=shadow_only` sa potwierdzone w logu.

## 5. Decyzja

Nowy R48-derived shadow run `target60-stop60-exit-replay-r2` zostal uruchomiony jako data-collection run dla `shadow_exit_replay_v1`, bez zmiany aktywnego BUY/REJECT, Gatekeeper policy, selectora, alpha ani live execution.
