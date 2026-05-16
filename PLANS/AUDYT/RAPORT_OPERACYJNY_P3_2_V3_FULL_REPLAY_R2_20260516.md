# RAPORT OPERACYJNY P3.2 V3 FULL REPLAY R2 - 2026-05-16

## Status

**NO-GO / fail-closed.**

Run P3.2 r2 zostal wykonany na czystym namespace i wygenerowal swieze V3 rows z pelnym payloadem, ale strict full replay nie przeszedl:

- `v3_rows=71`
- `replay_status=fail_closed`
- `status_counts.payload_hash_mismatch=71`
- `--strict` zakonczyl sie exit `2`
- `policy_hash_mismatch=0`
- `payload_absent=0`

P3.2 r2 nie spelnia warunku sukcesu `replay_status=full_replay_ok`.

## Zakres

Run dotyczy wylacznie P3.2 Commit 4: kontrolowany shadow rerun z `replay_payload_enabled=true` w izolowanym profilu:

- rollout config: `configs/rollout/shadow-burnin-v3-p32-replay-r2.toml`
- isolated Ghost Brain config: `configs/rollout/ghost_brain_v3_p32_replay.toml`
- artifact namespace: `shadow-burnin-v3-p32-replay-r2`

Non-goals zachowane:

- brak P2 promotion
- brak zmian active V2/V2.5 policy
- brak zmian progow/scoringu
- brak zmian IWIM
- brak zmian execution/live sender

## Archiwizacja i clean namespace

Przed rerunem wykryto stare artefakty r2, wiec run nie zostal wtedy uruchomiony.

Stary namespace zostal zarchiwizowany do:

```text
/root/Gho_artifact_archive/shadow-burnin-v3-p32-replay-r2.pre-clean-20260516T103230Z
```

Po archiwizacji preflight:

```bash
find logs/rollout/shadow-burnin-v3-p32-replay-r2 \
  logs/shadow_run/shadow-burnin-v3-p32-replay-r2 \
  logs/shadow_run/shadow-burnin-v3-p32-replay-r2-buys.jsonl \
  datasets/events/shadow-burnin-v3-p32-replay-r2 \
  data/rollout/shadow-burnin-v3-p32-replay-r2 \
  -type f 2>/dev/null | head
```

zwrocil pusty wynik. Namespace byl czysty przed startem runu.

## Wykonanie

Komenda:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r2.toml
```

Wynik procesu:

- exit code: `124`
- interpretacja: oczekiwane zakonczenie przez `timeout 30m`
- po zakonczeniu `pgrep -af 'ghost-launcher|cargo run|target/release/ghost-launcher'` nie wykazal aktywnego procesu

Log startowy potwierdzil V3 sidecar:

```text
enabled=false shadow_emit=true policy_version=1 materialization_version=1 hash=9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c
```

## Swieze artefakty

Wygenerowane artefakty po archiwizacji:

- `logs/rollout/shadow-burnin-v3-p32-replay-r2/system.log.2026-05-16`
- `logs/rollout/shadow-burnin-v3-p32-replay-r2/oracle.log.2026-05-16`
- `logs/rollout/shadow-burnin-v3-p32-replay-r2/decisions/seer_runtime_coverage_audit.jsonl`
- `logs/rollout/shadow-burnin-v3-p32-replay-r2/decisions/shadow-burnin-v3-p32-replay-r2/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- `logs/rollout/shadow-burnin-v3-p32-replay-r2/decisions/shadow-burnin-v3-p32-replay-r2/v2.2/legacy_live/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568090_20260516_103248_0000.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568149_20260516_103248_0000.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568149_20260516_103748_0001.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568149_20260516_104252_0002.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568149_20260516_104756_0003.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568149_20260516_105259_0004.jsonl`
- `datasets/events/shadow-burnin-v3-p32-replay-r2/exec_launcher-1778927568149_20260516_105801_0005.jsonl`
- `data/rollout/shadow-burnin-v3-p32-replay-r2/snapshots/shadow_ledger_snapshot_1778929188090.bin`
- `data/rollout/shadow-burnin-v3-p32-replay-r2/snapshots/shadow_ledger_snapshot_1778929248090.bin`
- `data/rollout/shadow-burnin-v3-p32-replay-r2/snapshots/shadow_ledger_snapshot_1778929308090.bin`

