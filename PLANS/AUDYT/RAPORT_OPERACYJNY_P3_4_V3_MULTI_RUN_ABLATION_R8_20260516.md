# Raport operacyjny P3.4 V3 Multi-Run Ablation r8

Data: 2026-05-16
Namespace: `shadow-burnin-v3-p32-replay-r8`
Config: `configs/rollout/shadow-burnin-v3-p32-replay-r8.toml`

## Status

`INFRA-BLOCKED`

R8 nie jest zaliczalnym dowodem P3.4. Przebieg został zatrzymany po krótkim buforze diagnostycznym,
ponieważ primary Yellowstone global stream był blokowany przez limit współbieżnych strumieni i nie
wygenerował świeżych decision rows.

## Zakres

- P3.4 multi-run real ablation readiness.
- Shadow-only runtime.
- Brak P2 promotion.
- Brak zmian active V2/V2.5, IWIM, execution/live sender.
- Brak zmian scoringu i progów.

## Preflight

Preflight początkowo zatrzymał start, ponieważ lokalny baseline stamp wskazywał stary commit
`f58ce36e044fef210a62a5ee380652b6f85da7ed` zamiast aktualnego HEAD
`0c53b82b0f8d5f6f30f7626e5d1d180f5551819e`.

Wykonano wymagany check:

```bash
cargo test --workspace --no-run
```

Wynik: `OK`.

Następnie odświeżono lokalny `.ghost/baseline_accepted_revision` do HEAD i ponowiono preflight:

```bash
bash ./scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r8.toml
```

Wynik: `OK`.

## Runtime

Uruchomiono:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r8.toml
```

Przebieg został zatrzymany przed naturalnym timeoutem, ponieważ:

- `primary_global` wielokrotnie zwracał `ResourceExhausted`,
- `funding_lane_full_chain` ustanowił stream, ale sam funding lane nie wystarcza do decyzji V3,
- nie powstał `gatekeeper_v2_decisions.jsonl`,
- `v3_shadow_report.py` zwrócił `status=no_rows`,
- `v3_full_replay_report.py --strict` nie miał pliku decyzji do walidacji.

Po zatrzymaniu:

- brak aktywnego `ghost-launcher`,
- brak aktywnego `cargo run`,
- brak świeżych decision rows w namespace r8.

## Wyniki raportów

### Shadow Report

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r8.toml \
  --json
```

Wynik istotny:

- `status=no_rows`
- `v3_rows=0`
- `replay_status=unavailable`
- `full_snapshot_payload_rows=0`
- `stale_against_config=false`

### Full Replay Report

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r8.toml \
  --strict \
  --json
```

Wynik istotny:

- exit code: `1`
- brak pliku `logs/rollout/shadow-burnin-v3-p32-replay-r8/decisions/gatekeeper_v2_decisions.jsonl`

## Werdykt

R8 jest `INFRA-BLOCKED`, nie `NO-GO` modelowym.

Nie dowodzi regresji P3.3 ani błędu full replay. Dowodzi tylko, że w momencie uruchomienia nie było
dostępnego primary Yellowstone global stream. P3.4 nie może być walidowany bez świeżych decision rows.

## Następny krok

Uruchomić kolejny namespace P3.4 po zwolnieniu limitu Yellowstone albo po zapewnieniu oddzielnej
pojemności streamu dla primary global stream. Warunek zaliczenia pozostaje ten sam:

- świeże V3 rows,
- `v3_full_replay_report.py --strict` z `replay_status=full_replay_ok`,
- real counterfactual ablation w `scripts/v3_replay_ablation_report.py`,
- brak `payload_hash_mismatch`, `policy_hash_mismatch`, `verdict_mismatch`, `reason_mismatch`,
- brak P2 promotion.
