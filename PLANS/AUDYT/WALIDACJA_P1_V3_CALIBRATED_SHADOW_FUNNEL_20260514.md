# Walidacja P1 V3 Calibrated Shadow Funnel - 2026-05-14

## Zakres

Zrealizowano tylko P1.1-P1.5 jako addytywny V3 sidecar.

In scope:

- konfiguracja `GatekeeperV3Config` z backward-compatible defaultami,
- przepiecie ewaluatora V3 na `GatekeeperV3Config`,
- deterministyczne hashe `v3_policy_config_hash` i `v3_feature_snapshot_hash`,
- addytywne pola JSONL `v3_*`,
- rozszerzenie `scripts/v3_shadow_report.py`,
- namespace artefaktow `shadow-burnin-v3-p1`,
- targeted tests i clean-start shadow rerun.

Out of scope:

- aktywna polityka V2/V2.5,
- `GatekeeperBuffer::evaluate_from_features()`,
- IWIM,
- live sender / execution,
- zmiana progow policy,
- `decision_plane = "v3_shadow"`,
- bump schemy JSONL.

## Commity

- `ede4bdc Add Gatekeeper V3 shadow config`
- `a7a5d64 Wire Gatekeeper V3 shadow sidecar`
- `a29c87c Expand Gatekeeper V3 shadow report`
- `5fb3c63 Use Gatekeeper V3 P1 shadow artifact namespace`

## Pliki i kontrakty

- `ghost-brain/src/config/gatekeeper_v3_config.rs` dodaje `GatekeeperV3Config` z defaultami: `enabled=false`, `shadow_emit_enabled=false`, `policy_version=1`, `materialization_version=1`, `promotion.enabled=false`.
- `ghost-brain/src/config/ghost_brain_config.rs` dodaje `#[serde(default)] pub gatekeeper_v3: GatekeeperV3Config`, wiec stare TOML bez `[gatekeeper_v3]` nadal sie laduja.
- `ghost-brain/ghost_brain_config.toml` jawnie ustawia `[gatekeeper_v3] shadow_emit_enabled=true`; `enabled=false` pozostaje poza semantyka P1.
- `ghost-launcher/src/components/gatekeeper_v3.rs` uzywa `&GatekeeperV3Config`; wrapper kompatybilny pozostaje adapterem testowym.
- `ghost-brain/src/oracle/decision_logger.rs` dostal tylko addytywne `Option<T>` pola V3 z `#[serde(default, skip_serializing_if = "Option::is_none")]`; stare JSONL bez P1 fields nadal sie parsuje.
- `configs/rollout/shadow-burnin.toml` kieruje artefakty do `shadow-burnin-v3-p1`; progi policy i `payer_strategy` nie zostaly zmienione.

## Weryfikacja targeted

Przeszly:

```bash
cargo test -p ghost-core materialized
cargo test -p ghost-core feature_builder
cargo test -p ghost-brain gatekeeper_v3_config
cargo test -p ghost-brain decision_logger
cargo test -p ghost-brain reason_code
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher v3_shadow
python3 -m unittest scripts/test_v3_shadow_report.py
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

Przed rerunem raport zwracal `status=no_rows`, co bylo oczekiwane dla pustego namespace P1.

## Clean P1 Rerun

Uruchomiono:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin.toml
```

Start runtime potwierdzil:

- `execution_mode=Shadow`
- `entry_mode=shadow_only`
- `Gatekeeper V3 sidecar config: enabled=false shadow_emit=true policy_version=1 materialization_version=1`
- artifact namespace `shadow-burnin-v3-p1`

Proces zostal zatrzymany kontrolowanym `SIGTERM` po uzyskaniu wystarczajacego P1 evidence i po pojawieniu sie lawiny `Transport channel disconnected` po `SIGINT`. To nie byl naturalny 30-minutowy timeout.

Glowne artefakty:

- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl`
- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/decisions/seer_runtime_coverage_audit.jsonl`
- `/root/Gho/datasets/events/shadow-burnin-v3-p1/`
- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/system.log.2026-05-14`
- `/root/Gho/logs/rollout/shadow-burnin-v3-p1/oracle.log.2026-05-14`

## Finalny Raport P1

Komenda:

```bash
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

