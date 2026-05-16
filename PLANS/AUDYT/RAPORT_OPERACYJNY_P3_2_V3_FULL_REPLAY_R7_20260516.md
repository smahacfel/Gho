# RAPORT OPERACYJNY P3.2 V3 FULL REPLAY R7 - 2026-05-16

## Status

**GO dla P3.2 full replay parity.**

P3.2 r7 zostal uruchomiony po remediacji granicy JSONL hash payloadu i jako
pierwszy swiezy run przeszedl strict full replay:

- `v3_rows=20`
- `replay_status=full_replay_ok`
- `status_counts.full_replay_ok=20`
- `bad_rows=0`
- `payload_hash_mismatch=0`
- `policy_hash_mismatch=0`
- `verdict_mismatch=0`
- `stage_mismatch=0`
- `reason_mismatch=0`
- `score_mismatch=0`

To zamyka techniczny blocker P3.2 znany z r2-r6. Nie jest to zgoda na P2
promotion.

## Zakres

Run dotyczy wylacznie walidacji P3.2 full replay payload po commicie:

```text
f58ce36 Canonicalize V3 replay payload hash at JSONL boundary
```

Profil:

- rollout config: `configs/rollout/shadow-burnin-v3-p32-replay-r7.toml`
- isolated Ghost Brain config: `configs/rollout/ghost_brain_v3_p32_replay.toml`
- artifact namespace: `shadow-burnin-v3-p32-replay-r7`

Non-goals zachowane:

- brak P2 promotion
- brak zmian active V2/V2.5 policy
- brak zmian progow/scoringu V3
- brak zmian IWIM
- brak zmian execution/live sender
- brak tworzenia `decision_plane=v3_shadow`

## Preflight

Przed runem preflight zostal wykonany na configu r7:

```bash
bash scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r7.toml
```

Wynik:

- baseline accepted revision: `f58ce36e044fef210a62a5ee380652b6f85da7ed`
- execution profile: `execution_mode=Shadow`, `entry_mode=shadow_only`
- trigger balance: `0.047172000 SOL >= 0.007200000 SOL`
- metrics port: free `0.0.0.0:9090`
- preflight: all runtime checks passed

Przed startem namespace r7 nie mial starych artefaktow.

## Wykonanie

Komenda:

```bash
timeout 30m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p32-replay-r7.toml
```

Run zostal zatrzymany kontrolowanie po uzyskaniu rozstrzygajacej probki
strict full replay. Po zatrzymaniu:

```bash
pgrep -af 'ghost-launcher|cargo run'
```

nie wykazal aktywnego procesu.

## Artefakty

Swiezy decision log:

```text
logs/rollout/shadow-burnin-v3-p32-replay-r7/decisions/shadow-burnin-v3-p32-replay-r7/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl
```

Liczba wierszy:

```text
20
```

Logi r7:

- `logs/rollout/shadow-burnin-v3-p32-replay-r7/system.log.2026-05-16`
- `logs/rollout/shadow-burnin-v3-p32-replay-r7/oracle.log.2026-05-16`
- `logs/rollout/shadow-burnin-v3-p32-replay-r7/decisions/seer_runtime_coverage_audit.jsonl`

## Strict Full Replay

Polecenie:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r7.toml \
  --strict --json
```

Wynik:

- `status=ok`
- `replay_status=full_replay_ok`
- `total_rows=20`
- `bad_rows=0`
- `v3_rows=20`
- `status_counts.full_replay_ok=20`

Wszystkie row results mialy:

```text
status=full_replay_ok
detail=null
```

## Shadow Report

Polecenie:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r7.toml \
  --json
```

Wynik operacyjny:

- `status=ok`
- `replay_status=full`
- `artifact_freshness.stale_against_config=false`
- `counts.raw_rows=20`
- `counts.deduped_rows=20`
- `counts.v3_rows=20`
- `pre_dedupe_conflicts.conflict_groups=0`
- `hash_coverage.v3_policy_config_hash.coverage=1.0`
- `hash_coverage.v3_feature_snapshot_hash.coverage=1.0`
- `hash_consistency.policy_hash_unique_count=1`
- `hash_consistency.snapshot_hash_unique_count=20`
- `replay.full_snapshot_payload_rows=20`
- `replay.hash_only_rows=0`

V3 reason distribution:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `14`
- `PENDING_V3_WAIT_EVIDENCE`: `5`
- `PENDING_V3_WAIT_SAMPLE`: `1`

Active vs V3:

- active `REJECT` -> V3 `REJECT`: `14`
- active `REJECT` -> V3 `PENDING`: `6`

## Hash Probe

Po runie sprawdzono brak warningow:

```bash
rg -n \
  'V3 replay payload hash self-check mismatch|V3 replay payload hash logger-boundary mismatch|V3 replay payload hash post-serialize mismatch' \
  logs/rollout/shadow-burnin-v3-p32-replay-r7
```

Wynik: brak trafien.

Interpretacja: r7 nie pokazuje mismatchu w runtime self-checku, logger-boundary
probe ani post-serialize probe.

## P3 Replay / Ablation Report

Dodatkowy offline report:

```bash
python3 scripts/v3_replay_ablation_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r7.toml \
  --json
```

Wynik:

- `status=ok`
- `replay.status=full`
- `replay.full_snapshot_payload_rows=20`
- `certification.p3_status=insufficient_data`
- `certification.no_p2_promotion=true`
- `certification.insufficient_evidence_gates`:
  - `dominant_manipulation_contradiction_requires_more_evidence`
  - `low_outcome_label_coverage`

Istotne ograniczenie: obecny P3 ablation report nadal uzywa
`reason_group_proxy`, mimo ze r7 ma juz full replay payload. To oznacza, ze P3.2
jest domkniete technicznie, ale P3.3 musi zastapic proxy prawdziwym
counterfactual recompute.

## Decyzja operacyjna

P3.2 r7 jest **GO** dla full replay parity:

- payload jest obecny
- hash payloadu jest replay-stable
- policy payload/hash jest spojny
- Rust validator odtwarza verdict/stage/reason/score dla 20/20 V3 rows
- stale artifacts nie sa zmieszane z runem

P3.2 r7 nie jest **GO** dla P2:

- brak outcome label coverage
- dominujacy bucket `REJECT_V3_MANIPULATION_CONTRADICTION` nadal wymaga realnej
  ablation
- obecny P3 ablation report jest jeszcze proxy, nie counterfactual recompute

## Nastepny krok

Uruchomic P3.3 jako narzedzie offline:

1. uzyc full replay payloadu jako wejscia,
2. odtworzyc baseline V3 przez Rust evaluator,
3. wykonac kontrfaktyczne warianty na tym samym `MaterializedFeatureSet`,
4. policzyc zmiany verdict/stage/reason/confidence,
5. utrzymac `no_p2_promotion=true`.

P3.3 ma odpowiedziec, czy dominujacy bucket
`REJECT_V3_MANIPULATION_CONTRADICTION` wnosi realna selekcje ryzyka, czy jest
zbyt szerokim hamulcem decyzji.
