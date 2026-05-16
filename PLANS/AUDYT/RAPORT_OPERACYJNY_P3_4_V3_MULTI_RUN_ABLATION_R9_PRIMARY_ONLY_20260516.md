# Raport operacyjny P3.4 V3 Multi-Run Ablation r9 primary-only

Data: 2026-05-16
Namespace: `shadow-burnin-v3-p32-replay-r9-primary-only`
Config: `configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml`

## Status

`DIAGNOSTIC-GO`

R9 jest zaliczalnym dowodem dla warstwy full replay i real counterfactual ablation, ale nie jest
pełnym dowodem FSC/funding completeness. Profil świadomie ustawia `funding_lane_mode = "disabled"`,
żeby ominąć limit jednoczesnych streamów Yellowstone po tym, jak r8 został zablokowany przez
`ResourceExhausted` na primary global stream.

## Zakres

- P3.4 multi-run real ablation diagnostic.
- Shadow-only runtime.
- V3 replay payload enabled przez izolowany brain config.
- Brak P2 promotion.
- Brak zmian active V2/V2.5, IWIM, execution/live sender.
- Brak zmian scoringu i progów.

## Dlaczego primary-only

R8 z `funding_lane_mode = "full_chain"` ustanowił dedicated funding lane, ale primary global stream
był blokowany przez `ResourceExhausted`. Bez primary streamu nie powstały decision rows.

R9 wyłącza dedicated funding lane, dzięki czemu primary global stream mógł działać i wygenerować
świeże V3 decision rows. Konsekwencja: wszystkie rows mają zdegradowane FSC/sybil funding evidence,
więc wynik nie może być użyty jako pełna walidacja funding completeness ani jako promotion gate.

## Preflight

Wykonano:

```bash
cargo test --workspace --no-run
```

Wynik: `OK`.

Następnie:

```bash
bash ./scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml
```

Wynik: `OK`.

## Runtime

Uruchomiono:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml
```

Proces został zatrzymany kontrolowanym `SIGTERM` po zebraniu wystarczającej próbki diagnostycznej.
Po zatrzymaniu `pgrep` nie pokazywał aktywnego `ghost-launcher` ani `cargo run`.

## Wyniki r9

### Shadow report

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml \
  --json
```

Najważniejsze:

- `status=ok`
- `v3_rows=28`
- `replay_status=full`
- `full_snapshot_payload_rows=28`
- `v3_policy_config_hash.coverage=1.0`
- `v3_feature_snapshot_hash.coverage=1.0`
- `snapshot_hash_unique_count=28`
- `duplicate_row_count=0`
- `stale_against_config=false`
- `fsc.degraded=28`
- `sybil.degraded=28`
- `manipulation_contradiction.degraded=28`

Rozkład V3:

- `REJECT_V3_MANIPULATION_CONTRADICTION=21`
- `PENDING_V3_WAIT_EVIDENCE=7`
- V3 verdicts: `REJECT=21`, `PENDING=7`

### Strict full replay

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml \
  --strict \
  --json
```

Najważniejsze:

- `status=ok`
- `replay_status=full_replay_ok`
- `v3_rows=28`
- `bad_rows=0`
- `status_counts.full_replay_ok=28`
- `payload_hash_mismatch=0`
- `policy_hash_mismatch=0`

### Real counterfactual ablation

```bash
python3 scripts/v3_replay_ablation_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml \
  --json
```

Najważniejsze:

- `status=ok`
- `ablation.mode=full_replay_counterfactual`
- `ablation.replay_status=full_replay_ok`
- `baseline_status_counts.full_replay_ok=28`
- `p3_status=insufficient_data`
- `no_p2_promotion=true`

Warianty:

- `no_manipulation_contradiction.changed_verdict_rows=21/28`
- `no_manipulation_contradiction.verdict_distribution.PENDING=28`
- `no_organic_broadening.changed_verdict_rows=0`
- `no_sybil_fsc_cpv_caps.changed_verdict_rows=0`
- `no_alpha_cap.changed_verdict_rows=0`
- `no_execution_cap.changed_verdict_rows=0`

Interpretacja: `REJECT_V3_MANIPULATION_CONTRADICTION` jest ponownie decyzyjnie przyczynowy. Po
usunięciu tego komponentu wszystkie r9 rows przechodzą do `PENDING`, nie do `BUY`.

## Multi-run comparison z r7

Porównano r9 z r7:

```bash
python3 scripts/v3_replay_ablation_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml \
  --compare-decisions-log /root/Gho/logs/rollout/shadow-burnin-v3-p32-replay-r7/decisions/shadow-burnin-v3-p32-replay-r7/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl \
  --json
```

Wynik:

- r9 rows: `28`, V3 `REJECT=21`, `PENDING=7`
- r7 rows: `20`, V3 `REJECT=14`, `PENDING=6`
- policy hash set: wspólny `9b55a78eb05943e6bd89b28d7f78ef9eac714346476a86553877bce47d07ab1c`
- shared reason codes:
  - `PENDING_V3_WAIT_EVIDENCE`
  - `REJECT_V3_MANIPULATION_CONTRADICTION`
- right-only r7 reason:
  - `PENDING_V3_WAIT_SAMPLE`
- reason Jaccard: `0.666667`

Wniosek multi-run: dominujący reject bucket i policy hash są stabilne między r7 i r9. Różnice w
reason set dotyczą małej próbki i obecności `PENDING_V3_WAIT_SAMPLE` w r7. R9 nie jest w pełni
porównywalny z r7 pod kątem funding/FSC, bo działał primary-only.

## Werdykt

P3.4 r9 primary-only:

- `GO` dla potwierdzenia, że full replay i real counterfactual ablation działają na drugim świeżym
  przebiegu,
- `GO` dla hipotezy, że `REJECT_V3_MANIPULATION_CONTRADICTION` jest stabilnie decyzyjnie
  przyczynowy,
- `NO-GO` dla P2 promotion,
- `NO-GO` dla pełnej walidacji funding/FSC completeness,
- `INSUFFICIENT_DATA` dla oceny ekonomicznej, bo outcome label coverage nadal wynosi `0.0`.

## Następny krok

Nie przechodzić do P2.

Następny użyteczny krok to P3.5: outcome-label join / shadow lifecycle economics dla r7+r9 oraz
kolejny canonical full-chain run, gdy provider stream capacity pozwoli uruchomić primary global i
dedicated funding lane jednocześnie.