Wynik po rerunie:

- `status=ok`
- `raw_rows=29`
- `deduped_rows=29`
- `v3_rows=29`
- `bad_rows=0`
- `no_v3_rows=0`
- `duplicate_rows_removed=0`
- `v3_policy_config_hash.coverage=1.0`
- `v3_feature_snapshot_hash.coverage=1.0`
- `policy_hash_unique_count=1`
- `snapshot_hash_unique_count=29`
- `snapshot_uniqueness.duplicate_row_count=0`
- `replay_status=hash_only`

Hash/config matrix:

- V2/V2.5 routing `config_hash`: `05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68`
- V3 policy hash: `cbac13ab21b1a09d4d1e47b1656f6593815e728d738c2e130fbc757402e3861d`
- count: `29`

`replay_status=hash_only` jest poprawnym wynikiem dla P1, bo JSONL ma stabilne snapshot hashes, ale nie zawiera pelnego payloadu `MaterializedFeatureSet` do pelnego replay.

## Interpretacja PENDING / REJECT vs P0

P1 poprawia interpretowalnosc wzgledem P0, bo rozbija aktywne V2/V2.5 `REJECT` na osobna warstwe V3:

- active `REJECT` -> V3 `REJECT`: `20`
- active `REJECT` -> V3 `PENDING`: `9`

V3 reason distribution:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `20`
- `PENDING_V3_WAIT_EVIDENCE`: `7`
- `PENDING_V3_WAIT_SAMPLE`: `2`

Stage distribution:

- `RISK`: `20`
- `EVIDENCE`: `9`

Component buckets pokazuja, ze final confidence zostalo wyzerowane we wszystkich 29 wierszach, ale z dwoch roznych powodow:

- `hard_risk`: `20`
- `insufficient_evidence`: `9`

To daje lepsza diagnostyke niz sam P0: `REJECT` nie jest juz jednolita kategoria interpretacyjna. P1 rozroznia przypadki, ktore V3 odrzuca przez risk/manipulation contradiction, od przypadkow, ktore V3 klasyfikuje jako brak wystarczajacej probki/evidence i trzyma w `PENDING`.

Actionability:

- stages `opportunity` i `risk`: `actionable=29`
- stages `evidence` i `confidence`: `not_actionable=29`
- `organic_broadening`, `pdd_sequence`, `tx_segments`: `actionable=18`, `wait_sample=11`
- `fsc`, `sybil`, `execution`, `manipulation_contradiction`: degraded/shadow-only dla 29 wierszy

Wniosek: P1 daje uzyteczna interpretacje PENDING/REJECT dla shadow funnel, ale nie jest dowodem pelnego replay ani dowodem wykonania transakcji. Execution w raporcie ma `missing=29`, `success_count=0`, zgodnie z zasada, ze submit/no_dispatch/missing nie sa sukcesem.

## Acceptance

- Stare TOML bez `[gatekeeper_v3]`: pokryte defaultem i testem `gatekeeper_v3_config`.
- Stare JSONL bez P1 fields: pokryte `decision_logger`.
- Stabilny V3 config hash: jeden hash dla 29 wierszy.
- Stabilny feature snapshot hash: 29 unikalnych hashy, zero duplikatow.
- V3 sidecar-only: `enabled=false`, `shadow_emit_enabled=true`, brak zmian active policy/IWIM/execution.
- P1 report: `status=ok`, `v3_rows=29`, hash coverage `1.0`, `replay_status=hash_only`.

## Residual Risk

- Rerun nie zakonczyl sie naturalnym 30-minutowym timeoutem; zostal zatrzymany po uzyskaniu wystarczajacego P1 evidence.
- `hash_only` nie pozwala odtworzyc pelnego `MaterializedFeatureSet`; potwierdza deterministyczne porownanie snapshot hash, nie full replay.
- Brak execution success jest oczekiwany w shadow-only, ale oznacza, ze P1 waliduje interpretacje decyzji, nie lifecycle wykonania.
