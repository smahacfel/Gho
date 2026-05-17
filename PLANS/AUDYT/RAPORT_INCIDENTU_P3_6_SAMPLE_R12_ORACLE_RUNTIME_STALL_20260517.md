# Raport incydentu P3.6 sample R12: OracleRuntime stall po 12:14 UTC

Data: 2026-05-17
Namespace: `shadow-burnin-v3-p36-sample-r12-primary-only`
Status incydentu: `ROOT_CAUSE_IDENTIFIED_AND_CODE_REMEDIATED`
Decyzja: R12 sample run jest niepoprawny jako wielogodzinny dataset. Do analizy mozna uzyc tylko pierwszych 74 V3 rows sprzed awarii OracleRuntime.

## Executive summary

Run P3.6 sample R12 nie stal dlatego, ze RPC/Yellowstone przestalo dostarczac dane. Seer nadal odbieral i emitowal eventy przez Event Bus, ale o `2026-05-17T12:14:21Z` padl task `OracleRuntime`.

Bez `OracleRuntime` nowe `NewPoolDetected` i `PoolTransaction` nadal pojawialy sie w logach Seer/Trigger, ale nie byly juz przetwarzane do nowych sesji obserwacyjnych i decyzji Gatekeeper/V3. To stworzylo falszywy stan "proces zyje", podczas gdy pipeline decyzyjny byl martwy.

Przyczyna kodowa: race na liczniku `oracle_runtime_account_update_queue_depth`. Stary kod wysylal event do workera przed inkrementacja licznika. Worker mogl przetworzyc update szybciej i wykonac `fetch_sub(1)` na zerze. To ustawialo atomowy licznik na `usize::MAX`; kolejny dispatch robil `fetch_add(1) + 1`, co skonczylo sie panicem `attempt to add with overflow`.

## Impact

- Ostatni decision log mtime: `2026-05-17 12:14:15.727072025 +0000`.
- Ostatni potwierdzony `OracleRuntime` processing `NewPoolDetected`: `2026-05-17T12:14:21.183141Z`.
- Po awarii Event Bus pokazuje spadek odbiorcow z `receivers=6` do `receivers=5`, przy nadal aktywnych emisjach Seer.
- Wielogodzinna czesc po `12:14:21Z` nie wygenerowala nowych decyzji V3.
- R12 ma tylko `v3_rows=74`, a nie wielogodzinny dataset.
- Strict replay dla tych 74 rows jest poprawny: `replay_status=full_replay_ok`, `status_counts.full_replay_ok=74`.

## Evidence

Log panicu:

```text
thread 'tokio-rt-worker' (1450581) panicked at ghost-launcher/src/oracle_runtime.rs:9248:25:
attempt to add with overflow
```

Kontekst tuz przed panicem:

```text
2026-05-17T12:14:21.183141Z INFO ghost_launcher::oracle_runtime:
OBSLUGUJE EVENT NewPoolDetected: pool=55PUeipaMPgA7tQGAyEjpioy5j5TasDMQvp2WENZfAEU
```

Po awarii Seer nadal emitowal eventy, ale juz z mniejsza liczba odbiorcow:

```text
Seer: Event emitted to Event Bus for new pool: ... receivers=5
Seer: PoolTransaction ZOSTALA PRZEKAZANA DO MAGISTRALI ZDARZEN: receivers=5
```

Przy kontrolowanym zamknieciu starego runu launcher ujawnil oczekujacy blad JoinHandle:

```text
Oracle Runtime shutdown error: task 16 panicked with message "attempt to add with overflow"
```

## Root cause

Stary przeplyw w `dispatch_account_update_to_worker`:

1. `worker_tx.send(event)`
2. `queue_depth.fetch_add(1) + 1`

Stary przeplyw w workerze:

1. `work_rx.recv().await`
2. `process_runtime_account_update_event(...)`
3. `queue_depth.fetch_sub(1).saturating_sub(1)`

Problem: `mpsc::UnboundedSender::send` moze natychmiast udostepnic event workerowi. Worker moze zejsc do `fetch_sub(1)` zanim dispatch wykona `fetch_add(1)`. `saturating_sub(1)` zabezpieczal tylko wartosc lokalna `remaining`, ale nie zabezpieczal samego atomika przed underflow. Atomik przechodzil na `usize::MAX`, a pozniejszy `fetch_add(1) + 1` panikowal na overflow.