Mtime decision log:

```text
2026-05-16T11:02:23Z logs/rollout/shadow-burnin-v3-p32-replay-r2/.../v25_shadow/.../gatekeeper_v2_decisions.jsonl
```

## Walidacja full replay

Polecenie:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r2.toml \
  --json
```

Wynik:

- `status=fail_closed`
- `replay_status=fail_closed`
- `total_rows=71`
- `bad_rows=0`
- `v3_rows=71`
- `status_counts.payload_hash_mismatch=71`

Polecenie strict:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r2.toml \
  --strict --json
```

Wynik:

- exit code: `2`
- `status=fail_closed`
- `replay_status=fail_closed`
- `status_counts.payload_hash_mismatch=71`

Przykladowy pierwszy mismatch:

```text
expected b13e0a214533aa5e65a8f6c326086bb21f15a34ae3e550092caf9036b61eb57b
recomputed e30f7cff159041722d534959d7c07194ccfa679f456cd87b1ae6d13540757490
```

## Walidacja shadow report

Polecenie:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r2.toml \
  --json
```

Wynik operacyjny:

- `status=ok`
- `replay_status=full`
- `artifact_freshness.stale_against_config=false`
- `counts.raw_rows=71`
- `counts.deduped_rows=71`
- `counts.v3_rows=71`
- `pre_dedupe_conflicts.conflict_groups=0`
- `hash_coverage.v3_policy_config_hash.coverage=1.0`
- `hash_coverage.v3_feature_snapshot_hash.coverage=1.0`
- `hash_consistency.policy_hash_unique_count=1`
- `hash_consistency.snapshot_hash_unique_count=71`
- `replay.full_snapshot_payload_rows=71`
- `replay.hash_only_rows=0`

Interpretacja: payload jest obecny i swiezy, ale sam fakt obecnosci payloadu nie dowodzi kontraktu full replay. Decydujacy jest fail-closed validator, ktory wykazal `payload_hash_mismatch` dla wszystkich V3 rows.

## V3 distribution z tego runu

Reason codes V3:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `44`
- `REJECT_V3_LOW_ORGANIC_BROADENING`: `8`
- `PENDING_V3_WAIT_EVIDENCE`: `15`
- `PENDING_V3_WAIT_SAMPLE`: `4`

Stage distribution:

- `RISK`: `44`
- `EVIDENCE`: `19`
- `OPPORTUNITY`: `8`

Active vs V3:

- active `REJECT` -> V3 `REJECT`: `52`
- active `REJECT` -> V3 `PENDING`: `19`

Evidence/actionability highlights:

- `manipulation_contradiction`: `clean=10`, `degraded=61`
- `fsc`: `clean=11`, `degraded=60`
- `sybil`: `clean=11`, `degraded=60`
- `organic_broadening`: `clean=44`, `insufficient_sample=27`

## Decyzja operacyjna

P3.2 r2 jest **NO-GO** jako full replay readiness.

Zatwierdzone czesciowo:

- czysty namespace po archiwizacji
- runtime uruchomiony i zakonczony kontrolowanym timeoutem
- swieze V3 rows > 0
- pelny payload obecny w 71/71 V3 rows
- policy hash spójny w 71/71 rows
- stale artifacts nie zostaly zmieszane ze swiezym runem

Blokujace:

- `payload_hash_mismatch=71/71`
- `replay_status=fail_closed`
- `--strict` exit `2`
- brak `full_replay_ok`

## Wniosek

Nie wolno traktowac P3.2 full replay jako domknietego. Obecny runtime potrafi emitowac payload, ale kontrakt replay-stable snapshot hash nie jest spelniony.

Nastepny krok powinien byc techniczny P3.2 follow-up:

1. zreprodukowac mismatch na pojedynczym row,
2. porownac runtime hash payload source z validator hash source,
3. doprowadzic do tego, aby `v3_feature_snapshot_hash` byl liczony z dokladnie tej samej reprezentacji, ktora trafia do `v3_materialized_feature_snapshot`,
4. dopiero potem wykonac kolejny clean rerun w nowym albo ponownie wyczyszczonym namespace.