## Remediation implemented

Zmienione pliki:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/main.rs`

Zmiany w `oracle_runtime.rs`:

- Inkrementacja `queue_depth` nastepuje przed `worker_tx.send(event)`.
- Gdy `send` sie nie powiedzie, licznik jest cofany.
- Inkrementacja uzywa `fetch_update(... checked_add(1))`, wiec saturacja licznika nie wywola overflow panic.
- Dekrementacja uzywa atomowego `saturating_sub` wewnatrz `fetch_update`, wiec atomik nie moze zejsc przez zero do `usize::MAX`.
- Dodano jawny warning `OracleRuntime AccountUpdate queue depth underflow prevented`.
- Dodano testy:
  - `test_account_update_queue_depth_does_not_underflow`
  - `test_account_update_queue_depth_saturation_is_fail_closed`

Zmiany w `main.rs`:

- `OracleRuntime` nie jest juz tylko wrzucany do listy taskow oczekiwanych dopiero po `Ctrl+C`.
- Launcher wykonuje `tokio::select!` na:
  - `signal::ctrl_c()`
  - przedwczesnym zakonczeniu/paniku `OracleRuntime`
- Jesli `OracleRuntime` zatrzyma sie przed shutdown signalem, proces konczy sie kodem `6`.
- Celem jest fail-fast zamiast wielogodzinnego silent stall.

## Verification

Wykonane lokalnie:

```text
cargo test -p ghost-launcher test_account_update_queue_depth
```

Wynik:

```text
2 passed
```

```text
cargo test -p ghost-launcher test_account_update_worker_preserves_all_updates_for_same_mint
```

Wynik:

```text
1 passed
```

```text
cargo check -p ghost-launcher --bin ghost-launcher
```

Wynik:

```text
Finished dev profile
```

```text
cargo build --release -p ghost-launcher --bin ghost-launcher
```

Wynik:

```text
Finished release profile
```

Release binary zostala odswiezona:

```text
2026-05-17 19:46:38.845444703 +0000 target/release/ghost-launcher
```

```text
git diff --check
```

Wynik:

```text
OK
```

Raporty R12 po zatrzymaniu:

```text
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml --json
```

Kluczowe wyniki:

- `status=ok`
- `v3_rows=74`
- `replay_status=full`
- `stale_against_config=false`
- `v3_reason_codes.REJECT_V3_MANIPULATION_CONTRADICTION=54`
- `v3_reason_codes.PENDING_V3_WAIT_EVIDENCE=18`
- `v3_reason_codes.PENDING_V3_WAIT_SAMPLE=2`

```text
python3 scripts/v3_full_replay_report.py --config configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml --strict --json
```

Kluczowe wyniki:

- `status=ok`
- `replay_status=full_replay_ok`
- `v3_rows=74`
- `status_counts.full_replay_ok=74`

## Prevention contract

Od tej poprawki analogiczny problem nie powinien juz trwac godzinami niezauwazony:

1. Ten konkretny overflow nie powinien sie powtorzyc, bo licznik kolejki nie moze juz zejsc przez zero ani panikowac na `+1`.
2. Jesli `OracleRuntime` padnie z innego powodu, launcher ma zakonczyc proces kodem `6`, zamiast zostawic Seer/Trigger jako falszywie zywy run.
3. Dla dlugich rolloutow nalezy monitorowac nie tylko istnienie procesu i eventy Seer, ale tez postep decision rows oraz obecny status tasku `OracleRuntime`.

## Operational decision

R12 sample run jest zamkniety jako incydent operacyjny.

Po zamknieciu incydentu nie ma aktywnego procesu `ghost-launcher` ani `cargo run` dla tego rolloutu.

Uzyteczne artefakty:

- pierwsze 74 V3 rows,
- strict full replay proof dla tych 74 rows,
- dowod awarii runtime po `12:14:21Z`.

Nie nalezy traktowac R12 jako wielogodzinnej probki kalibracyjnej P3.6.

Nastepny run powinien isc w nowym namespace po zbudowaniu release binary z ta poprawka.
